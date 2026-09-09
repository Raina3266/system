//! Current Wayle media title for Waybar.
//!
//! This consumes Wayle's native media D-Bus interface.  Keeping selection here
//! and in the popup on the same service avoids a second, disagreeing MPRIS
//! implementation in Waybar.

use std::collections::HashMap;
use std::io::{self, Write};
use std::thread;
use std::time::Duration;

use serde::Serialize;
use zbus::blocking::Connection;
use zbus::proxy::CacheProperties;

const POLL_INTERVAL: Duration = Duration::from_millis(750);
const RETRY_DELAY: Duration = Duration::from_secs(3);
const MEDIA_ICON: &str = "\u{f0386}";

#[zbus::proxy(
    interface = "com.wayle.Media1",
    default_service = "com.wayle.Media1",
    default_path = "/com/wayle/Media",
    gen_async = false
)]
trait WayleMedia {
    fn list_players(&self) -> zbus::Result<Vec<(String, String, String)>>;
    fn get_active_player(&self) -> zbus::Result<String>;
    fn get_player_info(&self, player_id: String) -> zbus::Result<HashMap<String, String>>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Track {
    id: String,
    identity: String,
    state: String,
    title: String,
    artist: String,
    album: String,
    art_url: String,
    length_us: Option<u64>,
}

impl Track {
    fn read(
        proxy: &WayleMediaProxy<'_>,
        player: &(String, String, String),
    ) -> zbus::Result<Self> {
        let info = proxy.get_player_info(player.0.clone())?;
        Ok(Self {
            id: player.0.clone(),
            identity: value_or(&info, "identity", &player.1),
            state: value_or(&info, "playback_state", &player.2),
            title: value_or(&info, "title", ""),
            artist: value_or(&info, "artist", ""),
            album: value_or(&info, "album", ""),
            art_url: value_or(&info, "art_url", ""),
            length_us: info.get("length_us").and_then(|value| value.parse().ok()),
        })
    }

    fn is_playing(&self) -> bool {
        self.state.eq_ignore_ascii_case("playing")
    }

    fn display_title(&self) -> &str {
        if self.title.trim().is_empty() {
            if self.identity.trim().is_empty() {
                "Unknown title"
            } else {
                self.identity.trim()
            }
        } else {
            self.title.trim()
        }
    }

    fn richness(&self) -> usize {
        [
            !self.title.trim().is_empty(),
            !self.artist.trim().is_empty(),
            !self.album.trim().is_empty(),
            !self.art_url.trim().is_empty(),
            self.length_us.is_some(),
        ]
        .into_iter()
        .filter(|present| *present)
        .count()
    }
}

fn value_or(info: &HashMap<String, String>, key: &str, fallback: &str) -> String {
    info.get(key)
        .filter(|value| !value.trim().is_empty())
        .map_or_else(|| fallback.to_owned(), Clone::clone)
}

pub fn watch() -> Result<(), String> {
    loop {
        if let Some(proxy) = media() {
            loop {
                match current_track(&proxy) {
                    Ok(track) => {
                        if !print_badge(track.as_ref()) {
                            return Ok(());
                        }
                    }
                    Err(error) => {
                        eprintln!("control-centre: Wayle media unavailable: {error}");
                        break;
                    }
                }
                thread::sleep(POLL_INTERVAL);
            }
        }

        if !print_badge(None) {
            return Ok(());
        }
        thread::sleep(RETRY_DELAY);
    }
}

fn media() -> Option<WayleMediaProxy<'static>> {
    let connection = Connection::session().ok()?;
    WayleMediaProxy::builder(&connection)
        .cache_properties(CacheProperties::No)
        .build()
        .ok()
}

fn current_track(proxy: &WayleMediaProxy<'_>) -> zbus::Result<Option<Track>> {
    let players = proxy.list_players()?;
    if players.is_empty() {
        return Ok(None);
    }

    let active = proxy.get_active_player().unwrap_or_default();
    let selected = players
        .iter()
        .find(|player| player.0 == active)
        .or_else(|| {
            players
                .iter()
                .find(|player| player.2.eq_ignore_ascii_case("playing"))
        })
        .unwrap_or(&players[0]);

    let selected = Track::read(proxy, selected)?;
    if !is_browser_or_proxy(&selected.id) {
        return Ok(Some(selected));
    }

    for player in players.iter().filter(|player| is_bridge(&player.0)) {
        let Ok(bridge) = Track::read(proxy, player) else {
            continue;
        };
        if duplicate_metadata(&selected, &bridge) {
            return Ok(Some(preferred_duplicate(selected, bridge)));
        }
    }

    Ok(Some(selected))
}

fn preferred_duplicate(selected: Track, bridge: Track) -> Track {
    if selected.is_playing() != bridge.is_playing() {
        if bridge.is_playing() {
            bridge
        } else {
            selected
        }
    } else if bridge.richness() >= selected.richness() {
        bridge
    } else {
        selected
    }
}

fn duplicate_metadata(left: &Track, right: &Track) -> bool {
    let title = normalized(&left.title);
    if title.is_empty() || title != normalized(&right.title) {
        return false;
    }

    same_non_empty(&left.artist, &right.artist)
        || same_non_empty(&left.album, &right.album)
        || same_non_empty(&left.art_url, &right.art_url)
        || lengths_match(left.length_us, right.length_us)
}

fn same_non_empty(left: &str, right: &str) -> bool {
    let left = normalized(left);
    !left.is_empty() && left == normalized(right)
}

fn lengths_match(left: Option<u64>, right: Option<u64>) -> bool {
    left.zip(right)
        .is_some_and(|(left, right)| left.abs_diff(right) <= 2_000_000)
}

fn normalized(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn is_bridge(id: &str) -> bool {
    id.to_ascii_lowercase().contains("mprisence")
}

fn is_browser_or_proxy(id: &str) -> bool {
    let id = id.to_ascii_lowercase();
    [
        "playerctld",
        "plasma-browser-integration",
        "chromium",
        "chrome",
        "firefox",
        "brave",
        "vivaldi",
        "microsoft-edge",
        "opera",
        "zen",
    ]
    .iter()
    .any(|candidate| id.contains(candidate))
}

#[derive(Serialize)]
struct Badge {
    text: String,
    class: &'static str,
    alt: &'static str,
    tooltip: String,
}

fn badge(track: Option<&Track>) -> Badge {
    let Some(track) = track else {
        return Badge {
            text: MEDIA_ICON.to_owned(),
            class: "inactive",
            alt: "inactive",
            tooltip: String::from("No media playing"),
        };
    };

    let class = if track.is_playing() {
        "playing"
    } else {
        "paused"
    };
    let title = track.display_title();
    let tooltip = if track.artist.trim().is_empty() {
        title.to_owned()
    } else {
        format!("{title} — {}", track.artist.trim())
    };

    Badge {
        text: format!("{MEDIA_ICON}  {}", truncate_title(title)),
        class,
        alt: class,
        tooltip,
    }
}

fn print_badge(track: Option<&Track>) -> bool {
    let Ok(line) = serde_json::to_string(&badge(track)) else {
        return false;
    };
    println!("{line}");
    io::stdout().flush().is_ok()
}

fn truncate_title(title: &str) -> String {
    let mut output = String::new();
    let mut characters = 0;
    let mut ideographs = 0;

    for character in title.chars() {
        if characters == 50 || (is_cjk_ideograph(character) && ideographs == 35) {
            output.push('…');
            return output;
        }
        output.push(character);
        characters += 1;
        if is_cjk_ideograph(character) {
            ideographs += 1;
        }
    }

    output
}

fn is_cjk_ideograph(character: char) -> bool {
    matches!(
        character,
        '\u{3400}'..='\u{4dbf}'
            | '\u{4e00}'..='\u{9fff}'
            | '\u{f900}'..='\u{faff}'
            | '\u{20000}'..='\u{2ebef}'
            | '\u{30000}'..='\u{3134f}'
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(id: &str, title: &str, artist: &str) -> Track {
        Track {
            id: id.to_owned(),
            identity: String::from("Browser"),
            state: String::from("Playing"),
            title: title.to_owned(),
            artist: artist.to_owned(),
            album: String::new(),
            art_url: String::new(),
            length_us: None,
        }
    }

    #[test]
    fn exactly_fifty_characters_are_untouched() {
        let title = "a".repeat(50);
        assert_eq!(truncate_title(&title), title);
    }

    #[test]
    fn the_fifty_first_character_is_truncated() {
        assert_eq!(truncate_title(&"a".repeat(51)), format!("{}…", "a".repeat(50)));
    }

    #[test]
    fn the_thirty_sixth_ideograph_is_truncated() {
        assert_eq!(truncate_title(&"真".repeat(36)), format!("{}…", "真".repeat(35)));
    }

    #[test]
    fn a_mixed_title_stops_at_whichever_limit_arrives_first() {
        let title = format!("{}abcdefghijklmnop", "真".repeat(35));
        assert_eq!(truncate_title(&title), format!("{}…", "真".repeat(35)));
    }

    #[test]
    fn a_richer_bridge_replaces_playerctld_for_the_same_track() {
        let selected = track(
            "org.mpris.MediaPlayer2.playerctld",
            "Being a Good Girl Hurts",
            "YENA",
        );
        let mut bridge = track(
            "org.mpris.MediaPlayer2.mprisence.tab-1",
            "Being a Good Girl Hurts",
            "YENA",
        );
        bridge.album = String::from("Good Morning");
        assert!(duplicate_metadata(&selected, &bridge));
        assert!(is_bridge(&preferred_duplicate(selected, bridge).id));
    }

    #[test]
    fn a_real_elisa_player_is_not_treated_as_the_bridge() {
        assert!(!is_bridge("org.mpris.MediaPlayer2.elisa"));
    }

    #[test]
    fn title_only_matches_do_not_merge_unrelated_players() {
        let left = track("org.mpris.MediaPlayer2.chromium", "Intro", "");
        let right = track("org.mpris.MediaPlayer2.mprisence.tab-1", "Intro", "");
        assert!(!duplicate_metadata(&left, &right));
    }
}
