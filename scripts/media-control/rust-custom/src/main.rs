use std::io::Read;
use std::os::fd::AsRawFd;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use std::{env, io};

use clap::{CommandFactory, Parser, Subcommand};
use media_control::{AppResult, artwork, credit, model};

#[derive(Parser)]
#[command(
    name = "media-control",
    about = "Headless media backend for the Astal panel"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<MediaCommand>,
}

#[derive(Subcommand)]
enum MediaCommand {
    /// Resident media backend; JSONL IPC on stdin/stdout
    Service,
    /// JSONL native snapshots on stdin; JSONL models/actions on stdout
    Model,
    /// JSON track metadata on stdin; one JSON credit reply on stdout
    Credit,
    /// Send one playerctl command to the first playing player, or the first player
    Playerctl {
        #[arg(value_name = "command", value_parser = clap::builder::NonEmptyStringValueParser::new())]
        command: String,
    },
    /// Find local cover art for a xesam:url or absolute filesystem path
    Artwork {
        #[arg(value_name = "xesam-url|path")]
        url: String,
    },
}

fn main() {
    if let Err(error) = run() {
        eprintln!("media-control: {error}");
        std::process::exit(2);
    }
}

fn run() -> AppResult<()> {
    match Cli::parse().command {
        Some(MediaCommand::Model) => model::serve(),
        Some(MediaCommand::Service) => model::serve_service(),
        Some(MediaCommand::Credit) => credit::print_credit(),
        Some(MediaCommand::Playerctl { command }) => media_key(&command),
        Some(MediaCommand::Artwork { url }) => artwork::print_path(&url),
        None => {
            Cli::command().print_help()?;
            Ok(())
        }
    }
}

struct PlayerProcess(Child);

impl Drop for PlayerProcess {
    fn drop(&mut self) {
        if !matches!(self.0.try_wait(), Ok(Some(_))) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

fn player_output(
    program: &std::ffi::OsStr,
    args: &[&str],
    deadline: Instant,
) -> AppResult<(bool, String)> {
    let mut child = PlayerProcess(
        Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?,
    );
    let mut pipe = child
        .0
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("missing player query pipe"))?;
    let flags = unsafe { libc::fcntl(pipe.as_raw_fd(), libc::F_GETFL) };
    if flags < 0
        || unsafe { libc::fcntl(pipe.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0
    {
        return Err(io::Error::last_os_error().into());
    }
    let mut bytes = Vec::new();
    let mut eof = false;
    loop {
        let mut buffer = [0; 4096];
        loop {
            match pipe.read(&mut buffer) {
                Ok(0) => {
                    eof = true;
                    break;
                }
                Ok(size) => {
                    if bytes.len() + size > 16 * 1024 {
                        return Err(io::Error::other("player query exceeds 16 KiB").into());
                    }
                    bytes.extend_from_slice(&buffer[..size]);
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(error.into()),
            }
        }
        if let Some(status) = child.0.try_wait()?
            && eof
        {
            return Ok((status.success(), String::from_utf8(bytes)?));
        }
        if Instant::now() >= deadline {
            return Err(
                io::Error::other("player query timed out; no media command was sent").into(),
            );
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn select_player(
    players: &[String],
    mut playing: impl FnMut(&str) -> AppResult<bool>,
) -> AppResult<Option<&str>> {
    for player in players {
        if playing(player)? {
            return Ok(Some(player));
        }
    }
    Ok(players.first().map(String::as_str))
}

fn media_key(command: &str) -> AppResult<()> {
    let program = env::var_os("MEDIA_CONTROL_PLAYERCTL").unwrap_or_else(|| "playerctl".into());
    let deadline = Instant::now() + Duration::from_secs(5);
    let (success, output) = player_output(&program, &["-l"], deadline)?;
    if !success {
        return Ok(());
    }
    let players: Vec<_> = output.split_ascii_whitespace().map(str::to_owned).collect();
    if players.len() > 128
        || players
            .iter()
            .any(|player| player.chars().any(char::is_control))
    {
        return Err(io::Error::other("invalid or excessive native player list").into());
    }
    let target = select_player(&players, |player| {
        let query_deadline = deadline.min(Instant::now() + Duration::from_millis(1500));
        let (success, status) = player_output(&program, &["-p", player, "status"], query_deadline)?;
        Ok(success && status.trim_end_matches('\n') == "Playing")
    })?;
    let Some(target) = target else {
        return Ok(());
    };
    if Instant::now() >= deadline {
        return Err(io::Error::other("player selection expired; no media command was sent").into());
    }
    Err(Command::new(program)
        .args(["-p", target, command])
        .exec()
        .into())
}

#[cfg(test)]
mod media_key_tests {
    use super::*;

    #[test]
    fn cli_preserves_native_commands_verbatim_without_a_fixed_command_allowlist() {
        Cli::command().debug_assert();
        for command in [
            "play-pause",
            "next",
            "previous",
            "custom-native-command",
            "metadata title",
        ] {
            let cli = Cli::try_parse_from(["media-control", "playerctl", command]).unwrap();
            let Some(MediaCommand::Playerctl { command: parsed }) = cli.command else {
                panic!("expected playerctl subcommand");
            };
            assert_eq!(parsed, command);
        }
        let cli = Cli::try_parse_from(["media-control", "playerctl", "--", "--version"]).unwrap();
        assert!(
            matches!(cli.command, Some(MediaCommand::Playerctl { command }) if command == "--version")
        );
    }

    #[test]
    fn media_keys_choose_first_playing_and_preserve_native_fallback_order() {
        let players = vec!["recent".into(), "playing".into(), "also-playing".into()];
        let mut reads = Vec::new();
        let target = select_player(&players, |name| {
            reads.push(name.to_owned());
            Ok(name != "recent")
        })
        .unwrap();
        assert_eq!(target, Some("playing"));
        assert_eq!(reads, ["recent", "playing"]);
        assert_eq!(
            select_player(&players, |_| Ok(false)).unwrap(),
            Some("recent")
        );
        assert_eq!(
            select_player(&[], |_| panic!("empty lists must not query")).unwrap(),
            None
        );
    }

    #[test]
    fn uncertain_selection_does_not_fall_back_to_a_different_player() {
        let players = vec!["first".into(), "second".into()];
        assert!(
            select_player(&players, |_| Err(io::Error::other("query timeout").into())).is_err()
        );
    }

    #[test]
    fn native_query_is_bounded_and_preserves_nonzero_status() {
        let program = std::ffi::OsStr::new("sh");
        let deadline = Instant::now() + Duration::from_secs(2);
        let (success, text) =
            player_output(program, &["-c", "printf Paused; exit 1"], deadline).unwrap();
        assert!(!success);
        assert_eq!(text, "Paused");
        let started = Instant::now();
        assert!(
            player_output(
                program,
                &["-c", "exec sleep 2"],
                started + Duration::from_millis(30)
            )
            .is_err()
        );
        assert!(started.elapsed() < Duration::from_secs(1));
    }
}
