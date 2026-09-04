use std::env;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::Command;
use std::thread;
use std::time::Duration;

use crate::model::{Player, media_label, row_text};
use crate::mpris::{change_volume, executable, player_command, snapshot, toggle_pin};
use crate::notifications::{Notifications, Subscription};
use crate::text::{clean_field, json_escape, truncate_display};

const DEFAULT_INTERVAL_MS: u64 = 750;

/// The bar button's notification badge.
const BELL_QUIET: &str = "󰂜";
const BELL_ACTIVE: &str = "󰂚";
const BELL_SILENCED: &str = "󰂛";

/// The media half of the bar button shares the centre of the bar with the
/// lyrics module, so it gets less room than the old media-only module had.
const BAR_MEDIA_WIDTH: usize = 46;

/// What separates the badge from the media label.
const BAR_SEPARATOR: &str = "  ·  ";

pub(crate) fn launch_menu() -> Result<(), String> {
    let current_exe = env::current_exe().map_err(|error| error.to_string())?;
    let mode = format!("media:{} rofi", current_exe.display());
    let theme = theme_path();

    let mut command = Command::new(executable("MEDIA_CONTROL_ROFI", "rofi"));
    command.args([
        "-show",
        "media",
        "-modes",
        &mode,
        "-no-custom",
        "-matching",
        "fuzzy",
        "-kb-custom-1",
        "Alt+p",
        "-kb-custom-2",
        "Alt+h",
        "-kb-custom-3",
        "Alt+l",
        "-kb-custom-4",
        "Alt+space",
        "-kb-custom-5",
        "Alt+Up",
        "-kb-custom-6",
        "Alt+Down",
        "-timeout-delay",
        "1",
        "-timeout-action",
        "kb-custom-8",
    ]);
    if let Some(theme) = theme.as_deref() {
        command.arg("-theme").arg(theme);
    }

    command
        .status()
        .map_err(|error| format!("could not launch rofi: {error}"))?;
    Ok(())
}

fn theme_path() -> Option<String> {
    if let Ok(path) = env::var("MEDIA_CONTROL_THEME") {
        if !path.is_empty() {
            return Some(path);
        }
    }

    let config_home = env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")));
    if let Some(path) = config_home.map(|home| home.join("rofi/media-control.rasi")) {
        if path.exists() {
            return Some(path.to_string_lossy().into_owned());
        }
    }

    env::var("MEDIA_CONTROL_FALLBACK_THEME").ok()
}

pub(crate) fn rofi_mode() -> Result<(), String> {
    let return_value = env::var("ROFI_RETV")
        .ok()
        .and_then(|value| value.parse::<u8>().ok())
        .unwrap_or(0);
    let selected = env::var("ROFI_INFO").ok().filter(|value| !value.is_empty());

    if (1..=15).contains(&return_value) {
        let fallback = snapshot().first().map(|player| player.id.clone());
        if let Some(player) = selected.or(fallback) {
            match return_value {
                1 | 13 => player_command(&player, "play-pause")?,
                10 => toggle_pin(&player),
                11 => player_command(&player, "previous")?,
                12 => player_command(&player, "next")?,
                14 => change_volume(&player, 0.10)?,
                15 => change_volume(&player, -0.10)?,
                _ => {}
            }
            thread::sleep(Duration::from_millis(90));
        }
    }

    render_rofi(&snapshot()).map_err(|error| error.to_string())
}

fn render_rofi(players: &[Player]) -> io::Result<()> {
    let stdout = io::stdout();
    let mut output = stdout.lock();
    output.write_all(b"\0prompt\x1fMedia\n")?;
    output.write_all(b"\0use-hot-keys\x1ftrue\n")?;
    output.write_all(b"\0keep-selection\x1ftrue\n")?;

    if players.is_empty() {
        output.write_all(b"\0message\x1fNo playing or paused media\n")?;
        return output.flush();
    }

    output.write_all(b"\0message\x1fEnter: play/pause  |  buttons act on the selected row\n")?;
    for player in players {
        let row = row_text(player);
        write!(output, "{row}\0info\x1f{}", clean_field(&player.id))?;
        if let Some(state) = rofi_row_state(player) {
            write!(output, "\x1f{state}\x1ftrue")?;
        }
        output.write_all(b"\n")?;
    }
    output.flush()
}

pub(crate) fn rofi_row_state(player: &Player) -> Option<&'static str> {
    if player.pinned {
        Some("urgent")
    } else if player.status.is_playing() {
        Some("active")
    } else {
        None
    }
}

pub(crate) fn toggle() -> Result<(), String> {
    let players = snapshot();
    if let Some((player, command)) = waybar_toggle_action(&players) {
        player_command(player, command)?;
    }
    Ok(())
}

pub(crate) fn waybar_toggle_action(players: &[Player]) -> Option<(&str, &'static str)> {
    players
        .first()
        .map(|player| (player.id.as_str(), "play-pause"))
}

pub(crate) fn waybar(arguments: &[String]) -> Result<(), String> {
    let watch = arguments.iter().any(|argument| argument == "--watch");
    let interval = arguments
        .windows(2)
        .find(|pair| pair[0] == "--interval-ms")
        .and_then(|pair| pair[1].parse::<u64>().ok())
        .unwrap_or(DEFAULT_INTERVAL_MS)
        .max(100);

    // The button opens the notification centre as well as showing the current
    // track, so the count travels with the media label rather than through a
    // second Waybar module that would need its own slot in the bar.
    let notifications = Subscription::start();

    loop {
        let line = waybar_json(snapshot().first(), notifications.get());
        if writeln!(io::stdout(), "{line}").is_err() {
            return Ok(());
        }
        let _ = io::stdout().flush();
        if !watch {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(interval));
    }
}

/// The badge glyph and the CSS class that goes with the daemon's state.
fn notification_badge(notifications: Notifications) -> (String, &'static str) {
    if notifications.dnd {
        (BELL_SILENCED.to_owned(), "dnd")
    } else if notifications.count > 0 {
        (
            format!("{BELL_ACTIVE} {}", notifications.count),
            "notification",
        )
    } else {
        (BELL_QUIET.to_owned(), "quiet")
    }
}

/// The bar button's label: the notification badge, then the current track.
///
/// Waybar escapes this text before setting it as markup, so it stays plain and
/// the colours come from the classes below and `themes/waybar.css`.
pub(crate) fn bar_text(player: Option<&Player>, notifications: Notifications) -> String {
    let (badge, _) = notification_badge(notifications);
    match player {
        Some(player) => format!(
            "{badge}{BAR_SEPARATOR}{}  {}",
            player.status.icon(),
            truncate_display(&media_label(player), BAR_MEDIA_WIDTH),
        ),
        None => badge,
    }
}

/// What hovering the button explains, in the order it matters.
pub(crate) fn bar_tooltip(player: Option<&Player>, notifications: Notifications) -> String {
    let mut lines = Vec::new();
    lines.push(match notifications.count {
        0 => "No notifications".to_owned(),
        1 => "1 notification".to_owned(),
        count => format!("{count} notifications"),
    });
    if notifications.dnd {
        lines.push("Do Not Disturb is on".to_owned());
    }
    match player {
        Some(player) => {
            lines.push(media_label(player));
            lines.push(format!("Player: {}", player.source));
        }
        None => lines.push("No active media".to_owned()),
    }
    lines.join("\n")
}

pub(crate) fn waybar_json(player: Option<&Player>, notifications: Notifications) -> String {
    let (_, notification_class) = notification_badge(notifications);
    let media_class = player.map_or("empty", |player| player.status.class());
    let classes = vec![notification_class, media_class];
    let classes: Vec<String> = classes.iter().map(|class| format!("\"{class}\"")).collect();
    format!(
        "{{\"text\":\"{}\",\"tooltip\":\"{}\",\"class\":[{}],\"alt\":\"{}\"}}",
        json_escape(&bar_text(player, notifications)),
        json_escape(&bar_tooltip(player, notifications)),
        classes.join(","),
        media_class,
    )
}
