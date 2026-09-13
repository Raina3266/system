//! MPRIS players, read and driven straight over the session bus.
//!
//! This is the bus layer the Rofi-era media control used, carried forward.
//! Talking to the players directly rather than through a media library is what
//! lets the two things MPRIS is fussy about be got right: `mpris:trackid` is
//! typed as an object path and a seek that names anything else is required to
//! be ignored, and `playerctld` mirrors another player wholesale and must not
//! be shown as a second one.

use std::collections::HashMap;
use std::time::Duration;

use zbus::blocking::{Connection, Proxy, fdo::DBusProxy};
use zbus::names::OwnedBusName;
use zbus::zvariant::{ObjectPath, OwnedValue, Value};

const PLAYER_INTERFACE: &str = "org.mpris.MediaPlayer2.Player";
const ROOT_INTERFACE: &str = "org.mpris.MediaPlayer2";
const MPRIS_PATH: &str = "/org/mpris/MediaPlayer2";
const MPRIS_PREFIX: &str = "org.mpris.MediaPlayer2.";

/// Players this desktop deliberately does not surface.
///
/// `playerctld` proxies whichever player was last active and copies its
/// identity and metadata wholesale, so it shows up as a second card under the
/// real player's own name. `kdeconnect` publishes the phone, not this machine.
const EXCLUDED: &[&str] = &["kdeconnect", "playerctld"];

/// A player's `Metadata` property, read once per refresh.
type Metadata = HashMap<String, OwnedValue>;

/// How long a player is given to act on a command before it is judged to have
/// ignored it. Generous: a browser tab reached through a bridge and a
/// native-messaging hop can take over a second to report where it landed, and
/// the cost of judging too early is commanding it twice, which is audible.
const SETTLE: Duration = Duration::from_millis(2500);
const SETTLE_POLL: Duration = Duration::from_millis(120);
/// A player that landed this close to where it was sent has obeyed.
const SEEK_TOLERANCE: Duration = Duration::from_secs(2);
/// Two players further apart than this are playing separately, not mirroring
/// one another. Wide enough that reading them a moment apart is not read as
/// disagreement.
const POSITION_TOLERANCE: Duration = Duration::from_secs(5);

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
            "Playing" => Self::Playing,
            "Paused" => Self::Paused,
            _ => Self::Stopped,
        }
    }

    pub fn is_playing(self) -> bool {
        self == Self::Playing
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Playing => "Playing",
            Self::Paused => "Paused",
            Self::Stopped => "Stopped",
        }
    }
}

/// One player, as the card draws it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Player {
    /// The bus name, which is how every control call finds it again.
    pub bus: String,
    /// Other players publishing this same playback, which idempotent commands
    /// are mirrored onto so a card still works if the wrong twin was shown.
    pub also: Vec<String>,
    pub source: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub art_url: Option<String>,
    pub track_url: Option<String>,
    pub status: Status,
    pub position: Duration,
    pub length: Option<Duration>,
    pub can_control: bool,
    pub can_seek: bool,
    pub can_go_next: bool,
    pub can_go_previous: bool,
}

impl Player {
    /// How far through the track the seek bar sits, 0.0 to 1.0.
    pub fn progress(&self) -> f64 {
        let Some(length) = self.length.filter(|length| !length.is_zero()) else {
            return 0.0;
        };
        (self.position.as_secs_f64() / length.as_secs_f64()).clamp(0.0, 1.0)
    }

    /// Every player this card stands for, itself first.
    fn targets(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.bus.as_str()).chain(self.also.iter().map(String::as_str))
    }
}

/// A session-bus handle the panel keeps for as long as it is open.
pub struct Players {
    connection: Connection,
}

impl Players {
    pub fn connect() -> Result<Self, String> {
        Connection::session()
            .map(|connection| Self { connection })
            .map_err(|error| format!("could not reach the session bus: {error}"))
    }

    fn player<'a>(&self, bus: &'a str) -> Result<Proxy<'a>, zbus::Error> {
        Proxy::new(&self.connection, bus, MPRIS_PATH, PLAYER_INTERFACE)
    }

    /// Every MPRIS player worth a card, playing ones first, one per playback.
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
            .filter(|player| player.status != Status::Stopped)
            .collect();

        players.sort_by_key(|player| u8::from(!player.status.is_playing()));
        merge_duplicates(players)
    }

    fn read(&self, bus: &str) -> Option<Player> {
        let proxy = self.player(bus).ok()?;
        let metadata: Metadata = proxy.get_property("Metadata").unwrap_or_default();
        let status: String = proxy.get_property("PlaybackStatus").unwrap_or_default();
        let position: i64 = proxy.get_property("Position").unwrap_or(0);

        let root = Proxy::new(&self.connection, bus, MPRIS_PATH, ROOT_INTERFACE).ok();
        let identity = root
            .as_ref()
            .and_then(|root| root.get_property::<String>("Identity").ok())
            .filter(|identity| !identity.trim().is_empty())
            .filter(|identity| !echoes_the_bus_name(identity))
            .unwrap_or_else(|| friendly_source(bus));

        Some(Player {
            bus: bus.to_owned(),
            also: Vec::new(),
            source: identity,
            title: metadata_string(&metadata, "xesam:title").unwrap_or_default(),
            artist: metadata_string(&metadata, "xesam:artist").unwrap_or_default(),
            album: metadata_string(&metadata, "xesam:album").unwrap_or_default(),
            art_url: metadata_string(&metadata, "mpris:artUrl"),
            track_url: metadata_string(&metadata, "xesam:url"),
            status: Status::parse(&status),
            position: Duration::from_micros(position.max(0) as u64),
            length: metadata_u64(&metadata, "mpris:length")
                .map(Duration::from_micros)
                .filter(|length| !length.is_zero()),
            can_control: proxy.get_property("CanControl").unwrap_or(true),
            can_seek: proxy.get_property("CanSeek").unwrap_or(false),
            can_go_next: proxy.get_property("CanGoNext").unwrap_or(false),
            can_go_previous: proxy.get_property("CanGoPrevious").unwrap_or(false),
        })
    }

    /// Plays or pauses a card's playback.
    ///
    /// The command is named rather than toggled. A toggle leaves the decision
    /// to the player, and a bridge republishing a browser tab decides from its
    /// own copy of the tab's state; when the two disagree the toggle resolves
    /// to the state the tab is already in and the press does nothing. Naming
    /// it also makes it idempotent, which is what lets a card send it to every
    /// player it stands for.
    pub fn set_playing(&self, player: &Player, play: bool) {
        let method = if play { "Play" } else { "Pause" };
        for bus in player.targets() {
            self.call(bus, method);
        }

        if self.settles(&player.bus, play) {
            return;
        }
        // MPRIS asks a player that cannot honour a command to ignore it rather
        // than fail, and some publishers answer Play and Pause that way while
        // still acting on the toggle.
        self.call(&player.bus, "PlayPause");
    }

    pub fn next(&self, player: &Player) {
        // Relative, so it goes to one player only: sent to two publishers of
        // one playback it would skip two tracks.
        self.call(&player.bus, "Next");
    }

    pub fn previous(&self, player: &Player) {
        self.call(&player.bus, "Previous");
    }

    /// Jumps to `fraction` through the track.
    ///
    /// `SetPosition` names the track it applies to, and MPRIS requires a
    /// player to ignore the call when that identifier is not the track it is
    /// on — so the identifier is read back off the bus as the object path the
    /// spec says it is. A player that publishes none, or ignores the absolute
    /// call anyway, gets a relative `Seek` measured from its own position,
    /// which lands in the same place.
    ///
    /// One player is moved, not all of them: a second seek at the same
    /// destination is still a second seek, and the stutter is audible. The
    /// others are tried only if the first did not move.
    pub fn seek_to(&self, player: &Player, fraction: f64) {
        let Some(length) = player.length else {
            return;
        };
        let target = length.mul_f64(fraction.clamp(0.0, 1.0));

        for bus in player.targets() {
            if self.seek_one(bus, target) {
                return;
            }
        }
    }

    fn seek_one(&self, bus: &str, target: Duration) -> bool {
        let Ok(proxy) = self.player(bus) else {
            return false;
        };
        let before = self.position(bus);

        if let Some(track) = self.track_id(&proxy) {
            let sent = proxy
                .call::<_, _, ()>("SetPosition", &(track, micros(target)))
                .is_ok();
            if sent && self.acted(bus, target, before) {
                return true;
            }
        }

        let Some(current) = self.position(bus) else {
            return false;
        };
        proxy
            .call::<_, _, ()>("Seek", &(micros(target) - micros(current),))
            .is_ok()
            && self.acted(bus, target, before)
    }

    fn track_id<'a>(&self, proxy: &Proxy<'a>) -> Option<ObjectPath<'static>> {
        let metadata: Metadata = proxy.get_property("Metadata").ok()?;
        let raw = metadata_path(&metadata, "mpris:trackid")?;
        // "/" and the spec's NoTrack sentinel both name no track, and every
        // player that follows the spec ignores a seek carrying them.
        if raw == "/" || raw.ends_with("/NoTrack") {
            return None;
        }
        ObjectPath::try_from(raw).ok().map(ObjectPath::into_owned)
    }

    /// Whether the player did something with a seek.
    ///
    /// Not yet at the target is not the same as having ignored it: a player
    /// that streams buffers first and reports its old position for a moment.
    /// Only one still sitting exactly where it started has done nothing.
    fn acted(&self, bus: &str, target: Duration, before: Option<Duration>) -> bool {
        let mut waited = Duration::ZERO;
        loop {
            let Some(now) = self.position(bus) else {
                return true;
            };
            if now.abs_diff(target) <= SEEK_TOLERANCE {
                return true;
            }
            if waited >= SETTLE {
                return before.is_none_or(|before| now.abs_diff(before) > SEEK_TOLERANCE);
            }
            std::thread::sleep(SETTLE_POLL);
            waited += SETTLE_POLL;
        }
    }

    fn settles(&self, bus: &str, play: bool) -> bool {
        let mut waited = Duration::ZERO;
        loop {
            if self.status(bus).is_playing() == play {
                return true;
            }
            if waited >= SETTLE {
                return false;
            }
            std::thread::sleep(SETTLE_POLL);
            waited += SETTLE_POLL;
        }
    }

    fn status(&self, bus: &str) -> Status {
        self.player(bus)
            .ok()
            .and_then(|proxy| proxy.get_property::<String>("PlaybackStatus").ok())
            .map_or(Status::Stopped, |status| Status::parse(&status))
    }

    fn position(&self, bus: &str) -> Option<Duration> {
        let position: i64 = self.player(bus).ok()?.get_property("Position").ok()?;
        Some(Duration::from_micros(position.max(0) as u64))
    }

    fn call(&self, bus: &str, method: &str) {
        let _ = self
            .player(bus)
            .and_then(|proxy| proxy.call::<_, _, ()>(method, &()));
    }
}

fn is_player(name: &OwnedBusName) -> bool {
    let name = name.as_str();
    name.starts_with(MPRIS_PREFIX)
        && !EXCLUDED
            .iter()
            .any(|excluded| name.to_ascii_lowercase().contains(excluded))
}

/// Most players publish their own name as `Identity` — `Elisa`, `YouTube
/// Music` — and the card shows it untouched. A few echo their bus name back
/// instead, which is dotted and carries no spaces; that gets the same cleanup
/// as a bus publishing no identity at all.
fn echoes_the_bus_name(identity: &str) -> bool {
    identity.contains('.') && !identity.contains(' ')
}

/// Chromium numbers its instances (`instance1234`) and mprisence gives every
/// tab a `p` and a hex id (`pa8082085c72e81fe`). Neither names an app.
fn is_instance(part: &str) -> bool {
    let digits = |rest: &str| !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit());
    part.strip_prefix("instance").is_some_and(digits)
        || part
            .strip_prefix('p')
            .is_some_and(|rest| !rest.is_empty() && rest.chars().all(|c| c.is_ascii_hexdigit()))
        || digits(part)
}

/// The player's own name, for a bus that publishes no usable `Identity`.
///
/// mprisence opens one bus per browser tab and names it for the publisher, the
/// site and the tab: `mprisence_web.youtube_music.pa8082085c72e81fe`. Neither
/// the leading segment nor the whole tail is a name worth putting on a card,
/// so drop the publisher and the per-tab id and keep what names the app.
pub fn friendly_source(bus: &str) -> String {
    let tail = bus.strip_prefix(MPRIS_PREFIX).unwrap_or(bus);
    let mut parts: Vec<&str> = tail.split('.').filter(|part| !part.is_empty()).collect();

    if parts.len() > 1 && parts[0].split('_').next() == Some("mprisence") {
        parts.remove(0);
    }
    if parts.len() > 1 && is_instance(parts[parts.len() - 1]) {
        parts.pop();
    }

    parts
        .join(" ")
        .split(['_', '-', ' '])
        .filter(|word| !word.is_empty())
        .map(|word| {
            let mut characters = word.chars();
            characters.next().map_or_else(String::new, |first| {
                first.to_uppercase().collect::<String>() + characters.as_str()
            })
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn micros(value: Duration) -> i64 {
    i64::try_from(value.as_micros()).unwrap_or(i64::MAX)
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
    .filter(|text| !text.trim().is_empty())
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
        // The spec types this one `o`. Some players publish it as a string
        // anyway, and either still names the track the player is on.
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

/// Folds every publisher of one piece of playback into a single card.
///
/// Identity is deliberately not what decides it. Two publishers of one
/// playback routinely disagree about who they are — a bridge names the app or
/// site it mirrors while the player names itself — so requiring them to agree
/// leaves a player and its mirror side by side. The metadata carries it
/// instead: a shared title is something a mirror and an unrelated track can
/// both have, so two further fields have to line up, and a position the two
/// disagree about settles it the other way. Publishers that do agree on their
/// identity are asked for one field, because one app publishing itself twice
/// is not a coincidence to guard against.
fn merge_duplicates(players: Vec<Player>) -> Vec<Player> {
    let mut kept: Vec<Player> = Vec::with_capacity(players.len());

    for player in players {
        match kept.iter_mut().find(|keeper| same_playback(keeper, &player)) {
            // The list arrives playing-first, so the keeper is the better card
            // already; the other is still a way to reach the same playback.
            Some(keeper) => keeper.also.push(player.bus),
            None => kept.push(player),
        }
    }
    kept
}

fn same_playback(left: &Player, right: &Player) -> bool {
    same_text(&left.title, &right.title)
        && agreeing_fields(left, right) >= required_agreement(left, right)
        && !positions_disagree(left, right)
}

fn agreeing_fields(left: &Player, right: &Player) -> usize {
    [
        same_text(&left.artist, &right.artist),
        same_text(&left.album, &right.album),
        same_optional(&left.art_url, &right.art_url),
        same_optional(&left.track_url, &right.track_url),
        near_length(left.length, right.length),
    ]
    .into_iter()
    .filter(|agrees| *agrees)
    .count()
}

fn required_agreement(left: &Player, right: &Player) -> usize {
    if same_text(&left.source, &right.source) { 1 } else { 2 }
}

fn positions_disagree(left: &Player, right: &Player) -> bool {
    !left.position.is_zero()
        && !right.position.is_zero()
        && left.position.abs_diff(right.position) > POSITION_TOLERANCE
}

fn same_text(left: &str, right: &str) -> bool {
    let left = normalised(left);
    !left.is_empty() && left == normalised(right)
}

fn same_optional(left: &Option<String>, right: &Option<String>) -> bool {
    left.as_deref()
        .zip(right.as_deref())
        .is_some_and(|(left, right)| same_text(left, right))
}

fn near_length(left: Option<Duration>, right: Option<Duration>) -> bool {
    left.zip(right)
        .is_some_and(|(left, right)| left.abs_diff(right) <= Duration::from_secs(2))
}

fn normalised(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

#[cfg(test)]
mod tests;
