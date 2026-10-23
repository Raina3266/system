//! Pure media policy for the frontend's native MPRIS/audio snapshots.
//!
//! The `media-control model` resident command dispatches to [`serve`]. Each input
//! line is a complete [`Request`]; each flushed output line is a [`Response`].
//! The caller retains selection by sending the previous `activePlayer` as
//! `selectedPlayer`, executes [`NativeAction`]s through native players, and
//! resolves each group's `streamId` against its current streams for volume.
//! No player or stream is cached. This module performs no external I/O other
//! than the stdio protocol and never executes D-Bus or audio commands.

use std::collections::{HashMap, HashSet};
use std::io::{self, BufRead, BufWriter, Write};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::playback::{same_playback, source_name};

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
    /// Effective position in seconds, sampled/extrapolated by the frontend.
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
                title: group.title.to_owned(),
                primary: group.primary.bus.clone(),
                players: group.players.iter().map(|p| p.bus.clone()).collect(),
                display_name: display_name(group.primary),
                stream_id: matched.node.map(|node| node.id),
                stream_reason: matched.reason.to_owned(),
            })
            .collect(),
        ..Response::default()
    };
    if let Some(action) = &request.action {
        plan_action(request, action, &groups, active, &mut response);
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
    title: &'a str,
    players: Vec<&'a Player>,
    primary: &'a Player,
}

impl Members<'_> {
    fn contains(&self, bus: &str) -> bool {
        self.players.iter().any(|player| player.bus == bus)
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

fn group_players(players: &[Player]) -> Vec<Members<'_>> {
    let mut buses = HashSet::new();
    let mut candidates: Vec<_> = players
        .iter()
        .enumerate()
        .filter(|(_, player)| {
            !player.stopped
                && !proxy(player)
                && (!browser_player(player) || browser_media_allowed(&player.url))
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
                title: &player.title,
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
                    !player
                        .length
                        .is_some_and(|length| length.is_finite() && length > 0.0),
                    !player.can_seek,
                )
            })
            .expect("groups contain at least one player");
        group.title = &group.primary.title;
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
    if url
        .bytes()
        .any(|byte| byte.is_ascii_whitespace() || byte.is_ascii_control() || byte == b'\\')
    {
        return None;
    }
    let (scheme, rest) = url.split_once("://")?;
    if !scheme.eq_ignore_ascii_case("https") && !scheme.eq_ignore_ascii_case("http") {
        return None;
    }
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..end];
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
        path: rest[end..].split(['?', '#']).next().unwrap_or(""),
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
    if !group.title.trim().is_empty() {
        titles.push(group.title.trim());
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
    let claims = candidates.clone();
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
            let (node, reason) =
                match options.as_slice() {
                    [] => (None, reasons[index]),
                    [node] => {
                        if claims.iter().enumerate().any(|(other_index, claim)| {
                            other_index != index && claim.contains(node)
                        }) {
                            (None, SHARED)
                        } else {
                            (Some(nodes[*node]), UNIQUE)
                        }
                    }
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
    response: &mut Response,
) {
    match action.kind.as_str() {
        "pauseAll" => {
            response.actions = pause_actions(&request.players);
            return;
        }
        "togglePauseAll" => {
            response.actions = pause_actions(&request.players);
            if response.actions.is_empty()
                && let Some(index) = active
            {
                response.actions = groups[index]
                    .players
                    .iter()
                    .map(|p| native_action(p, "play", None))
                    .collect();
            }
            return;
        }
        "toggle" | "next" | "previous" | "seek" => {}
        _ => {
            response.error = Some("Unknown media action.".to_owned());
            return;
        }
    }
    if action.kind == "seek" && action.position < 0.0 {
        response.error = Some("Seek position must be finite and nonnegative.".to_owned());
        return;
    }
    let target = if action.player.is_empty() {
        active
    } else {
        groups
            .iter()
            .position(|group| group.contains(&action.player))
    };
    let Some(index) = target else {
        response.error = Some("The requested media source is no longer available.".to_owned());
        return;
    };
    let group = &groups[index];
    match action.kind.as_str() {
        "toggle" => {
            let operation = if group.players.iter().any(|p| p.playing) {
                "pause"
            } else {
                "play"
            };
            response.actions = group
                .players
                .iter()
                .map(|p| native_action(p, operation, None))
                .collect();
        }
        "next" if group.primary.can_next => {
            response.actions = vec![native_action(group.primary, "next", None)];
        }
        "previous" if group.primary.can_previous => {
            response.actions = vec![native_action(group.primary, "previous", None)];
        }
        "seek" => {
            response.actions = group
                .players
                .iter()
                .filter(|p| p.can_seek)
                .map(|p| native_action(p, "seek", Some(action.position)))
                .collect();
        }

        _ => {}
    }
}

#[cfg(test)]
media_unit_tests!(model);
