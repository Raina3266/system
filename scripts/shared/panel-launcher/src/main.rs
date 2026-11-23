use clap::{Arg, ArgAction, CommandFactory, Parser, Subcommand, builder::OsStringValueParser};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::env;
use std::ffi::OsString;
use std::fs;
use std::io::{self, Write};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::{Path, PathBuf};
use std::process::{self, Command, Stdio};

#[derive(Deserialize)]
struct Config {
    name: String,
    commands: Vec<String>,
    strict_help: bool,
    check_response: bool,
    astal: PathBuf,
    bundle: PathBuf,
    environment: BTreeMap<String, String>,
    forced_environment: BTreeMap<String, String>,
}

impl Config {
    fn cli(&self) -> clap::Command {
        let help = Arg::new("help").short('h').long("help").help("Print help");
        let help = if self.strict_help {
            help.action(ArgAction::SetTrue)
        } else {
            help.num_args(0..)
                .allow_hyphen_values(true)
                .value_parser(OsStringValueParser::new())
        };
        clap::Command::new(self.name.clone())
            .no_binary_name(true)
            .disable_help_flag(true)
            .disable_help_subcommand(true)
            .args_conflicts_with_subcommands(true)
            .arg(help)
            .subcommands(self.commands.iter().map(|name| {
                let command = clap::Command::new(name.clone()).disable_help_flag(true);
                if name == "toggle" {
                    command.arg(Arg::new("monitor").allow_hyphen_values(true))
                } else {
                    command
                }
            }))
    }

    fn child_command(&self, executable: &Path) -> Command {
        let mut command = Command::new(executable);
        for (key, value) in &self.environment {
            if env::var_os(key).is_none_or(|value| value.is_empty()) {
                command.env(key, value);
            }
        }
        command.envs(&self.forced_environment);
        command
    }
}

#[derive(Debug, Parser)]
#[command(version, args_conflicts_with_subcommands = true)]
struct Cli {
    /// Panel configuration followed by arguments parsed using that configuration.
    #[arg(long, value_names = ["PATH", "PANEL_ARGS"], num_args = 1.., allow_hyphen_values = true)]
    config: Option<Vec<OsString>>,
    #[command(subcommand)]
    command: Option<LauncherCommand>,
}

#[derive(Debug, Subcommand)]
enum LauncherCommand {
    /// Run a desktop integration helper.
    Runtime {
        #[command(subcommand)]
        command: desktop_runtime::Action,
    },
}

enum Action {
    Help,
    Run,
    Request(Request),
}

#[derive(Serialize)]
struct Request {
    command: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    monitor: Option<String>,
}

fn parse_action(config: &Config, args: &[OsString]) -> Result<Action, clap::Error> {
    let matches = config.cli().try_get_matches_from(args)?;
    if (config.strict_help && matches.get_flag("help"))
        || (!config.strict_help && matches.contains_id("help"))
    {
        return Ok(Action::Help);
    }
    let (command, arguments) = matches.subcommand().ok_or_else(|| {
        config.cli().error(
            clap::error::ErrorKind::MissingSubcommand,
            "a panel command is required",
        )
    })?;
    if command == "run" {
        Ok(Action::Run)
    } else {
        Ok(Action::Request(Request {
            command: command.to_owned(),
            monitor: (command == "toggle")
                .then(|| arguments.get_one::<String>("monitor").cloned())
                .flatten()
                .filter(|monitor| !monitor.is_empty()),
        }))
    }
}

fn exec_error(name: &str, executable: &Path, error: io::Error) -> i32 {
    eprintln!(
        "{name}: cannot execute '{}': {error}; check the configured executable and its permissions",
        executable.display()
    );
    match error.kind() {
        io::ErrorKind::NotFound => 127,
        io::ErrorKind::PermissionDenied => 126,
        // ENOEXEC: the file exists but is not an executable program.
        _ if error.raw_os_error() == Some(8) => 126,
        _ => 1,
    }
}

fn has_runtime_dir(command: &Command) -> bool {
    command
        .get_envs()
        .find(|(key, _)| *key == "XDG_RUNTIME_DIR")
        .map(|(_, value)| value.is_some_and(|value| !value.is_empty()))
        .unwrap_or_else(|| env::var_os("XDG_RUNTIME_DIR").is_some_and(|value| !value.is_empty()))
}

fn response_body(response: &[u8]) -> &[u8] {
    // Shell command substitution removes every trailing LF, but not CR or spaces.
    let end = response
        .iter()
        .rposition(|byte| *byte != b'\n')
        .map_or(0, |index| index + 1);
    &response[..end]
}

fn response_has_error(response: &[u8]) -> bool {
    serde_json::from_slice::<serde_json::Value>(response)
        .ok()
        .is_some_and(|value| {
            value
                .as_object()
                .is_some_and(|object| object.contains_key("error"))
        })
}

fn launch(config: &Config, action: Action) -> i32 {
    let is_run = matches!(action, Action::Run);
    let (mut command, executable) = match action {
        Action::Help => {
            print!("{}", config.cli().render_help());
            return 0;
        }
        Action::Run => {
            let command = config.child_command(&config.bundle);
            if !has_runtime_dir(&command) {
                eprintln!("{}: XDG_RUNTIME_DIR is required", config.name);
                return 1;
            }
            (command, &config.bundle)
        }
        Action::Request(request) => {
            let mut command = config.child_command(&config.astal);
            command.args([
                "--instance",
                &config.name,
                &serde_json::to_string(&request).expect("request contains only strings"),
            ]);
            (command, &config.astal)
        }
    };

    if is_run || !config.check_response {
        return exec_error(&config.name, executable, command.exec());
    }

    let output = match command
        .stdin(Stdio::inherit())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .output()
    {
        Ok(output) => output,
        Err(error) => return exec_error(&config.name, executable, error),
    };
    if !output.status.success() {
        return output
            .status
            .code()
            .unwrap_or_else(|| output.status.signal().map_or(1, |signal| 128 + signal));
    }
    let response = response_body(&output.stdout);
    let mut stdout = io::stdout().lock();
    if let Err(error) = stdout
        .write_all(response)
        .and_then(|()| stdout.write_all(b"\n"))
        .and_then(|()| stdout.flush())
    {
        eprintln!("{}: cannot write response: {error}", config.name);
        return 1;
    }
    i32::from(response_has_error(response))
}

fn main() {
    process::exit(main_status(env::args_os().skip(1)));
}

fn main_status(args: impl Iterator<Item = OsString>) -> i32 {
    let cli =
        match Cli::try_parse_from(std::iter::once(OsString::from("panel-launcher")).chain(args)) {
            Ok(cli) => cli,
            Err(error) => {
                let _ = error.print();
                return error.exit_code();
            }
        };
    if let Some(LauncherCommand::Runtime { command }) = cli.command {
        return desktop_runtime::status(command);
    }
    let Some(arguments) = cli.config else {
        let error = Cli::command().error(
            clap::error::ErrorKind::MissingRequiredArgument,
            "--config PATH is required for panel commands",
        );
        let _ = error.print();
        return error.exit_code();
    };
    let (path, args) = arguments
        .split_first()
        .expect("clap requires a config path");
    let path = Path::new(path);
    let contents = match fs::read(path) {
        Ok(contents) => contents,
        Err(error) => {
            eprintln!(
                "panel-launcher: cannot read config '{}': {error}; pass a readable JSON file with --config",
                path.display()
            );
            return 1;
        }
    };
    let config: Config = match serde_json::from_slice(&contents) {
        Ok(config) => config,
        Err(error) => {
            eprintln!(
                "panel-launcher: invalid config '{}': {error}; check the generated launcher configuration",
                path.display()
            );
            return 1;
        }
    };
    match parse_action(&config, args) {
        Ok(action) => launch(&config, action),
        Err(error) => {
            let _ = error.print();
            error.exit_code()
        }
    }
}

#[cfg(test)]
mod tests;

mod desktop_runtime {
    use clap::Subcommand;
    use std::{
        env,
        ffi::{OsStr, OsString},
        fs,
        io::{self, Read},
        num::NonZeroU32,
        os::{
            fd::{AsRawFd, FromRawFd, OwnedFd},
            unix::{
                ffi::OsStrExt,
                fs::{MetadataExt, OpenOptionsExt},
                process::CommandExt,
            },
        },
        path::{Path, PathBuf},
        process::{Child, Command, Stdio},
        thread,
        time::{Duration, Instant},
    };
    #[derive(Debug, Subcommand)]
    pub(super) enum Action {
        /// Print the focused Niri output as JSON.
        FocusedOutput,
        /// Toggle between Pinyin and the GB keyboard input method.
        InputToggle,
        /// Execute a program that is killed if its parent dies.
        WithParentDeath {
            #[arg(required = true, trailing_var_arg = true, num_args = 1.., value_name = "PROGRAM")]
            command: Vec<OsString>,
        },
        /// Add --resync when rclone has no saved listing, then execute it.
        Bisync {
            directory: PathBuf,
            #[arg(required = true, trailing_var_arg = true, num_args = 1.., value_name = "PROGRAM")]
            command: Vec<OsString>,
        },
        /// Debounce watcher output before starting a user service.
        Watch {
            seconds: NonZeroU32,
            systemctl: OsString,
            unit: OsString,
            #[arg(required = true, trailing_var_arg = true, num_args = 1.., value_name = "PROGRAM")]
            command: Vec<OsString>,
        },
        /// Toggle the calendar popup, starting its service if necessary.
        YcalToggle {
            systemctl: OsString,
            python: OsString,
            script: OsString,
            popup: OsString,
        },
    }

    type Result<T> = io::Result<T>;
    fn bad() -> io::Error {
        io::Error::other("invalid runtime arguments or response")
    }
    struct Managed(Child);
    impl Drop for Managed {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    fn ready(fd: i32, deadline: Option<Instant>) -> Result<bool> {
        loop {
            let timeout = match deadline {
                None => -1,
                Some(d) => {
                    let left = d.saturating_duration_since(Instant::now());
                    if left.is_zero() {
                        return Ok(false);
                    }
                    left.as_millis().saturating_add(1).min(i32::MAX as u128) as i32
                }
            };
            let mut p = libc::pollfd {
                fd,
                events: libc::POLLIN,
                revents: 0,
            };
            let n = unsafe { libc::poll(&mut p, 1, timeout) };
            if n > 0 {
                return Ok(true);
            }
            if n < 0 {
                let e = io::Error::last_os_error();
                if e.kind() != io::ErrorKind::Interrupted {
                    return Err(e);
                }
            }
        }
    }
    fn pidfd(pid: i32) -> Result<OwnedFd> {
        let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0u32) };
        if fd < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(unsafe { OwnedFd::from_raw_fd(fd as i32) })
        }
    }
    fn bounded(cmd: &mut Command, limit: Duration) -> Result<Vec<u8>> {
        let deadline = Some(Instant::now() + limit);
        let mut child = Managed(
            cmd.stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()?,
        );
        let mut pipe = child.0.stdout.take().unwrap();
        let mut bytes = Vec::new();
        let mut buf = [0; 4096];
        let timeout = || io::Error::from(io::ErrorKind::TimedOut);
        loop {
            if !ready(pipe.as_raw_fd(), deadline)? {
                return Err(timeout());
            }
            let n = pipe.read(&mut buf)?;
            if n == 0 {
                break;
            }
            if bytes.len() + n > 65536 {
                return Err(bad());
            }
            bytes.extend_from_slice(&buf[..n]);
        }
        if !ready(pidfd(child.0.id() as i32)?.as_raw_fd(), deadline)? {
            return Err(timeout());
        }
        if !child.0.wait()?.success() {
            return Err(io::Error::other("subcommand failed"));
        }
        Ok(bytes)
    }
    fn name(bytes: &[u8]) -> Result<Option<String>> {
        let val: serde_json::Value = serde_json::from_slice(bytes)?;
        if val.is_null() {
            return Ok(None);
        }
        let v = val.get("name").ok_or_else(bad)?;
        if v.is_null() {
            return Ok(None);
        }
        let s = v.as_str().filter(|s| valid(s)).ok_or_else(bad)?;
        Ok(Some(s.into()))
    }
    fn valid(s: &str) -> bool {
        !s.trim().is_empty() && s.len() <= 255 && !s.chars().any(char::is_control)
    }
    fn toggle(tool: &OsStr) -> Result<()> {
        input_toggle(|args| bounded(Command::new(tool).args(args), Duration::from_secs(2)))
    }
    fn input_toggle(mut run: impl FnMut(&[&str]) -> Result<Vec<u8>>) -> Result<()> {
        let bytes = run(&["-n"])?;
        let s = std::str::from_utf8(&bytes)
            .map_err(|_| bad())?
            .trim_end_matches('\n');
        if !valid(s) {
            return Err(bad());
        }
        run(&[
            "-s",
            if s == "pinyin" {
                "keyboard-gb"
            } else {
                "pinyin"
            },
        ])?;
        Ok(())
    }
    fn same_parent(before: i32, after: i32) -> bool {
        before > 1 && before == after
    }
    fn parent_exec(program: &OsStr, args: &[OsString]) -> Result<()> {
        let before = unsafe { libc::getppid() };
        if unsafe { libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL, 0, 0, 0) } < 0 {
            return Err(io::Error::last_os_error());
        }
        if !same_parent(before, unsafe { libc::getppid() }) {
            return Ok(());
        }
        Err(Command::new(program).args(args).exec())
    }
    fn resync(dir: &Path) -> Result<bool> {
        for e in fs::read_dir(dir)? {
            if e?.file_name().as_bytes().ends_with(b".lst") {
                return Ok(false);
            }
        }
        Ok(true)
    }
    fn parse_pid(s: &str) -> Option<i32> {
        let s = s.trim();
        s.bytes()
            .all(|b| b.is_ascii_digit())
            .then(|| s.parse::<i32>().ok())
            .flatten()
            .filter(|p| *p > 1)
    }
    fn signal(path: &Path, python: &OsStr, script: &OsStr) -> Result<()> {
        let uid = unsafe { libc::geteuid() };
        let f = fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK | libc::O_NOFOLLOW)
            .open(path)?;
        let m = f.metadata()?;
        if !m.is_file() || m.uid() != uid || m.len() > 32 {
            return Err(bad());
        }
        let mut s = String::new();
        f.take(33).read_to_string(&mut s)?;
        let pid = parse_pid(&s).ok_or_else(bad)?;
        let fd = pidfd(pid)?;
        let proc = Path::new("/proc").join(pid.to_string());
        if fs::metadata(&proc)?.uid() != uid
            || fs::read_link(proc.join("exe"))? != fs::canonicalize(python)?
        {
            return Err(bad());
        }
        let cmd = fs::read(proc.join("cmdline"))?;
        if cmd.split(|b| *b == 0).nth(1) != Some(script.as_bytes()) {
            return Err(bad());
        }
        if unsafe {
            libc::syscall(
                libc::SYS_pidfd_send_signal,
                fd.as_raw_fd(),
                libc::SIGUSR1,
                std::ptr::null::<libc::siginfo_t>(),
                0u32,
            )
        } < 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
    fn watch(delay: Duration, ctl: &OsStr, unit: &OsStr, cmd: &mut Command) -> Result<()> {
        let mut child = Managed(cmd.stdout(Stdio::piped()).spawn()?);
        let mut pipe = child.0.stdout.take().unwrap();
        debounce(&mut pipe, delay, || {
            bounded(
                Command::new(ctl).args([
                    OsStr::new("--user"),
                    OsStr::new("start"),
                    OsStr::new("--no-block"),
                    unit,
                ]),
                Duration::from_secs(5),
            )
            .map(drop)
        })?;
        if !child.0.wait()?.success() {
            return Err(io::Error::other("watcher failed"));
        }
        Ok(())
    }
    fn debounce(
        pipe: &mut (impl Read + AsRawFd),
        delay: Duration,
        mut start: impl FnMut() -> Result<()>,
    ) -> Result<()> {
        let mut deadline = None;
        let mut buf = [0; 8192];
        loop {
            if !ready(pipe.as_raw_fd(), deadline)? {
                start()?;
                deadline = None;
                continue;
            }
            let n = pipe.read(&mut buf)?;
            if n == 0 {
                return Ok(());
            }
            if buf[..n].contains(&b'\n') {
                deadline = Some(Instant::now() + delay)
            }
        }
    }
    fn run(action: Action) -> Result<()> {
        match action {
            Action::FocusedOutput => {
                let data = bounded(
                    Command::new(env::var_os("NIRI").unwrap_or_else(|| "niri".into())).args([
                        "msg",
                        "-j",
                        "focused-output",
                    ]),
                    Duration::from_secs(2),
                )?;
                println!("{}", serde_json::json!({"name":name(&data)?}));
                Ok(())
            }
            Action::InputToggle => toggle(OsStr::new("/run/current-system/sw/bin/fcitx5-remote")),
            Action::WithParentDeath { command } => parent_exec(&command[0], &command[1..]),
            Action::Bisync { directory, command } => {
                let mut cmd = Command::new(&command[0]);
                cmd.args(&command[1..]);
                if resync(&directory)? {
                    cmd.arg("--resync");
                }
                Err(cmd.exec())
            }
            Action::Watch {
                seconds,
                systemctl,
                unit,
                command,
            } => watch(
                Duration::from_secs(seconds.get().into()),
                &systemctl,
                &unit,
                Command::new(&command[0]).args(&command[1..]),
            ),
            Action::YcalToggle {
                systemctl,
                python,
                script,
                popup,
            } => {
                let home = env::var_os("HOME").ok_or_else(bad)?;
                let path = Path::new(&home).join(".cache/waybar-ycal/popup.pid");
                let send = || signal(&path, &python, &script).is_ok();
                if send() {
                    return Ok(());
                }
                if bounded(
                    Command::new(systemctl).args(["--user", "start", "waybar-ycal.service"]),
                    Duration::from_secs(5),
                )
                .is_ok()
                {
                    for _ in 0..5 {
                        thread::sleep(Duration::from_millis(200));
                        if send() {
                            return Ok(());
                        }
                    }
                }
                Command::new(popup).spawn()?;
                Ok(())
            }
        }
    }
    pub(super) fn status(action: Action) -> i32 {
        let focused_output = matches!(action, Action::FocusedOutput);
        match run(action) {
            Ok(()) => 0,
            Err(e) => {
                if focused_output {
                    println!("{{\"name\":null}}");
                }
                eprintln!("panel-launcher runtime: {e}");
                1
            }
        }
    }
    #[cfg(test)]
    crate::tests::desktop_runtime_tests!();
}
