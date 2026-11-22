//! Conservative duplicate matching and source names retained from media-panel.
//!
//! The matching policy is pure. Private-owner recovery below supplements native
//! Astal players without changing their discovery, controls, or matching policy.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use dbus::arg::{ArgType, PropMap, RefArg, Variant};
use dbus::blocking::SyncConnection;
use dbus::channel::{BusType, Channel, Sender};
use dbus::{Message, MessageType};
use serde::Serialize;

use crate::model::{NativeAction, Player, Stream};

const MPRIS_PREFIX: &str = "org.mpris.MediaPlayer2.";
/// Wide enough that reading players a moment apart is not disagreement.
const POSITION_TOLERANCE: f64 = 5.0;

/// Title alone can collide, so two more fields must agree and a differing
/// position splits them. Publishers that already agree on identity need one.
pub(crate) fn same_playback(left: &Player, right: &Player) -> bool {
    same_text(&left.title, &right.title)
        && agreeing_fields(left, right) >= required_agreement(left, right)
        && !positions_disagree(left, right)
}

fn agreeing_fields(left: &Player, right: &Player) -> usize {
    [
        same_text(&left.artist, &right.artist),
        same_text(&left.album, &right.album),
        same_text(&left.art_url, &right.art_url),
        same_text(&left.url, &right.url),
        near_length(left.length, right.length),
    ]
    .into_iter()
    .filter(|agrees| *agrees)
    .count()
}

fn required_agreement(left: &Player, right: &Player) -> usize {
    if same_text(&source_name(left), &source_name(right)) {
        1
    } else {
        2
    }
}

fn positions_disagree(left: &Player, right: &Player) -> bool {
    left.position != 0.0
        && right.position != 0.0
        && (left.position - right.position).abs() > POSITION_TOLERANCE
}

fn same_text(left: &str, right: &str) -> bool {
    let left = normalised(left);
    !left.is_empty() && left == normalised(right)
}

fn near_length(left: Option<f64>, right: Option<f64>) -> bool {
    known_length(left)
        .zip(known_length(right))
        .is_some_and(|(left, right)| (left - right).abs() <= 2.0)
}

// The old MPRIS reader discarded zero/invalid lengths before matching.
// Representative ranking uses the same definition of a usable length.
pub(crate) fn known_length(length: Option<f64>) -> Option<f64> {
    length.filter(|length| length.is_finite() && *length > 0.0)
}

fn normalised(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

pub(crate) fn source_name(player: &Player) -> Cow<'_, str> {
    if player.identity.trim().is_empty() || echoes_bus_name(&player.identity) {
        Cow::Owned(friendly_source(&player.bus))
    } else {
        Cow::Borrowed(&player.identity)
    }
}

/// A few players echo their dotted bus name rather than publishing a usable
/// `Identity`; those get the same cleanup as a player with no identity at all.
pub(crate) fn echoes_bus_name(identity: &str) -> bool {
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
/// mprisence names a bus for publisher, site and tab —
/// `mprisence_web.youtube_music.pa8082085c72e81fe` — so drop the publisher and
/// the per-tab id and keep the middle, which names the app.
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

const MPRIS_ROOT: &str = "org.mpris.MediaPlayer2";
const MPRIS_PLAYER: &str = "org.mpris.MediaPlayer2.Player";
const MPRIS_PATH: &str = "/org/mpris/MediaPlayer2";
const DBUS: &str = "org.freedesktop.DBus";
const DBUS_PATH: &str = "/org/freedesktop/DBus";
const PROPERTIES: &str = "org.freedesktop.DBus.Properties";
const OWNER_RULE: &str = "type='signal',sender='org.freedesktop.DBus',path='/org/freedesktop/DBus',interface='org.freedesktop.DBus',member='NameOwnerChanged'";
const REQUEST_TIMEOUT: Duration = Duration::from_millis(1500);
const STARTUP_RETRY: Duration = Duration::from_millis(250);
const POSITION_INTERVAL: Duration = Duration::from_millis(1000);
const MAX_PROBES: usize = 4;
const MESSAGE_BATCH: usize = 64;

/// Raw source observations, never display interpolation or an optimistic seek.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveredPlayer {
    #[serde(flatten)]
    pub player: Player,
    pub instance: u64,
    pub track_id: String,
    pub rate: f64,
    pub position_supported: bool,
    /// Advances even when a real position read repeats the preceding value.
    pub position_revision: u64,
    pub cover: String,
}

/// The same clock domain as GLib.get_monotonic_time(), in seconds.
pub fn monotonic_seconds() -> f64 {
    let mut time = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // CLOCK_MONOTONIC is available on every supported Linux host.
    if unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut time) } != 0 {
        return 0.0;
    }
    time.tv_sec as f64 + time.tv_nsec as f64 / 1e9
}

/// Event-driven recovery of stream-matched connections with private MPRIS exports.
/// Failure to connect is local to this fallback; callers can keep using Astal.
pub struct Recovery {
    shared: Arc<RecoveryShared>,
    worker: Option<JoinHandle<()>>,
}

struct RecoveryShared {
    connection: Arc<SyncConnection>,
    state: Mutex<RecoveryState>,
    wake: Mutex<UnixStream>,
}

impl RecoveryShared {
    fn state(&self) -> MutexGuard<'_, RecoveryState> {
        self.state.lock().unwrap_or_else(|error| error.into_inner())
    }

    fn wake(&self) {
        // A full socket already contains a wakeup; never block a caller on I/O.
        let _ = self
            .wake
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .write(&[1]);
    }
}

impl Recovery {
    pub fn new() -> crate::AppResult<Self> {
        let mut channel = Channel::get_private(BusType::Session)?;
        channel.set_watch_enabled(true);
        let connection = Arc::new(SyncConnection::from(channel));
        let _: () = connection
            .with_proxy(DBUS, DBUS_PATH, REQUEST_TIMEOUT)
            .method_call(DBUS, "AddMatch", (OWNER_RULE,))?;
        let (wake, reader) = UnixStream::pair()?;
        wake.set_nonblocking(true)?;
        reader.set_nonblocking(true)?;
        let state = RecoveryState {
            own_bus: connection.unique_name().to_string(),
            ..RecoveryState::default()
        };
        let shared = Arc::new(RecoveryShared {
            connection,
            state: Mutex::new(state),
            wake: Mutex::new(wake),
        });
        let worker_shared = shared.clone();
        let worker = thread::Builder::new()
            .name("media-recovery".into())
            .spawn(move || recovery_worker(worker_shared, reader))?;
        Ok(Self {
            shared,
            worker: Some(worker),
        })
    }

    pub fn update(&self, streams: &[Stream], native_buses: &[String]) {
        let changed =
            self.shared
                .state()
                .update(streams, native_buses, self.shared.connection.as_ref());
        if changed {
            self.shared.wake();
        }
    }

    pub fn players(&self) -> Vec<RecoveredPlayer> {
        self.shared.state().snapshots()
    }

    pub fn take_changed(&self) -> bool {
        std::mem::take(&mut self.shared.state().changed)
    }

    /// Validates the latest observations and queues the unique-owner message
    /// under one lock, without waiting for a reply. The service supplies policy;
    /// no worker update can retarget the command between validation and send.
    pub fn dispatch(
        &self,
        operation: &NativeAction,
        validate: impl FnOnce(Vec<RecoveredPlayer>) -> Result<(), String>,
    ) -> crate::AppResult<()> {
        let mut state = self.shared.state();
        state.dispatch(operation, self.shared.connection.as_ref(), validate)?;
        drop(state);
        self.shared.wake();
        Ok(())
    }

    /// Joins an in-flight read, or requests one bounded read even while paused.
    pub fn refresh_position(&self, bus: &str) {
        let mut state = self.shared.state();
        if !state.stopped
            && let Some(player) = state.players.get_mut(bus).filter(|player| player.available)
        {
            player.request_position();
        }
        drop(state);
        self.shared.wake();
    }
}

impl Drop for Recovery {
    fn drop(&mut self) {
        self.shared.state().stopped = true;
        self.shared.wake();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn application_key(value: &str) -> String {
    let leaf = value.rsplit(['/', '\\']).next().unwrap_or_default();
    let leaf = leaf.strip_prefix('.').unwrap_or(leaf);
    let leaf = leaf.strip_suffix("-wrapped").unwrap_or(leaf);
    let leaf = leaf.strip_suffix(".desktop").unwrap_or(leaf);
    leaf.rsplit('.').next().unwrap_or_default().to_lowercase()
}

fn unvariant(mut value: &dyn RefArg) -> &dyn RefArg {
    while value.arg_type() == ArgType::Variant {
        let Some(inner) = value.as_iter().and_then(|mut values| values.next()) else {
            break;
        };
        value = inner;
    }
    value
}

fn number(value: &dyn RefArg) -> Option<f64> {
    let value = unvariant(value);
    let value = match value.arg_type() {
        ArgType::Double => value.as_f64(),
        ArgType::Byte
        | ArgType::Int16
        | ArgType::UInt16
        | ArgType::Int32
        | ArgType::UInt32
        | ArgType::Int64
        | ArgType::UInt64 => value
            .as_i64()
            .map(|n| n as f64)
            .or_else(|| value.as_u64().map(|n| n as f64)),
        // RefArg's numeric accessors also accept booleans: explicitly reject them.
        _ => None,
    }?;
    value.is_finite().then_some(value)
}

fn seconds(value: &dyn RefArg) -> Option<f64> {
    number(value)
        .filter(|value| *value >= 0.0)
        .map(|value| value / 1e6)
}

fn text(value: &dyn RefArg) -> String {
    let value = unvariant(value);
    match value.arg_type() {
        ArgType::String | ArgType::ObjectPath => value.as_str().unwrap_or_default().to_owned(),
        _ => String::new(),
    }
}

fn boolean(value: &dyn RefArg) -> bool {
    let value = unvariant(value);
    value.arg_type() == ArgType::Boolean && value.as_i64() == Some(1)
}

fn refarg_json(value: &dyn RefArg) -> serde_json::Value {
    fn convert(value: &dyn RefArg, depth: usize) -> serde_json::Value {
        use serde_json::Value;
        if depth > 16 {
            return Value::Null;
        }
        let value = unvariant(value);
        match value.arg_type() {
            ArgType::Boolean => Value::Bool(boolean(value)),
            ArgType::Byte | ArgType::UInt16 | ArgType::UInt32 | ArgType::UInt64 => {
                value.as_u64().map_or(Value::Null, Value::from)
            }
            ArgType::Int16 | ArgType::Int32 | ArgType::Int64 => {
                value.as_i64().map_or(Value::Null, Value::from)
            }
            ArgType::Double => number(value).map_or(Value::Null, Value::from),
            ArgType::String | ArgType::ObjectPath | ArgType::Signature => value
                .as_str()
                .map_or(Value::Null, |text| Value::String(text.to_owned())),
            ArgType::Array if value.signature().starts_with("a{") => {
                let mut result = serde_json::Map::new();
                if let Some(mut entries) = value.as_iter() {
                    while let (Some(key), Some(value)) = (entries.next(), entries.next()) {
                        if let Some(key) = key.as_str() {
                            result.insert(key.to_owned(), convert(value, depth + 1));
                        }
                    }
                }
                Value::Object(result)
            }
            ArgType::Array | ArgType::Struct => Value::Array(
                value
                    .as_iter()
                    .into_iter()
                    .flatten()
                    .map(|value| convert(value, depth + 1))
                    .collect(),
            ),
            _ => Value::Null,
        }
    }
    convert(value, 0)
}

fn valid_track_id(value: &str) -> bool {
    !value.is_empty()
        && value != "/org/mpris/MediaPlayer2/TrackList/NoTrack"
        && dbus::Path::new(value).is_ok()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PlayerInterface {
    Root,
    Player,
}

impl PlayerInterface {
    fn name(self) -> &'static str {
        match self {
            Self::Root => MPRIS_ROOT,
            Self::Player => MPRIS_PLAYER,
        }
    }
    fn index(self) -> usize {
        match self {
            Self::Root => 0,
            Self::Player => 1,
        }
    }
    fn parse(value: &str) -> Option<Self> {
        match value {
            MPRIS_ROOT => Some(Self::Root),
            MPRIS_PLAYER => Some(Self::Player),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Baseline {
    version: u64,
    position_generation: u64,
}

struct DirectPlayer {
    snapshot: RecoveredPlayer,
    available: bool,
    loaded: [bool; 2],
    reloading: [bool; 2],
    reload_requested: [bool; 2],
    version: u64,
    property_versions: HashMap<String, u64>,
    position_generation: u64,
    sample_generation: u64,
    position_read: Option<u64>,
    position_requested: bool,
    next_poll: Option<Instant>,
}

impl DirectPlayer {
    fn new(owner: &str, instance: u64) -> Self {
        Self {
            snapshot: RecoveredPlayer {
                player: Player {
                    bus: owner.into(),
                    stopped: true,
                    ..Player::default()
                },
                instance,
                track_id: String::new(),
                rate: 1.0,
                position_supported: false,
                position_revision: 0,
                cover: String::new(),
            },
            available: false,
            loaded: [false; 2],
            reloading: [false; 2],
            reload_requested: [false; 2],
            version: 0,
            property_versions: HashMap::new(),
            position_generation: 0,
            sample_generation: 0,
            position_read: None,
            position_requested: false,
            next_poll: None,
        }
    }

    fn baseline(&self) -> Baseline {
        Baseline {
            version: self.version,
            position_generation: self.position_generation,
        }
    }

    fn property_is_current(&self, property: &str, baseline: Baseline) -> bool {
        self.property_versions.get(property).copied().unwrap_or(0) <= baseline.version
    }

    fn apply_baseline(
        &mut self,
        mut properties: PropMap,
        interface: PlayerInterface,
        baseline: Baseline,
        now: Instant,
    ) {
        properties.retain(|key, _| self.property_is_current(key, baseline));
        let current_metadata = self.property_is_current("Metadata", baseline);
        if self.position_generation != baseline.position_generation || !current_metadata {
            properties.remove("Position");
        }
        let preserve_sample = self.snapshot.position_supported
            && self.sample_generation > baseline.position_generation;
        self.apply_properties(
            properties,
            interface == PlayerInterface::Player && current_metadata,
            preserve_sample,
            now,
        );
    }

    fn signal(
        &mut self,
        interface: PlayerInterface,
        properties: PropMap,
        invalidated: Vec<String>,
        now: Instant,
    ) {
        self.version += 1;
        for key in properties.keys().chain(invalidated.iter()) {
            self.property_versions.insert(key.clone(), self.version);
        }
        self.apply_properties(properties, false, false, now);
        if !invalidated.is_empty() {
            self.reload_requested[interface.index()] = true;
            if invalidated
                .iter()
                .any(|key| key == "Metadata" || key == "Position")
            {
                self.set_position(None);
                self.position_requested = true;
            }
        }
    }

    fn apply_properties(
        &mut self,
        properties: PropMap,
        full_player: bool,
        preserve_sample: bool,
        now: Instant,
    ) {
        let player = &mut self.snapshot.player;
        for (key, target) in [
            ("Identity", &mut player.identity),
            ("DesktopEntry", &mut player.desktop_entry),
        ] {
            if let Some(value) = properties.get(key) {
                *target = text(value.0.as_ref());
            }
        }
        for (key, target) in [
            ("CanPlay", &mut player.can_play),
            ("CanPause", &mut player.can_pause),
            ("CanSeek", &mut player.can_seek),
            ("CanGoNext", &mut player.can_next),
            ("CanGoPrevious", &mut player.can_previous),
        ] {
            if let Some(value) = properties.get(key) {
                *target = boolean(value.0.as_ref());
            }
        }
        if let Some(value) = properties.get("Rate") {
            self.snapshot.rate = number(value.0.as_ref()).unwrap_or(1.0);
        }
        if let Some(value) = properties.get("PlaybackStatus") {
            let status = text(value.0.as_ref());
            let playing = status == "Playing";
            let stopped = status != "Playing" && status != "Paused";
            if (player.playing, player.stopped) != (playing, stopped) {
                self.position_generation += 1;
                if player.playing && !playing && !stopped {
                    self.position_requested = true;
                }
            }
            player.playing = playing;
            player.stopped = stopped;
        }
        let mut track_changed = false;
        if properties.contains_key("Metadata") || full_player {
            let previous = (
                self.snapshot.track_id.clone(),
                player.title.clone(),
                player.url.clone(),
            );
            let metadata = properties
                .get("Metadata")
                .map(|value| refarg_json(value.0.as_ref()))
                .unwrap_or_default();
            let string = |key: &str| {
                metadata
                    .get(key)
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_owned()
            };
            self.snapshot.track_id = string("mpris:trackid");
            player.title = string("xesam:title");
            player.album = string("xesam:album");
            player.url = string("xesam:url");
            player.art_url = string("mpris:artUrl");
            player.artist = metadata
                .get("xesam:artist")
                .and_then(serde_json::Value::as_array)
                .map(|values| {
                    values
                        .iter()
                        .filter_map(serde_json::Value::as_str)
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default();
            player.length = metadata
                .get("mpris:length")
                .and_then(serde_json::Value::as_f64)
                .filter(|value| value.is_finite() && *value > 0.0)
                .map(|value| value / 1e6);
            player.browser_hint = metadata
                .get("mprisence:browser")
                .cloned()
                .unwrap_or_default();
            track_changed = previous
                != (
                    self.snapshot.track_id.clone(),
                    player.title.clone(),
                    player.url.clone(),
                );
            if track_changed {
                self.position_generation += 1;
            }
        }
        if let Some(value) = properties.get("Position") {
            self.set_position(seconds(value.0.as_ref()));
        } else if track_changed && !preserve_sample {
            self.set_position(None);
            self.position_requested = true;
        }
        self.update_poll(now);
    }

    fn set_position(&mut self, position: Option<f64>) {
        self.position_generation += 1;
        self.snapshot.position_revision += 1;
        self.snapshot.position_supported = position.is_some();
        self.snapshot.player.position = position.unwrap_or(0.0);
        if position.is_some() {
            self.sample_generation = self.position_generation;
            self.position_requested = false;
        }
    }

    fn request_position(&mut self) {
        if self.position_read.is_none() {
            self.position_requested = true;
        }
    }

    fn finish_position(&mut self, generation: u64, position: Option<f64>) {
        self.position_read = None;
        if self.position_generation == generation {
            // A failed read is not a source sample. Preserve the last confirmed
            // position and let the service's freshness guard age it out.
            if let Some(position) = position {
                self.set_position(Some(position));
            }
        } else if !(self.snapshot.position_supported && self.sample_generation > generation) {
            // A track/seek/status boundary needs one fresh read, even while paused.
            self.position_requested = true;
        }
    }

    fn update_poll(&mut self, now: Instant) {
        if self.available && self.snapshot.player.playing {
            self.next_poll.get_or_insert(now + POSITION_INTERVAL);
        } else {
            self.next_poll = None;
        }
    }

    fn control(&self, operation: &NativeAction) -> Result<Message, String> {
        let method = match operation.operation.as_str() {
            "play" if self.snapshot.player.can_play => "Play",
            // Pause-all intentionally includes players misreporting CanPause.
            "pause" => "Pause",
            "next" if self.snapshot.player.can_next => "Next",
            "previous" if self.snapshot.player.can_previous => "Previous",
            "seek" if self.snapshot.player.can_seek && valid_track_id(&self.snapshot.track_id) => {
                "SetPosition"
            }
            "play" | "next" | "previous" | "seek" => {
                return Err("The media operation is unavailable for this track.".into());
            }
            _ => return Err("Unsupported native media operation.".into()),
        };
        let mut message =
            Message::new_method_call(&self.snapshot.player.bus, MPRIS_PATH, MPRIS_PLAYER, method)?;
        if method == "SetPosition" {
            let value = operation
                .value
                .filter(|value| value.is_finite() && *value >= 0.0)
                .ok_or("Seek position must be finite and nonnegative.")?;
            let microseconds = (value * 1e6).trunc();
            if !microseconds.is_finite() || microseconds >= i64::MAX as f64 {
                return Err("Seek position is outside the MPRIS range.".into());
            }
            message = message.append2(
                dbus::Path::new(self.snapshot.track_id.clone())?,
                microseconds as i64,
            );
        }
        message.set_no_reply(true);
        Ok(message)
    }
}

#[derive(Clone, Debug)]
enum Query {
    ListNames,
    Alias {
        name: String,
        revision: u64,
    },
    Credentials {
        owner: String,
        instance: u64,
    },
    Subscribe {
        owner: String,
        instance: u64,
    },
    Properties {
        owner: String,
        instance: u64,
        interface: PlayerInterface,
        baseline: Baseline,
    },
    Position {
        owner: String,
        instance: u64,
        generation: u64,
    },
}

impl Query {
    fn owner(&self) -> Option<&str> {
        match self {
            Self::Credentials { owner, .. }
            | Self::Subscribe { owner, .. }
            | Self::Properties { owner, .. }
            | Self::Position { owner, .. } => Some(owner),
            _ => None,
        }
    }
}

struct Pending {
    query: Query,
    deadline: Instant,
}

#[derive(Debug)]
struct BusFailure {
    name: String,
}

impl BusFailure {
    fn malformed() -> Self {
        Self {
            name: "InvalidReply".into(),
        }
    }
}

fn retryable_startup_error(name: &str) -> bool {
    matches!(
        name,
        "org.freedesktop.DBus.Error.UnknownObject"
            | "org.freedesktop.DBus.Error.UnknownInterface"
            | "org.freedesktop.DBus.Error.UnknownMethod"
    )
}

fn daemon_message(method: &str) -> Message {
    Message::new_method_call(DBUS, DBUS_PATH, DBUS, method).expect("constant D-Bus method")
}

fn properties_message(owner: &str, method: &str) -> Message {
    Message::new_method_call(owner, MPRIS_PATH, PROPERTIES, method)
        .expect("verified unique D-Bus owner")
}

fn player_rule(owner: &str) -> String {
    format!("type='signal',sender='{owner}',path='{MPRIS_PATH}'")
}

#[derive(Default)]
struct RecoveryState {
    own_bus: String,
    stopped: bool,
    changed: bool,
    listed: bool,
    initial_started: bool,
    initial_closed: HashSet<String>,
    owners: BTreeSet<String>,
    keys: BTreeSet<String>,
    stream_ids: Vec<u32>,
    native_buses: BTreeSet<String>,
    aliases: HashMap<String, String>,
    alias_revisions: HashMap<String, u64>,
    alias_pending: HashSet<String>,
    alias_checked: HashSet<String>,
    credentials: HashMap<String, String>,
    checked: HashSet<String>,
    retried: HashSet<String>,
    retries: HashMap<String, Instant>,
    probing: HashMap<String, u64>,
    next_instance: u64,
    players: BTreeMap<String, DirectPlayer>,
    pending: HashMap<u32, Pending>,
}

impl RecoveryState {
    fn snapshots(&self) -> Vec<RecoveredPlayer> {
        self.players
            .values()
            .filter(|player| player.available)
            .map(|player| player.snapshot.clone())
            .collect()
    }

    fn normal_owners(&self) -> HashSet<String> {
        self.native_buses
            .iter()
            .map(|bus| self.aliases.get(bus).unwrap_or(bus).clone())
            .collect()
    }

    fn update(&mut self, streams: &[Stream], native_buses: &[String], sender: &dyn Sender) -> bool {
        if self.stopped {
            return false;
        }
        let keys = streams
            .iter()
            .flat_map(|stream| {
                [
                    "application.name",
                    "application.id",
                    "application.process.binary",
                ]
                .into_iter()
                .filter_map(|key| stream.properties.get(key).and_then(|value| value.as_str()))
                .map(application_key)
                .filter(|key| !key.is_empty())
            })
            .collect::<BTreeSet<_>>();
        let mut stream_ids = streams.iter().map(|stream| stream.id).collect::<Vec<_>>();
        stream_ids.sort_unstable();
        let native_buses = native_buses.iter().cloned().collect::<BTreeSet<_>>();
        if (keys == self.keys)
            && (stream_ids == self.stream_ids)
            && (native_buses == self.native_buses)
        {
            return false;
        }
        if native_buses != self.native_buses {
            self.alias_checked.clear();
        }
        self.keys = keys;
        self.stream_ids = stream_ids;
        self.native_buses = native_buses;
        self.checked.clear();
        self.reconcile(sender);
        true
    }

    fn reconcile(&mut self, sender: &dyn Sender) {
        let normal = self.normal_owners();
        let remove = self
            .players
            .keys()
            .chain(self.probing.keys())
            .filter(|owner| !self.owners.contains(*owner) || normal.contains(*owner))
            .cloned()
            .collect::<HashSet<_>>();
        for owner in remove {
            self.remove_player(&owner, sender);
            self.retries.remove(&owner);
        }
    }

    fn remove_player(&mut self, owner: &str, sender: &dyn Sender) {
        if let Some(player) = self.players.remove(owner) {
            self.changed |= player.available;
            let mut message = daemon_message("RemoveMatch").append1(player_rule(owner));
            message.set_no_reply(true);
            let _ = sender.send(message);
        }
        self.probing.remove(owner);
        self.pending
            .retain(|_, pending| pending.query.owner() != Some(owner));
    }

    fn owner_changed(&mut self, name: String, owner: String, sender: &dyn Sender) {
        if name.starts_with(':') {
            if name == self.own_bus {
                return;
            }
            if !owner.is_empty() {
                self.owners.insert(name);
            } else {
                self.owners.remove(&name);
                self.credentials.remove(&name);
                self.checked.remove(&name);
                self.retried.remove(&name);
                self.retries.remove(&name);
                if !self.listed {
                    self.initial_closed.insert(name.clone());
                }
                self.remove_player(&name, sender);
            }
        } else if name.starts_with(MPRIS_PREFIX) {
            *self.alias_revisions.entry(name.clone()).or_default() += 1;
            self.alias_checked.insert(name.clone());
            if owner.is_empty() {
                self.aliases.remove(&name);
            } else {
                self.aliases.insert(name, owner);
            }
        } else {
            return;
        }
        self.reconcile(sender);
    }

    fn request(&mut self, sender: &dyn Sender, message: Message, query: Query, now: Instant) {
        match sender.send(message) {
            Ok(serial) => {
                self.pending.insert(
                    serial,
                    Pending {
                        query,
                        deadline: now + REQUEST_TIMEOUT,
                    },
                );
            }
            Err(()) => self.complete(
                sender,
                query,
                Err(BusFailure {
                    name: "Disconnected".into(),
                }),
                now,
            ),
        }
    }

    fn pump(&mut self, sender: &dyn Sender, now: Instant) {
        if self.stopped {
            return;
        }
        if !self.initial_started {
            self.initial_started = true;
            self.request(sender, daemon_message("ListNames"), Query::ListNames, now);
        }
        let due = self
            .retries
            .iter()
            .filter(|(_, at)| **at <= now)
            .map(|(owner, _)| owner.clone())
            .collect::<Vec<_>>();
        for owner in due {
            self.retries.remove(&owner);
            self.checked.remove(&owner);
        }

        let aliases = self
            .native_buses
            .iter()
            .filter(|name| {
                !name.starts_with(':')
                    && !self.aliases.contains_key(*name)
                    && !self.alias_checked.contains(*name)
                    && !self.alias_pending.contains(*name)
            })
            .take(MAX_PROBES.saturating_sub(self.alias_pending.len()))
            .cloned()
            .collect::<Vec<_>>();
        for name in aliases {
            self.alias_pending.insert(name.clone());
            let revision = self.alias_revisions.get(&name).copied().unwrap_or(0);
            let message = daemon_message("GetNameOwner").append1(name.clone());
            self.request(sender, message, Query::Alias { name, revision }, now);
        }
        self.reconcile(sender);

        let mut reads = Vec::new();
        for (owner, player) in &mut self.players {
            if !player.available {
                continue;
            }
            if player.next_poll.is_some_and(|at| at <= now) {
                player.next_poll = Some(now + POSITION_INTERVAL);
                player.request_position();
            }
            for interface in [PlayerInterface::Root, PlayerInterface::Player] {
                let index = interface.index();
                if player.reload_requested[index] && !player.reloading[index] {
                    player.reload_requested[index] = false;
                    player.reloading[index] = true;
                    reads.push((
                        properties_message(owner, "GetAll").append1(interface.name()),
                        Query::Properties {
                            owner: owner.clone(),
                            instance: player.snapshot.instance,
                            interface,
                            baseline: player.baseline(),
                        },
                    ));
                }
            }
            if player.position_requested && player.position_read.is_none() {
                player.position_requested = false;
                player.position_read = Some(player.position_generation);
                reads.push((
                    properties_message(owner, "Get").append2(MPRIS_PLAYER, "Position"),
                    Query::Position {
                        owner: owner.clone(),
                        instance: player.snapshot.instance,
                        generation: player.position_generation,
                    },
                ));
            }
        }
        for (message, query) in reads {
            self.request(sender, message, query, now);
        }

        // Resolve native owners first: they must never receive fallback probes.
        if !self.listed || self.keys.is_empty() || !self.alias_pending.is_empty() {
            return;
        }
        let normal = self.normal_owners();
        while self.probing.len() < MAX_PROBES {
            let owner = self
                .owners
                .iter()
                .find(|owner| {
                    !normal.contains(*owner)
                        && !self.players.contains_key(*owner)
                        && !self.probing.contains_key(*owner)
                        && !self.checked.contains(*owner)
                        && !self.retries.contains_key(*owner)
                })
                .cloned();
            let Some(owner) = owner else { break };
            self.checked.insert(owner.clone());
            self.next_instance += 1;
            let instance = self.next_instance;
            self.probing.insert(owner.clone(), instance);
            if self.credentials.contains_key(&owner) {
                self.start_player(sender, owner, instance, now);
            } else {
                self.request(
                    sender,
                    daemon_message("GetConnectionUnixProcessID").append1(owner.clone()),
                    Query::Credentials { owner, instance },
                    now,
                );
            }
        }
    }

    fn current_probe(&self, owner: &str, instance: u64) -> bool {
        self.owners.contains(owner)
            && self.probing.get(owner) == Some(&instance)
            && !self.normal_owners().contains(owner)
    }

    fn start_player(&mut self, sender: &dyn Sender, owner: String, instance: u64, now: Instant) {
        if !self.current_probe(&owner, instance) {
            return;
        }
        if !self
            .credentials
            .get(&owner)
            .is_some_and(|application| !application.is_empty() && self.keys.contains(application))
        {
            self.probing.remove(&owner);
            return;
        }
        self.players
            .insert(owner.clone(), DirectPlayer::new(&owner, instance));
        self.request(
            sender,
            daemon_message("AddMatch").append1(player_rule(&owner)),
            Query::Subscribe { owner, instance },
            now,
        );
    }

    fn fail_probe(&mut self, sender: &dyn Sender, owner: &str, error: &BusFailure, now: Instant) {
        self.remove_player(owner, sender);
        if self.owners.contains(owner)
            && !self.normal_owners().contains(owner)
            && retryable_startup_error(&error.name)
            && self.retried.insert(owner.to_owned())
        {
            self.retries.insert(owner.to_owned(), now + STARTUP_RETRY);
        }
    }

    fn complete(
        &mut self,
        sender: &dyn Sender,
        query: Query,
        result: Result<Message, BusFailure>,
        now: Instant,
    ) {
        match query {
            Query::ListNames => {
                if let Ok(message) = result
                    && let Ok(names) = message.read1::<Vec<String>>()
                {
                    self.owners.extend(names.into_iter().filter(|name| {
                        name.starts_with(':')
                            && name != &self.own_bus
                            && !self.initial_closed.contains(name)
                    }));
                }
                self.initial_closed.clear();
                self.listed = true;
            }
            Query::Alias { name, revision } => {
                self.alias_pending.remove(&name);
                if self.alias_revisions.get(&name).copied().unwrap_or(0) == revision {
                    self.alias_checked.insert(name.clone());
                    if let Ok(message) = result
                        && let Ok(owner) = message.read1::<String>()
                    {
                        self.aliases.insert(name, owner);
                    }
                }
                self.reconcile(sender);
            }
            Query::Credentials { owner, instance } => {
                if !self.current_probe(&owner, instance) {
                    return;
                }
                let application = result
                    .ok()
                    .and_then(|message| message.read1::<u32>().ok())
                    .and_then(|pid| std::fs::read_link(format!("/proc/{pid}/exe")).ok())
                    .map(|path| application_key(&path.to_string_lossy()))
                    .unwrap_or_default();
                self.credentials.insert(owner.clone(), application);
                self.start_player(sender, owner, instance, now);
            }
            Query::Subscribe { owner, instance } => {
                if !self.current_probe(&owner, instance) {
                    return;
                }
                if let Err(error) = result {
                    self.fail_probe(sender, &owner, &error, now);
                    return;
                }
                for interface in [PlayerInterface::Root, PlayerInterface::Player] {
                    let Some(player) = self.players.get_mut(&owner) else {
                        break;
                    };
                    player.reloading[interface.index()] = true;
                    // Startup signals precede either baseline, including ones
                    // received while AddMatch's reply was still in flight.
                    let baseline = Baseline {
                        version: 0,
                        position_generation: 0,
                    };
                    self.request(
                        sender,
                        properties_message(&owner, "GetAll").append1(interface.name()),
                        Query::Properties {
                            owner: owner.clone(),
                            instance,
                            interface,
                            baseline,
                        },
                        now,
                    );
                }
            }
            Query::Properties {
                owner,
                instance,
                interface,
                baseline,
            } => {
                let Some(player) = self
                    .players
                    .get_mut(&owner)
                    .filter(|player| player.snapshot.instance == instance)
                else {
                    return;
                };
                let properties = result.and_then(|message| {
                    message
                        .read1::<PropMap>()
                        .map_err(|_| BusFailure::malformed())
                });
                player.reloading[interface.index()] = false;
                let properties = match properties {
                    Ok(properties) => properties,
                    Err(error) => {
                        if !player.available {
                            self.fail_probe(sender, &owner, &error, now);
                        }
                        return;
                    }
                };
                player.apply_baseline(properties, interface, baseline, now);
                player.loaded[interface.index()] = true;
                if player.loaded == [true; 2] && !player.available {
                    player.available = true;
                    self.probing.remove(&owner);
                    if !player.snapshot.position_supported {
                        player.position_requested = true;
                    }
                    player.update_poll(now);
                }
                self.changed |= player.available;
            }
            Query::Position {
                owner,
                instance,
                generation,
            } => {
                if let Some(player) = self
                    .players
                    .get_mut(&owner)
                    .filter(|player| player.snapshot.instance == instance)
                {
                    let position = result
                        .ok()
                        .and_then(|message| message.read1::<Variant<Box<dyn RefArg>>>().ok())
                        .and_then(|value| seconds(value.0.as_ref()));
                    let previous = player.snapshot.position_revision;
                    player.finish_position(generation, position);
                    self.changed |=
                        player.available && player.snapshot.position_revision != previous;
                }
            }
        }
    }

    fn message(&mut self, sender: &dyn Sender, mut message: Message, now: Instant) {
        if self.stopped {
            return;
        }
        if let Some(serial) = message.get_reply_serial() {
            if let Some(pending) = self.pending.remove(&serial) {
                let result = message.as_result().map(|_| ()).map_err(|error| BusFailure {
                    name: error.name().unwrap_or_default().to_owned(),
                });
                self.complete(sender, pending.query, result.map(|_| message), now);
            }
            return;
        }
        if message.msg_type() != MessageType::Signal {
            return;
        }
        let interface = message
            .interface()
            .map(|value| value.to_string())
            .unwrap_or_default();
        let member = message
            .member()
            .map(|value| value.to_string())
            .unwrap_or_default();
        let path = message
            .path()
            .map(|value| value.to_string())
            .unwrap_or_default();
        let owner = message
            .sender()
            .map(|value| value.to_string())
            .unwrap_or_default();
        if owner == DBUS && interface == DBUS && member == "NameOwnerChanged" && path == DBUS_PATH {
            if let Ok((name, _, owner)) = message.read3::<String, String, String>() {
                self.owner_changed(name, owner, sender);
            }
            return;
        }
        if path != MPRIS_PATH {
            return;
        }
        let Some(player) = self.players.get_mut(&owner) else {
            return;
        };
        if interface == PROPERTIES && member == "PropertiesChanged" {
            if let Ok((interface, properties, invalidated)) =
                message.read3::<String, PropMap, Vec<String>>()
                && let Some(interface) = PlayerInterface::parse(&interface)
            {
                player.signal(interface, properties, invalidated, now);
                self.changed |= player.available;
            }
        } else if interface == MPRIS_PLAYER
            && member == "Seeked"
            && let Some(value) = message.iter_init().get_refarg()
        {
            player.set_position(seconds(value.as_ref()));
            self.changed |= player.available;
        }
    }

    fn expire(&mut self, sender: &dyn Sender, now: Instant) {
        let expired = self
            .pending
            .iter()
            .filter(|(_, pending)| pending.deadline <= now)
            .map(|(serial, _)| *serial)
            .collect::<Vec<_>>();
        for serial in expired {
            if let Some(pending) = self.pending.remove(&serial) {
                self.complete(
                    sender,
                    pending.query,
                    Err(BusFailure {
                        name: "org.freedesktop.DBus.Error.NoReply".into(),
                    }),
                    now,
                );
            }
        }
    }

    fn timeout_ms(&self, now: Instant) -> i32 {
        let next = self
            .pending
            .values()
            .map(|pending| pending.deadline)
            .chain(self.retries.values().copied())
            .chain(self.players.values().filter_map(|player| player.next_poll))
            .min();
        next.map_or(-1, |at| {
            let remaining = at.saturating_duration_since(now);
            if remaining.is_zero() {
                0
            } else {
                (remaining.as_millis() + 1).min(i32::MAX as u128) as i32
            }
        })
    }

    fn dispatch(
        &mut self,
        operation: &NativeAction,
        sender: &dyn Sender,
        validate: impl FnOnce(Vec<RecoveredPlayer>) -> Result<(), String>,
    ) -> Result<(), String> {
        if self.stopped
            || !self.owners.contains(&operation.player)
            || self.normal_owners().contains(&operation.player)
        {
            return Err("The media player disappeared.".into());
        }
        validate(self.snapshots())?;
        let player = self
            .players
            .get_mut(&operation.player)
            .filter(|player| player.available)
            .ok_or("The media player disappeared.")?;
        sender
            .send(player.control(operation)?)
            .map_err(|()| "Unable to send the media operation.".to_owned())?;
        if operation.operation == "seek" {
            player.position_generation += 1;
            player.position_requested = true;
        }
        Ok(())
    }

    fn disconnected(&mut self) {
        self.stopped = true;
        self.changed |= self.players.values().any(|player| player.available);
        self.players.clear();
        self.probing.clear();
        self.pending.clear();
        self.retries.clear();
    }
}

fn recovery_worker(shared: Arc<RecoveryShared>, mut wake: UnixStream) {
    // Only this thread receives. SyncConnection::process and blocking proxy
    // calls must not compete with it for replies; senders only queue messages.
    let connection = shared.connection.as_ref();
    let channel = connection.channel();
    loop {
        if channel.read_write(Some(Duration::ZERO)).is_err() {
            break;
        }
        let timeout = {
            let mut state = shared.state();
            if state.stopped {
                break;
            }
            // Bound lock ownership under a signal flood, including during Drop.
            for _ in 0..MESSAGE_BATCH {
                let Some(message) = channel.pop_message() else {
                    break;
                };
                state.message(connection, message, Instant::now());
            }
            let now = Instant::now();
            state.expire(connection, now);
            state.pump(connection, now);
            state.timeout_ms(Instant::now())
        };
        // Flush nonblocking writes, then check libdbus's queue before sleeping.
        // Data already read into that queue need not leave the socket readable.
        if channel.read_write(Some(Duration::ZERO)).is_err() {
            break;
        }
        if let Some(message) = channel.pop_message() {
            shared.state().message(connection, message, Instant::now());
            continue;
        }
        let watch = channel.watch();
        let mut fds = [
            libc::pollfd {
                fd: watch.fd,
                events: (if watch.read { libc::POLLIN } else { 0 })
                    | (if watch.write { libc::POLLOUT } else { 0 }),
                revents: 0,
            },
            libc::pollfd {
                fd: wake.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        // A wake socket makes idle recovery truly passive, including while paused.
        if unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, timeout) } < 0 {
            if io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
                continue;
            }
            break;
        }
        if fds[1].revents & libc::POLLIN != 0 {
            let mut buffer = [0; 1024];
            for _ in 0..MESSAGE_BATCH {
                if !matches!(wake.read(&mut buffer), Ok(size) if size > 0) {
                    break;
                }
            }
        }
    }
    shared.state().disconnected();
}

#[cfg(test)]
mod recovery_tests {
    use super::*;
    use serde_json::json;
    use std::cell::{Cell, RefCell};

    #[derive(Default)]
    struct RecordingSender {
        serial: Cell<u32>,
        messages: RefCell<Vec<Message>>,
    }

    impl Sender for RecordingSender {
        fn send(&self, mut message: Message) -> Result<u32, ()> {
            let serial = self.serial.get() + 1;
            self.serial.set(serial);
            message.set_serial(serial);
            self.messages.borrow_mut().push(message);
            Ok(serial)
        }
    }

    fn properties<const N: usize>(values: [(&str, Box<dyn RefArg>); N]) -> PropMap {
        values
            .into_iter()
            .map(|(key, value)| (key.into(), Variant(value)))
            .collect()
    }

    fn metadata(track: &str, title: &str) -> PropMap {
        properties([
            (
                "mpris:trackid",
                Box::new(dbus::Path::new(track.to_owned()).unwrap()),
            ),
            ("xesam:title", Box::new(title.to_owned())),
            (
                "xesam:artist",
                Box::new(vec!["One".to_owned(), "Two".to_owned()]),
            ),
            ("xesam:album", Box::new("Album".to_owned())),
            ("xesam:url", Box::new("file:///track.ogg".to_owned())),
            ("mpris:artUrl", Box::new("file:///cover.png".to_owned())),
            ("mpris:length", Box::new(200_000_000_f64)),
            ("mprisence:browser", Box::new(true)),
        ])
    }

    fn ready_player(owner: &str) -> DirectPlayer {
        let mut player = DirectPlayer::new(owner, 7);
        player.available = true;
        player.apply_properties(
            properties([
                ("Identity", Box::new("Haruna".to_owned())),
                ("Metadata", Box::new(metadata("/track/one", "First"))),
                ("Position", Box::new(10_000_000_i64)),
                ("PlaybackStatus", Box::new("Playing".to_owned())),
                ("CanPlay", Box::new(true)),
                ("CanPause", Box::new(false)),
                ("CanSeek", Box::new(true)),
                ("CanGoNext", Box::new(true)),
                ("CanGoPrevious", Box::new(true)),
                ("Rate", Box::new(1.25_f64)),
            ]),
            false,
            false,
            Instant::now(),
        );
        player
    }

    fn stream(application: &str) -> Stream {
        Stream {
            id: 42,
            properties: serde_json::from_value(json!({
                "application.process.binary": application,
            }))
            .unwrap(),
            ..Stream::default()
        }
    }

    fn state_with_player() -> RecoveryState {
        RecoveryState {
            listed: true,
            initial_started: true,
            owners: BTreeSet::from([":1.20".into()]),
            players: BTreeMap::from([(":1.20".into(), ready_player(":1.20"))]),
            ..RecoveryState::default()
        }
    }

    fn operation(kind: &str, value: Option<f64>) -> NativeAction {
        NativeAction {
            player: ":1.20".into(),
            operation: kind.into(),
            value,
        }
    }

    #[test]
    fn recovery_is_send_and_sync() {
        fn assert_traits<T: Send + Sync>() {}
        assert_traits::<Recovery>();
        assert_traits::<RecoveredPlayer>();
    }

    #[test]
    fn application_keys_match_the_native_stream_convention() {
        for value in [
            "Haruna",
            "org.kde.haruna",
            "org.kde.haruna.desktop",
            "/nix/store/package/bin/.haruna-wrapped",
            "C:\\bin\\haruna",
        ] {
            assert_eq!(application_key(value), "haruna", "{value}");
        }
        assert_eq!(application_key(""), "");
        let sender = RecordingSender::default();
        let mut state = RecoveryState::default();
        for key in [
            "application.name",
            "application.id",
            "application.process.binary",
        ] {
            let input = Stream {
                id: 1,
                properties: serde_json::Map::from_iter([(
                    key.to_owned(),
                    json!("org.kde.haruna.desktop"),
                )]),
                ..Stream::default()
            };
            state.update(&[input], &[], &sender);
            assert_eq!(state.keys, BTreeSet::from(["haruna".into()]));
        }
    }

    #[test]
    fn variant_metadata_and_integer_or_double_microseconds_are_preserved() {
        let metadata = metadata("/track/one", "Title");
        let value = refarg_json(&Variant(Variant(metadata)));
        assert_eq!(value["mpris:trackid"], "/track/one");
        assert_eq!(value["xesam:artist"], json!(["One", "Two"]));
        assert_eq!(value["mpris:length"], 200_000_000_f64);
        assert_eq!(value["mprisence:browser"], true);
        assert_eq!(refarg_json(&u64::MAX), json!(u64::MAX));
        assert_eq!(refarg_json(&f64::NAN), serde_json::Value::Null);
        assert_eq!(seconds(&Variant(1_250_000_i64)), Some(1.25));
        assert_eq!(seconds(&Variant(1_250_000_f64)), Some(1.25));
        for value in [&true as &dyn RefArg, &-1_i64, &f64::INFINITY, &f64::NAN] {
            assert_eq!(seconds(value), None);
        }
        let player = ready_player(":1.20");
        assert_eq!(player.snapshot.player.artist, "One, Two");
        assert_eq!(player.snapshot.player.length, Some(200.0));
        assert_eq!(player.snapshot.player.browser_hint, true);
        assert_eq!(player.snapshot.rate, 1.25);
    }

    #[test]
    fn initial_and_reload_baselines_never_roll_back_newer_signals() {
        let now = Instant::now();
        let mut player = DirectPlayer::new(":1.20", 1);
        let baseline = player.baseline();
        player.signal(
            PlayerInterface::Root,
            properties([("Identity", Box::new("New identity".to_owned()))]),
            vec![],
            now,
        );
        player.signal(
            PlayerInterface::Player,
            properties([
                ("Metadata", Box::new(metadata("/track/two", "New track"))),
                ("PlaybackStatus", Box::new("Paused".to_owned())),
                ("CanSeek", Box::new(false)),
                ("Position", Box::new(42_000_000_f64)),
            ]),
            vec![],
            now,
        );
        player.apply_baseline(
            properties([("Identity", Box::new("Old identity".to_owned()))]),
            PlayerInterface::Root,
            baseline,
            now,
        );
        player.apply_baseline(
            properties([
                ("Metadata", Box::new(metadata("/track/one", "Old track"))),
                ("PlaybackStatus", Box::new("Playing".to_owned())),
                ("CanSeek", Box::new(true)),
                ("Position", Box::new(1_000_000_i64)),
            ]),
            PlayerInterface::Player,
            baseline,
            now,
        );
        assert_eq!(player.snapshot.player.identity, "New identity");
        assert_eq!(player.snapshot.player.title, "New track");
        assert_eq!(player.snapshot.player.position, 42.0);
        assert!(!player.snapshot.player.playing);
        assert!(!player.snapshot.player.can_seek);

        let reload = player.baseline();
        player.signal(
            PlayerInterface::Player,
            properties([("Metadata", Box::new(metadata("/track/three", "Newest")))]),
            vec![],
            now,
        );
        player.apply_baseline(
            properties([
                ("Metadata", Box::new(metadata("/track/two", "New track"))),
                ("Position", Box::new(60_000_000_i64)),
            ]),
            PlayerInterface::Player,
            reload,
            now,
        );
        assert_eq!(player.snapshot.player.title, "Newest");
        assert!(!player.snapshot.position_supported);
        assert!(player.position_requested);
    }

    #[test]
    fn seeked_during_startup_survives_both_initial_snapshots() {
        let mut player = DirectPlayer::new(":1.20", 1);
        player.set_position(Some(37.5));
        player.apply_baseline(
            properties([
                ("Metadata", Box::new(metadata("/track/one", "First"))),
                ("Position", Box::new(1_000_000_i64)),
            ]),
            PlayerInterface::Player,
            Baseline {
                version: 0,
                position_generation: 0,
            },
            Instant::now(),
        );
        assert_eq!(player.snapshot.player.position, 37.5);
        assert!(player.snapshot.position_supported);
    }

    #[test]
    fn stale_track_and_seek_reads_cannot_replace_newer_samples() {
        let mut player = ready_player(":1.20");
        let generation = player.position_generation;
        player.position_read = Some(generation);
        player.signal(
            PlayerInterface::Player,
            properties([("Metadata", Box::new(metadata("/track/two", "Second")))]),
            vec![],
            Instant::now(),
        );
        player.finish_position(generation, Some(100.0));
        assert!(!player.snapshot.position_supported);
        assert!(player.position_requested);

        let generation = player.position_generation;
        player.position_read = Some(generation);
        player.set_position(Some(15.0));
        let revision = player.snapshot.position_revision;
        player.finish_position(generation, Some(2.0));
        assert_eq!(player.snapshot.player.position, 15.0);
        assert_eq!(player.snapshot.position_revision, revision);
        assert!(!player.position_requested);
        player.finish_position(player.position_generation, None);
        assert_eq!(player.snapshot.player.position, 15.0);
        assert_eq!(player.snapshot.position_revision, revision);
        player.finish_position(player.position_generation, Some(15.0));
        assert!(player.snapshot.position_revision > revision);
    }

    #[test]
    fn invalidated_position_is_refreshed_while_paused_and_seeked_accepts_both_wire_types() {
        let now = Instant::now();
        let sender = RecordingSender::default();
        let mut state = state_with_player();
        let player = state.players.get_mut(":1.20").unwrap();
        player.signal(
            PlayerInterface::Player,
            properties([("PlaybackStatus", Box::new("Paused".to_owned()))]),
            vec![],
            now,
        );
        player.position_requested = false;
        player.signal(
            PlayerInterface::Player,
            PropMap::new(),
            vec!["Position".into()],
            now,
        );
        assert!(!player.snapshot.position_supported);
        assert!(player.position_requested);
        assert!(player.reload_requested[PlayerInterface::Player.index()]);
        assert!(player.next_poll.is_none());
        let baseline = player.baseline();
        for (value, expected) in [
            (Box::new(13_000_000_i64) as Box<dyn RefArg>, 13.0),
            (Box::new(14_500_000_f64), 14.5),
        ] {
            let mut signal = Message::new_signal(MPRIS_PATH, MPRIS_PLAYER, "Seeked").unwrap();
            value.append(&mut dbus::arg::IterAppend::new(&mut signal));
            signal.set_sender(Some(":1.20".into()));
            state.message(&sender, signal, now);
            assert_eq!(state.players[":1.20"].snapshot.player.position, expected);
        }
        let player = state.players.get_mut(":1.20").unwrap();
        player.apply_baseline(
            properties([("Position", Box::new(1_000_000_i64))]),
            PlayerInterface::Player,
            baseline,
            now,
        );
        assert_eq!(player.snapshot.player.position, 14.5);
        assert!(player.snapshot.position_supported);
    }

    #[test]
    fn dispatch_validation_sees_latest_state_and_rejection_sends_nothing() {
        let sender = RecordingSender::default();
        let mut state = state_with_player();
        let observed = state.snapshots()[0].clone();
        state.players.get_mut(":1.20").unwrap().signal(
            PlayerInterface::Player,
            properties([("Metadata", Box::new(metadata("/track/two", "Second")))]),
            vec![],
            Instant::now(),
        );
        let generation = state.players[":1.20"].position_generation;
        let result = state.dispatch(&operation("seek", Some(12.0)), &sender, |latest| {
            assert_eq!(latest[0].track_id, "/track/two");
            if latest[0].track_id != observed.track_id {
                Err("The observed track changed.".into())
            } else {
                Ok(())
            }
        });
        assert_eq!(result.unwrap_err(), "The observed track changed.");
        assert!(sender.messages.borrow().is_empty());
        assert_eq!(state.players[":1.20"].position_generation, generation);
    }

    #[test]
    fn commands_are_ordered_reply_free_and_seek_is_not_optimistic() {
        let sender = RecordingSender::default();
        let mut state = state_with_player();
        let baseline = state.players[":1.20"].baseline();
        let revision = state.players[":1.20"].snapshot.position_revision;
        for kind in ["play", "pause", "next", "previous"] {
            state
                .dispatch(&operation(kind, None), &sender, |_| Ok(()))
                .unwrap();
        }
        state
            .dispatch(&operation("seek", Some(12.345678)), &sender, |_| Ok(()))
            .unwrap();
        let messages = sender.messages.borrow();
        assert_eq!(
            messages
                .iter()
                .map(|m| m.member().unwrap().to_string())
                .collect::<Vec<_>>(),
            ["Play", "Pause", "Next", "Previous", "SetPosition"]
        );
        assert!(messages.iter().all(|m| m.get_no_reply()));
        assert!(
            messages
                .iter()
                .all(|m| m.destination().as_deref() == Some(":1.20"))
        );
        let (track, microseconds) = messages[4].read2::<dbus::Path<'_>, i64>().unwrap();
        assert_eq!(&*track, "/track/one");
        assert_eq!(microseconds, 12_345_678);
        let player = state.players.get_mut(":1.20").unwrap();
        assert_eq!(player.snapshot.player.position, 10.0);
        assert_eq!(player.snapshot.position_revision, revision);
        assert!(player.position_requested);
        player.apply_baseline(
            properties([
                ("Metadata", Box::new(metadata("/track/one", "First"))),
                ("Position", Box::new(9_000_000_i64)),
            ]),
            PlayerInterface::Player,
            baseline,
            Instant::now(),
        );
        assert_eq!(player.snapshot.player.position, 10.0);
        player.finish_position(baseline.position_generation, Some(9.0));
        assert_eq!(player.snapshot.player.position, 10.0);
        assert!(player.position_requested);
    }

    #[test]
    fn seek_rejects_invalid_paths_and_nonfinite_or_overflowing_positions() {
        let mut player = ready_player(":1.20");
        for value in [
            None,
            Some(-1.0),
            Some(f64::NAN),
            Some(f64::INFINITY),
            Some(f64::MAX),
        ] {
            assert!(player.control(&operation("seek", value)).is_err());
        }
        for track in [
            "",
            "not/a/path",
            "/org/mpris/MediaPlayer2/TrackList/NoTrack",
        ] {
            player.snapshot.track_id = track.into();
            assert!(player.control(&operation("seek", Some(1.0))).is_err());
        }
    }

    #[test]
    fn position_poll_is_playing_only_and_never_overlaps_a_read() {
        let sender = RecordingSender::default();
        let mut state = state_with_player();
        let now = Instant::now();
        state.players.get_mut(":1.20").unwrap().next_poll = Some(now);
        state.pump(&sender, now);
        assert_eq!(state.pending.len(), 1);
        state.pump(&sender, now + POSITION_INTERVAL);
        state.players.get_mut(":1.20").unwrap().request_position();
        state.pump(&sender, now + POSITION_INTERVAL);
        assert_eq!(state.pending.len(), 1);
        assert_eq!(sender.messages.borrow().len(), 1);
        let player = state.players.get_mut(":1.20").unwrap();
        player.signal(
            PlayerInterface::Player,
            properties([("PlaybackStatus", Box::new("Paused".to_owned()))]),
            vec![],
            now,
        );
        assert!(player.next_poll.is_none());
        let message = sender.messages.borrow_mut().remove(0);
        state.message(
            &sender,
            message.method_return().append1(Variant(12_000_000_f64)),
            now,
        );
        state.pump(&sender, now);
        // Crossing Playing -> Paused invalidates the earlier read and asks once.
        assert_eq!(sender.messages.borrow().len(), 1);
        let message = sender.messages.borrow_mut().remove(0);
        state.message(
            &sender,
            message.method_return().append1(Variant(12_000_000_i64)),
            now,
        );
        state.pump(&sender, now + POSITION_INTERVAL * 10);
        assert!(sender.messages.borrow().is_empty());
        assert_eq!(state.players[":1.20"].snapshot.player.position, 12.0);
        assert!(state.pending.is_empty());
        assert_eq!(state.timeout_ms(now), -1);
    }

    #[test]
    fn list_names_is_once_credentials_are_bounded_and_only_matching_apps_are_probed() {
        let sender = RecordingSender::default();
        let now = Instant::now();
        let mut state = RecoveryState {
            own_bus: ":1.0".into(),
            ..RecoveryState::default()
        };
        state.update(&[stream("haruna")], &[], &sender);
        state.pump(&sender, now);
        let list = sender.messages.borrow_mut().remove(0);
        state.owner_changed(":1.99".into(), String::new(), &sender);
        let names = (1..=9)
            .map(|id| format!(":1.{id}"))
            .chain([":1.0".into(), ":1.99".into()])
            .collect::<Vec<_>>();
        state.message(&sender, list.method_return().append1(names), now);
        state.pump(&sender, now);
        assert!(!state.owners.contains(":1.99"));
        assert!(!state.owners.contains(":1.0"));
        assert_eq!(state.probing.len(), MAX_PROBES);
        assert_eq!(state.pending.len(), MAX_PROBES);
        assert!(sender.messages.borrow().iter().all(|message| {
            message.destination().as_deref() == Some(DBUS)
                && message.member().as_deref() == Some("GetConnectionUnixProcessID")
        }));

        let sender = RecordingSender::default();
        let mut state = RecoveryState {
            listed: true,
            initial_started: true,
            owners: BTreeSet::from([":1.1".into(), ":1.2".into(), ":1.3".into()]),
            credentials: HashMap::from([
                (":1.1".into(), "haruna".into()),
                (":1.2".into(), "unrelated".into()),
                (":1.3".into(), String::new()),
            ]),
            ..RecoveryState::default()
        };
        state.update(&[stream("haruna")], &[], &sender);
        state.pump(&sender, now);
        assert_eq!(state.players.len(), 1);
        assert!(state.players.contains_key(":1.1"));
        let subscription = sender.messages.borrow_mut().remove(0);
        assert_eq!(subscription.member().as_deref(), Some("AddMatch"));
        state.message(&sender, subscription.method_return(), now);
        let messages = sender.messages.borrow();
        assert_eq!(messages.len(), 2);
        assert!(
            messages
                .iter()
                .all(|message| message.destination().as_deref() == Some(":1.1")
                    && message.member().as_deref() == Some("GetAll"))
        );
        drop(messages);
        state.expire(&sender, now + REQUEST_TIMEOUT);
        sender.messages.borrow_mut().clear();
        state.pump(&sender, now + REQUEST_TIMEOUT * 20);
        assert!(sender.messages.borrow().is_empty());
        assert_eq!(state.timeout_ms(now + REQUEST_TIMEOUT * 20), -1);
    }

    #[test]
    fn verified_private_players_survive_stream_loss_until_owner_exit_or_native_takeover() {
        let sender = RecordingSender::default();
        let mut state = state_with_player();
        state.update(&[stream("haruna")], &[], &sender);
        let instance = state.players[":1.20"].snapshot.instance;
        state.update(&[], &[], &sender);
        assert_eq!(state.snapshots()[0].instance, instance);
        let normal = "org.mpris.MediaPlayer2.haruna".to_owned();
        state.aliases.insert(normal.clone(), ":1.20".into());
        state.update(&[], &[normal], &sender);
        assert!(state.snapshots().is_empty());
        assert!(state.changed);
        assert_eq!(
            sender.messages.borrow()[0].member().as_deref(),
            Some("RemoveMatch")
        );

        let mut state = state_with_player();
        state.credentials.insert(":1.20".into(), "haruna".into());
        state.owner_changed(":1.20".into(), String::new(), &sender);
        assert!(state.snapshots().is_empty());
        assert!(!state.credentials.contains_key(":1.20"));
    }

    #[test]
    fn native_alias_resolution_precedes_application_probes_and_rejects_stale_replies() {
        let sender = RecordingSender::default();
        let now = Instant::now();
        let alias = "org.mpris.MediaPlayer2.haruna".to_owned();
        let mut state = state_with_player();
        state.update(&[stream("haruna")], std::slice::from_ref(&alias), &sender);
        state.pump(&sender, now);
        let query = sender.messages.borrow_mut().remove(0);
        assert_eq!(query.member().as_deref(), Some("GetNameOwner"));
        assert!(state.probing.is_empty());
        state.owner_changed(alias, ":1.21".into(), &sender);
        state.message(&sender, query.method_return().append1(":1.20"), now);
        assert!(!state.snapshots().is_empty());
        state.update(&[], &[":1.20".into()], &sender);
        assert!(state.snapshots().is_empty());
    }

    #[test]
    fn startup_retries_once_after_250ms_only_for_missing_exports() {
        let sender = RecordingSender::default();
        let now = Instant::now();
        let mut state = RecoveryState {
            listed: true,
            initial_started: true,
            owners: BTreeSet::from([":1.20".into()]),
            credentials: HashMap::from([(":1.20".into(), "haruna".into())]),
            ..RecoveryState::default()
        };
        state.update(&[stream("haruna")], &[], &sender);
        state.pump(&sender, now);
        let first_instance = state.players[":1.20"].snapshot.instance;
        let error = BusFailure {
            name: "org.freedesktop.DBus.Error.UnknownObject".into(),
        };
        state.fail_probe(&sender, ":1.20", &error, now);
        state.pump(&sender, now + Duration::from_millis(249));
        assert!(state.players.is_empty());
        state.pump(&sender, now + STARTUP_RETRY);
        assert!(state.players[":1.20"].snapshot.instance > first_instance);
        state.fail_probe(&sender, ":1.20", &error, now + STARTUP_RETRY);
        state.pump(&sender, now + STARTUP_RETRY * 10);
        assert!(state.players.is_empty());
        assert!(state.retries.is_empty());
        for name in ["UnknownObject", "UnknownInterface", "UnknownMethod"] {
            assert!(retryable_startup_error(&format!(
                "org.freedesktop.DBus.Error.{name}"
            )));
        }
        for name in [
            "NoReply",
            "AccessDenied",
            "ServiceUnknown",
            "NameHasNoOwner",
        ] {
            assert!(!retryable_startup_error(&format!(
                "org.freedesktop.DBus.Error.{name}"
            )));
        }
    }

    struct LiveExport {
        connection: Arc<SyncConnection>,
        stopped: Arc<std::sync::atomic::AtomicBool>,
        stalled: Arc<std::sync::atomic::AtomicBool>,
        reads: Arc<std::sync::atomic::AtomicUsize>,
        commands: Arc<Mutex<Vec<String>>>,
        worker: Option<JoinHandle<()>>,
    }

    impl LiveExport {
        fn new() -> Self {
            use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
            let connection = Arc::new(SyncConnection::new_session().unwrap());
            let stopped = Arc::new(AtomicBool::new(false));
            let stalled = Arc::new(AtomicBool::new(false));
            let reads = Arc::new(AtomicUsize::new(0));
            let commands = Arc::new(Mutex::new(Vec::new()));
            let worker = {
                let connection = connection.clone();
                let stopped = stopped.clone();
                let stalled = stalled.clone();
                let reads = reads.clone();
                let commands = commands.clone();
                thread::spawn(move || {
                    let mut position = 10.5;
                    while !stopped.load(Ordering::Relaxed) {
                        if connection
                            .channel()
                            .read_write(Some(Duration::from_millis(10)))
                            .is_err()
                        {
                            break;
                        }
                        while let Some(message) = connection.channel().pop_message() {
                            if message.msg_type() != MessageType::MethodCall {
                                continue;
                            }
                            let member = message.member().unwrap().to_string();
                            let interface = message.interface().unwrap().to_string();
                            let reply = if interface == PROPERTIES && member == "GetAll" {
                                let interface = message.read1::<String>().unwrap();
                                let props = if interface == MPRIS_ROOT {
                                    properties([
                                        ("Identity", Box::new("Haruna".to_owned())),
                                        ("DesktopEntry", Box::new("org.kde.haruna".to_owned())),
                                    ])
                                } else {
                                    properties([
                                        (
                                            "Metadata",
                                            Box::new(metadata("/track/one", "Private export")),
                                        ),
                                        ("PlaybackStatus", Box::new("Paused".to_owned())),
                                        ("Position", Box::new(position * 1e6)),
                                        ("Rate", Box::new(1.25_f64)),
                                        ("CanPlay", Box::new(true)),
                                        ("CanPause", Box::new(false)),
                                        ("CanSeek", Box::new(true)),
                                        ("CanGoNext", Box::new(true)),
                                    ])
                                };
                                Some(message.method_return().append1(props))
                            } else if interface == PROPERTIES && member == "Get" {
                                reads.fetch_add(1, Ordering::Relaxed);
                                (!stalled.load(Ordering::Relaxed)).then(|| {
                                    message.method_return().append1(Variant(position * 1e6))
                                })
                            } else if interface == MPRIS_PLAYER {
                                assert!(message.get_no_reply());
                                commands.lock().unwrap().push(member.clone());
                                if member == "SetPosition" {
                                    let (track, micros) =
                                        message.read2::<dbus::Path<'_>, i64>().unwrap();
                                    assert_eq!(&*track, "/track/one");
                                    position = micros as f64 / 1e6;
                                    connection
                                        .send(
                                            Message::new_signal(MPRIS_PATH, MPRIS_PLAYER, "Seeked")
                                                .unwrap()
                                                .append1(position * 1e6),
                                        )
                                        .unwrap();
                                }
                                None
                            } else {
                                dbus::channel::default_reply(&message)
                            };
                            if let Some(reply) = reply {
                                connection.send(reply).unwrap();
                            }
                        }
                    }
                })
            };
            Self {
                connection,
                stopped,
                stalled,
                reads,
                commands,
                worker: Some(worker),
            }
        }

        fn owner(&self) -> String {
            self.connection.unique_name().to_string()
        }

        fn status(&self, status: &str) {
            self.connection
                .send(
                    Message::new_signal(MPRIS_PATH, PROPERTIES, "PropertiesChanged")
                        .unwrap()
                        .append3(
                            MPRIS_PLAYER,
                            properties([("PlaybackStatus", Box::new(status.to_owned()))]),
                            Vec::<String>::new(),
                        ),
                )
                .unwrap();
        }
    }

    impl Drop for LiveExport {
        fn drop(&mut self) {
            self.stopped
                .store(true, std::sync::atomic::Ordering::Relaxed);
            let _ = self.worker.take().unwrap().join();
        }
    }

    fn wait_until(mut predicate: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(4);
        while !predicate() {
            assert!(
                Instant::now() < deadline,
                "timed out waiting for private MPRIS recovery"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    #[ignore = "uses a session bus; run with dbus-run-session -- cargo test --lib private_export -- --ignored"]
    fn private_export_round_trips_real_dbus_and_shutdown_does_not_wait_for_replies() {
        use std::sync::atomic::Ordering;
        // No RequestName: like Haruna, the object exists only on a unique owner.
        let export = LiveExport::new();
        let owner = export.owner();
        let application = std::env::current_exe().unwrap();
        let streams = [stream(application.to_str().unwrap())];
        let recovery = Recovery::new().unwrap();
        recovery.update(&streams, &[]);
        wait_until(|| {
            recovery
                .players()
                .iter()
                .any(|player| player.player.bus == owner)
        });
        let snapshot = recovery
            .players()
            .into_iter()
            .find(|p| p.player.bus == owner)
            .unwrap();
        assert_eq!(snapshot.player.title, "Private export");
        assert_eq!(snapshot.player.artist, "One, Two");
        assert_eq!(snapshot.player.position, 10.5);
        assert_eq!(snapshot.player.length, Some(200.0));
        assert_eq!(snapshot.rate, 1.25);
        assert!(snapshot.position_supported);
        let instance = snapshot.instance;
        recovery.update(&[], &[]);
        assert_eq!(recovery.players()[0].instance, instance);
        drop(export);
        wait_until(|| recovery.players().is_empty());

        // A later connection is discovered through NameOwnerChanged, not polling.
        recovery.update(&streams, &[]);
        let export = LiveExport::new();
        let owner = export.owner();
        wait_until(|| {
            recovery
                .players()
                .iter()
                .any(|player| player.player.bus == owner)
        });
        assert!(recovery.players()[0].instance > instance);
        export.status("Playing");
        wait_until(|| export.reads.load(Ordering::Relaxed) >= 1);
        export.status("Paused");
        wait_until(|| {
            let state = recovery.shared.state();
            state.players.get(&owner).is_some_and(|player| {
                !player.snapshot.player.playing
                    && player.position_read.is_none()
                    && !player.position_requested
            })
        });
        let reads = export.reads.load(Ordering::Relaxed);
        thread::sleep(Duration::from_millis(1100));
        assert_eq!(export.reads.load(Ordering::Relaxed), reads);

        export.stalled.store(true, Ordering::Relaxed);
        recovery.refresh_position(&owner);
        wait_until(|| export.reads.load(Ordering::Relaxed) > reads);
        let started = Instant::now();
        for (kind, value) in [("pause", None), ("seek", Some(12.345678)), ("next", None)] {
            recovery
                .dispatch(
                    &NativeAction {
                        player: owner.clone(),
                        operation: kind.into(),
                        value,
                    },
                    |_| Ok(()),
                )
                .unwrap();
        }
        recovery.update(&streams, &[]);
        recovery.take_changed();
        recovery.players();
        assert!(
            started.elapsed() < Duration::from_millis(250),
            "main-thread recovery calls waited for a reply"
        );
        wait_until(|| export.commands.lock().unwrap().len() == 3);
        assert_eq!(
            *export.commands.lock().unwrap(),
            ["Pause", "SetPosition", "Next"]
        );
        wait_until(|| {
            recovery.players().iter().any(|player| {
                player.player.bus == owner && (player.player.position - 12.345678).abs() < 1e-9
            })
        });
        let started = Instant::now();
        drop(recovery);
        assert!(
            started.elapsed() < Duration::from_millis(500),
            "shutdown waited for a missing D-Bus reply"
        );
    }
}

#[cfg(test)]
media_unit_tests!(playback);
