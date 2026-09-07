//! MPRIS players, read and driven straight over the session bus.
//!
//! `media-control` used to shell out to `playerctl` for this. A panel that
//! redraws a seek bar twice a second cannot afford a process per tick, so the
//! players are read from D-Bus directly.

use std::collections::HashMap;
use std::time::Duration;

use zbus::blocking::{fdo::DBusProxy, Connection, Proxy};
use zbus::names::OwnedBusName;
use zbus::zvariant::{OwnedValue, Value};

const PLAYER_INTERFACE: &str = "org.mpris.MediaPlayer2.Player";
const MPRIS_PATH: &str = "/org/mpris/MediaPlayer2";
const MPRIS_PREFIX: &str = "org.mpris.MediaPlayer2.";

/// A player's `Metadata` property, read once per refresh.
type Metadata = HashMap<String, OwnedValue>;

/// Players this desktop deliberately does not surface here.
const EXCLUDED: &[&str] = &["kdeconnect", "playerctld"];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Status {
    Playing,
    Paused,
    #[default]
    Stopped,
}

impl Status {
    fn parse(value: &str) -> Self {
        match value {
            "Playing" => Status::Playing,
            "Paused" => Status::Paused,
            _ => Status::Stopped,
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Status::Playing => "󰏤",
            _ => "󰐊",
        }
    }

    pub fn is_playing(self) -> bool {
        self == Status::Playing
    }
}

/// One player, as the card draws it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Player {
    /// The bus name, which is how every control call finds it again.
    pub bus: String,
    pub source: String,
    pub title: String,
    pub artist: String,
    pub status: Status,
    pub position: Duration,
    pub length: Duration,
}

impl Player {
    /// How far through the track the seek bar sits, 0.0 to 1.0.
    pub fn progress(&self) -> f64 {
        if self.length.is_zero() {
            return 0.0;
        }
        (self.position.as_secs_f64() / self.length.as_secs_f64()).clamp(0.0, 1.0)
    }

    pub fn has_length(&self) -> bool {
        !self.length.is_zero()
    }
}

/// A session-bus handle the panel keeps for as long as it is open.
pub struct Players {
    connection: Connection,
}

impl Players {
    pub fn connect() -> Result<Self, String> {
        Connection::session()
            .map(|connection| Players { connection })
            .map_err(|error| format!("could not reach the session bus: {error}"))
    }

    fn player<'a>(&self, bus: &'a str) -> Result<Proxy<'a>, zbus::Error> {
        Proxy::new(&self.connection, bus, MPRIS_PATH, PLAYER_INTERFACE)
    }

    /// Every MPRIS player currently on the bus, playing ones first.
    pub fn snapshot(&self) -> Vec<Player> {
        let Ok(dbus) = DBusProxy::new(&self.connection) else {
            return Vec::new();
        };
        let Ok(names) = dbus.list_names() else {
            return Vec::new();
        };

        let mut players: Vec<Player> = names
            .into_iter()
            .filter(is_player)
            .filter_map(|name| self.read(name.as_str()))
            .collect();

        // A playing player is the one the panel is about; past that, keep the
        // bus order so rows do not swap around under the pointer.
        players.sort_by_key(|player| u8::from(!player.status.is_playing()));
        players
    }

    fn read(&self, bus: &str) -> Option<Player> {
        let proxy = self.player(bus).ok()?;
        let metadata: HashMap<String, OwnedValue> =
            proxy.get_property("Metadata").ok().unwrap_or_default();
        let status: String = proxy.get_property("PlaybackStatus").unwrap_or_default();
        let position: i64 = proxy.get_property("Position").unwrap_or(0);

        Some(Player {
            bus: bus.to_owned(),
            source: friendly_source(bus),
            title: metadata_string(&metadata, "xesam:title").unwrap_or_default(),
            artist: metadata_string(&metadata, "xesam:artist").unwrap_or_default(),
            status: Status::parse(&status),
            position: micros(position.max(0) as u64),
            length: micros(metadata_u64(&metadata, "mpris:length").unwrap_or(0)),
        })
    }

    pub fn play_pause(&self, bus: &str) {
        let _ = self
            .player(bus)
            .and_then(|proxy| proxy.call::<_, _, ()>("PlayPause", &()));
    }

    pub fn previous(&self, bus: &str) {
        let _ = self
            .player(bus)
            .and_then(|proxy| proxy.call::<_, _, ()>("Previous", &()));
    }

    pub fn next(&self, bus: &str) {
        let _ = self
            .player(bus)
            .and_then(|proxy| proxy.call::<_, _, ()>("Next", &()));
    }

    /// Jump to `fraction` through the track. MPRIS has no proportional seek,
    /// so the track id is read back and `SetPosition` given an absolute point.
    pub fn seek_to(&self, bus: &str, fraction: f64, length: Duration) {
        let Ok(proxy) = self.player(bus) else {
            return;
        };
        let Ok(metadata) = proxy.get_property::<HashMap<String, OwnedValue>>("Metadata") else {
            return;
        };
        let Some(track) = metadata_path(&metadata, "mpris:trackid") else {
            return;
        };

        let target = length.as_micros() as f64 * fraction.clamp(0.0, 1.0);
        #[allow(clippy::cast_possible_truncation)]
        let target = target as i64;
        let track = zbus::zvariant::ObjectPath::try_from(track.as_str());
        if let Ok(track) = track {
            let _ = proxy.call::<_, _, ()>("SetPosition", &(track, target));
        }
    }
}

fn is_player(name: &OwnedBusName) -> bool {
    let name = name.as_str();
    name.starts_with(MPRIS_PREFIX)
        && !EXCLUDED
            .iter()
            .any(|excluded| name.to_ascii_lowercase().contains(excluded))
}

/// The player's own name, as a person would say it.
pub fn friendly_source(bus: &str) -> String {
    let tail = bus.strip_prefix(MPRIS_PREFIX).unwrap_or(bus);
    // Chromium-style buses carry an instance suffix: `chromium.instance1234`.
    let name = tail.split('.').next().unwrap_or(tail);
    let mut characters = name.chars();
    characters.next().map_or_else(String::new, |first| {
        first.to_uppercase().collect::<String>() + characters.as_str()
    })
}

fn micros(value: u64) -> Duration {
    Duration::from_micros(value)
}

fn metadata_string(metadata: &Metadata, key: &str) -> Option<String> {
    match &**metadata.get(key)? {
        Value::Str(text) => Some(text.to_string()),
        // `xesam:artist` is an array; the card shows the first name in it.
        Value::Array(array) => array.iter().find_map(|item| match item {
            Value::Str(text) => Some(text.to_string()),
            _ => None,
        }),
        _ => None,
    }
}

fn metadata_u64(metadata: &Metadata, key: &str) -> Option<u64> {
    match &**metadata.get(key)? {
        Value::U64(value) => Some(*value),
        Value::I64(value) => u64::try_from(*value).ok(),
        Value::U32(value) => Some(u64::from(*value)),
        _ => None,
    }
}

fn metadata_path(metadata: &Metadata, key: &str) -> Option<String> {
    match &**metadata.get(key)? {
        Value::ObjectPath(path) => Some(path.to_string()),
        Value::Str(text) => Some(text.to_string()),
        _ => None,
    }
}

/// `1:23`, growing an hours field only when the track needs one.
pub fn clock(duration: Duration) -> String {
    let total = duration.as_secs();
    let (hours, minutes, seconds) = (total / 3600, (total % 3600) / 60, total % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

#[cfg(test)]
mod tests;
