use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::env;
use std::ffi::OsStr;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const DEFAULT_INTERVAL_MS: u64 = 750;
const DISPLAY_WIDTH: usize = 60;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PlaybackStatus {
    Playing,
    Paused,
}

impl PlaybackStatus {
    fn is_playing(self) -> bool {
        matches!(self, Self::Playing)
    }

    fn class(self) -> &'static str {
        match self {
            Self::Playing => "playing",
            Self::Paused => "paused",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Self::Playing => "󰐊",
            Self::Paused => "󰏤",
        }
    }
}

#[derive(Clone, Debug)]
struct Player {
    id: String,
    source: String,
    title: String,
    artist: String,
    status: PlaybackStatus,
    volume: Option<u8>,
    pinned: bool,
    activity: u128,
}

#[derive(Clone, Debug, Default)]
struct PlayerState {
    pinned: bool,
    was_playing: bool,
    activity: u128,
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("menu") => launch_menu(),
        Some("rofi") => rofi_mode(),
        Some("waybar") => waybar(&args[1..]),
        Some("toggle") => toggle(),
        Some("pause-all") => pause_all(),
        Some("list") => print_list(),
        Some("help") | Some("--help") | Some("-h") | None => {
            print_help();
            Ok(())
        }
        Some(other) => Err(format!("unknown command: {other}")),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("media-control: {message}");
            ExitCode::FAILURE
        }
    }
}

fn print_help() {
    println!(
        "media-control\n\n\
         Usage:\n\
           media-control menu\n\
           media-control waybar --watch [--interval-ms 750]\n\
           media-control toggle\n\
           media-control pause-all\n\
           media-control list"
    );
}

fn executable(variable: &str, fallback: &str) -> String {
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

fn display_source(player: &str, source: &str) -> String {
    if is_mprisence_web_player(player) || source.eq_ignore_ascii_case("mprisence_web") {
        "Chrome".to_owned()
    } else {
        clean_text(source)
    }
}

fn is_excluded_player(player: &str, source: &str) -> bool {
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
    let authority_end = remainder
        .find(|character| matches!(character, '/' | '?' | '#'))
        .unwrap_or(remainder.len());
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

fn is_publishable_web_media_url(url: &str) -> bool {
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
        return path_has_value(&path, "/video/")
            || path_has_value(&path, "/bangumi/play/");
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

fn should_include_player(player: &str, source: &str, url: &str) -> bool {
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

fn snapshot() -> Vec<Player> {
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
        let artist = player_property(&id, &["metadata", "--format", "{{artist}}"]) 
            .unwrap_or_default();
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

fn compare_players(left: &Player, right: &Player) -> Ordering {
    right
        .pinned
        .cmp(&left.pinned)
        .then_with(|| right.status.is_playing().cmp(&left.status.is_playing()))
        .then_with(|| right.activity.cmp(&left.activity))
        .then_with(|| left.id.cmp(&right.id))
}

fn print_list() -> Result<(), String> {
    for player in snapshot() {
        println!("{}", row_text(&player));
    }
    Ok(())
}

fn launch_menu() -> Result<(), String> {
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

fn rofi_mode() -> Result<(), String> {
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

fn rofi_row_state(player: &Player) -> Option<&'static str> {
    if player.pinned {
        Some("urgent")
    } else if player.status.is_playing() {
        Some("active")
    } else {
        None
    }
}

fn media_label(player: &Player) -> String {
    if player.artist.is_empty() {
        player.title.clone()
    } else {
        format!("{} — {}", player.title, player.artist)
    }
}

fn row_text(player: &Player) -> String {
    let pin = if player.pinned { "󰐃 " } else { "" };
    format!(
        "{}{} {} {{{}}} {}",
        pin,
        player.status.icon(),
        volume_label(player.volume),
        player.source,
        truncate_display(&media_label(player), DISPLAY_WIDTH)
    )
}

fn volume_label(volume: Option<u8>) -> String {
    volume
        .map(|value| format!("{value:>3}%"))
        .unwrap_or_else(|| "  --".to_owned())
}

fn player_command(player: &str, command: &str) -> Result<(), String> {
    let output = playerctl(["-p", player, command]).map_err(|error| error.to_string())?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_owned())
    }
}

fn change_volume(player: &str, delta: f64) -> Result<(), String> {
    let Some(current) = player_property(player, &["volume"])
        .and_then(|value| value.parse::<f64>().ok())
    else {
        // Volume is optional in MPRIS. Browser-tab bridges such as mprisence
        // intentionally omit it, so volume shortcuts should be a safe no-op.
        return Ok(());
    };
    let target = (current + delta).clamp(0.0, 1.0);
    let value = format!("{target:.2}");
    let output = playerctl(["-p", player, "volume", &value]).map_err(|error| error.to_string())?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_owned())
    }
}

fn toggle_pin(player: &str) {
    let mut state = read_state();
    let entry = state.entry(player.to_owned()).or_insert_with(|| PlayerState {
        activity: now_millis(),
        ..PlayerState::default()
    });
    entry.pinned = !entry.pinned;
    write_state(&state);
}

fn toggle() -> Result<(), String> {
    let players = snapshot();
    if let Some((player, command)) = waybar_toggle_action(&players) {
        player_command(player, command)?;
    }
    Ok(())
}

fn waybar_toggle_action(players: &[Player]) -> Option<(&str, &'static str)> {
    players
        .first()
        .map(|player| (player.id.as_str(), "play-pause"))
}

fn pause_all() -> Result<(), String> {
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

fn waybar(arguments: &[String]) -> Result<(), String> {
    let watch = arguments.iter().any(|argument| argument == "--watch");
    let interval = arguments
        .windows(2)
        .find(|pair| pair[0] == "--interval-ms")
        .and_then(|pair| pair[1].parse::<u64>().ok())
        .unwrap_or(DEFAULT_INTERVAL_MS)
        .max(100);

    loop {
        let line = waybar_json(snapshot().first());
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

fn waybar_json(player: Option<&Player>) -> String {
    match player {
        Some(player) => {
            let label = media_label(player);
            let text = truncate_display(&label, DISPLAY_WIDTH);
            let tooltip = format!("{}\nPlayer: {}", label, player.source);
            format!(
                "{{\"text\":\"{}\",\"tooltip\":\"{}\",\"class\":\"{}\",\"alt\":\"{}\"}}",
                json_escape(&text),
                json_escape(&tooltip),
                player.status.class(),
                player.status.class()
            )
        }
        None => "{\"text\":\"\",\"tooltip\":\"No active media\",\"class\":\"empty\",\"alt\":\"empty\"}".to_owned(),
    }
}

fn state_path() -> Option<PathBuf> {
    env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state")))
        .map(|root| root.join("media-control/state.tsv"))
}

fn read_state() -> HashMap<String, PlayerState> {
    let Some(path) = state_path() else {
        return HashMap::new();
    };
    let Ok(contents) = fs::read_to_string(path) else {
        return HashMap::new();
    };

    contents
        .lines()
        .filter_map(|line| {
            let mut fields = line.split('\t');
            let id = hex_decode(fields.next()?)?;
            let pinned = fields.next()? == "1";
            let was_playing = fields.next()? == "1";
            let activity = fields.next()?.parse::<u128>().ok()?;
            Some((
                id,
                PlayerState {
                    pinned,
                    was_playing,
                    activity,
                },
            ))
        })
        .collect()
}

fn write_state(state: &HashMap<String, PlayerState>) {
    let Some(path) = state_path() else {
        return;
    };
    let Some(parent) = path.parent() else {
        return;
    };
    if fs::create_dir_all(parent).is_err() {
        return;
    }

    let mut entries: Vec<_> = state.iter().collect();
    entries.sort_by(|left, right| right.1.activity.cmp(&left.1.activity));
    entries.truncate(128);

    let mut contents = String::new();
    for (id, value) in entries {
        contents.push_str(&format!(
            "{}\t{}\t{}\t{}\n",
            hex_encode(id),
            u8::from(value.pinned),
            u8::from(value.was_playing),
            value.activity
        ));
    }

    let temporary = path.with_extension("tmp");
    if fs::write(&temporary, contents).is_ok() {
        let _ = fs::rename(temporary, path);
    }
}

fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn friendly_source(id: &str) -> String {
    let source = id
        .split(".instance")
        .next()
        .unwrap_or(id)
        .rsplit('.')
        .next()
        .unwrap_or(id);
    let mut characters = source.chars();
    match characters.next() {
        Some(first) => first.to_uppercase().chain(characters).collect(),
        None => "Unknown".to_owned(),
    }
}

fn title_from_value(value: &str) -> String {
    let path = value.strip_prefix("file://").unwrap_or(value);
    let name = Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(value);
    percent_decode(name)
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let (Some(high), Some(low)) = (hex_value(bytes[index + 1]), hex_value(bytes[index + 2])) {
                decoded.push((high << 4) | low);
                index += 3;
                continue;
            }
        }
        decoded.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

fn truncate_display(value: &str, limit: usize) -> String {
    let mut output = String::new();
    let mut width = 0;
    let mut truncated = false;

    for character in value.chars() {
        let character_width = if character.is_ascii() { 1 } else { 2 };
        if width + character_width > limit.saturating_sub(1) {
            truncated = true;
            break;
        }
        output.push(character);
        width += character_width;
    }

    if truncated {
        output.push('…');
    }
    output
}

fn clean_text(value: &str) -> String {
    value
        .chars()
        .map(|character| if character.is_control() { ' ' } else { character })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn clean_field(value: &str) -> String {
    value
        .chars()
        .filter(|character| !character.is_control())
        .collect()
}

fn json_escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            value if value.is_control() => escaped.push_str(&format!("\\u{:04x}", value as u32)),
            value => escaped.push(value),
        }
    }
    escaped
}

fn hex_encode(value: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(value.len() * 2);
    for byte in value.as_bytes() {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }
    encoded
}

fn hex_decode(value: &str) -> Option<String> {
    if value.len() % 2 != 0 {
        return None;
    }
    let mut bytes = Vec::with_capacity(value.len() / 2);
    for pair in value.as_bytes().chunks_exact(2) {
        bytes.push((hex_value(pair[0])? << 4) | hex_value(pair[1])?);
    }
    String::from_utf8(bytes).ok()
}

fn hex_value(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncates_ascii_at_about_forty_columns() {
        let input = "123456789012345678901234567890123456789012345";
        let result = truncate_display(input, 40);
        assert!(result.ends_with('…'));
        assert_eq!(result.chars().count(), 40);
    }

    #[test]
    fn counts_cjk_as_two_columns() {
        let input = "这是一个用于测试中文标题截断长度的字符串这是额外文字";
        let result = truncate_display(input, 40);
        assert!(result.ends_with('…'));
        assert!(result.chars().count() <= 20);
    }

    #[test]
    fn state_key_round_trip() {
        let player = "chromium.instance1234";
        assert_eq!(hex_decode(&hex_encode(player)).as_deref(), Some(player));
    }

    #[test]
    fn escapes_waybar_json() {
        assert_eq!(json_escape("a\"b\\c\n"), "a\\\"b\\\\c\\n");
    }

    #[test]
    fn visible_media_labels_include_the_artist_when_available() {
        let player = Player {
            id: "youtube-music".to_owned(),
            source: "Chrome".to_owned(),
            title: "Delulu".to_owned(),
            artist: "SZA".to_owned(),
            status: PlaybackStatus::Playing,
            volume: None,
            pinned: false,
            activity: 0,
        };

        assert_eq!(media_label(&player), "Delulu — SZA");
        assert!(row_text(&player).ends_with("{Chrome} Delulu — SZA"));
        assert!(waybar_json(Some(&player)).contains("\"text\":\"Delulu — SZA\""));
    }

    #[test]
    fn visible_media_labels_fall_back_to_the_title_without_an_artist() {
        let player = Player {
            id: "browser-video".to_owned(),
            source: "Chrome".to_owned(),
            title: "Video title".to_owned(),
            artist: String::new(),
            status: PlaybackStatus::Paused,
            volume: None,
            pinned: false,
            activity: 0,
        };

        assert_eq!(media_label(&player), "Video title");
        assert!(waybar_json(Some(&player)).contains("\"text\":\"Video title\""));
    }

    #[test]
    fn pinned_players_sort_first() {
        let make = |id: &str, pinned: bool, status: PlaybackStatus, activity| Player {
            id: id.to_owned(),
            source: String::new(),
            title: String::new(),
            artist: String::new(),
            status,
            volume: None,
            pinned,
            activity,
        };
        let mut values = vec![
            make("new", false, PlaybackStatus::Playing, 3),
            make("pin", true, PlaybackStatus::Paused, 1),
            make("old", false, PlaybackStatus::Paused, 2),
        ];
        values.sort_by(compare_players);
        assert_eq!(values[0].id, "pin");
        assert_eq!(values[1].id, "new");
    }

    #[test]
    fn pinned_rofi_style_takes_priority_over_playback_status() {
        let make = |pinned, status| Player {
            id: String::new(),
            source: String::new(),
            title: String::new(),
            artist: String::new(),
            status,
            volume: None,
            pinned,
            activity: 0,
        };

        assert_eq!(
            rofi_row_state(&make(true, PlaybackStatus::Playing)),
            Some("urgent")
        );
        assert_eq!(
            rofi_row_state(&make(false, PlaybackStatus::Playing)),
            Some("active")
        );
        assert_eq!(
            rofi_row_state(&make(false, PlaybackStatus::Paused)),
            None
        );
    }

    #[test]
    fn waybar_click_toggles_the_displayed_player_even_when_paused() {
        let paused = Player {
            id: "paused-player".to_owned(),
            source: String::new(),
            title: String::new(),
            artist: String::new(),
            status: PlaybackStatus::Paused,
            volume: None,
            pinned: false,
            activity: 1,
        };

        assert_eq!(
            waybar_toggle_action(&[paused]),
            Some(("paused-player", "play-pause"))
        );
        assert_eq!(waybar_toggle_action(&[]), None);
    }

    #[test]
    fn unavailable_volume_has_an_explicit_placeholder() {
        assert_eq!(volume_label(None), "  --");
        assert_eq!(volume_label(Some(7)), "  7%");
        assert_eq!(volume_label(Some(100)), "100%");
    }

    #[test]
    fn mprisence_web_uses_chrome_as_its_display_source() {
        assert_eq!(
            display_source("mprisence_web.web.youtube.p123", "mprisence_web"),
            "Chrome"
        );
        assert_eq!(display_source("spotify", "Spotify"), "Spotify");
    }

    #[test]
    fn tauon_and_kid3_are_excluded_from_media_control() {
        assert!(is_excluded_player("tauon", "Tauon Music Box"));
        assert!(is_excluded_player(
            "org.mpris.MediaPlayer2.kid3",
            "Kid3"
        ));
        assert!(!is_excluded_player(
            "mprisence_web.web.youtube.p123",
            "mprisence_web"
        ));
    }

    #[test]
    fn keeps_canonical_youtube_and_bilibili_media_urls() {
        for url in [
            "https://www.youtube.com/watch?v=H9vpsqr0U8A",
            "https://music.youtube.com/watch?v=BTlj6Sls7KE",
            "https://youtu.be/H9vpsqr0U8A",
            "https://www.youtube.com/shorts/H9vpsqr0U8A",
            "https://www.bilibili.com/video/BV1F54y127A8/?spm_id_from=333",
            "https://www.bilibili.com/bangumi/play/ep123456",
            "https://live.bilibili.com/123456",
        ] {
            assert!(is_publishable_web_media_url(url), "should keep {url}");
        }
    }

    #[test]
    fn hides_home_search_and_listing_pages_from_mprisence() {
        for url in [
            "https://www.bilibili.com/",
            "https://search.bilibili.com/all?keyword=test",
            "https://music.youtube.com/",
            "https://www.youtube.com/results?search_query=test",
            "https://www.google.com/search?q=test",
            "https://example.com/",
        ] {
            assert!(!is_publishable_web_media_url(url), "should hide {url}");
        }
    }

    #[test]
    fn leaves_non_browser_players_and_real_generic_media_unchanged() {
        assert!(should_include_player("vlc", "VLC", ""));
        assert!(should_include_player(
            "mprisence_web.generic.p123",
            "mprisence_web",
            "https://example.com/videos/episode-1"
        ));
    }
}
