//! Media policy and the resident media service.
//!
//! [`analyze`] and the `model` command ([`serve`]) retain their stateless public
//! protocol. The `service` command ([`serve_service`]) owns selection, timelines,
//! bounded user intents, and guarded native action transactions. Its frontend
//! only captures native objects, compares opaque guards, and dispatches setters.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::io::{self, BufRead, BufWriter, Write};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::credit::http_authority;
use crate::playback::{NativePlayback, known_length, same_playback, source_name};

/// Maximum request-line size in bytes, excluding the terminating LF.
pub const MAX_REQUEST_BYTES: usize = 1024 * 1024;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Request {
    pub request_id: u64,
    #[serde(default)]
    pub selected_player: String,
    pub players: Vec<Player>,
    pub streams: Vec<Stream>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<Action>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Player {
    pub bus: String,
    pub identity: String,
    pub desktop_entry: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub url: String,
    pub art_url: String,
    /// Null means absent. Boolean bridge hints mark a browser player but do not
    /// identify which browser owns its audio stream.
    #[serde(default)]
    pub browser_hint: Value,
    pub playing: bool,
    pub stopped: bool,
    /// Confirmed position in seconds for matching (never a display estimate).
    /// Use zero when unsupported or unknown; never send a negative sentinel.
    pub position: f64,
    /// Track length in seconds; absent, nonfinite or nonpositive means unknown.
    #[serde(default)]
    pub length: Option<f64>,
    #[serde(default)]
    pub can_play: bool,
    #[serde(default)]
    pub can_pause: bool,
    pub can_seek: bool,
    pub can_next: bool,
    pub can_previous: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Stream {
    /// Snapshot-local native audio ID for volume control; zero is not yet bound.
    pub id: u32,
    pub name: String,
    pub nickname: String,
    pub ready: bool,
    pub properties: Map<String, Value>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Action {
    pub kind: String,
    /// Empty uses the selected group. An explicit bus must still belong to a
    /// current group; it never falls back to a different source.
    #[serde(default)]
    pub player: String,
    #[serde(default)]
    pub position: f64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Response {
    pub request_id: u64,
    pub groups: Vec<Group>,
    pub active_player: String,
    pub actions: Vec<NativeAction>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Group {
    pub title: String,
    pub primary: String,
    pub players: Vec<String>,
    pub display_name: String,
    /// Live native stream ID for volume; null when ownership is ambiguous or
    /// details are not ready. The frontend must resolve it in its fresh streams.
    pub stream_id: Option<u32>,
    pub stream_reason: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeAction {
    pub player: String,
    /// One of `play`, `pause`, `next`, `previous`, or `seek`.
    pub operation: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<f64>,
}

impl Response {
    fn failure(request_id: u64, error: &str) -> Self {
        Self {
            request_id,
            error: Some(error.to_owned()),
            ..Self::default()
        }
    }
}

/// Analyze one fresh snapshot without side effects or retained state.
///
/// Duplicate matching retains the original panel's conservative metadata and
/// five-second position checks, comparing only against a playing-first keeper.
/// Cards and their members retain snapshot order; only after grouping does a
/// playing-first, capability/length-ranked representative supply display metadata.
/// Volume matching uses trimmed track evidence. Invalid positions
/// are rejected, not replaced with zero or treated as unknown merge evidence.
/// Next/previous address only the group's primary, gated by its capability.
pub fn analyze(request: &Request) -> Response {
    if request
        .players
        .iter()
        .any(|p| !p.position.is_finite() || p.position < 0.0)
    {
        return Response::failure(
            request.request_id,
            "Player positions must be finite and nonnegative.",
        );
    }
    if request
        .action
        .as_ref()
        .is_some_and(|action| !action.position.is_finite())
    {
        return Response::failure(request.request_id, "Action position must be finite.");
    }

    let groups = group_players(&request.players);
    let matches = match_groups(&groups, &request.streams);
    let active = groups
        .iter()
        .position(|group| group.contains(&request.selected_player))
        .or_else(|| groups.iter().position(|group| group.primary.playing))
        .or_else(|| (!groups.is_empty()).then_some(0));
    let mut response = Response {
        request_id: request.request_id,
        active_player: active
            .map(|index| groups[index].primary.bus.clone())
            .unwrap_or_default(),
        groups: groups
            .iter()
            .zip(matches)
            .map(|(group, matched)| Group {
                title: group.primary.title.clone(),
                primary: group.primary.bus.clone(),
                players: group.players.iter().map(|p| p.bus.clone()).collect(),
                display_name: group.display_name(),
                stream_id: matched.node.map(|node| node.id),
                stream_reason: matched.reason.to_owned(),
            })
            .collect(),
        ..Response::default()
    };
    if let Some(action) = &request.action {
        match plan_action(request, action, &groups, active) {
            Ok(actions) => response.actions = actions,
            Err(error) => response.error = Some(error.to_owned()),
        }
    }
    response
}

/// Serve line-delimited JSON until EOF or an I/O failure. Invalid, oversized,
/// and non-UTF-8 lines produce a safe error with requestId 0; the next line is
/// still served. Every response is explicitly flushed for a resident caller.
pub fn serve() -> crate::AppResult<()> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    serve_io(stdin.lock(), BufWriter::new(stdout.lock()))
}

fn serve_io(mut input: impl BufRead, mut output: impl Write) -> crate::AppResult<()> {
    let mut bytes = Vec::new();
    loop {
        let response = match read_line(&mut input, &mut bytes)? {
            Line::Eof => return Ok(()),
            Line::TooLong => Response::failure(0, "Request line exceeds the 1 MiB limit."),
            Line::Data => match serde_json::from_slice::<Request>(&bytes) {
                Ok(request) => analyze(&request),
                // Serde data errors can quote arbitrary input. Report only the
                // location, never titles, URLs, credentials, or control bytes.
                Err(error) => Response::failure(
                    0,
                    &format!(
                        "Invalid JSON request at line {}, column {}.",
                        error.line(),
                        error.column()
                    ),
                ),
            },
        };
        serde_json::to_writer(&mut output, &response)?;
        output.write_all(b"\n")?;
        output.flush()?;
    }
}

enum Line {
    Eof,
    Data,
    TooLong,
}

/// Drain oversized lines without ever buffering the entire rejected payload.
fn read_line(input: &mut impl BufRead, bytes: &mut Vec<u8>) -> io::Result<Line> {
    bytes.clear();
    let mut started = false;
    let mut too_long = false;
    loop {
        let available = input.fill_buf()?;
        if available.is_empty() {
            return Ok(if !started {
                Line::Eof
            } else if too_long {
                Line::TooLong
            } else {
                Line::Data
            });
        }
        started = true;
        let newline = available.iter().position(|byte| *byte == b'\n');
        let length = newline.unwrap_or(available.len());
        let consumed = length + usize::from(newline.is_some());
        if !too_long {
            if length > MAX_REQUEST_BYTES - bytes.len() {
                too_long = true;
                bytes.clear();
            } else {
                bytes.extend_from_slice(&available[..length]);
            }
        }
        input.consume(consumed);
        if newline.is_some() {
            if bytes.last() == Some(&b'\r') {
                bytes.pop();
            }
            return Ok(if too_long { Line::TooLong } else { Line::Data });
        }
    }
}

struct Members<'a> {
    players: Vec<&'a Player>,
    primary: &'a Player,
}

impl Members<'_> {
    fn contains(&self, bus: &str) -> bool {
        self.players.iter().any(|player| player.bus == bus)
    }

    fn playback_actions(&self, operation: &str) -> Vec<NativeAction> {
        self.players
            .iter()
            .map(|player| native_action(player, operation, None))
            .collect()
    }

    fn display_name(&self) -> String {
        let primary = self.primary;
        let source = if browser_player(primary) && primary.url.is_empty() {
            self.players
                .iter()
                .copied()
                .find(|peer| browser_peer_corroborates(primary, peer))
                .unwrap_or(primary)
        } else {
            primary
        };
        display_name(source)
    }
}

fn proxy(player: &Player) -> bool {
    let bus = player.bus.to_lowercase();
    bus.contains("playerctld") || bus.contains("kdeconnect")
}

fn browser_player(player: &Player) -> bool {
    let identity = player.identity.to_lowercase();
    !player.browser_hint.is_null()
        || ["chrom", "firefox", "edge", "brave", "vivaldi", "opera"]
            .iter()
            .any(|name| identity.contains(name))
}

// Missing URLs need independent allowlist evidence, never another URL-less
// source or a match that would rescue an explicitly disallowed URL.
fn browser_peer_corroborates(player: &Player, peer: &Player) -> bool {
    !peer.stopped
        && !proxy(peer)
        && browser_player(peer)
        && browser_media_allowed(&peer.url)
        && same_playback(player, peer)
}

fn group_players(players: &[Player]) -> Vec<Members<'_>> {
    let mut buses = HashSet::new();
    let mut candidates: Vec<_> = players
        .iter()
        .enumerate()
        .filter(|(_, player)| {
            !player.stopped
                && !proxy(player)
                && (!browser_player(player)
                    || browser_media_allowed(&player.url)
                    || player.url.is_empty()
                        && players
                            .iter()
                            .any(|peer| browser_peer_corroborates(player, peer)))
                && buses.insert(player.bus.as_str())
        })
        .collect();
    let order: HashMap<_, _> = candidates
        .iter()
        .map(|(index, player)| (player.bus.as_str(), *index))
        .collect();
    candidates.sort_by_key(|(_, player)| !player.playing);

    let mut groups: Vec<Members<'_>> = Vec::new();
    for (_, player) in candidates {
        if let Some(group) = groups
            .iter_mut()
            .find(|group| same_playback(group.primary, player))
        {
            group.players.push(player);
        } else {
            groups.push(Members {
                players: vec![player],
                primary: player,
            });
        }
    }
    // Keep matching seeds fixed until grouping is complete. Snapshot order
    // breaks representative-quality ties without reordering cards or commands.
    for group in &mut groups {
        group
            .players
            .sort_by_key(|player| order[player.bus.as_str()]);
        group.primary = group
            .players
            .iter()
            .copied()
            .min_by_key(|player| {
                (
                    !player.playing,
                    !(player.can_play || player.can_pause),
                    known_length(player.length).is_none(),
                    !player.can_seek,
                )
            })
            .expect("groups contain at least one player");
    }
    groups.sort_by_key(|group| order[group.players[0].bus.as_str()]);
    groups
}

struct WebUrl<'a> {
    host: &'a str,
    path: &'a str,
}

/// A deliberately narrow HTTP(S) authority parser, not a substring host test.
/// Reject credentials, escapes and backslashes rather than guessing how a
/// browser might interpret a malformed authority. No URL is fetched here.
fn web_url(url: &str) -> Option<WebUrl<'_>> {
    let (authority, path_query) = http_authority(url)?;
    let host = if let Some((host, port)) = authority.split_once(':') {
        if port.is_empty()
            || !port.bytes().all(|byte| byte.is_ascii_digit())
            || port.parse::<u16>().is_err()
        {
            return None;
        }
        host
    } else {
        authority
    };
    if host.is_empty()
        || !host
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-'))
        || host.split('.').any(str::is_empty)
    {
        return None;
    }
    Some(WebUrl {
        host,
        path: path_query.split(['?', '#']).next().unwrap_or(""),
    })
}

fn host_matches(host: &str, domain: &str) -> bool {
    host.eq_ignore_ascii_case(domain)
        || host
            .get(..host.len().saturating_sub(domain.len()))
            .is_some_and(|prefix| prefix.ends_with('.'))
            && host
                .get(host.len().saturating_sub(domain.len())..)
                .is_some_and(|suffix| suffix.eq_ignore_ascii_case(domain))
}

fn browser_media_allowed(url: &str) -> bool {
    // More specific subdomains must precede their parent domain.
    const ALLOWED: &[(&str, &[&str])] = &[
        ("music.youtube.com", &["watch", "playlist"]),
        ("live.bilibili.com", &["*"]),
        (
            "youtube.com",
            &["watch", "shorts", "live", "embed", "playlist", "v", "e"],
        ),
        (
            "bilibili.com",
            &[
                "video",
                "bangumi",
                "cheese",
                "medialist",
                "list",
                "festival",
                "audio",
            ],
        ),
        ("youtu.be", &["*"]),
        ("b23.tv", &["*"]),
        (
            "spotify.com",
            &[
                "track",
                "album",
                "playlist",
                "artist",
                "show",
                "episode",
                "collection",
                "search",
            ],
        ),
    ];
    let Some(url) = web_url(url) else {
        return false;
    };
    let path = url.path.to_ascii_lowercase();
    let mut segments = path.split('/').filter(|part| !part.is_empty());
    let mut first = segments.next();
    if first.is_some_and(|part| part.starts_with("intl-")) {
        first = segments.next();
    }
    let Some(first) = first else {
        return false;
    };
    ALLOWED
        .iter()
        .find(|(domain, _)| host_matches(url.host, domain))
        .is_some_and(|(_, paths)| paths[0] == "*" || paths.contains(&first))
}

fn display_name(player: &Player) -> String {
    if browser_player(player)
        && let Some(url) = web_url(&player.url)
    {
        for (domain, name) in [
            ("music.youtube.com", "YouTube Music"),
            ("youtube.com", "YouTube"),
            ("youtu.be", "YouTube"),
            ("live.bilibili.com", "Bilibili Live"),
            ("bilibili.com", "Bilibili"),
            ("b23.tv", "Bilibili"),
            ("spotify.com", "Spotify"),
        ] {
            if host_matches(url.host, domain) {
                return name.to_owned();
            }
        }
    }
    source_name(player).into_owned()
}

fn canonical(value: &str) -> String {
    let name = value.trim().to_lowercase();
    let name = name.strip_suffix(".desktop").unwrap_or(&name);
    match name {
        "google chrome"
        | "google-chrome"
        | "google-chrome-stable"
        | "chrome"
        | "org.chromium.chromium"
        | "org.google.chrome" => "chromium",
        "mozilla firefox" | "org.mozilla.firefox" => "firefox",
        other => other,
    }
    .to_owned()
}

fn browser_key(key: &str) -> bool {
    matches!(
        key,
        "chromium"
            | "firefox"
            | "brave"
            | "brave-browser"
            | "vivaldi"
            | "opera"
            | "microsoft-edge"
            | "microsoft edge"
    )
}

fn add_key(keys: &mut Vec<String>, value: &str) {
    let key = canonical(value);
    if !key.is_empty() && !keys.contains(&key) {
        keys.push(key);
    }
}

/// PipeWire properties are scalars. Composite values provide no identity or
/// track evidence; in particular, JSON presence is not a browser name.
fn value_text(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(value)) => value.trim().to_owned(),
        Some(Value::Number(value)) => match value.as_f64() {
            // Normalize integer-valued JSON numbers like JavaScript String(42.0).
            Some(number) if value.is_f64() && number.fract() == 0.0 && number.abs() < 1e21 => {
                format!("{number:.0}")
            }
            _ => value.to_string(),
        },
        Some(Value::Bool(value)) => value.to_string(),
        _ => String::new(),
    }
}

struct Source<'a> {
    keys: Vec<String>,
    titles: Vec<&'a str>,
    urls: Vec<&'a str>,
    browser: bool,
}

fn source<'a>(group: &Members<'a>) -> Source<'a> {
    let mut keys = Vec::new();
    let mut titles = Vec::new();
    let mut urls = Vec::new();
    // Revisit primary last so its explicit browser hint wins over another
    // publisher's generic browser identity.
    for player in group.players.iter().copied().chain([group.primary]) {
        add_key(&mut keys, &player.identity);
        add_key(&mut keys, &player.desktop_entry);
        let hint = canonical(&value_text(Some(&player.browser_hint)));
        if browser_key(&hint) {
            keys.retain(|key| !browser_key(key));
            add_key(&mut keys, &hint);
        }
        if !player.title.trim().is_empty() {
            titles.push(player.title.trim());
        }
        if !player.url.trim().is_empty() {
            urls.push(player.url.trim());
        }
    }
    if !group.primary.title.trim().is_empty() {
        titles.push(group.primary.title.trim());
    }
    let browser = keys.iter().any(|key| browser_key(key));
    Source {
        keys,
        titles,
        urls,
        browser,
    }
}

fn node_keys(node: &Stream) -> Vec<String> {
    let mut keys = Vec::new();
    add_key(&mut keys, &node.name);
    add_key(&mut keys, &node.nickname);
    for property in [
        "application.name",
        "application.id",
        "application.process.binary",
    ] {
        add_key(&mut keys, &value_text(node.properties.get(property)));
    }
    keys
}

const NO_MATCH: &str = "No matching audio stream. Start playback in this source.";
const READING: &str = "Reading audio stream details. Try again in a moment.";
const BROWSER_SHARED: &str =
    "Several sources share this browser; a distinct track-specific audio stream is required.";
const MULTIPLE: &str =
    "Multiple audio streams match this source; independent volume control is unavailable.";
const SHARED: &str = "Shared or ambiguous audio stream; independent volume control is unavailable.";
const UNIQUE: &str = "Unique application playback stream";

struct StreamMatch<'a> {
    node: Option<&'a Stream>,
    reason: &'static str,
}

fn match_groups<'a>(groups: &[Members<'_>], streams: &'a [Stream]) -> Vec<StreamMatch<'a>> {
    let sources: Vec<_> = groups.iter().map(source).collect();
    let mut ids = HashSet::new();
    let nodes: Vec<_> = streams
        .iter()
        .filter(|node| node.id == 0 || ids.insert(node.id))
        .collect();
    let keys: Vec<_> = nodes.iter().map(|node| node_keys(node)).collect();
    let mut candidates = Vec::new();
    let mut reasons = Vec::new();
    let mut detailed_counts = Vec::new();
    for src in &sources {
        let scoped: Vec<_> = (0..nodes.len())
            .filter(|&index| keys[index].iter().any(|key| src.keys.contains(key)))
            .collect();
        let ready: Vec<_> = scoped
            .iter()
            .copied()
            .filter(|&index| nodes[index].ready && nodes[index].id != 0)
            .collect();
        let detailed: Vec<_> = ready
            .iter()
            .copied()
            .filter(|&index| {
                let props = &nodes[index].properties;
                ["media.name", "media.title"]
                    .iter()
                    .any(|key| src.titles.contains(&value_text(props.get(*key)).as_str()))
                    || ["media.filename", "media.url"]
                        .iter()
                        .any(|key| src.urls.contains(&value_text(props.get(*key)).as_str()))
            })
            .collect();
        detailed_counts.push(detailed.len());
        reasons.push(if scoped.is_empty() {
            NO_MATCH
        } else if ready.is_empty() {
            READING
        } else {
            "Application playback stream"
        });
        candidates.push(if detailed.is_empty() { ready } else { detailed });
    }

    // Keep original claims even when a browser source is denied volume control.
    // Paused sources participate equally: playback state is not ownership.
    let mut claims = vec![0usize; nodes.len()];
    for options in &candidates {
        for &node in options {
            claims[node] += 1;
        }
    }
    for (index, src) in sources.iter().enumerate() {
        if src.browser
            && detailed_counts[index] != 1
            && sources.iter().enumerate().any(|(other_index, other)| {
                other_index != index
                    && other.browser
                    && other
                        .keys
                        .iter()
                        .any(|key| browser_key(key) && src.keys.contains(key))
            })
        {
            candidates[index].clear();
            reasons[index] = BROWSER_SHARED;
        }
    }
    candidates
        .iter()
        .enumerate()
        .map(|(index, options)| {
            let (node, reason) = match options.as_slice() {
                [] => (None, reasons[index]),
                [node] if claims[*node] > 1 => (None, SHARED),
                [node] => (Some(nodes[*node]), UNIQUE),
                _ => (None, MULTIPLE),
            };
            StreamMatch { node, reason }
        })
        .collect()
}

fn native_action(player: &Player, operation: &str, value: Option<f64>) -> NativeAction {
    NativeAction {
        player: player.bus.clone(),
        operation: operation.to_owned(),
        value,
    }
}

fn pause_actions(players: &[Player]) -> Vec<NativeAction> {
    let mut buses = HashSet::new();
    players
        .iter()
        .filter(|p| !proxy(p) && p.playing && buses.insert(p.bus.as_str()))
        .map(|p| native_action(p, "pause", None))
        .collect()
}

fn plan_action(
    request: &Request,
    action: &Action,
    groups: &[Members<'_>],
    active: Option<usize>,
) -> Result<Vec<NativeAction>, &'static str> {
    match action.kind.as_str() {
        "pauseAll" | "togglePauseAll" => {
            let mut actions = pause_actions(&request.players);
            if action.kind == "togglePauseAll"
                && actions.is_empty()
                && let Some(index) = active
            {
                actions = groups[index].playback_actions("play");
            }
            return Ok(actions);
        }
        "toggle" | "next" | "previous" | "seek" => {}
        _ => return Err("Unknown media action."),
    }
    if action.kind == "seek" && action.position < 0.0 {
        return Err("Seek position must be finite and nonnegative.");
    }
    let target = if action.player.is_empty() {
        active
    } else {
        groups
            .iter()
            .position(|group| group.contains(&action.player))
    };
    let index = target.ok_or("The requested media source is no longer available.")?;
    let group = &groups[index];
    Ok(match action.kind.as_str() {
        "toggle" => {
            let operation = if group.players.iter().any(|p| p.playing) {
                "pause"
            } else {
                "play"
            };
            group.playback_actions(operation)
        }
        "next" if group.primary.can_next => vec![native_action(group.primary, "next", None)],
        "previous" if group.primary.can_previous => {
            vec![native_action(group.primary, "previous", None)]
        }
        "seek" => group
            .players
            .iter()
            .filter(|p| p.can_seek)
            .map(|p| native_action(p, "seek", Some(action.position)))
            .collect(),
        _ => Vec::new(),
    })
}

#[cfg(test)]
media_unit_tests!(model);

// Resident service. Keep the pure model protocol above independent of this bridge.

const MAX_PENDING: usize = 8;
const MAX_ACTION_AGE: f64 = 5.0;
const MAX_POSITION_AGE: f64 = 5.0;
const MAX_SAFE_INTEGER: u64 = (1 << 53) - 1;
const NO_TRACK: &str = "/org/mpris/MediaPlayer2/TrackList/NoTrack";
const PLAYER_GUARD_FIELDS: &[&str] = &[
    "bus",
    "instance",
    "trackId",
    "seekRevision",
    "positionSupported",
    "identity",
    "desktopEntry",
    "title",
    "artist",
    "album",
    "url",
    "artUrl",
    "browserHint",
    "playing",
    "stopped",
    "length",
    "lengthMicros",
    "rate",
    "canPlay",
    "canPause",
    "canSeek",
    "canNext",
    "canPrevious",
    "available",
];
const STREAM_GUARD_FIELDS: &[&str] = &["id", "instance", "name", "nickname", "ready", "properties"];
const CHANGED: &str = "player identity, playback, capabilities, seek dispatch, source grouping, or stream ownership changed";

/// Only native fields appear here: the frontend need not understand any policy.
/// Array membership/order and complete property maps are significant; positions,
/// sampling revisions, artwork cache paths, volume, and mute are not.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
struct NativeGuard {
    players: Vec<Value>,
    streams: Vec<Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct PlayerView {
    #[serde(flatten)]
    player: Player,
    instance: u64,
    track_id: String,
    track_key: String,
    position_supported: bool,
    position_revision: u64,
    seek_revision: u64,
    rate: f64,
    length_micros: Value,
    cover: String,
    volume: Option<f64>,
    available: bool,
}

#[derive(Clone, Debug)]
struct ResidentPlayer {
    view: PlayerView,
    raw: Value,
    raw_position: Option<f64>,
    position_revision: u64,
    native: bool,
}

impl ResidentPlayer {
    fn available(&self) -> bool {
        self.view.available
    }
}

#[derive(Clone, Debug)]
struct StreamView {
    id: u32,
    instance: u64,
    ready: bool,
    volume: Option<f64>,
    muted: bool,
    raw: Value,
}

#[derive(Clone, Debug, Default)]
struct NativeSnapshot {
    players: Vec<ResidentPlayer>,
    streams: Vec<Stream>,
    stream_views: Vec<StreamView>,
    guard: NativeGuard,
}

fn partial(raw: &Value, fields: &[&str]) -> Value {
    Value::Object(
        fields
            .iter()
            .filter_map(|key| {
                raw.get(*key)
                    .map(|value| ((*key).to_owned(), value.clone()))
            })
            .collect(),
    )
}

fn number(raw: &Value, key: &str) -> Result<Option<f64>, &'static str> {
    match raw.get(key) {
        Some(Value::Null) => Ok(None),
        Some(Value::Number(value)) => value
            .as_f64()
            .filter(|v| v.is_finite())
            .map(Some)
            .ok_or("Native numeric fields must be finite numbers or null."),
        _ => Err("Native numeric fields must be numbers or null."),
    }
}

fn revision(raw: &Value, key: &str) -> Result<u64, &'static str> {
    raw.get(key)
        .and_then(Value::as_u64)
        .ok_or("Native instances and revisions must be nonnegative integers.")
}

fn string(raw: &Value, key: &str) -> Result<String, &'static str> {
    raw.get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or("Native text fields must be strings.")
}

fn valid_track_id(track: &str) -> bool {
    track != NO_TRACK && dbus::Path::new(track).is_ok()
}

fn track_key(player: &Player, track: &str) -> String {
    if valid_track_id(track) {
        track.to_owned()
    } else {
        // This is a freshness identity, never a fabricated MPRIS object path.
        serde_json::to_string(&[&player.title, &player.artist, &player.album, &player.url])
            .expect("strings serialize")
    }
}

impl NativeSnapshot {
    fn parse(raw: Value) -> Result<Self, &'static str> {
        #[derive(Deserialize)]
        struct Input {
            players: Vec<Value>,
            streams: Vec<Value>,
        }
        let input: Input = serde_json::from_value(raw)
            .map_err(|_| "A snapshot must contain native players and streams arrays.")?;
        let mut snapshot = Self::default();
        let mut buses = HashSet::new();
        for raw in input.players {
            let bus = string(&raw, "bus")?;
            if bus.is_empty() || !buses.insert(bus) {
                return Err("Native player buses must be nonempty and unique.");
            }
            for key in [
                "playing",
                "stopped",
                "canPlay",
                "canPause",
                "canSeek",
                "canNext",
                "canPrevious",
                "positionSupported",
            ] {
                if raw.get(key).and_then(Value::as_bool).is_none() {
                    return Err("Native player flags must be booleans.");
                }
            }
            let available = match raw.get("available") {
                None => true,
                Some(Value::Bool(available)) => *available,
                _ => return Err("Native player available must be a boolean when supplied."),
            };
            let instance = revision(&raw, "instance")?;
            let seek_revision = revision(&raw, "seekRevision")?;
            let position_revision = revision(&raw, "positionRevision")?;
            let track_id = string(&raw, "trackId")?;
            let cover = string(&raw, "cover")?;
            let raw_position =
                number(&raw, "position")?.filter(|v| *v >= 0.0 && raw["positionSupported"] == true);
            // The metadata is arbitrary JSON. Only a positive numeric value can
            // supply the fallback; never coerce a string or reject other variants.
            let length_micros = raw.get("lengthMicros").cloned().unwrap_or(Value::Null);
            let length = known_length(number(&raw, "length")?).or_else(|| {
                length_micros
                    .as_f64()
                    .filter(|v| v.is_finite() && *v > 0.0)
                    .map(|v| v / 1e6)
                    .filter(|v| *v > 0.0)
            });
            let volume = number(&raw, "volume")?;
            let rate = number(&raw, "rate")?.unwrap_or(1.0);
            let mut normalized = raw.clone();
            normalized["position"] = serde_json::json!(raw_position.unwrap_or(0.0));
            normalized["length"] = serde_json::json!(length);
            let player: Player =
                serde_json::from_value(normalized).map_err(|_| "Invalid native player fields.")?;
            let track_key = track_key(&player, &track_id);
            snapshot
                .guard
                .players
                .push(partial(&raw, PLAYER_GUARD_FIELDS));
            snapshot.players.push(ResidentPlayer {
                view: PlayerView {
                    player,
                    instance,
                    track_id,
                    track_key,
                    position_supported: raw_position.is_some(),
                    position_revision,
                    seek_revision,
                    rate,
                    length_micros,
                    cover,
                    volume,
                    available,
                },
                raw,
                raw_position,
                position_revision,
                native: true,
            });
        }
        let mut ids = HashSet::new();
        for raw in input.streams {
            let stream: Stream = serde_json::from_value(raw.clone())
                .map_err(|_| "Invalid native audio stream fields.")?;
            let instance = revision(&raw, "instance")?;
            let volume = number(&raw, "volume")?;
            let muted = raw
                .get("muted")
                .and_then(Value::as_bool)
                .ok_or("Native stream muted must be a boolean.")?;
            if stream.id != 0 && !ids.insert(stream.id) {
                return Err("Bound native audio stream IDs must be unique.");
            }
            snapshot
                .guard
                .streams
                .push(partial(&raw, STREAM_GUARD_FIELDS));
            snapshot.stream_views.push(StreamView {
                id: stream.id,
                instance,
                ready: stream.ready,
                volume,
                muted,
                raw,
            });
            snapshot.streams.push(stream);
        }
        Ok(snapshot)
    }
}

#[derive(Clone, Debug, PartialEq)]
struct ActionGuard {
    native: NativeGuard,
    // Owner/watch/revision evidence fences Rust intents, never JS raw guards.
    native_playback: Vec<NativePlayback>,
    recovered: Vec<Value>,
    groups: Vec<Group>,
    selected: String,
    selection_revision: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum VolumeTarget {
    Player { bus: String, instance: u64 },
    Stream { id: u32, instance: u64 },
}

#[derive(Clone, Debug)]
enum VolumeMutation {
    Volume(f64),
    Mute(bool),
}

#[derive(Clone, Debug)]
struct VolumeIntent {
    player: String,
    primary: String,
    target: VolumeTarget,
    mutation: VolumeMutation,
}

#[derive(Clone, Debug)]
enum IntentAction {
    Playback(Action),
    Volume(VolumeIntent),
}

impl IntentAction {
    fn kind(&self) -> &str {
        match self {
            Self::Playback(action) => &action.kind,
            Self::Volume(intent) => match intent.mutation {
                VolumeMutation::Volume(_) => "volume",
                VolumeMutation::Mute(_) => "mute",
            },
        }
    }

    fn player(&self) -> &str {
        match self {
            Self::Playback(action) => &action.player,
            Self::Volume(intent) => &intent.player,
        }
    }
}

#[derive(Clone, Debug)]
struct Intent {
    action: IntentAction,
    created: f64,
    guard: ActionGuard,
}

#[derive(Clone, Debug)]
struct PositionRequest {
    bus: String,
    instance: u64,
    generation: u64,
}

struct PositionResult {
    request: PositionRequest,
    position: Option<f64>,
}

#[derive(Clone, Debug)]
struct Timeline {
    key: (u64, String, String, String),
    raw: Option<f64>,
    revision: u64,
    seek_revision: u64,
    playing: bool,
    stopped: bool,
    rate: f64,
    anchor: f64,
    at: f64,
    sampled_at: f64,
    sample_position: Option<f64>,
    trusted: bool,
    generation: u64,
    read_pending: bool,
}

impl Timeline {
    fn projected(&self, now: f64) -> f64 {
        self.anchor
            + if self.playing {
                (now - self.at).max(0.0) * self.rate
            } else {
                0.0
            }
    }

    fn fresh(&self, now: f64) -> bool {
        now >= self.at && (!self.playing || now - self.sampled_at <= MAX_POSITION_AGE)
    }

    fn supported(&self, now: f64) -> bool {
        self.trusted && self.fresh(now) && self.projected(now).is_finite()
    }

    /// Returns whether a new guarded position read is useful. Reobserving a
    /// snapshot never renews a clock: only a raw change/revision confirms data.
    fn observe(
        previous: Option<&Self>,
        source: &ResidentPlayer,
        now: f64,
        generation: u64,
    ) -> (Self, bool) {
        let view = &source.view;
        let player = &view.player;
        let key = (
            view.instance,
            view.track_key.clone(),
            player.url.clone(),
            player.title.clone(),
        );
        let same_track = previous.is_some_and(|p| p.key == key);
        let same_instance = previous.is_some_and(|p| p.key.0 == view.instance);
        let fresh_sample = previous.is_none()
            || !same_instance
            || previous.is_some_and(|p| {
                p.raw != source.raw_position || p.revision != source.position_revision
            });
        let seek_changed =
            same_instance && previous.is_some_and(|p| p.seek_revision != view.seek_revision);
        let continuing = same_track && !fresh_sample && !seek_changed;
        let can_anchor = source
            .raw_position
            .is_some_and(|p| !player.stopped && (p > 0.0 || !player.playing));
        let mut sample = Self {
            key,
            raw: source.raw_position,
            revision: source.position_revision,
            seek_revision: view.seek_revision,
            playing: player.playing,
            stopped: player.stopped,
            rate: view.rate,
            anchor: source.raw_position.unwrap_or(0.0),
            at: now,
            sampled_at: now,
            sample_position: fresh_sample.then_some(source.raw_position).flatten(),
            trusted: can_anchor && fresh_sample,
            generation,
            read_pending: false,
        };
        if continuing {
            let previous = previous.expect("continuing timeline");
            sample.anchor = previous.projected(now);
            sample.sampled_at = if previous.playing {
                previous.sampled_at
            } else {
                now
            };
            sample.sample_position = previous.sample_position;
            sample.trusted = previous.trusted
                && previous.fresh(now)
                && !player.stopped
                && (sample.anchor > 0.0 || !player.playing);
        }
        let query = !player.stopped
            && (previous.is_none()
                || previous.is_some_and(|p| p.stopped)
                || !same_track
                || seek_changed
                || (!player.playing
                    && previous.is_some_and(|p| {
                        p.playing || !sample.trusted || p.read_pending || p.rate != view.rate
                    })));
        (sample, query)
    }

    fn unchanged(&self, source: &ResidentPlayer) -> bool {
        let v = &source.view;
        self.key
            == (
                v.instance,
                v.track_key.clone(),
                v.player.url.clone(),
                v.player.title.clone(),
            )
            && self.raw == source.raw_position
            && self.revision == source.position_revision
            && self.seek_revision == v.seek_revision
            && self.playing == v.player.playing
            && self.stopped == v.player.stopped
            && self.rate == v.rate
    }
}

/// Service setters extend the native wire operation without changing the pure
/// model's public NativeAction schema. A streamId takes precedence over player;
/// player then identifies the owning source, not an Astal player to resolve.
/// Optional muted requests an explicit native setter alongside stream volume;
/// the frontend must not infer unmuting or choose a fallback of its own.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ServiceOperation {
    player: String,
    operation: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    value: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream_id: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    muted: Option<bool>,
}

impl From<NativeAction> for ServiceOperation {
    fn from(action: NativeAction) -> Self {
        Self {
            player: action.player,
            operation: action.operation,
            value: action.value,
            stream_id: None,
            muted: None,
        }
    }
}

impl ServiceOperation {
    fn playback(&self) -> NativeAction {
        debug_assert!(self.stream_id.is_none() && self.muted.is_none());
        NativeAction {
            player: self.player.clone(),
            operation: self.operation.clone(),
            value: self.value,
        }
    }
}

#[derive(Clone, Debug)]
struct PendingApply {
    id: u64,
    expires_at: f64,
    operation: ServiceOperation,
    before: ActionGuard,
    expected: ActionGuard,
    recovered: bool,
    observed_after: bool,
}

#[derive(Clone, Debug)]
struct Transaction {
    intent: Intent,
    original: ActionGuard,
    guard: ActionGuard,
    operations: VecDeque<ServiceOperation>,
    pending: Option<PendingApply>,
    interrupted: Option<&'static str>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ApplyMessage {
    #[serde(rename = "type")]
    kind: &'static str,
    id: u64,
    operation: ServiceOperation,
    guard: NativeGuard,
    expires_at: f64,
    #[serde(skip)]
    recovered: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ServiceAction {
    kind: String,
    #[serde(default)]
    player: String,
    #[serde(default)]
    position: f64,
}

impl From<ServiceAction> for Action {
    fn from(action: ServiceAction) -> Self {
        Self {
            kind: action.kind,
            player: action.player,
            position: action.position,
        }
    }
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum ServiceMessage {
    Snapshot {
        snapshot: Value,
    },
    Action {
        action: ServiceAction,
        #[serde(rename = "createdAt")]
        created_at: f64,
        snapshot: Value,
    },
    Volume {
        player: String,
        value: f64,
        #[serde(rename = "createdAt")]
        created_at: f64,
        snapshot: Value,
    },
    Mute {
        player: String,
        muted: bool,
        #[serde(rename = "createdAt")]
        created_at: f64,
        snapshot: Value,
    },
    Select {
        player: String,
        snapshot: Value,
    },
    Cycle {
        snapshot: Value,
    },
    Tick {
        #[serde(default)]
        snapshot: Option<Value>,
    },
    Report {
        error: String,
    },
    Result {
        id: u64,
        ok: bool,
        #[serde(default)]
        error: Option<String>,
        snapshot: Value,
    },
}

#[derive(Serialize)]
struct Transport {
    enabled: bool,
    reason: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct VolumeDescriptor {
    enabled: bool,
    value: f64,
    key: String,
    reason: String,
    mute_enabled: bool,
    muted: bool,
    mute_reason: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProgressView {
    #[serde(flatten)]
    view: PlayerView,
    sample_position: Option<f64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ServiceState<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    ready: bool,
    groups: &'a [Group],
    active_player: &'a str,
    players: Vec<&'a PlayerView>,
    progress: Vec<ProgressView>,
    transports: BTreeMap<&'a str, Transport>,
    volumes: BTreeMap<&'a str, VolumeDescriptor>,
    failure: &'a str,
    guard: &'a NativeGuard,
}

/// The state machine is deliberately desktop-independent. I/O only executes
/// effects returned by advance(); tests can acknowledge them with raw fixtures.
struct Runtime {
    native: NativeSnapshot,
    native_playback: BTreeMap<(String, u64), NativePlayback>,
    recovered: Vec<ResidentPlayer>,
    recovered_revisions: HashMap<(String, u64), u64>,
    timelines: HashMap<String, Timeline>,
    reads: VecDeque<PositionRequest>,
    next_generation: u64,
    groups: Vec<Group>,
    active: String,
    selected: String,
    selection_revision: u64,
    failure: String,
    queue: VecDeque<Intent>,
    transaction: Option<Transaction>,
    next_id: u64,
    initialized: bool,
    last_native_update: f64,
    last_capture_request: f64,
}

fn failure_text(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).take(512).collect()
}

impl Runtime {
    fn new(now: f64) -> Self {
        Self {
            native: NativeSnapshot::default(),
            native_playback: BTreeMap::new(),
            recovered: Vec::new(),
            recovered_revisions: HashMap::new(),
            timelines: HashMap::new(),
            reads: VecDeque::new(),
            next_generation: 0,
            groups: Vec::new(),
            active: String::new(),
            selected: String::new(),
            selection_revision: 0,
            failure: String::new(),
            queue: VecDeque::new(),
            transaction: None,
            // A restarted frontend also fences its child generation. Using the
            // process's monotonic epoch prevents accidental cross-restart IDs.
            next_id: ((now.max(0.0) * 1e6) as u64).min(MAX_SAFE_INTEGER - 1),
            initialized: false,
            last_native_update: now,
            last_capture_request: now,
        }
    }

    fn players(&self) -> impl Iterator<Item = &ResidentPlayer> {
        self.native.players.iter().chain(self.recovered.iter())
    }

    fn player(&self, bus: &str) -> Option<&ResidentPlayer> {
        self.players().find(|p| p.view.player.bus == bus)
    }

    fn guard(&self) -> ActionGuard {
        ActionGuard {
            native: self.native.guard.clone(),
            native_playback: self.native_playback.values().cloned().collect(),
            recovered: self
                .recovered
                .iter()
                .map(|p| partial(&p.raw, PLAYER_GUARD_FIELDS))
                .collect(),
            groups: self.groups.clone(),
            selected: self.selected.clone(),
            selection_revision: self.selection_revision,
        }
    }

    fn fail(&mut self, error: &str) {
        self.failure = failure_text(error);
        self.queue.clear();
        self.transaction = None;
    }

    fn protocol_failure(&mut self, error: &str) {
        self.fail(error);
        // An unrepresentable state must never leave an apparently current
        // ownership decision available to the frontend.
        self.native = NativeSnapshot::default();
        self.native_playback.clear();
        self.recovered.clear();
        self.recovered_revisions.clear();
        self.timelines.clear();
        self.reads.clear();
        self.groups.clear();
        self.active.clear();
        self.selected.clear();
        self.selection_revision += 1;
        self.initialized = false;
    }

    fn snapshot(&mut self, raw: Value, now: f64) -> bool {
        match NativeSnapshot::parse(raw) {
            Ok(snapshot) => {
                self.native = snapshot;
                self.native_playback.retain(|(bus, instance), _| {
                    self.native
                        .players
                        .iter()
                        .any(|p| p.view.player.bus == *bus && p.view.instance == *instance)
                });
                self.apply_native_playback();
                // A native takeover wins over a previously verified adapter.
                self.recovered.retain(|p| {
                    !self
                        .native
                        .players
                        .iter()
                        .any(|n| n.view.player.bus == p.view.player.bus)
                });
                self.initialized = true;
                self.last_native_update = now;
                self.synchronize(now);
                true
            }
            Err(error) => {
                self.fail(error);
                false
            }
        }
    }

    /// Recovery supplies the complete set of currently verified watch results;
    /// absence withdraws an override rather than retaining unverified playback.
    fn update_native_playback(&mut self, statuses: Vec<NativePlayback>, now: f64) {
        self.native_playback = statuses
            .into_iter()
            .filter(|status| {
                self.native
                    .players
                    .iter()
                    .any(|p| p.view.player.bus == status.bus && p.view.instance == status.instance)
            })
            .map(|status| ((status.bus.clone(), status.instance), status))
            .collect();
        self.apply_native_playback();
        self.synchronize(now);
    }

    fn apply_native_playback(&mut self) {
        for source in &mut self.native.players {
            let player = &mut source.view.player;
            if let Some(status) = self
                .native_playback
                .get(&(player.bus.clone(), source.view.instance))
            {
                player.playing = status.playing;
                player.stopped = status.stopped;
            } else {
                // Raw capture and its frontend guard remain untouched, including
                // while an invalidation withdraws a previously verified status.
                player.playing = source.raw["playing"].as_bool().expect("parsed native flag");
                player.stopped = source.raw["stopped"].as_bool().expect("parsed native flag");
            }
        }
    }

    fn update_recovered(&mut self, players: Vec<crate::playback::RecoveredPlayer>, now: f64) {
        let native: HashSet<_> = self
            .native
            .players
            .iter()
            .map(|p| p.view.player.bus.as_str())
            .collect();
        self.recovered = players
            .into_iter()
            .filter(|p| !native.contains(p.player.bus.as_str()))
            .map(|p| {
                let raw_position = p
                    .position_supported
                    .then_some(p.player.position)
                    .filter(|position| position.is_finite() && *position >= 0.0);
                let mut player = p.player;
                player.position = raw_position.unwrap_or(0.0);
                player.length = known_length(player.length);
                let key = track_key(&player, &p.track_id);
                let seek_revision = *self
                    .recovered_revisions
                    .entry((player.bus.clone(), p.instance))
                    .or_default();
                let view = PlayerView {
                    player,
                    instance: p.instance,
                    track_id: p.track_id,
                    track_key: key,
                    position_supported: raw_position.is_some(),
                    position_revision: p.position_revision,
                    seek_revision,
                    rate: if p.rate.is_finite() { p.rate } else { 1.0 },
                    length_micros: Value::Null,
                    cover: p.cover,
                    volume: None,
                    available: true,
                };
                let mut raw = serde_json::to_value(&view).expect("finite player view");
                raw["available"] = Value::Bool(true);
                ResidentPlayer {
                    view,
                    raw,
                    raw_position,
                    position_revision: p.position_revision,
                    native: false,
                }
            })
            .collect();
        self.recovered_revisions.retain(|(bus, instance), _| {
            self.recovered
                .iter()
                .any(|p| p.view.player.bus == *bus && p.view.instance == *instance)
        });
        self.synchronize(now);
    }

    fn synchronize(&mut self, now: f64) {
        let players: Vec<_> = self.players().cloned().collect();
        self.timelines
            .retain(|bus, _| players.iter().any(|p| p.view.player.bus == *bus));
        for player in players {
            let bus = &player.view.player.bus;
            let previous = self.timelines.get(bus);
            if previous.is_some_and(|p| p.unchanged(&player)) {
                continue;
            }
            self.next_generation += 1;
            let (mut sample, query) =
                Timeline::observe(previous, &player, now, self.next_generation);
            // Recovery already owns coalesced, generation-fenced private-owner
            // reads. A competing Properties.Get could revive pre-seek timing.
            if query && player.native {
                sample.read_pending = true;
                self.reads.push_back(PositionRequest {
                    bus: bus.clone(),
                    instance: player.view.instance,
                    generation: sample.generation,
                });
            }
            self.timelines.insert(bus.clone(), sample);
        }
        // Retire superseded queries before they can fill the bounded I/O queue.
        self.reads.retain(|r| {
            self.timelines.get(&r.bus).is_some_and(|t| {
                t.generation == r.generation && t.key.0 == r.instance && t.read_pending
            })
        });
        self.refresh_model(now);
    }

    fn position_result(&mut self, result: PositionResult, now: f64) {
        let Some(sample) = self.timelines.get_mut(&result.request.bus) else {
            return;
        };
        if sample.generation != result.request.generation
            || sample.key.0 != result.request.instance
            || !sample.read_pending
        {
            return;
        }
        sample.read_pending = false;
        if let Some(position) = result.position.filter(|p| p.is_finite() && *p >= 0.0) {
            sample.anchor = position;
            sample.at = now;
            sample.sampled_at = now;
            sample.sample_position = Some(position);
            sample.trusted = !sample.stopped && (position > 0.0 || !sample.playing);
            self.refresh_model(now);
        }
    }

    fn refresh_model(&mut self, now: f64) {
        let response = analyze(&Request {
            selected_player: self.selected.clone(),
            players: self
                .players()
                .map(|source| {
                    let mut p = source.view.player.clone();
                    p.position = self
                        .timelines
                        .get(&p.bus)
                        .filter(|t| t.supported(now))
                        .and_then(|t| t.sample_position)
                        .unwrap_or(0.0);
                    p
                })
                .collect(),
            streams: self.native.streams.clone(),
            ..Request::default()
        });
        self.groups = response.groups;
        self.active = response.active_player;
        if !self
            .groups
            .iter()
            .any(|g| g.players.contains(&self.selected))
            && self.selected != self.active
        {
            self.selected = self.active.clone();
            self.selection_revision += 1;
        }
        // Position reads and freshness expiry can change membership/ownership
        // without changing any raw identity field. Fence those decisions too.
        self.observe_transaction();
    }

    fn stale(&self, guard: &ActionGuard, created: f64, now: f64) -> Option<&'static str> {
        if guard.selected != self.selected || guard.selection_revision != self.selection_revision {
            Some("selection changed")
        } else if *guard != self.guard() {
            Some(CHANGED)
        } else if !created.is_finite() || now < created || now - created > MAX_ACTION_AGE {
            Some("action expired (maximum age is 5 seconds)")
        } else {
            None
        }
    }

    fn observe_transaction(&mut self) {
        let current = self.guard();
        let volume_valid = self
            .transaction
            .as_ref()
            .is_none_or(|tx| self.volume_current(&tx.intent.action));
        if let Some(tx) = &mut self.transaction {
            let valid = tx.pending.as_mut().map_or(current == tx.guard, |pending| {
                if current == pending.expected {
                    pending.observed_after = true;
                    true
                } else {
                    !pending.observed_after && current == pending.before
                }
            });
            if !valid || !volume_valid {
                tx.interrupted = Some(
                    if current.selection_revision != tx.guard.selection_revision {
                        "selection changed"
                    } else {
                        CHANGED
                    },
                );
                self.queue.clear();
            }
        }
    }

    fn enqueue(&mut self, action: Action, created: f64, now: f64) {
        if !self.initialized {
            self.failure = "Media worker is not ready; action was not queued.".into();
            return;
        }
        if !matches!(
            action.kind.as_str(),
            "toggle" | "next" | "previous" | "seek" | "pauseAll" | "togglePauseAll" | "jump"
        ) || !action.position.is_finite()
            || (action.kind == "seek" && action.position < 0.0)
            || (action.kind == "jump" && ![-15.0, 15.0].contains(&action.position))
        {
            self.failure = "Invalid media action: use toggle, next, previous, seek, pauseAll, togglePauseAll, or a 15-second jump with finite seconds.".into();
            return;
        }
        self.enqueue_intent(IntentAction::Playback(action), created, now);
    }

    fn enqueue_intent(&mut self, action: IntentAction, created: f64, now: f64) {
        if !self.initialized {
            self.failure = "Media worker is not ready; action was not queued.".into();
            return;
        }
        let guard = self.guard();
        if let Some(reason) = self.stale(&guard, created, now) {
            self.failure = format!(
                "Discarded stale queued {}: {reason}; action was not retried.",
                action.kind()
            );
            return;
        }
        let entry = Intent {
            action,
            created,
            guard,
        };
        if entry.action.kind() == "seek"
            && self.queue.back().is_some_and(|tail| {
                tail.action.kind() == "seek" && tail.action.player() == entry.action.player()
            })
        {
            *self.queue.back_mut().expect("queued seek") = entry;
        } else if self.queue.len() == MAX_PENDING {
            self.failure = "Media action queue is full (8 pending); action was not queued.".into();
        } else {
            self.queue.push_back(entry);
        }
    }

    fn select(&mut self, bus: String, now: f64) {
        if !self.groups.iter().any(|g| g.players.contains(&bus)) {
            self.failure = "The requested media source is no longer available.".into();
            return;
        }
        if self.selected != bus {
            self.selected = bus;
            self.selection_revision += 1;
        }
        self.refresh_model(now);
    }

    fn handle(&mut self, message: ServiceMessage, now: f64) {
        match message {
            ServiceMessage::Snapshot { snapshot } => {
                self.snapshot(snapshot, now);
            }
            ServiceMessage::Tick { snapshot } => {
                if let Some(snapshot) = snapshot {
                    self.snapshot(snapshot, now);
                } else {
                    self.refresh_model(now);
                }
            }
            ServiceMessage::Action {
                action,
                created_at,
                snapshot,
            } => {
                if self.snapshot(snapshot, now) {
                    self.enqueue(action.into(), created_at, now);
                }
            }
            ServiceMessage::Volume {
                player,
                value,
                created_at,
                snapshot,
            } => {
                if self.snapshot(snapshot, now) {
                    self.enqueue_volume(player, VolumeMutation::Volume(value), created_at, now);
                }
            }
            ServiceMessage::Mute {
                player,
                muted,
                created_at,
                snapshot,
            } => {
                if self.snapshot(snapshot, now) {
                    self.enqueue_volume(player, VolumeMutation::Mute(muted), created_at, now);
                }
            }
            ServiceMessage::Select { player, snapshot } => {
                if self.snapshot(snapshot, now) {
                    self.select(player, now);
                }
            }
            ServiceMessage::Cycle { snapshot } => {
                if self.snapshot(snapshot, now) && !self.groups.is_empty() {
                    let index = self
                        .groups
                        .iter()
                        .position(|g| g.players.contains(&self.selected))
                        .or_else(|| self.groups.iter().position(|g| g.primary == self.active));
                    let next = index.map_or(0, |i| (i + 1) % self.groups.len());
                    self.select(self.groups[next].primary.clone(), now);
                }
            }
            ServiceMessage::Report { error } => {
                self.failure = failure_text(&error);
            }
            ServiceMessage::Result {
                id,
                ok,
                error,
                snapshot,
            } => {
                // An orphan/duplicate reply must not replace a newer raw capture.
                if !self
                    .transaction
                    .as_ref()
                    .and_then(|t| t.pending.as_ref())
                    .is_some_and(|p| p.id == id && !p.recovered)
                {
                    return;
                }
                if self.snapshot(snapshot, now) {
                    self.finish_apply(id, ok, error.as_deref(), now);
                }
            }
        }
    }

    fn request(&self, action: Action, now: f64) -> Request {
        Request {
            selected_player: self.selected.clone(),
            action: Some(action),
            players: self
                .players()
                .map(|p| {
                    let mut player = p.view.player.clone();
                    player.position = self
                        .timelines
                        .get(&player.bus)
                        .filter(|t| t.supported(now))
                        .and_then(|t| t.sample_position)
                        .unwrap_or(0.0);
                    player
                })
                .collect(),
            streams: self.native.streams.clone(),
            ..Request::default()
        }
    }

    fn jump(&self, action: &Action, now: f64) -> Result<Action, &'static str> {
        let group = self
            .groups
            .iter()
            .find(|g| {
                if action.player.is_empty() {
                    g.primary == self.active
                } else {
                    g.players.contains(&action.player)
                }
            })
            .ok_or("The requested media source is no longer available.")?;
        let player = self
            .player(&group.primary)
            .ok_or("The media player disappeared.")?;
        let sample = self
            .timelines
            .get(&group.primary)
            .filter(|t| t.supported(now))
            .and_then(|t| t.sample_position)
            .ok_or("A fresh native position is unavailable for this track.")?;
        let length = known_length(player.view.player.length)
            .ok_or("The track duration is unavailable for a 15-second jump.")?;
        if !player.view.player.can_seek || !valid_track_id(&player.view.track_id) {
            return Err("Seeking is unavailable for this track.");
        }
        Ok(Action {
            kind: "seek".into(),
            player: action.player.clone(),
            position: (sample + action.position).clamp(0.0, length),
        })
    }

    fn preflight(&self, operations: &[ServiceOperation]) -> Result<(), &'static str> {
        for operation in operations {
            let source = self
                .player(&operation.player)
                .ok_or("The media player disappeared.")?;
            if !source.available() {
                return Err("The media player disappeared.");
            }
            if matches!(operation.operation.as_str(), "volume" | "mute") {
                let group = self
                    .target_group(&operation.player)
                    .filter(|g| g.primary == operation.player)
                    .ok_or("Media source or audio stream ownership is no longer current.")?;
                let mutation = if operation.operation == "mute" {
                    VolumeMutation::Mute(operation.muted.ok_or("Native mute requires a boolean.")?)
                } else {
                    VolumeMutation::Volume(
                        operation
                            .value
                            .filter(|v| v.is_finite() && (0.0..=1.0).contains(v))
                            .ok_or("Native volume must be between zero and one.")?,
                    )
                };
                let valid = match self.volume_target(group, &mutation) {
                    Some(VolumeTarget::Player { bus, .. }) => {
                        operation.stream_id.is_none() && bus == operation.player
                    }
                    Some(VolumeTarget::Stream { id, .. }) => operation.stream_id == Some(id),
                    None => false,
                };
                if !valid {
                    return Err(
                        "Media volume target or audio stream ownership changed; action was not retried.",
                    );
                }
            }
            if operation.operation == "seek"
                && (!source.view.player.can_seek
                    || !valid_track_id(&source.view.track_id)
                    || !operation.value.is_some_and(|v| v.is_finite() && v >= 0.0))
            {
                return Err("Seeking is unavailable for this track.");
            }
        }
        Ok(())
    }

    fn expected_guard(
        &self,
        before: &ActionGuard,
        operation: &ServiceOperation,
        native: bool,
    ) -> Option<ActionGuard> {
        let mut expected = before.clone();
        if operation.operation == "seek" {
            let players = if native {
                &mut expected.native.players
            } else {
                &mut expected.recovered
            };
            let player = players
                .iter_mut()
                .find(|p| p["bus"].as_str() == Some(&operation.player))?;
            let revision = player["seekRevision"].as_u64()?.checked_add(1)?;
            if revision > MAX_SAFE_INTEGER {
                return None;
            }
            player["seekRevision"] = Value::from(revision);
        }
        Some(expected)
    }

    /// Produce at most one native dispatch, holding its pre/post conditions until
    /// the result arrives. No model RPC or untrusted external plan is involved.
    fn advance(&mut self, now: f64) -> Option<ApplyMessage> {
        self.refresh_model(now);
        loop {
            if let Some(mut tx) = self.transaction.take() {
                if let Some(pending) = &tx.pending {
                    if now < tx.intent.created || now - tx.intent.created > MAX_ACTION_AGE {
                        self.fail("Native plan interrupted: action expired (maximum age is 5 seconds); pending actions were discarded, not retried.");
                    } else {
                        let _ = pending;
                        self.transaction = Some(tx);
                    }
                    return None;
                }
                if let Some(reason) = tx
                    .interrupted
                    .or_else(|| self.stale(&tx.guard, tx.intent.created, now))
                    .or_else(|| {
                        (!self.volume_current(&tx.intent.action))
                            .then_some("media volume target or audio stream ownership changed")
                    })
                {
                    self.fail(&format!("Native plan interrupted: {reason}; remaining operations and pending actions were discarded, not retried."));
                    return None;
                }
                let Some(operation) = tx.operations.pop_front() else {
                    self.complete(tx);
                    continue;
                };
                if let Err(error) = self.preflight(std::slice::from_ref(&operation)) {
                    self.fail(error);
                    return None;
                }
                let native = operation.stream_id.is_some()
                    || self
                        .player(&operation.player)
                        .expect("preflight target")
                        .native;
                let before = self.guard();
                let Some(expected) = self.expected_guard(&before, &operation, native) else {
                    self.fail(
                        "Native seek dispatch revision is exhausted; action was not retried.",
                    );
                    return None;
                };
                let id = self.allocate_id()?;
                // Matching can change when a sample expires even if no native
                // property changes while this operation crosses the IPC pipe.
                let expires_at = self
                    .timelines
                    .values()
                    .filter(|sample| sample.playing && sample.supported(now))
                    .fold(tx.intent.created + MAX_ACTION_AGE, |deadline, sample| {
                        deadline.min(sample.sampled_at + MAX_POSITION_AGE)
                    });
                tx.pending = Some(PendingApply {
                    id,
                    expires_at,
                    operation: operation.clone(),
                    before,
                    expected,
                    recovered: !native,
                    observed_after: false,
                });
                let message = ApplyMessage {
                    kind: "apply",
                    id,
                    guard: self.apply_guard(&operation),
                    operation,
                    expires_at,
                    recovered: !native,
                };
                self.transaction = Some(tx);
                return Some(message);
            }
            let intent = self.queue.pop_front()?;
            if let Some(reason) = self.stale(&intent.guard, intent.created, now) {
                self.failure = format!(
                    "Discarded stale queued {}: {reason}; action was not retried.",
                    intent.action.kind()
                );
                continue;
            }
            let operations = match self.plan(&intent.action, now) {
                Ok(operations) => operations,
                Err(error) => {
                    self.fail(&error);
                    return None;
                }
            };
            if let Err(error) = self.preflight(&operations) {
                self.fail(error);
                return None;
            }
            let guard = self.guard();
            self.transaction = Some(Transaction {
                original: intent.guard.clone(),
                intent,
                guard,
                operations: operations.into(),
                pending: None,
                interrupted: None,
            });
        }
    }

    fn finish_apply(&mut self, id: u64, ok: bool, error: Option<&str>, now: f64) {
        let Some(mut tx) = self.transaction.take() else {
            return;
        };
        let Some(pending) = tx.pending.take() else {
            self.transaction = Some(tx);
            return;
        };
        if pending.id != id {
            tx.pending = Some(pending);
            self.transaction = Some(tx);
            return;
        }
        if !ok || error.is_some_and(|e| !e.is_empty()) {
            let error = error
                .filter(|e| !e.is_empty())
                .map(failure_text)
                .unwrap_or_else(|| "native dispatch returned false".into());
            self.fail(&format!(
                "Native {} failed: {error}; pending actions were discarded, not retried.",
                pending.operation.operation
            ));
        } else if let Some(reason) = tx
            .interrupted
            .or_else(|| (now > pending.expires_at).then_some("native operation expired"))
            .or_else(|| self.stale(&pending.expected, tx.intent.created, now))
            .or_else(|| {
                (!self.volume_current(&tx.intent.action))
                    .then_some("media volume target or audio stream ownership changed")
            })
        {
            let seek_note = if pending.operation.operation == "seek" {
                " Seek apply must advance only its own seek dispatch revision once."
            } else {
                ""
            };
            self.fail(&format!("Native plan changed during apply ({reason}); remaining actions and pending actions were discarded, not retried.{seek_note}"));
        } else {
            tx.guard = pending.expected;
            if tx.operations.is_empty() {
                self.complete(tx);
            } else {
                self.transaction = Some(tx);
            }
        }
    }

    fn validate_recovered_apply(
        &mut self,
        id: u64,
        players: Vec<crate::playback::RecoveredPlayer>,
        statuses: Vec<NativePlayback>,
        now: f64,
    ) -> Result<(), String> {
        // Both observation sets come from the same locked dispatch boundary.
        self.update_native_playback(statuses, now);
        self.update_recovered(players, now);
        let tx = self
            .transaction
            .as_ref()
            .ok_or("The media transaction ended.")?;
        let pending = tx
            .pending
            .as_ref()
            .filter(|p| p.id == id && p.recovered)
            .ok_or("The media operation is no longer pending.")?;
        if let Some(reason) = tx
            .interrupted
            .or_else(|| (now > pending.expires_at).then_some("native operation expired"))
            .or_else(|| self.stale(&pending.before, tx.intent.created, now))
        {
            return Err(format!("Media state changed before dispatch: {reason}."));
        }
        self.preflight(std::slice::from_ref(&pending.operation))
            .map_err(str::to_owned)
    }

    fn finish_recovered(
        &mut self,
        id: u64,
        result: Result<(), String>,
        players: Vec<crate::playback::RecoveredPlayer>,
        now: f64,
    ) {
        let Some(pending) = self
            .transaction
            .as_ref()
            .and_then(|tx| tx.pending.as_ref())
            .filter(|p| p.id == id && p.recovered)
            .cloned()
        else {
            return;
        };
        if result.is_ok()
            && pending.operation.operation == "seek"
            && let Some(source) = self.player(&pending.operation.player)
        {
            let key = (source.view.player.bus.clone(), source.view.instance);
            *self.recovered_revisions.entry(key).or_default() += 1;
        }
        self.update_recovered(players, now);
        self.finish_apply(
            id,
            result.is_ok(),
            result.as_ref().err().map(String::as_str),
            now,
        );
    }

    fn complete(&mut self, tx: Transaction) {
        // Only an adjacent, sole, same-target seek can inherit exactly our own
        // successful revisions. A barrier or any other state change is not ours.
        if tx.intent.action.kind() == "seek"
            && tx.original != tx.guard
            && self.queue.len() == 1
            && let Some(queued) = self.queue.front_mut()
            && queued.action.kind() == "seek"
            && queued.action.player() == tx.intent.action.player()
            && queued.guard == tx.original
        {
            queued.guard = tx.guard;
        }
        self.failure.clear();
    }

    fn allocate_id(&mut self) -> Option<u64> {
        let Some(id) = self
            .next_id
            .checked_add(1)
            .filter(|id| *id <= MAX_SAFE_INTEGER)
        else {
            self.fail("Native action IDs are exhausted; restart the media worker.");
            return None;
        };
        self.next_id = id;
        Some(id)
    }

    fn capture_id(&mut self, now: f64) -> Option<u64> {
        if self.capture_due(now) {
            self.allocate_id()
        } else {
            None
        }
    }

    fn capture_due(&mut self, now: f64) -> bool {
        if now - self.last_native_update >= 5.0 && now - self.last_capture_request >= 5.0 {
            self.last_capture_request = now;
            true
        } else {
            false
        }
    }

    fn target_group(&self, bus: &str) -> Option<&Group> {
        self.groups.iter().find(|group| {
            if bus.is_empty() {
                group.primary == self.active
            } else {
                group.players.iter().any(|player| player == bus)
            }
        })
    }

    fn mpris_volume(&self, player: &ResidentPlayer) -> Option<f64> {
        // The web bridge's constant Volume=1 is not a working tab volume API.
        (player.native
            && !player
                .view
                .player
                .bus
                .starts_with("org.mpris.MediaPlayer2.mprisence_web."))
        .then_some(player.view.volume)
        .flatten()
        .filter(|value| *value >= 0.0)
    }

    fn volume_stream(&self, group: &Group) -> Option<&StreamView> {
        let id = group.stream_id?;
        self.native.stream_views.iter().find(|stream| {
            stream.id == id
                && id != 0
                && stream.ready
                && stream.volume.is_some_and(|value| value >= 0.0)
        })
    }

    fn volume_target(&self, group: &Group, mutation: &VolumeMutation) -> Option<VolumeTarget> {
        let player = self.player(&group.primary).filter(|p| p.available())?;
        if matches!(mutation, VolumeMutation::Volume(_)) && self.mpris_volume(player).is_some() {
            return Some(VolumeTarget::Player {
                bus: group.primary.clone(),
                instance: player.view.instance,
            });
        }
        self.volume_stream(group)
            .map(|stream| VolumeTarget::Stream {
                id: stream.id,
                instance: stream.instance,
            })
    }

    fn volume(&self, group: &Group) -> VolumeDescriptor {
        let primary = self.player(&group.primary);
        let valid = primary.is_some_and(|p| p.available());
        let stream = self.volume_stream(group);
        let mute_enabled = valid && stream.is_some();
        let mute_reason = if mute_enabled {
            "Application stream mute".to_owned()
        } else if !valid {
            "Media source or audio stream ownership is no longer current.".to_owned()
        } else {
            format!(
                "Native mute requires a ready, uniquely owned application audio stream.{}{}",
                if group.stream_reason.is_empty() {
                    ""
                } else {
                    " "
                },
                group.stream_reason
            )
        };
        let mut descriptor = VolumeDescriptor {
            enabled: false,
            value: 0.0,
            key: String::new(),
            reason: String::new(),
            mute_enabled,
            muted: mute_enabled && stream.is_some_and(|s| s.muted),
            mute_reason,
        };
        if let Some((player, volume)) = primary.and_then(|p| self.mpris_volume(p).map(|v| (p, v))) {
            descriptor.enabled = valid;
            descriptor.value = volume.clamp(0.0, 1.0);
            descriptor.key = format!("player:{}", player.view.instance);
            descriptor.reason = "Media volume (0–100%)".into();
        } else if let Some(stream) = stream {
            descriptor.enabled = valid;
            descriptor.value = stream.volume.expect("ready volume stream").clamp(0.0, 1.0);
            descriptor.key = format!("stream:{}", stream.instance);
            descriptor.reason = "Application stream volume (0–100%)".into();
        } else {
            let web_bridge = group
                .primary
                .starts_with("org.mpris.MediaPlayer2.mprisence_web.");
            descriptor.reason = format!(
                "{}{}",
                if web_bridge {
                    "This browser bridge does not support tab volume. "
                } else {
                    ""
                },
                if group.stream_reason.is_empty() {
                    "Independent media volume is unavailable."
                } else {
                    &group.stream_reason
                }
            );
        }
        descriptor
    }

    fn enqueue_volume(&mut self, player: String, mutation: VolumeMutation, created: f64, now: f64) {
        if matches!(mutation, VolumeMutation::Volume(value) if !value.is_finite()) {
            self.failure = "Media volume must be a finite number.".into();
            return;
        }
        let Some(group) = self.target_group(&player) else {
            self.failure = "The requested media source is no longer available.".into();
            return;
        };
        if !self.player(&group.primary).is_some_and(|p| p.available()) {
            self.failure = "Media source or audio stream ownership is no longer current.".into();
            return;
        }
        let descriptor = self.volume(group);
        let Some(target) = self.volume_target(group, &mutation) else {
            self.failure = match mutation {
                VolumeMutation::Volume(_) => descriptor.reason,
                VolumeMutation::Mute(_) => descriptor.mute_reason,
            };
            return;
        };
        let intent = VolumeIntent {
            player,
            primary: group.primary.clone(),
            target,
            mutation,
        };
        self.enqueue_intent(IntentAction::Volume(intent), created, now);
    }

    fn volume_current(&self, action: &IntentAction) -> bool {
        match action {
            IntentAction::Playback(_) => true,
            IntentAction::Volume(intent) => {
                self.target_group(&intent.player).is_some_and(|group| {
                    group.primary == intent.primary
                        && self.volume_target(group, &intent.mutation).as_ref()
                            == Some(&intent.target)
                })
            }
        }
    }

    fn plan(&self, action: &IntentAction, now: f64) -> Result<Vec<ServiceOperation>, String> {
        match action {
            IntentAction::Playback(action) => {
                let action = if action.kind == "jump" {
                    self.jump(action, now).map_err(str::to_owned)?
                } else {
                    action.clone()
                };
                let response = analyze(&self.request(action, now));
                if let Some(error) = response.error {
                    return Err(error);
                }
                Ok(response
                    .actions
                    .into_iter()
                    .map(ServiceOperation::from)
                    .collect())
            }
            IntentAction::Volume(intent) => {
                if !self.volume_current(action) {
                    return Err("Media volume target or audio stream ownership changed; action was not retried.".into());
                }
                let stream_id = match intent.target {
                    VolumeTarget::Stream { id, .. } => Some(id),
                    _ => None,
                };
                let (operation, value, muted) = match intent.mutation {
                    VolumeMutation::Volume(value) => (
                        "volume",
                        Some(value.clamp(0.0, 1.0)),
                        (stream_id.is_some() && value > 0.0).then_some(false),
                    ),
                    VolumeMutation::Mute(muted) => ("mute", None, Some(muted)),
                };
                Ok(vec![ServiceOperation {
                    player: intent.primary.clone(),
                    operation: operation.into(),
                    value,
                    stream_id,
                    muted,
                }])
            }
        }
    }

    fn apply_guard(&self, operation: &ServiceOperation) -> NativeGuard {
        let mut guard = self.native.guard.clone();
        // Ordinary samples do not invalidate an intent when grouping is stable,
        // but an apply must not cross an unseen sample that could regroup it.
        for (expected, source) in guard.players.iter_mut().zip(&self.native.players) {
            expected["position"] = source.raw["position"].clone();
            expected["positionRevision"] = source.raw["positionRevision"].clone();
        }
        if matches!(operation.operation.as_str(), "volume" | "mute") {
            // Values normally do not affect ownership. Fence the exact raw
            // getter immediately before a setter so a readiness/fallback switch
            // cannot sneak past the frontend's otherwise opaque field checks.
            if operation.operation == "volume"
                && let Some(source) = self.player(&operation.player).filter(|p| p.native)
                && let Some(expected) = guard
                    .players
                    .iter_mut()
                    .find(|p| p["bus"] == operation.player)
            {
                expected["volume"] = source.raw["volume"].clone();
            }
            if let Some(id) = operation.stream_id
                && let Some(stream) = self.native.stream_views.iter().find(|s| s.id == id)
                && let Some(expected) = guard.streams.iter_mut().find(|s| s["id"] == id)
            {
                expected["volume"] = stream.raw["volume"].clone();
            }
        }
        guard
    }

    fn transport(&self, group: &Group) -> Transport {
        let Some(primary) = self.player(&group.primary).filter(|p| p.available()) else {
            return Transport {
                enabled: false,
                reason: "Media source changed or is not ready. Refresh and try again.".into(),
            };
        };
        let members: Option<Vec<_>> = group
            .players
            .iter()
            .map(|bus| self.player(bus).filter(|p| p.available()))
            .collect();
        let Some(members) = members else {
            return Transport {
                enabled: false,
                reason: "A media group member is unavailable. Refresh and try again.".into(),
            };
        };
        let playing = members.iter().any(|p| p.view.player.playing);
        let supported = members.iter().any(|p| {
            if playing {
                p.view.player.can_pause
            } else {
                p.view.player.can_play
            }
        });
        let operation = if playing { "pause" } else { "play" };
        let reason = if supported {
            if playing {
                "Pause".into()
            } else {
                "Play".into()
            }
        } else if primary
            .view
            .player
            .bus
            .starts_with("org.mpris.MediaPlayer2.mprisence_web.")
        {
            format!(
                "The browser bridge does not expose {operation} control for this source. Use the browser's playback controls."
            )
        } else {
            format!("This media source does not advertise {operation} support.")
        };
        Transport {
            enabled: supported,
            reason,
        }
    }

    fn progress(&self, now: f64) -> Vec<ProgressView> {
        self.players()
            .map(|p| {
                let mut view = p.view.clone();
                let sample = self
                    .timelines
                    .get(&view.player.bus)
                    .filter(|t| t.supported(now));
                view.position_supported = sample.is_some();
                view.player.position = sample.map_or(0.0, |t| {
                    let position = t.projected(now).max(0.0);
                    known_length(view.player.length).map_or(position, |length| position.min(length))
                });
                ProgressView {
                    view,
                    sample_position: sample.and_then(|t| t.sample_position),
                }
            })
            .collect()
    }

    fn state(&self, now: f64) -> ServiceState<'_> {
        ServiceState {
            kind: "state",
            ready: true,
            groups: &self.groups,
            active_player: &self.active,
            players: self.players().map(|p| &p.view).collect(),
            progress: self.progress(now),
            transports: self
                .groups
                .iter()
                .map(|g| (g.primary.as_str(), self.transport(g)))
                .collect(),
            volumes: self
                .groups
                .iter()
                .map(|g| (g.primary.as_str(), self.volume(g)))
                .collect(),
            failure: &self.failure,
            guard: &self.native.guard,
        }
    }
}

#[derive(Serialize)]
struct ProgressMessage {
    #[serde(rename = "type")]
    kind: &'static str,
    progress: Vec<ProgressView>,
}

#[derive(PartialEq)]
struct ProgressObservation {
    bus: String,
    generation: u64,
    sampled_at: f64,
    supported: bool,
    sample_position: Option<f64>,
}

/// Timeout polling still expires timing and recomputes ownership, but only UI
/// ticks or actual observations may publish another projected display frame.
#[derive(Default)]
struct ServiceOutput {
    state: Vec<u8>,
    observations: Vec<ProgressObservation>,
}

impl ServiceOutput {
    fn update(
        &mut self,
        runtime: &Runtime,
        now: f64,
        tick: bool,
    ) -> crate::AppResult<Option<Vec<u8>>> {
        let mut state = runtime.state(now);
        let progress = std::mem::take(&mut state.progress);
        let signature = service_bytes(&state)?;
        let observations: Vec<_> = runtime
            .players()
            .filter_map(|player| {
                let bus = &player.view.player.bus;
                runtime
                    .timelines
                    .get(bus)
                    .map(|sample| ProgressObservation {
                        bus: bus.clone(),
                        generation: sample.generation,
                        sampled_at: sample.sampled_at,
                        supported: sample.supported(now),
                        sample_position: sample.sample_position,
                    })
            })
            .collect();
        if signature != self.state {
            state.progress = progress;
            let bytes = service_bytes(&state)?;
            self.state = signature;
            self.observations = observations;
            Ok(Some(bytes))
        } else if tick || observations != self.observations {
            let bytes = service_bytes(&ProgressMessage {
                kind: "progress",
                progress,
            })?;
            self.observations = observations;
            Ok(Some(bytes))
        } else {
            Ok(None)
        }
    }
}

/// Bound serialized output as well as input. serde's errors are not displayed:
/// they may quote arbitrary native metadata or JSON from a rejected request.
fn service_bytes(message: &impl Serialize) -> crate::AppResult<Vec<u8>> {
    struct Limited(Vec<u8>);
    impl Write for Limited {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if bytes.len() > MAX_REQUEST_BYTES - 1 - self.0.len() {
                return Err(io::Error::other(
                    "Media service response exceeds the 1 MiB limit.",
                ));
            }
            self.0.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let mut buffer = Limited(Vec::new());
    serde_json::to_writer(&mut buffer, message)?;
    buffer.0.push(b'\n');
    Ok(buffer.0)
}

fn write_service(output: &mut impl Write, message: &impl Serialize) -> crate::AppResult<()> {
    output.write_all(&service_bytes(message)?)?;
    output.flush()?;
    Ok(())
}

// Astal does not poll paused tracks. These Rust-owned reads can confirm timing
// without overwriting its raw Position getter. Tokens fence track/status/rate,
// replacement, newer samples, and seek dispatch boundaries, even at equal values.
fn native_position(connection: &dbus::blocking::Connection, bus: &str) -> Option<f64> {
    use dbus::arg::{ArgType, RefArg, Variant};
    let proxy = connection.with_proxy(
        bus,
        "/org/mpris/MediaPlayer2",
        std::time::Duration::from_millis(1500),
    );
    let (Variant(value),): (Variant<Box<dyn RefArg>>,) = proxy
        .method_call(
            "org.freedesktop.DBus.Properties",
            "Get",
            ("org.mpris.MediaPlayer2.Player", "Position"),
        )
        .ok()?;
    if !matches!(
        value.arg_type(),
        ArgType::Byte
            | ArgType::Int16
            | ArgType::UInt16
            | ArgType::Int32
            | ArgType::UInt32
            | ArgType::Int64
            | ArgType::UInt64
            | ArgType::Double
    ) {
        return None;
    }
    value
        .as_f64()
        .or_else(|| value.as_i64().map(|v| v as f64))
        .or_else(|| value.as_u64().map(|v| v as f64))
        .filter(|v| v.is_finite() && *v >= 0.0)
        .map(|v| v / 1e6)
}

struct NativePositions {
    requests: std::sync::mpsc::SyncSender<PositionRequest>,
    results: std::sync::mpsc::Receiver<PositionResult>,
}

impl NativePositions {
    fn new() -> Self {
        let (requests, receiver) = std::sync::mpsc::sync_channel::<PositionRequest>(4);
        let (sender, results) = std::sync::mpsc::sync_channel(4);
        std::thread::spawn(move || {
            let connection = dbus::blocking::Connection::new_session().ok();
            while let Ok(request) = receiver.recv() {
                let position = connection
                    .as_ref()
                    .and_then(|c| native_position(c, &request.bus));
                if sender.send(PositionResult { request, position }).is_err() {
                    break;
                }
            }
        });
        Self { requests, results }
    }
}

/// JSONL resident driver. A bounded stdin reader lets recovery and display time
/// advance while a frontend apply result (or the next line) is outstanding.
/// EOF/restart drops every pending intent; dispatch failures are never retried.
pub fn serve_service() -> crate::AppResult<()> {
    let recovery = match crate::playback::Recovery::new() {
        Ok(recovery) => Some(recovery),
        Err(error) => {
            eprintln!(
                "media-control: private-player recovery unavailable; continuing with native players only: {}",
                failure_text(&error.to_string())
            );
            None
        }
    };
    let stdout = io::stdout();
    serve_service_io(
        io::BufReader::new(io::stdin()),
        BufWriter::new(stdout.lock()),
        recovery,
        Some(NativePositions::new()),
    )
}

// Injectable I/O and absent D-Bus adapters keep protocol tests desktop-independent.
fn serve_service_io(
    mut input: impl BufRead + Send + 'static,
    mut output: impl Write,
    recovery: Option<crate::playback::Recovery>,
    positions: Option<NativePositions>,
) -> crate::AppResult<()> {
    use crate::playback::{Recovery, monotonic_seconds};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::{self, RecvTimeoutError, TrySendError};
    use std::time::Duration;

    enum Input {
        Data(Vec<u8>),
        Invalid(&'static str),
        Eof,
    }
    let closed = Arc::new(AtomicBool::new(false));
    let reader_closed = closed.clone();
    let (sender, receiver) = mpsc::sync_channel(8);
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        loop {
            let line = match read_line(&mut input, &mut bytes) {
                Ok(Line::Data) => Input::Data(std::mem::take(&mut bytes)),
                Ok(Line::TooLong) => Input::Invalid("Request line exceeds the 1 MiB limit."),
                Ok(Line::Eof) => {
                    // EOF is a cancellation boundary, not an action queued behind
                    // buffered input. Never drain mutations after the client dies.
                    reader_closed.store(true, Ordering::Release);
                    let _ = sender.send(Input::Eof);
                    break;
                }
                Err(_) => {
                    let _ = sender.send(Input::Invalid("Media service input failed."));
                    break;
                }
            };
            if sender.send(line).is_err() {
                break;
            }
        }
    });
    let mut runtime = Runtime::new(monotonic_seconds());
    write_service(&mut output, &serde_json::json!({"type": "ready"}))?;
    let mut emitted = ServiceOutput::default();
    loop {
        let input = match receiver.recv_timeout(Duration::from_millis(500)) {
            Ok(input) => Some(input),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => return Ok(()),
        };
        if closed.load(Ordering::Acquire) {
            return Ok(());
        }
        let now = monotonic_seconds();
        let received = input.is_some();
        let mut tick = false;
        // Observe status evidence before a result can complete an in-flight plan.
        if let Some(recovery) = &recovery
            && recovery.take_changed()
        {
            runtime.update_native_playback(recovery.native_playback(), now);
            runtime.update_recovered(recovery.players(), now);
        }
        match input {
            Some(Input::Eof) => return Ok(()),
            Some(Input::Invalid(error)) => runtime.fail(error),
            Some(Input::Data(bytes)) => match serde_json::from_slice::<ServiceMessage>(&bytes) {
                Ok(message) => {
                    tick = matches!(&message, ServiceMessage::Tick { .. });
                    runtime.handle(message, now);
                }
                Err(error) => runtime.fail(&format!(
                    "Invalid media service JSON at line {}, column {}.",
                    error.line(),
                    error.column()
                )),
            },
            None => {}
        }
        if let Some(recovery) = &recovery {
            let buses: Vec<_> = runtime
                .native
                .players
                .iter()
                .map(|p| p.view.player.bus.clone())
                .collect();
            recovery.update(&runtime.native.streams, &buses);
            let players: Vec<_> = runtime
                .native
                .players
                .iter()
                .map(|p| (p.view.player.bus.clone(), p.view.instance))
                .collect();
            recovery.watch_native(&players);
            if recovery.take_changed() {
                runtime.update_native_playback(recovery.native_playback(), now);
                runtime.update_recovered(recovery.players(), now);
            }
        }
        if let Some(positions) = &positions {
            for result in positions.results.try_iter() {
                runtime.position_result(result, now);
            }
        }
        while let Some(request) = runtime.reads.pop_front() {
            match positions
                .as_ref()
                .map(|p| p.requests.try_send(request.clone()))
            {
                Some(Ok(())) => {}
                Some(Err(TrySendError::Full(_))) => {
                    runtime.reads.push_front(request);
                    break;
                }
                None | Some(Err(TrySendError::Disconnected(_))) => {
                    runtime.position_result(
                        PositionResult {
                            request,
                            position: None,
                        },
                        now,
                    );
                }
            }
        }
        // Each native apply yields to the input loop. Private-owner sends stay
        // nonblocking, with fresh recovery observations before and after each one.
        loop {
            if closed.load(Ordering::Acquire) {
                return Ok(());
            }
            let now = monotonic_seconds();
            if let Some(recovery) = &recovery {
                runtime.update_native_playback(recovery.native_playback(), now);
                runtime.update_recovered(recovery.players(), now);
            }
            let Some(apply) = runtime.advance(now) else {
                break;
            };
            if closed.load(Ordering::Acquire) {
                return Ok(());
            }
            if apply.recovered {
                let result = recovery.as_ref().map_or_else(
                    || Err("The recovered media player disappeared.".into()),
                    |recovery| {
                        recovery
                            .dispatch(&apply.operation.playback(), |players, statuses| {
                                runtime.validate_recovered_apply(
                                    apply.id,
                                    players,
                                    statuses,
                                    monotonic_seconds(),
                                )
                            })
                            .map_err(|e| e.to_string())
                    },
                );
                let players = recovery.as_ref().map_or_else(Vec::new, Recovery::players);
                runtime.finish_recovered(apply.id, result, players, monotonic_seconds());
            } else {
                match service_bytes(&apply) {
                    Ok(bytes) => { output.write_all(&bytes)?; output.flush()?; }
                    Err(_) => runtime.protocol_failure("Media service apply exceeds the 1 MiB limit; pending actions were discarded, not retried."),
                }
                break;
            }
        }
        let now = monotonic_seconds();
        let bytes = match emitted.update(&runtime, now, tick) {
            Ok(bytes) => bytes,
            Err(_) => {
                runtime.protocol_failure("Media service state exceeds the 1 MiB limit; pending actions were discarded, not retried.");
                emitted.update(&runtime, now, true)?
            }
        };
        if let Some(bytes) = bytes {
            output.write_all(&bytes)?;
            output.flush()?;
        } else if received {
            // No-op snapshots still need a liveness reply. Never manufacture a
            // media change (or a display tick) just to satisfy the IPC watchdog.
            write_service(&mut output, &serde_json::json!({"type": "ack"}))?;
        }
        if let Some(id) = runtime.capture_id(now) {
            write_service(
                &mut output,
                &serde_json::json!({"type": "capture", "id": id}),
            )?;
        }
    }
}

#[cfg(test)]
media_unit_tests!(resident);
