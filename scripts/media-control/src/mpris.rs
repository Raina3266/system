use std::collections::HashSet;
use std::env;
use std::ffi::OsStr;
use std::io;
use std::process::{Command, Output, Stdio};

use crate::model::{PlaybackStatus, Player, PlayerState, compare_players, row_text};
use crate::state::{now_millis, read_state, write_state};
use crate::text::{clean_text, friendly_source, title_from_value};

pub(crate) fn executable(variable: &str, fallback: &str) -> String {
    env::var(variable).unwrap_or_else(|_| fallback.to_owned())
}

fn playerctl<I, S>(arguments: I) -> io::Result<std::process::Output>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    Command::new(executable("MEDIA_CONTROL_PLAYERCTL", "playerctl"))
        .args(arguments)
        .stdin(Stdio::null())
        .output()
}

fn playerctl_text(arguments: &[&str]) -> Option<String> {
    let output = playerctl(arguments).ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn player_property(player: &str, arguments: &[&str]) -> Option<String> {
    let mut command = vec!["-p", player];
    command.extend_from_slice(arguments);
    playerctl_text(&command)
}

fn player_source(player: &str) -> String {
    let source = player_property(player, &["metadata", "--format", "{{playerName}}"]);
    source
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| friendly_source(player))
}

fn is_mprisence_web_player(player: &str) -> bool {
    player.to_ascii_lowercase().contains("mprisence_web")
}

fn player_url(player: &str) -> String {
    player_property(player, &["metadata", "xesam:url"]).unwrap_or_default()
}

pub(crate) fn display_source(player: &str, source: &str) -> String {
    if is_mprisence_web_player(player) || source.eq_ignore_ascii_case("mprisence_web") {
        "Chrome".to_owned()
    } else {
        clean_text(source)
    }
}

pub(crate) fn is_excluded_player(player: &str, source: &str) -> bool {
    [player, source].iter().any(|value| {
        let normalized = value
            .chars()
            .filter(|character| character.is_ascii_alphanumeric())
            .flat_map(|character| character.to_lowercase())
            .collect::<String>();
        normalized.contains("tauon") || normalized.contains("kid3")
    })
}

fn web_url_parts(url: &str) -> Option<(&str, &str, &str)> {
    let url = url.trim();
    let remainder = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    let authority_end = remainder.find(['/', '?', '#']).unwrap_or(remainder.len());
    let authority = &remainder[..authority_end];
    let host = authority.rsplit('@').next()?.split(':').next()?;
    if host.is_empty() {
        return None;
    }

    let location = remainder[authority_end..]
        .split('#')
        .next()
        .unwrap_or_default();
    if let Some(query) = location.strip_prefix('?') {
        return Some((host, "/", query));
    }
    let (path, query) = location.split_once('?').unwrap_or((location, ""));
    Some((host, if path.is_empty() { "/" } else { path }, query))
}

fn host_matches(host: &str, domain: &str) -> bool {
    host == domain
        || host
            .strip_suffix(domain)
            .map(|prefix| prefix.ends_with('.'))
            .unwrap_or(false)
}

fn path_has_value(path: &str, prefix: &str) -> bool {
    path.strip_prefix(prefix)
        .map(|value| !value.is_empty() && value != "/")
        .unwrap_or(false)
}

fn query_has_value(query: &str, key: &str) -> bool {
    query.split('&').any(|field| {
        field
            .split_once('=')
            .map(|(name, value)| name.eq_ignore_ascii_case(key) && !value.is_empty())
            .unwrap_or(false)
    })
}

pub(crate) fn is_publishable_web_media_url(url: &str) -> bool {
    let Some((host, path, query)) = web_url_parts(url) else {
        return false;
    };
    let host = host.to_ascii_lowercase();
    let path = path.to_ascii_lowercase();

    if host_matches(&host, "youtu.be") {
        return path_has_value(&path, "/");
    }

    if host_matches(&host, "youtube.com") {
        if path == "/watch" {
            return query_has_value(query, "v");
        }
        return path_has_value(&path, "/shorts/")
            || path_has_value(&path, "/live/")
            || path_has_value(&path, "/embed/");
    }

    if host_matches(&host, "bilibili.com") {
        if host == "live.bilibili.com" {
            let room = path.trim_matches('/').split('/').next().unwrap_or_default();
            return !room.is_empty() && room.chars().all(|character| character.is_ascii_digit());
        }
        return path_has_value(&path, "/video/") || path_has_value(&path, "/bangumi/play/");
    }

    if path == "/" {
        return false;
    }
    let first_segment = path
        .trim_start_matches('/')
        .split('/')
        .next()
        .unwrap_or_default();
    !matches!(first_segment, "search" | "results")
}

pub(crate) fn should_include_player(player: &str, source: &str, url: &str) -> bool {
    !is_excluded_player(player, source)
        && (!is_mprisence_web_player(player) || is_publishable_web_media_url(url))
}

fn player_ids() -> Vec<String> {
    let Some(output) = playerctl_text(&["-l"]) else {
        return Vec::new();
    };

    let mut seen = HashSet::new();
    output
        .lines()
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .filter(|id| seen.insert((*id).to_owned()))
        .map(str::to_owned)
        .collect()
}

pub(crate) fn snapshot() -> Vec<Player> {
    let mut state = read_state();
    let now = now_millis();
    let mut players = Vec::new();

    for id in player_ids() {
        let status = match player_property(&id, &["status"]).as_deref() {
            Some("Playing") => PlaybackStatus::Playing,
            Some("Paused") => PlaybackStatus::Paused,
            _ => continue,
        };

        let source = player_source(&id);
        let url = if is_mprisence_web_player(&id) {
            player_url(&id)
        } else {
            String::new()
        };
        if !should_include_player(&id, &source, &url) {
            state.remove(&id);
            continue;
        }

        let entry = state.entry(id.clone()).or_insert_with(|| PlayerState {
            activity: now,
            ..PlayerState::default()
        });
        if status.is_playing() && !entry.was_playing {
            entry.activity = now;
        }
        entry.was_playing = status.is_playing();

        let title = player_property(&id, &["metadata", "--format", "{{title}}"])
            .filter(|value| !value.is_empty())
            .or_else(|| {
                if url.is_empty() {
                    player_property(&id, &["metadata", "xesam:url"])
                } else {
                    Some(url.clone())
                }
            })
            .map(|value| title_from_value(&value))
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "Untitled media".to_owned());
        let artist =
            player_property(&id, &["metadata", "--format", "{{artist}}"]).unwrap_or_default();
        let volume = player_property(&id, &["volume"])
            .and_then(|value| value.parse::<f64>().ok())
            .map(|value| (value.clamp(0.0, 1.0) * 100.0).round() as u8);
        let source = display_source(&id, &source);

        players.push(Player {
            id,
            source,
            title: clean_text(&title),
            artist: clean_text(&artist),
            status,
            volume,
            pinned: entry.pinned,
            activity: entry.activity,
        });
    }

    write_state(&state);
    players.sort_by(compare_players);
    players
}

pub(crate) fn print_list() -> Result<(), String> {
    for player in snapshot() {
        println!("{}", row_text(&player));
    }
    Ok(())
}

pub(crate) fn player_command(player: &str, command: &str) -> Result<(), String> {
    let output = playerctl(["-p", player, command]).map_err(|error| error.to_string())?;
    command_result(output)
}

fn command_result(output: Output) -> Result<(), String> {
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_owned())
    }
}

pub(crate) fn change_volume(player: &str, delta: f64) -> Result<(), String> {
    let Some(current) =
        player_property(player, &["volume"]).and_then(|value| value.parse::<f64>().ok())
    else {
        // Volume is optional in MPRIS. Browser-tab bridges such as mprisence
        // intentionally omit it, so volume shortcuts should be a safe no-op.
        return Ok(());
    };
    let target = (current + delta).clamp(0.0, 1.0);
    let value = format!("{target:.2}");
    let output = playerctl(["-p", player, "volume", &value]).map_err(|error| error.to_string())?;
    command_result(output)
}

pub(crate) fn toggle_pin(player: &str) {
    let mut state = read_state();
    let entry = state
        .entry(player.to_owned())
        .or_insert_with(|| PlayerState {
            activity: now_millis(),
            ..PlayerState::default()
        });
    entry.pinned = !entry.pinned;
    write_state(&state);
}

pub(crate) fn pause_all() -> Result<(), String> {
    for player in player_ids() {
        let source = player_source(&player);
        let url = if is_mprisence_web_player(&player) {
            player_url(&player)
        } else {
            String::new()
        };
        if !should_include_player(&player, &source, &url) {
            continue;
        }
        if player_property(&player, &["status"]).as_deref() == Some("Playing") {
            let _ = player_command(&player, "pause");
        }
    }
    Ok(())
}
