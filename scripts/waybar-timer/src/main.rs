use std::env;
use std::fs;
use std::io::{self, ErrorKind, Write};
use std::os::unix::net::UnixDatagram;
use std::path::PathBuf;
use std::process::{self, Command, Stdio};
use std::str;
use std::thread;
use std::time::{Duration, Instant};

const ICON: &str = "<span size='large'>󰔛</span>";
const FIVE_MINUTES: Duration = Duration::from_secs(5 * 60);
const MAX_TIMER: Duration = Duration::from_secs(2 * 60 * 60);
const POLL_INTERVAL: Duration = Duration::from_millis(100);

struct Timer {
    remaining: Duration,
    running: bool,
    last_tick: Instant,
}

impl Timer {
    fn new() -> Self {
        Self {
            remaining: Duration::from_secs(0),
            running: false,
            last_tick: Instant::now(),
        }
    }

    /// Update the remaining time. Returns true when the timer ends.
    fn tick(&mut self) -> bool {
        let now = Instant::now();

        // That prevents paused time from being counted.
        if !self.running {
            self.last_tick = now;
            return false;
        }

        let elapsed = now.duration_since(self.last_tick);
        self.last_tick = now;

        if elapsed >= self.remaining {
            self.remaining = Duration::from_secs(0);
            self.running = false;
            true
        } else {
            self.remaining -= elapsed;
            false
        }
    }

    /// Apply a command.
    fn apply(&mut self, command: &str) -> bool {
        let ended = self.tick();

        match command {
            "add" => {
                let next = self.remaining + FIVE_MINUTES;
                if next > MAX_TIMER {
                    self.remaining = Duration::ZERO;
                    self.running = false;
                } else {
                    self.remaining = next;
                }
                self.last_tick = Instant::now();
            }
            "toggle" => {
                if !self.remaining.is_zero() {
                    self.running = !self.running;
                    self.last_tick = Instant::now();
                }
            }
            "clear" => {
                self.remaining = Duration::from_secs(0);
                self.running = false;
                self.last_tick = Instant::now();
            }
            _ => eprintln!("waybar-timer: unknown command: {command}"),
        }

        ended
    }

    /// Round upward so a freshly-added five minutes is displayed as 5:00.
    fn displayed_seconds(&self) -> u64 {
        let seconds = self.remaining.as_secs();
        if self.remaining.subsec_nanos() == 0 {
            seconds
        } else {
            seconds.saturating_add(1)
        }
    }

    fn state(&self) -> State {
        if self.running {
            State::Run
        } else if self.remaining.is_zero() {
            State::Stop
        } else {
            State::Pause
        }
    }
}

#[derive(Debug, PartialEq)]
enum State {
    Run,
    Pause,
    Stop,
}

impl State {
    fn output(&self) -> &'static str {
        match self {
            State::Run => "running",
            State::Pause => "paused",
            State::Stop => "stopped",
        }
    }
}

struct SocketGuard {
    path: PathBuf,
}

impl Drop for SocketGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn socket_path() -> PathBuf {
    if let Some(runtime_dir) = env::var_os("XDG_RUNTIME_DIR") {
        return PathBuf::from(runtime_dir).join("waybar-countdown.sock");
    }

    // XDG_RUNTIME_DIR normally exists on NixOS desktop sessions. This fallback
    // keeps different users from sharing the same /tmp socket name.
    let user = env::var("UID")
        .or_else(|_| env::var("USER"))
        .unwrap_or_else(|_| "unknown".to_string());
    let safe_user: String = user
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();

    env::temp_dir().join(format!("waybar-countdown-{safe_user}.sock"))
}

fn emit(timer: &Timer, previous: &mut Option<(u64, State)>) -> io::Result<()> {
    let total_seconds = timer.displayed_seconds();
    let state = timer.state();
    let current = (total_seconds, state);

    if previous.as_ref() == Some(&current) {
        return Ok(());
    }

    let minutes = total_seconds / 60;
    let seconds = total_seconds % 60;
    let time = format!("{minutes:02}:{seconds:02}");

    let text = if total_seconds == 0 {
        ICON.to_string()
    } else {
        format!("{ICON} {time}")
    };

    let mut stdout = io::stdout().lock();
    writeln!(
        stdout,
        r#"{{"text":"{text}","tooltip":"Timer","class":"{}"}}"#,
        timer.state().output()
    )?;
    stdout.flush()?;

    *previous = Some(current);
    Ok(())
}

fn start_alarm() {
    thread::spawn(|| {
        let ffplay = env::var("WAYBAR_TIMER_FFPLAY")
            .ok()
            .or_else(|| option_env!("WAYBAR_TIMER_FFPLAY").map(str::to_owned))
            .unwrap_or_else(|| "ffplay".to_string());

        for _ in 0..3 {
            let result = Command::new(&ffplay)
                .args([
                    "-nodisp",
                    "-autoexit",
                    "-f",
                    "lavfi",
                    "-i",
                    "sine=frequency=880:duration=1",
                ])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();

            if let Err(error) = result {
                eprintln!("waybar-timer: could not run {ffplay}: {error}");
                break;
            }
        }
    });
}

fn run_server() -> io::Result<()> {
    let path = socket_path();

    // A socket can remain after Waybar is killed or reloaded.
    match fs::remove_file(&path) {
        Ok(()) => {}
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }

    let socket = UnixDatagram::bind(&path)?;
    let _guard = SocketGuard { path };
    socket.set_read_timeout(Some(POLL_INTERVAL))?;

    let mut timer = Timer::new();
    let mut previous_output = None;
    let mut buffer = [0_u8; 32];

    emit(&timer, &mut previous_output)?;

    loop {
        if timer.tick() {
            start_alarm();
        }
        emit(&timer, &mut previous_output)?;

        match socket.recv(&mut buffer) {
            Ok(length) => {
                let command = str::from_utf8(&buffer[..length]).unwrap_or("").trim();

                if timer.apply(command) {
                    start_alarm();
                }
                emit(&timer, &mut previous_output)?;
            }
            Err(error)
                if error.kind() == ErrorKind::WouldBlock || error.kind() == ErrorKind::TimedOut => {
            }
            Err(error) if error.kind() == ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
}

fn send_command(command: &str) -> io::Result<()> {
    let path = socket_path();
    let socket = UnixDatagram::unbound()?;

    // Retry briefly in case Waybar and a click command start at nearly the same time.
    let mut last_error = None;
    for _ in 0..5 {
        match socket.send_to(command.as_bytes(), &path) {
            Ok(_) => return Ok(()),
            Err(error)
                if error.kind() == ErrorKind::NotFound
                    || error.kind() == ErrorKind::ConnectionRefused =>
            {
                last_error = Some(error);
                thread::sleep(Duration::from_millis(25));
            }
            Err(error) => return Err(error),
        }
    }

    Err(last_error
        .unwrap_or_else(|| io::Error::new(ErrorKind::NotFound, "timer socket is unavailable")))
}

fn print_help(program: &str) {
    eprintln!(
        "Usage:\n  {program}          Run the continuous Waybar module\n  {program} add      Add five minutes\n  {program} toggle   Start or pause\n  {program} clear    Stop and clear"
    );
}

fn main() {
    let mut args = env::args();
    let program = args.next().unwrap_or_else(|| "waybar-timer".to_string());

    let result = match args.next().as_deref() {
        None => run_server(),
        Some("add") => send_command("add"),
        Some("toggle") => send_command("toggle"),
        Some("clear") | Some("stop") => send_command("clear"),
        Some("-h") | Some("--help") | Some("help") => {
            print_help(&program);
            return;
        }
        Some(other) => {
            eprintln!("waybar-timer: unknown argument: {other}");
            print_help(&program);
            process::exit(2);
        }
    };

    if let Err(error) = result {
        eprintln!("waybar-timer: {error}");
        process::exit(1);
    }
}
