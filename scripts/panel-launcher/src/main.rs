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
    ags: PathBuf,
    gjs: PathBuf,
    bundle: PathBuf,
    environment: BTreeMap<String, String>,
    forced_environment: BTreeMap<String, String>,
}

impl Config {
    fn usage(&self) -> String {
        let commands = self
            .commands
            .iter()
            .map(|command| match command.as_str() {
                "toggle" => "toggle [monitor]",
                command => command,
            })
            .collect::<Vec<_>>()
            .join(" | ");
        format!("Usage: {} {commands}", self.name)
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

enum Action<'a> {
    Help,
    Run,
    Request(Request<'a>),
}

#[derive(Serialize)]
struct Request<'a> {
    command: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    monitor: Option<&'a str>,
}

fn parse_action<'a>(config: &Config, args: &'a [OsString]) -> Result<Action<'a>, ()> {
    let command = args.first().and_then(|arg| arg.to_str()).ok_or(())?;
    if matches!(command, "-h" | "--help") {
        return if !config.strict_help || args.len() == 1 {
            Ok(Action::Help)
        } else {
            Err(())
        };
    }
    if !config.commands.iter().any(|allowed| allowed == command)
        || args.len() > if command == "toggle" { 2 } else { 1 }
    {
        return Err(());
    }
    if command == "run" {
        Ok(Action::Run)
    } else {
        Ok(Action::Request(Request {
            command,
            monitor: args
                .get(1)
                .map(|arg| arg.to_str().ok_or(()))
                .transpose()?
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

fn launch(config: &Config, action: Action<'_>) -> i32 {
    let is_run = matches!(action, Action::Run);
    let (mut command, executable) = match action {
        Action::Help => {
            println!("{}", config.usage());
            return 0;
        }
        Action::Run => {
            let mut command = config.child_command(&config.gjs);
            command.arg("-m").arg(&config.bundle);
            if !has_runtime_dir(&command) {
                eprintln!("{}: XDG_RUNTIME_DIR is required", config.name);
                return 1;
            }
            (command, &config.gjs)
        }
        Action::Request(request) => {
            let mut command = config.child_command(&config.ags);
            command.args([
                "request",
                "--instance",
                &config.name,
                "--",
                &serde_json::to_string(&request).expect("request contains only strings"),
            ]);
            (command, &config.ags)
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

fn main_status(mut args: impl Iterator<Item = OsString>) -> i32 {
    let launcher_usage = "Usage: panel-launcher --config PATH COMMAND [monitor]";
    if args.next().as_deref() != Some(std::ffi::OsStr::new("--config")) {
        eprintln!("{launcher_usage}");
        return 2;
    }
    let Some(path) = args.next().map(PathBuf::from) else {
        eprintln!("{launcher_usage}");
        return 2;
    };
    let contents = match fs::read(&path) {
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
    let args: Vec<_> = args.collect();
    match parse_action(&config, &args) {
        Ok(action) => launch(&config, action),
        Err(()) => {
            eprintln!("{}", config.usage());
            2
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_response_newline_semantics() {
        for (input, expected) in [
            (b"".as_slice(), b"".as_slice()),
            (b"\n\n", b""),
            (b"ok\n\n", b"ok"),
            (b"ok\r\n", b"ok\r"),
            (b"ok \n", b"ok "),
            (b"one\ntwo\n", b"one\ntwo"),
            (b"\xff\n", b"\xff"),
        ] {
            assert_eq!(response_body(input), expected);
        }
    }

    #[test]
    fn only_top_level_error_keys_are_failures() {
        for response in [
            r#"{"error":null}"#,
            r#"{"error":false}"#,
            r#"{"error":0}"#,
            r#"{"error":""}"#,
            r#"{"error":{}}"#,
            r#"{"result":"ok","error":[]}"#,
        ] {
            assert!(response_has_error(response.as_bytes()), "{response}");
        }
        for response in [
            "ok",
            "",
            "not json",
            "null",
            "false",
            r#""error""#,
            r#"{"result":{"error":true}}"#,
            r#"[{"error":true}]"#,
            r#"{"error":true} trailing"#,
        ] {
            assert!(!response_has_error(response.as_bytes()), "{response}");
        }
    }
}
