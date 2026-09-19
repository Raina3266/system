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
pub(crate) struct Track {
    pub(crate) id: String,
    pub(crate) identity: String,
    pub(crate) state: String,
    pub(crate) title: String,
    pub(crate) artist: String,
    pub(crate) album: String,
    pub(crate) art_url: String,
    pub(crate) length_us: Option<u64>,
}

impl Track {
    fn read(proxy: &WayleMediaProxy<'_>, player: &(String, String, String)) -> zbus::Result<Self> {
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
    let mut previous_badge = None;
    loop {
        if let Some(proxy) = media() {
            loop {
                match current_track(&proxy) {
                    Ok(track) => {
                        if !print_badge(track.as_ref(), &mut previous_badge) {
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

        if !print_badge(None, &mut previous_badge) {
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

pub(crate) fn preferred_duplicate(selected: Track, bridge: Track) -> Track {
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

pub(crate) fn duplicate_metadata(left: &Track, right: &Track) -> bool {
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

pub(crate) fn is_bridge(id: &str) -> bool {
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

#[derive(PartialEq, Eq, Serialize)]
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

fn print_badge(track: Option<&Track>, previous: &mut Option<Badge>) -> bool {
    let current = badge(track);
    if previous.as_ref() == Some(&current) {
        return true;
    }
    let Ok(line) = serde_json::to_string(&current) else {
        return false;
    };
    println!("{line}");
    if io::stdout().flush().is_err() {
        return false;
    }
    *previous = Some(current);
    true
}

pub(crate) fn truncate_title(title: &str) -> String {
    let mut output = String::new();
    let mut ideographs = 0;

    for (characters, character) in title.chars().enumerate() {
        if characters == 50 || (is_cjk_ideograph(character) && ideographs == 35) {
            output.push('…');
            return output;
        }
        output.push(character);
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
