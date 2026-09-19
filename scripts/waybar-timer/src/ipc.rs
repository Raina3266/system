//! The socket protocol: Waybar polls `status`, click actions send commands,
//! and the daemon raises the alarm when the countdown ends.

use std::env;
use std::fs;
use std::io::{self, ErrorKind, Write};
use std::os::unix::net::UnixDatagram;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::str;
use std::thread;
use std::time::Duration;

use crate::model::Timer;
use crate::waybar::status_json;

const POLL_INTERVAL: Duration = Duration::from_millis(100);

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

pub(crate) fn run_server() -> io::Result<()> {
    let path = socket_path();

    // Keep a live daemon's socket; remove only a stale socket left by an
    // unclean shutdown.
    if path.exists() {
        match UnixDatagram::unbound()?.send_to(b"status", &path) {
            Ok(_) => {
                return Err(io::Error::new(
                    ErrorKind::AlreadyExists,
                    "timer is already running",
                ));
            }
            Err(error) if error.kind() == ErrorKind::ConnectionRefused => {
                fs::remove_file(&path)?;
            }
            Err(error) => return Err(error),
        }
    }

    let socket = UnixDatagram::bind(&path)?;
    let _guard = SocketGuard { path };
    socket.set_read_timeout(Some(POLL_INTERVAL))?;

    let mut timer = Timer::new();
    let mut buffer = [0_u8; 32];

    loop {
        if timer.tick() {
            start_alarm();
        }

        match socket.recv_from(&mut buffer) {
            Ok((length, sender)) => {
                let command = str::from_utf8(&buffer[..length]).unwrap_or("").trim();
                if command == "status" {
                    if let Some(address) = sender.as_pathname() {
                        // A Waybar client may disappear during a reload.
                        let _ = socket.send_to(status_json(&timer).as_bytes(), address);
                    }
                } else if timer.apply(command) {
                    start_alarm();
                }
            }
            Err(error)
                if error.kind() == ErrorKind::WouldBlock || error.kind() == ErrorKind::TimedOut => {
            }
            Err(error) if error.kind() == ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
}

pub(crate) fn print_status() -> io::Result<()> {
    let path = socket_path();
    let client_path = path.with_file_name(format!("waybar-countdown-{}.sock", std::process::id()));
    let socket = UnixDatagram::bind(&client_path)?;
    let _guard = SocketGuard { path: client_path };
    socket.set_read_timeout(Some(Duration::from_secs(1)))?;
    socket.send_to(b"status", path)?;

    let mut response = [0_u8; 256];
    let length = socket.recv(&mut response)?;
    let mut stdout = io::stdout().lock();
    stdout.write_all(&response[..length])?;
    stdout.write_all(b"\n")
}

pub(crate) fn send_command(command: &str) -> io::Result<()> {
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
