//! Talking to every MPRIS player on the session bus directly.
//!
//! Wayle's own media service picks one player per piece of playback; these two
//! commands deliberately do not, because the question they answer is "what is
//! actually on the bus, and will it listen".
//!
//! `playerctl --all-players pause` reads each player's `CanPause` first and
//! skips the ones that answer no, so a browser bridge that cannot reach its
//! tab is never even asked. Publishers get that property wrong often enough —
//! and MPRIS tells a player that cannot honour a command to ignore it rather
//! than fail — that asking every player and letting those that mean it decline
//! stops more music than trusting what they advertise.

use std::collections::BTreeSet;
use std::thread;
use std::time::Duration;

use zbus::blocking::{Connection, fdo::DBusProxy};
use zbus::names::BusName;

const MPRIS_PREFIX: &str = "org.mpris.MediaPlayer2.";
const MPRIS_PATH: &str = "/org/mpris/MediaPlayer2";
const PLAYER_INTERFACE: &str = "org.mpris.MediaPlayer2.Player";
const ROOT_INTERFACE: &str = "org.mpris.MediaPlayer2";
const PROPERTIES_INTERFACE: &str = "org.freedesktop.DBus.Properties";

/// How long a player is given to act on `Pause` before it is offered the
/// toggle instead. Long enough that a browser tab waking up is not mistaken
/// for one that ignored the command.
const SETTLE: Duration = Duration::from_millis(600);

/// What happened to one player.
#[derive(Debug, PartialEq, Eq)]
pub struct Outcome {
    pub bus_name: String,
    pub paused: bool,
}

/// One player as the bus describes it, for `media-players`.
#[derive(Debug)]
pub struct Description {
    pub bus_name: String,
    pub identity: String,
    pub status: String,
    pub title: String,
    pub can_control: bool,
    pub can_pause: bool,
    pub length: Option<i64>,
    pub position: Option<i64>,
}

impl Description {
    /// A player that says it cannot be controlled is one `playerctl` will
    /// refuse to send anything to, which is usually why "pause everything"
    /// appears to skip it.
    pub fn summary(&self) -> String {
        let control = if self.can_control && self.can_pause {
            String::from("controllable")
        } else {
            format!(
                "NOT controllable (CanControl={}, CanPause={})",
                self.can_control, self.can_pause
            )
        };
        let timing = match (self.position, self.length) {
            (Some(position), Some(length)) if length > 0 => {
                format!("{}s/{}s", position / 1_000_000, length / 1_000_000)
            }
            _ => String::from("no position/length published"),
        };
        format!(
            "{}\n    identity : {}\n    status   : {}\n    title    : {}\n    control  : {}\n    timing   : {}",
            self.bus_name, self.identity, self.status, self.title, control, timing
        )
    }
}

/// Keeps the MPRIS players out of a list of every name on the bus.
///
/// Unique names (`:1.42`) are dropped: every player also owns a well-known
/// name, and asking twice would pause it, read it back as paused, and never
/// reach the toggle for the ones that need it.
fn mpris_names<I, S>(names: I) -> Vec<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    names
        .into_iter()
        .filter_map(|name| {
            let name = name.as_ref();
            name.starts_with(MPRIS_PREFIX).then(|| name.to_string())
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// Pauses every player, and reports the ones still playing afterwards.
pub fn pause_all() -> Result<Vec<Outcome>, zbus::Error> {
    let connection = Connection::session()?;
    let bus = DBusProxy::new(&connection)?;
    let players = mpris_names(bus.list_names()?.iter().map(|name| name.as_str()));

    for player in &players {
        let _ = call(&connection, player, "Pause");
    }

    if players.is_empty() {
        return Ok(Vec::new());
    }
    thread::sleep(SETTLE);

    // A player that answered `Pause` and stayed where it was may still be one
    // that only implements the toggle.
    let mut stubborn = Vec::new();
    for player in &players {
        if is_playing(&connection, player) {
            let _ = call(&connection, player, "PlayPause");
            stubborn.push(player);
        }
    }

    if !stubborn.is_empty() {
        thread::sleep(SETTLE);
    }

    Ok(players
        .iter()
        .map(|player| Outcome {
            bus_name: player.clone(),
            paused: !is_playing(&connection, player),
        })
        .collect())
}

/// Describes every MPRIS player, so a source that will not respond can be
/// told apart from one the panel simply picked wrongly.
pub fn describe() -> Result<Vec<Description>, zbus::Error> {
    let connection = Connection::session()?;
    let bus = DBusProxy::new(&connection)?;
    let players = mpris_names(bus.list_names()?.iter().map(|name| name.as_str()));

    Ok(players
        .into_iter()
        .map(|bus_name| Description {
            identity: string_property(&connection, &bus_name, ROOT_INTERFACE, "Identity")
                .unwrap_or_default(),
            status: string_property(&connection, &bus_name, PLAYER_INTERFACE, "PlaybackStatus")
                .unwrap_or_default(),
            title: metadata_title(&connection, &bus_name).unwrap_or_default(),
            can_control: bool_property(&connection, &bus_name, "CanControl").unwrap_or(false),
            can_pause: bool_property(&connection, &bus_name, "CanPause").unwrap_or(false),
            length: metadata_length(&connection, &bus_name),
            position: int_property(&connection, &bus_name, "Position"),
            bus_name,
        })
        .collect())
}

fn property(
    connection: &Connection,
    bus_name: &str,
    interface: &str,
    name: &str,
) -> Option<zbus::zvariant::OwnedValue> {
    let bus = BusName::try_from(bus_name.to_owned()).ok()?;
    let reply = connection
        .call_method(
            Some(bus),
            MPRIS_PATH,
            Some(PROPERTIES_INTERFACE),
            "Get",
            &(interface, name),
        )
        .ok()?;
    reply.body().deserialize::<zbus::zvariant::Value<'_>>().ok()?.try_into().ok()
}

fn string_property(
    connection: &Connection,
    bus_name: &str,
    interface: &str,
    name: &str,
) -> Option<String> {
    String::try_from(property(connection, bus_name, interface, name)?).ok()
}

fn bool_property(connection: &Connection, bus_name: &str, name: &str) -> Option<bool> {
    bool::try_from(property(connection, bus_name, PLAYER_INTERFACE, name)?).ok()
}

fn int_property(connection: &Connection, bus_name: &str, name: &str) -> Option<i64> {
    i64::try_from(property(connection, bus_name, PLAYER_INTERFACE, name)?).ok()
}

fn metadata(
    connection: &Connection,
    bus_name: &str,
) -> Option<std::collections::HashMap<String, zbus::zvariant::OwnedValue>> {
    property(connection, bus_name, PLAYER_INTERFACE, "Metadata")?
        .try_into()
        .ok()
}

fn metadata_title(connection: &Connection, bus_name: &str) -> Option<String> {
    String::try_from(metadata(connection, bus_name)?.remove("xesam:title")?).ok()
}

fn metadata_length(connection: &Connection, bus_name: &str) -> Option<i64> {
    i64::try_from(metadata(connection, bus_name)?.remove("mpris:length")?).ok()
}

fn call(connection: &Connection, bus_name: &str, method: &str) -> Result<(), zbus::Error> {
    let name = BusName::try_from(bus_name.to_owned())?;
    connection.call_method(Some(name), MPRIS_PATH, Some(PLAYER_INTERFACE), method, &())?;
    Ok(())
}

fn is_playing(connection: &Connection, bus_name: &str) -> bool {
    let Ok(name) = BusName::try_from(bus_name.to_owned()) else {
        return false;
    };
    let reply = connection.call_method(
        Some(name),
        MPRIS_PATH,
        Some(PROPERTIES_INTERFACE),
        "Get",
        &(PLAYER_INTERFACE, "PlaybackStatus"),
    );
    let Ok(reply) = reply else {
        return false;
    };
    reply
        .body()
        .deserialize::<zbus::zvariant::Value<'_>>()
        .ok()
        .and_then(|value| String::try_from(value).ok())
        .is_some_and(|status| status.eq_ignore_ascii_case("playing"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mpris_names_keeps_only_players() {
        let names = mpris_names([
            "org.freedesktop.DBus",
            "org.mpris.MediaPlayer2.elisa",
            ":1.42",
            "org.mpris.MediaPlayer2.mprisence_web.spotify.pabc",
            "com.wayle.Shell1",
        ]);

        assert_eq!(
            names,
            vec![
                "org.mpris.MediaPlayer2.elisa".to_string(),
                "org.mpris.MediaPlayer2.mprisence_web.spotify.pabc".to_string(),
            ]
        );
    }

    #[test]
    fn mpris_names_deduplicates_and_sorts() {
        let names = mpris_names([
            "org.mpris.MediaPlayer2.zzz",
            "org.mpris.MediaPlayer2.aaa",
            "org.mpris.MediaPlayer2.zzz",
        ]);

        assert_eq!(
            names,
            vec![
                "org.mpris.MediaPlayer2.aaa".to_string(),
                "org.mpris.MediaPlayer2.zzz".to_string(),
            ]
        );
    }

    #[test]
    fn mpris_names_is_empty_without_players() {
        assert!(mpris_names(["org.freedesktop.DBus", ":1.7"]).is_empty());
    }

    #[test]
    fn mpris_names_rejects_the_bare_prefix_owner() {
        // `org.mpris.MediaPlayer2` itself is not a player; only names under it.
        assert!(mpris_names(["org.mpris.MediaPlayer2"]).is_empty());
    }
}
