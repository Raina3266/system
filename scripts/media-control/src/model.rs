//! Pure media policy for Quickshell's native MPRIS/PipeWire snapshots.
//!
//! The `media-control` resident command dispatches to [`serve`]. Each input
//! line is a complete [`Request`]; each flushed output line is a
//! [`Response`]. The caller retains selection by sending the previous
//! `activePlayer` as `selectedPlayer`. No player, stream, or selector is cached.
//! QML executes [`NativeAction`]s through native players and passes a [`Route`]
//! to the existing `stream-move` command. This module performs no external I/O
//! other than the stdio protocol and never executes D-Bus or routing commands.

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
    pub position: f64,
    /// Track length in seconds; absent, nonfinite or nonpositive means unknown.
    #[serde(default)]
    pub length: Option<f64>,
    pub can_seek: bool,
    pub can_next: bool,
    pub can_previous: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Stream {
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
    #[serde(default)]
    pub destination: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Response {
    pub request_id: u64,
    pub groups: Vec<Group>,
    pub active_player: String,
    pub actions: Vec<NativeAction>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub route: Option<Route>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Group {
    pub title: String,
    pub artist: String,
    pub primary: String,
    pub players: Vec<String>,
    pub display_name: String,
    pub art_url: String,
    /// Null when ownership or stable stream identity cannot be established.
    pub stream_id: Option<u32>,
    /// Always empty or `serial:<positive decimal integer>`, never a node ID.
    pub selector: String,
    pub routing_reason: String,
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

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Route {
    /// The requested member's bus, even if another publisher is now primary.
    pub player: String,
    pub selector: String,
    pub destination: String,
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
/// Cards and their members retain snapshot order; display metadata comes from
/// the keeper. Routing uses trimmed track evidence. Nonfinite positions are
/// rejected, not replaced with zero or treated as unknown merge evidence.
/// Next/previous address only the group's primary, gated by its capability.
pub fn analyze(request: &Request) -> Response {
    if request.players.iter().any(|p| !p.position.is_finite()) {
        return Response::failure(request.request_id, "Player positions must be finite.");
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
                artist: group.artist.to_owned(),
                primary: group.primary.bus.clone(),
                players: group.players.iter().map(|p| p.bus.clone()).collect(),
                display_name: display_name(group.primary),
                art_url: artwork_url(&group.primary.art_url),
                stream_id: matched.node.map(|node| node.id),
                selector: matched.node.map(selector_for).unwrap_or_default(),
                routing_reason: matched.reason.to_owned(),
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
    artist: &'a str,
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
                artist: &player.artist,
                players: vec![player],
                primary: player,
            });
        }
    }
    // Keeper selection is playing-first, not the order of cards or commands.
    for group in &mut groups {
        group
            .players
            .sort_by_key(|player| order[player.bus.as_str()]);
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

fn artwork_url(url: &str) -> String {
    if web_url(url).is_some_and(|url| url.host.eq_ignore_ascii_case("i.ytimg.com"))
        && let Some((path, _)) = url.split_once('?')
    {
        return path.to_owned();
    }
    url.to_owned()
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
            // JavaScript String(42.0) is "42". Keep integer JSON values exact,
            // and leave exponential/fractional serials ineligible for routing.
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

fn selector_for(node: &Stream) -> String {
    if node.ready {
        let serial = value_text(node.properties.get("object.serial"));
        if !serial.is_empty()
            && serial.bytes().all(|byte| byte.is_ascii_digit())
            && serial.bytes().any(|byte| byte != b'0')
        {
            return format!("serial:{serial}");
        }
    }
    String::new()
}

const NO_MATCH: &str = "No matching audio stream. Start playback in this source.";
const READING: &str = "Reading audio stream details. Try again in a moment.";
const NO_STABLE_ID: &str = "No stable audio stream identity is available for independent routing.";
const BROWSER_SHARED: &str =
    "Several sources share this browser; a distinct track-specific audio stream is required.";
const MULTIPLE: &str =
    "Multiple audio streams match this source; independent routing is unavailable.";
const SHARED: &str = "Shared or ambiguous audio stream; independent routing is unavailable.";
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
    let selectors: Vec<_> = nodes.iter().map(|node| selector_for(node)).collect();
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
            .filter(|&index| !selectors[index].is_empty())
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
            if scoped.iter().any(|&index| !nodes[index].ready) {
                READING
            } else {
                NO_STABLE_ID
            }
        } else {
            "Application playback stream"
        });
        candidates.push(if detailed.is_empty() { ready } else { detailed });
    }

    // Keep original claims even when a browser source is denied routing.
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
        "toggle" | "next" | "previous" | "seek" | "move" => {}
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
        "move" => {
            let selector = &response.groups[index].selector;
            if action.destination.trim().is_empty() {
                response.error = Some("An output destination is required.".to_owned());
            } else if selector.is_empty() {
                response.error = Some(response.groups[index].routing_reason.clone());
            } else {
                response.route = Some(Route {
                    player: if action.player.is_empty() {
                        group.primary.bus.clone()
                    } else {
                        action.player.clone()
                    },
                    selector: selector.clone(),
                    destination: action.destination.clone(),
                });
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufReader, Cursor};

    use serde_json::json;

    fn player(bus: &str, app: &str, title: &str) -> Player {
        Player {
            bus: bus.to_owned(),
            identity: app.to_owned(),
            title: title.to_owned(),
            length: Some(203.0),
            playing: true,
            can_seek: true,
            can_next: true,
            can_previous: true,
            ..Player::default()
        }
    }

    fn node(id: u32, app: &str, title: &str) -> Stream {
        Stream {
            id,
            name: app.to_owned(),
            ready: true,
            properties: Map::from_iter([
                ("application.name".to_owned(), json!(app)),
                ("media.name".to_owned(), json!(title)),
                ("object.serial".to_owned(), json!((1000 + id).to_string())),
            ]),
            ..Stream::default()
        }
    }

    fn request(players: Vec<Player>, streams: Vec<Stream>) -> Request {
        Request {
            request_id: 7,
            players,
            streams,
            ..Request::default()
        }
    }

    fn action(kind: &str, player: &str) -> Action {
        Action {
            kind: kind.to_owned(),
            player: player.to_owned(),
            ..Action::default()
        }
    }

    // Isolate stream ownership from player filtering and duplicate matching.
    fn routing<'a>(players: &[Player], streams: &'a [Stream]) -> Vec<StreamMatch<'a>> {
        let groups: Vec<_> = players
            .iter()
            .map(|player| Members {
                title: &player.title,
                artist: &player.artist,
                players: vec![player],
                primary: player,
            })
            .collect();
        match_groups(&groups, streams)
    }

    fn matched_ids(players: &[Player], streams: &[Stream]) -> Vec<Option<u32>> {
        routing(players, streams)
            .iter()
            .map(|matched| matched.node.map(|node| node.id))
            .collect()
    }

    #[test]
    fn routing_distinct_apps_and_order() {
        let mut players = vec![
            player("spotify", "Spotify", "Same"),
            player("firefox", "Firefox", "Same"),
        ];
        let mut nodes = vec![node(1, "Spotify", "Same"), node(2, "Firefox", "Same")];
        for _ in 0..2 {
            for _ in 0..2 {
                let matches = routing(&players, &nodes);
                for (source, matched) in players.iter().zip(matches) {
                    assert_eq!(matched.node.unwrap().name, source.identity);
                }
                nodes.reverse();
            }
            players.reverse();
        }
        assert_eq!(
            matched_ids(&[player("other", "Other", "Same")], &nodes),
            [None]
        );
    }

    #[test]
    fn routing_titles_and_urls_are_trimmed_application_scoped_evidence() {
        let mut players = vec![player("a", "Chromium", " A "), player("b", "Chromium", "B")];
        let mut nodes = vec![node(1, "Chromium", "A"), node(2, "Chromium", "B")];
        assert_eq!(matched_ids(&players, &nodes), [Some(1), Some(2)]);
        nodes.reverse();
        assert_eq!(matched_ids(&players, &nodes), [Some(1), Some(2)]);
        players[0].url = " https://example/media ".to_owned();
        nodes[1]
            .properties
            .insert("media.name".to_owned(), json!("Unknown"));
        for key in ["media.url", "media.filename"] {
            nodes[1]
                .properties
                .insert(key.to_owned(), json!(" https://example/media "));
            assert_eq!(matched_ids(&players, &nodes), [Some(1), Some(2)]);
            nodes[1].properties.remove(key);
        }
        nodes[1]
            .properties
            .insert("media.title".to_owned(), json!(" A "));
        assert_eq!(matched_ids(&players, &nodes), [Some(1), Some(2)]);
        assert_eq!(
            matched_ids(&[player("other", "Other", "A")], &nodes),
            [None]
        );
    }

    #[test]
    fn routing_shared_and_multiple_streams_are_never_consumed_in_order() {
        let players = vec![player("a", "Chromium", ""), player("b", "Chromium", "")];
        let mut nodes = vec![node(1, "Chromium", "")];
        let matches = routing(&players, &nodes);
        assert!(matches.iter().all(|matched| matched.node.is_none()));
        assert!(matches[0].reason.to_lowercase().contains("share"));
        nodes.push(node(2, "Chromium", ""));
        assert_eq!(routing(&players[..1], &nodes)[0].reason, MULTIPLE);
        let native = [player("a", "Spotify", ""), player("b", "Spotify", "")];
        let native_nodes = [node(3, "Spotify", "")];
        let matches = routing(&native, &native_nodes);
        assert!(matches.iter().all(|matched| matched.reason == SHARED));
    }

    #[test]
    fn routing_active_paused_and_aliases() {
        let mut players = vec![
            player("paused", "Google Chrome", ""),
            player("playing", "Chromium", ""),
        ];
        players[0].playing = false;
        let nodes = [node(1, "chrome", "")];
        assert_eq!(matched_ids(&players, &nodes), [None, None]);
        players.reverse();
        assert_eq!(matched_ids(&players, &nodes), [None, None]);
        assert_eq!(
            matched_ids(&players, &[node(2, "Spotify", "")]),
            [None, None]
        );
        assert_eq!(
            matched_ids(
                &[player("a", "Chrome", "")],
                &[node(1, "Chrome", ""), node(2, "Chromium", "")]
            ),
            [None]
        );
        for alias in [
            "Google Chrome",
            "google-chrome",
            "google-chrome-stable",
            "chrome",
            "org.chromium.chromium",
            "org.google.chrome",
            "Chromium.desktop",
        ] {
            assert_eq!(canonical(alias), "chromium");
        }
        for alias in [
            "Mozilla Firefox",
            "org.mozilla.firefox",
            " Firefox.desktop ",
        ] {
            assert_eq!(canonical(alias), "firefox");
        }
    }

    #[test]
    fn routing_missing_unready_and_unstable_streams() {
        let players = [player("a", "Spotify", "")];
        assert_eq!(routing(&players, &[])[0].reason, NO_MATCH);
        let mut stream = node(1, "Spotify", "");
        stream.ready = false;
        assert_eq!(routing(&players, &[stream.clone()])[0].reason, READING);
        assert_eq!(
            matched_ids(&[player("a", "Spot", "")], &[stream.clone()]),
            [None]
        );
        stream.ready = true;
        stream.properties.remove("object.serial");
        assert_eq!(routing(&players, &[stream])[0].reason, NO_STABLE_ID);
    }

    #[test]
    fn routing_native_keys_and_explicit_browser_hint_do_not_mutate_inputs() {
        let mut source = player("a", "Chromium", "");
        source.browser_hint = json!("Firefox");
        let mut firefox = node(1, "unrelated-node", "");
        firefox
            .properties
            .insert("application.process.binary".to_owned(), json!("firefox"));
        let chrome = node(2, "Chromium", "");
        let before = (source.clone(), firefox.clone(), chrome.clone());
        assert_eq!(
            matched_ids(&[source.clone()], &[chrome.clone(), firefox.clone()]),
            [Some(1)]
        );
        assert_eq!((source.clone(), firefox.clone(), chrome), before);
        for hint in [
            json!(true),
            json!(false),
            Value::Null,
            json!("unknown"),
            json!(["Firefox"]),
        ] {
            source.browser_hint = hint;
            assert_eq!(matched_ids(&[source.clone()], &[firefox.clone()]), [None]);
        }
        source = player("a", "Unknown", "");
        source.desktop_entry = "org.example.Player.desktop".to_owned();
        firefox
            .properties
            .insert("application.id".to_owned(), json!("org.example.Player"));
        assert_eq!(matched_ids(&[source], &[firefox]), [Some(1)]);
    }

    #[test]
    fn routing_each_native_application_key_and_nickname_is_used() {
        let players = [player("a", "org.example.Player", "")];
        for key in [
            "application.name",
            "application.id",
            "application.process.binary",
        ] {
            let mut stream = node(1, "unrelated", "");
            stream
                .properties
                .insert(key.to_owned(), json!(" Org.Example.Player.desktop "));
            assert_eq!(matched_ids(&players, &[stream]), [Some(1)]);
        }
        let mut stream = node(1, "unrelated", "");
        stream.nickname = "org.example.Player".to_owned();
        assert_eq!(matched_ids(&players, &[stream]), [Some(1)]);
    }

    #[test]
    fn routing_selectors_only_use_positive_stable_serials() {
        let mut stream = node(42, "Spotify", "");
        assert_eq!(selector_for(&stream), "serial:1042");
        for (serial, expected) in [
            (json!("42"), "serial:42"),
            (json!(42), "serial:42"),
            (json!(42.0), "serial:42"),
            (json!(" 0042 "), "serial:0042"),
        ] {
            stream.properties.insert("object.serial".to_owned(), serial);
            assert_eq!(selector_for(&stream), expected);
        }
        for invalid in [
            json!("0"),
            json!("000"),
            json!(""),
            json!(0),
            json!(-1),
            json!(1.2),
            json!("bad"),
            json!("+1"),
            json!("1.0"),
            json!("1e3"),
            json!(true),
            Value::Null,
        ] {
            stream
                .properties
                .insert("object.serial".to_owned(), invalid);
            assert_eq!(selector_for(&stream), "");
        }
        stream
            .properties
            .insert("object.serial".to_owned(), json!("42"));
        stream.ready = false;
        assert_eq!(selector_for(&stream), "");
        stream.ready = true;
        stream.properties.clear();
        for id in [0, 1, 42, u32::MAX] {
            stream.id = id;
            assert_eq!(selector_for(&stream), "");
        }
    }

    #[test]
    fn routing_paused_with_explicit_track_evidence() {
        let mut players = vec![
            player("a", "Chromium", "Paused track"),
            player("b", "Chromium", "Playing track"),
        ];
        players[0].playing = false;
        let mut nodes = vec![
            node(1, "chrome", "Paused track"),
            node(2, "chrome", "Playing track"),
        ];
        assert_eq!(matched_ids(&players, &nodes), [Some(1), Some(2)]);
        players.reverse();
        nodes.reverse();
        assert_eq!(matched_ids(&players, &nodes), [Some(2), Some(1)]);
    }

    #[test]
    fn routing_shared_stream_even_with_one_matching_title() {
        let mut players = vec![player("a", "Chromium", "A"), player("b", "Chromium", "B")];
        let nodes = [node(1, "chrome", "A")];
        for _ in 0..2 {
            let matches = routing(&players, &nodes);
            assert!(matches.iter().all(|matched| matched.node.is_none()));
            assert!(matches.iter().any(|matched| matched.reason == SHARED));
            assert!(
                matches
                    .iter()
                    .any(|matched| matched.reason == BROWSER_SHARED)
            );
            players.reverse();
        }
    }

    #[test]
    fn routing_paused_stream_cannot_be_claimed_by_new_playing_source() {
        let mut players = vec![
            player("a", "Chromium", "Old track"),
            player("b", "Chromium", "New track"),
        ];
        players[0].playing = false;
        let mut nodes = vec![node(1, "chrome", "Old track")];
        assert_eq!(matched_ids(&players, &nodes), [None, None]);
        let mut fresh = node(2, "chrome", "New track");
        fresh.ready = false;
        nodes.push(fresh);
        assert_eq!(matched_ids(&players, &nodes), [None, None]);
        nodes[1].ready = true;
        assert_eq!(matched_ids(&players, &nodes), [Some(1), Some(2)]);
    }

    #[test]
    fn routing_deduplicates_service_wrappers_but_not_distinct_nodes() {
        let players = [player("a", "Spotify", "Track")];
        let stream = node(1, "Spotify", "Track");
        assert_eq!(matched_ids(&players, &[stream.clone(), stream]), [Some(1)]);
        let stream = node(0, "Spotify", "Track");
        assert_eq!(matched_ids(&players, &[stream.clone(), stream]), [None]);
        let mut stream = node(2, "Spotify", "Track");
        stream
            .properties
            .insert("object.serial".to_owned(), json!("1001"));
        assert_eq!(
            matched_ids(&players, &[node(1, "Spotify", "Track"), stream]),
            [None]
        );
    }

    #[test]
    fn routing_browser_requires_exactly_one_detailed_match_when_shared() {
        let players = [player("a", "Chromium", "A"), player("b", "Chromium", "B")];
        let nodes = [
            node(1, "Chromium", "A"),
            node(2, "Chromium", "A"),
            node(3, "Chromium", "B"),
        ];
        let matches = routing(&players, &nodes);
        assert_eq!(matches[0].reason, BROWSER_SHARED);
        assert_eq!(matches[1].node.unwrap().id, 3);
    }

    #[test]
    fn browser_allowlist_checks_media_paths_and_most_specific_host() {
        for url in [
            "https://youtu.be/id",
            "https://www.youtube.com/watch?v=id",
            "https://youtube.com/shorts/id",
            "https://youtube.com/live/id",
            "https://youtube.com/embed/id",
            "https://youtube.com/playlist?list=id",
            "https://youtube.com/v/id",
            "https://youtube.com/e/id",
            "https://music.youtube.com/watch?v=id",
            "https://music.youtube.com/playlist?list=id",
            "https://www.bilibili.com/video/BV1",
            "https://bilibili.com/bangumi/play/id",
            "https://bilibili.com/cheese/id",
            "https://bilibili.com/medialist/id",
            "https://bilibili.com/list/id",
            "https://bilibili.com/festival/id",
            "https://bilibili.com/audio/id",
            "https://b23.tv/id",
            "https://live.bilibili.com/123",
            "https://open.spotify.com/track/id",
            "https://spotify.com/album/id",
            "https://spotify.com/playlist/id",
            "https://spotify.com/artist/id",
            "https://spotify.com/show/id",
            "https://spotify.com/episode/id",
            "https://spotify.com/collection/tracks",
            "https://open.spotify.com/intl-zh/track/id",
            "HTTPS://WWW.YOUTUBE.COM/WATCH?v=id",
            "http://youtube.com:8080/watch?v=id",
        ] {
            assert!(browser_media_allowed(url), "should allow {url}");
        }
        for url in [
            "",
            "https://youtube.com",
            "https://youtube.com/?v=id",
            "https://youtu.be/",
            "https://youtube.com/feed/subscriptions",
            "https://youtube.com/watchlater",
            "https://music.youtube.com/shorts/id",
            "https://music.youtube.com/",
            "https://bilibili.com/",
            "https://live.bilibili.com/",
            "https://b23.tv/",
            "https://spotify.com/intl-zh/",
            "https://github.com/watch/id",
            "https://docs.example.com/video/id",
            "file:///watch/id",
            "ftp://youtube.com/watch",
            "https://youtube.com/#watch/id",
            "https://youtube.com/?next=/watch/id",
        ] {
            assert!(!browser_media_allowed(url), "should reject {url}");
        }
    }

    #[test]
    fn browser_allowlist_and_display_names_respect_host_boundaries() {
        for url in [
            "https://notyoutube.com/watch",
            "https://youtube.com.evil.test/watch",
            "https://evil.test/youtube.com/watch",
            "https://youtube.com@evil.test/watch",
            "https://evil.test@youtube.com/watch",
            "https://youtube.com\\@evil.test/watch",
            "https://youtube.com%2eevil.test/watch",
            "https://youtube.com:bad/watch",
            "https://youtube.com:65536/watch",
            "https://youtube.com./watch",
            "https://youtube..com/watch",
            "https:// youtube.com/watch",
            "https://youtube.com/watch\n",
        ] {
            assert!(!browser_media_allowed(url), "should reject {url:?}");
            let mut p = player("a", "Chromium", "");
            p.url = url.to_owned();
            assert_eq!(display_name(&p), "Chromium");
        }
        for (url, name) in [
            ("https://music.youtube.com/watch", "YouTube Music"),
            ("https://www.youtube.com/watch", "YouTube"),
            ("https://youtu.be/id", "YouTube"),
            ("https://live.bilibili.com/123", "Bilibili Live"),
            ("https://www.bilibili.com/video/id", "Bilibili"),
            ("https://b23.tv/id", "Bilibili"),
            ("https://open.spotify.com/track/id", "Spotify"),
        ] {
            let mut p = player("a", "Chromium", "");
            p.url = url.to_owned();
            assert_eq!(display_name(&p), name);
        }
    }

    #[test]
    fn display_names_fall_back_to_friendly_sources_after_browser_overrides() {
        for identity in ["", " \t ", "mprisence_web.youtube_music.pa808"] {
            let mut p = player(
                "org.mpris.MediaPlayer2.mprisence_web.youtube_music.pa808",
                identity,
                "Track",
            );
            assert_eq!(display_name(&p), "Youtube Music");
            p.browser_hint = json!(true);
            p.url = "https://music.youtube.com/watch?v=id".to_owned();
            assert_eq!(display_name(&p), "YouTube Music");
        }
        let p = player("org.mpris.MediaPlayer2.elisa", "", "Track");
        let response = analyze(&request(vec![p], vec![]));
        assert_eq!(response.groups[0].display_name, "Elisa");
        assert_eq!(
            display_name(&player("bus", " Custom Player ", "")),
            " Custom Player "
        );
    }

    #[test]
    fn browser_hint_presence_filters_but_boolean_is_not_browser_identity() {
        for hint in [json!(true), json!(false), json!("Firefox"), json!("")] {
            let mut p = player("a", "Bridge", "Track");
            p.browser_hint = hint;
            assert!(browser_player(&p));
            assert!(analyze(&request(vec![p.clone()], vec![])).groups.is_empty());
            p.url = "https://youtube.com/watch?v=id".to_owned();
            assert_eq!(
                analyze(&request(vec![p], vec![])).groups[0].display_name,
                "YouTube"
            );
        }
        let mut p = player("a", "Native", "Track");
        p.url = "https://youtube.com/watch?v=id".to_owned();
        assert!(!browser_player(&p));
        assert_eq!(display_name(&p), "Native");
    }

    #[test]
    fn artwork_strips_query_only_for_exact_youtube_image_host() {
        for url in [
            "https://i.ytimg.com/vi/id/hq.jpg?format=avif",
            "http://i.ytimg.com/vi/id/hq.jpg?x=1",
        ] {
            assert_eq!(artwork_url(url), url.split_once('?').unwrap().0);
        }
        for url in [
            "https://i.ytimg.com/vi/id/hq.jpg",
            "https://i.ytimg.com.evil.test/a.jpg?x=1",
            "https://sub.i.ytimg.com/a.jpg?x=1",
            "https://evil.test/i.ytimg.com/a.jpg?x=1",
            "https://i.ytimg.com@evil.test/a.jpg?x=1",
            "https://example.com/a.jpg?x=1",
            "file:///tmp/cover.jpg?x=1",
            "",
        ] {
            assert_eq!(artwork_url(url), url);
        }
    }

    #[test]
    fn grouping_filters_stopped_proxies_and_nonmedia_browsers() {
        let mut stopped = player("stopped", "Native", "Stopped");
        stopped.stopped = true;
        let mut tab = player("tab", "Chromium", "Tab");
        tab.url = "https://github.com/project".to_owned();
        let response = analyze(&request(
            vec![
                stopped,
                player("org.mpris.MediaPlayer2.PLAYERCTLD", "Proxy", "Proxy"),
                player(
                    "org.mpris.MediaPlayer2.kdeconnect.instance",
                    "Proxy",
                    "Proxy2",
                ),
                tab,
                player("native", "Native", "Native"),
            ],
            vec![],
        ));
        assert_eq!(response.groups.len(), 1);
        assert_eq!(response.groups[0].players, ["native"]);
    }

    #[test]
    fn grouping_preserves_keeper_metadata_and_five_second_window() {
        let mut a = player("a", "Native", "Track");
        a.artist = "Artist".to_owned();
        a.length = None;
        a.playing = false;
        a.position = 10.0;
        let mut b = a.clone();
        b.bus = "b".to_owned();
        b.playing = true;
        b.position = 15.0;
        b.title = " TRACK ".to_owned();
        b.artist = " ARTIST ".to_owned();
        b.album = "A different album does not veto artist agreement".to_owned();
        b.art_url = "https://i.ytimg.com/vi/id/hq.jpg?format=avif".to_owned();
        let mut c = a.clone();
        c.bus = "c".to_owned();
        c.position = 20.0;
        let snapshot = request(vec![a, b, c], vec![]);
        let before = snapshot.clone();
        let response = analyze(&snapshot);
        assert_eq!(snapshot, before);
        assert_eq!(response.groups.len(), 1);
        assert_eq!(response.groups[0].players, ["a", "b", "c"]);
        assert_eq!(response.groups[0].primary, "b");
        assert_eq!(response.groups[0].title, " TRACK ");
        assert_eq!(response.groups[0].artist, " ARTIST ");
        assert_eq!(
            response.groups[0].art_url,
            "https://i.ytimg.com/vi/id/hq.jpg"
        );
        let mut snapshot = snapshot;
        snapshot.players[1].position = 15.001;
        snapshot.players[2].title = "Other".to_owned();
        assert_eq!(analyze(&snapshot).groups.len(), 3);
        snapshot.players[1].position = 10.0;
        snapshot.players[1].artist = "Different".to_owned();
        assert_eq!(analyze(&snapshot).groups.len(), 3);
        snapshot.players[1].artist = "Artist".to_owned();
        assert_eq!(analyze(&snapshot).groups.len(), 2);
    }

    #[test]
    fn grouping_compares_only_the_keeper_not_any_matching_member() {
        let mut players = vec![
            player("a", "Native", "Track"),
            player("b", "Native", "Track"),
            player("c", "Native", "Track"),
        ];
        players[0].position = 10.0;
        players[1].position = 15.0;
        players[2].position = 20.0;
        let response = analyze(&request(players, vec![]));
        assert_eq!(response.groups.len(), 2);
        assert_eq!(response.groups[0].players, ["a", "b"]);
        assert_eq!(response.groups[0].primary, "a");
        assert_eq!(response.groups[1].players, ["c"]);
    }

    #[test]
    fn grouping_keeps_snapshot_card_and_member_order_with_playing_first_keepers() {
        let mut a = player("a", "Native", "A");
        a.playing = false;
        let mut last = player("last", "Native", "Last");
        last.playing = false;
        let snapshot = request(
            vec![
                a,
                player("b", "Native", "B"),
                player("alias", "Native", "A"),
                last,
            ],
            vec![],
        );
        let response = analyze(&snapshot);
        assert_eq!(response.groups.len(), 3);
        assert_eq!(response.groups[0].players, ["a", "alias"]);
        assert_eq!(response.groups[0].primary, "alias");
        assert_eq!(response.groups[1].players, ["b"]);
        assert_eq!(response.groups[2].players, ["last"]);
        assert_eq!(response.active_player, "alias");
    }

    #[test]
    fn grouping_requires_support_beyond_title_and_more_for_cross_source_mirrors() {
        let mut first = player("a", "Native", "Track");
        first.length = None;
        let mut arriving = first.clone();
        arriving.bus = "b".to_owned();
        let mut snapshot = request(vec![first, arriving], vec![]);
        assert_eq!(analyze(&snapshot).groups.len(), 2);

        for player in &mut snapshot.players {
            player.artist = "Artist".to_owned();
        }
        assert_eq!(analyze(&snapshot).groups.len(), 1);
        snapshot.players[1].identity = "Different source".to_owned();
        assert_eq!(analyze(&snapshot).groups.len(), 2);
        for player in &mut snapshot.players {
            player.album = "Album".to_owned();
        }
        assert_eq!(analyze(&snapshot).groups.len(), 1);
    }

    #[test]
    fn grouping_uses_only_known_lengths_with_a_two_second_tolerance() {
        let first = player("a", "Native", "Track");
        let mut second = first.clone();
        second.bus = "b".to_owned();
        second.length = Some(205.0);
        let mut snapshot = request(vec![first, second], vec![]);
        assert_eq!(analyze(&snapshot).groups.len(), 1);
        for length in [
            None,
            Some(0.0),
            Some(-1.0),
            Some(205.001),
            Some(f64::NAN),
            Some(f64::INFINITY),
        ] {
            snapshot.players[1].length = length;
            let response = analyze(&snapshot);
            assert!(response.error.is_none());
            assert_eq!(response.groups.len(), 2);
        }
    }

    #[test]
    fn grouping_treats_zero_positions_as_unknown() {
        let mut watched = player("watched", "Native", "Track");
        watched.position = 92.0;
        let unread = player("unread", "Native", "Track");
        let response = analyze(&request(vec![watched, unread], vec![]));
        assert_eq!(response.groups.len(), 1);
        assert_eq!(response.groups[0].players, ["watched", "unread"]);
    }

    #[test]
    fn grouping_does_not_merge_empty_tracks_or_duplicate_bus_names() {
        for title in ["", "  "] {
            let response = analyze(&request(
                vec![player("a", "Native", title), player("b", "Native", title)],
                vec![],
            ));
            assert_eq!(response.groups.len(), 2);
        }
        let p = player("a", "Native", "Track");
        let mut snapshot = request(vec![p.clone(), p], vec![]);
        snapshot.action = Some(action("toggle", "a"));
        let response = analyze(&snapshot);
        assert_eq!(response.groups[0].players, ["a"]);
        assert_eq!(response.actions.len(), 1);
    }

    #[test]
    fn nonfinite_positions_are_rejected_not_merged_as_unknown() {
        for position in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let mut snapshot = request(
                vec![
                    player("a", "Native", "Track"),
                    player("b", "Native", "Track"),
                ],
                vec![],
            );
            snapshot.players[1].position = position;
            snapshot.action = Some(action("toggle", "a"));
            let response = analyze(&snapshot);
            assert!(response.error.is_some());
            assert!(response.groups.is_empty());
            assert!(response.actions.is_empty());
            assert_eq!(response.request_id, 7);
        }
    }

    #[test]
    fn selection_retains_any_member_else_playing_else_first() {
        let mut first = player("a", "Native", "A");
        first.playing = false;
        let mut alias = first.clone();
        alias.bus = "alias".to_owned();
        let second = player("b", "Native", "B");
        let mut snapshot = request(vec![first, alias, second], vec![]);
        snapshot.selected_player = "alias".to_owned();
        assert_eq!(analyze(&snapshot).active_player, "a");
        snapshot.selected_player = "gone".to_owned();
        assert_eq!(analyze(&snapshot).active_player, "b");
        snapshot.players[2].playing = false;
        assert_eq!(analyze(&snapshot).active_player, "a");
        snapshot.players.clear();
        assert_eq!(analyze(&snapshot).active_player, "");
    }

    #[test]
    fn toggle_fans_out_absolute_operations_not_double_toggles() {
        let mut snapshot = request(
            vec![
                player("a", "Native", "Track"),
                player("b", "Native", "Track"),
            ],
            vec![],
        );
        snapshot.players[0].playing = false;
        snapshot.action = Some(action("toggle", "a"));
        let response = analyze(&snapshot);
        assert_eq!(response.actions.len(), 2);
        assert!(
            response
                .actions
                .iter()
                .all(|a| a.operation == "pause" && a.value.is_none())
        );
        assert_eq!(
            response
                .actions
                .iter()
                .map(|a| a.player.as_str())
                .collect::<Vec<_>>(),
            ["a", "b"]
        );
        snapshot.players[1].playing = false;
        let response = analyze(&snapshot);
        assert!(response.actions.iter().all(|a| a.operation == "play"));
    }

    #[test]
    fn next_and_previous_only_address_capable_primary() {
        let mut snapshot = request(
            vec![
                player("a", "Native", "Track"),
                player("b", "Native", "Track"),
            ],
            vec![],
        );
        snapshot.players[0].playing = false;
        for kind in ["next", "previous"] {
            snapshot.action = Some(action(kind, "a"));
            assert_eq!(
                analyze(&snapshot).actions,
                [native_action(&snapshot.players[1], kind, None)]
            );
        }
        snapshot.players[1].can_next = false;
        snapshot.players[1].can_previous = false;
        for kind in ["next", "previous"] {
            snapshot.action = Some(action(kind, "a"));
            assert!(analyze(&snapshot).actions.is_empty());
        }
    }

    #[test]
    fn seek_only_addresses_capable_members_and_valid_positions() {
        let mut snapshot = request(
            vec![
                player("a", "Native", "Track"),
                player("b", "Native", "Track"),
            ],
            vec![],
        );
        snapshot.players[0].can_seek = false;
        snapshot.action = Some(Action {
            position: 12.5,
            ..action("seek", "a")
        });
        assert_eq!(
            analyze(&snapshot).actions,
            [native_action(&snapshot.players[1], "seek", Some(12.5))]
        );
        snapshot.action.as_mut().unwrap().position = 0.0;
        assert_eq!(analyze(&snapshot).actions[0].value, Some(0.0));
        for position in [-1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            snapshot.action.as_mut().unwrap().position = position;
            let response = analyze(&snapshot);
            assert!(response.actions.is_empty());
            assert!(response.error.is_some());
        }
    }

    #[test]
    fn pause_all_includes_stopped_and_browser_filtered_players_not_proxies() {
        let mut stopped = player("stopped", "Native", "A");
        stopped.stopped = true;
        let mut paused = player("paused", "Native", "B");
        paused.playing = false;
        let tab = player("tab", "Chromium", "Docs");
        let mut snapshot = request(
            vec![
                stopped,
                tab,
                paused,
                player("playerctld", "Proxy", "X"),
                player("kdeconnect", "Proxy", "Y"),
            ],
            vec![],
        );
        snapshot.players.push(snapshot.players[1].clone());
        for kind in ["pauseAll", "togglePauseAll"] {
            snapshot.action = Some(action(kind, "irrelevant"));
            let response = analyze(&snapshot);
            assert_eq!(
                response
                    .actions
                    .iter()
                    .map(|a| a.player.as_str())
                    .collect::<Vec<_>>(),
                ["stopped", "tab"]
            );
            assert!(response.actions.iter().all(|a| a.operation == "pause"));
        }
    }

    #[test]
    fn toggle_pause_all_when_idle_resumes_selected_group_only() {
        let mut players = vec![
            player("a", "Native", "A"),
            player("alias", "Native", "A"),
            player("b", "Native", "B"),
            player("playerctld", "Proxy", "Proxy"),
        ];
        for player in &mut players[..3] {
            player.playing = false;
        }
        let mut snapshot = request(players, vec![]);
        snapshot.selected_player = "alias".to_owned();
        snapshot.action = Some(action("togglePauseAll", "b"));
        let response = analyze(&snapshot);
        assert_eq!(
            response
                .actions
                .iter()
                .map(|a| a.player.as_str())
                .collect::<Vec<_>>(),
            ["a", "alias"]
        );
        assert!(response.actions.iter().all(|a| a.operation == "play"));
        snapshot.players.clear();
        assert!(analyze(&snapshot).actions.is_empty());
    }

    #[test]
    fn actions_default_to_selection_but_explicit_missing_player_never_falls_back() {
        let mut snapshot = request(
            vec![player("a", "Native", "A"), player("b", "Native", "B")],
            vec![],
        );
        snapshot.selected_player = "b".to_owned();
        snapshot.action = Some(action("toggle", ""));
        assert_eq!(analyze(&snapshot).actions[0].player, "b");
        for kind in ["toggle", "next", "previous", "seek", "move"] {
            snapshot.action = Some(action(kind, "gone"));
            let response = analyze(&snapshot);
            assert!(response.actions.is_empty());
            assert!(response.route.is_none());
            assert!(response.error.is_some());
        }
        snapshot.action = Some(action("unknown", ""));
        assert_eq!(
            analyze(&snapshot).error.as_deref(),
            Some("Unknown media action.")
        );
    }

    #[test]
    fn move_targets_requested_group_not_active_group_and_keeps_member_identity() {
        let mut snapshot = request(
            vec![
                player("a", "AppA", "A"),
                player("alias", "AppA", "A"),
                player("b", "AppB", "B"),
            ],
            vec![node(1, "AppA", "A"), node(2, "AppB", "B")],
        );
        snapshot.selected_player = "b".to_owned();
        snapshot.action = Some(Action {
            destination: "output-key".to_owned(),
            ..action("move", "alias")
        });
        let response = analyze(&snapshot);
        assert_eq!(response.active_player, "b");
        assert_eq!(
            response.route,
            Some(Route {
                player: "alias".to_owned(),
                selector: "serial:1001".to_owned(),
                destination: "output-key".to_owned()
            })
        );
        assert!(response.actions.is_empty());
        assert!(response.error.is_none());
        snapshot.action.as_mut().unwrap().destination.clear();
        assert!(analyze(&snapshot).error.is_some());
    }

    #[test]
    fn move_rechecks_serial_readiness_ambiguity_and_requested_membership_each_time() {
        let mut snapshot = request(
            vec![player("a", "AppA", "A"), player("b", "AppB", "B")],
            vec![node(1, "AppA", "A"), node(2, "AppB", "B")],
        );
        snapshot.action = Some(Action {
            destination: "output-key".to_owned(),
            ..action("move", "a")
        });
        assert_eq!(analyze(&snapshot).route.unwrap().selector, "serial:1001");
        snapshot.streams[0]
            .properties
            .insert("object.serial".to_owned(), json!("2001"));
        assert_eq!(analyze(&snapshot).route.unwrap().selector, "serial:2001");
        snapshot.streams[0].ready = false;
        assert!(analyze(&snapshot).route.is_none());
        snapshot.streams[0].ready = true;
        snapshot.streams.push(node(3, "AppA", "A"));
        assert!(analyze(&snapshot).route.is_none());
        snapshot.streams.pop();
        snapshot.streams[0].properties.remove("object.serial");
        let response = analyze(&snapshot);
        assert!(response.route.is_none());
        assert_eq!(response.groups[0].stream_id, None);
        assert_eq!(response.groups[0].selector, "");
        snapshot.streams.remove(0);
        assert!(analyze(&snapshot).route.is_none());
        snapshot.players.remove(0);
        let response = analyze(&snapshot);
        assert_eq!(response.active_player, "b");
        assert!(response.route.is_none());
        assert!(response.error.is_some());
    }

    #[test]
    fn protocol_camel_case_defaults_and_optional_output_fields() {
        let wire = json!({
            "requestId": 123,
            "players": [{"bus":"a", "identity":"Native", "desktopEntry":"native",
                "title":"Track", "artist":"Artist", "album":"Album", "url":"", "artUrl":"",
                "playing":true, "stopped":false, "position":0.0,
                "canSeek":true, "canNext":true, "canPrevious":true}],
            "streams": [], "action": {"kind":"toggle"}
        });
        let snapshot: Request = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(snapshot.selected_player, "");
        assert!(snapshot.players[0].browser_hint.is_null());
        assert_eq!(snapshot.players[0].length, None);
        assert_eq!(snapshot.action.as_ref().unwrap(), &action("toggle", ""));
        let value = serde_json::to_value(analyze(&snapshot)).unwrap();
        assert_eq!(value["requestId"], 123);
        assert_eq!(value["activePlayer"], "a");
        assert_eq!(value["groups"][0]["displayName"], "Native");
        assert_eq!(value["groups"][0]["streamId"], Value::Null);
        assert_eq!(value["groups"][0]["selector"], "");
        assert!(value["groups"][0].get("artUrl").is_some());
        assert!(value["groups"][0].get("routingReason").is_some());
        assert!(value.get("request_id").is_none());
        assert!(value.get("route").is_none());
        assert!(value.get("error").is_none());
        assert!(value["actions"][0].get("value").is_none());
        let mut with_length = wire.clone();
        with_length["players"][0]["length"] = json!(203.5);
        let snapshot: Request = serde_json::from_value(with_length.clone()).unwrap();
        assert_eq!(snapshot.players[0].length, Some(203.5));
        assert_eq!(
            serde_json::to_value(&snapshot).unwrap()["players"][0]["length"],
            203.5
        );
        with_length["players"][0]["length"] = Value::Null;
        assert_eq!(
            serde_json::from_value::<Request>(with_length)
                .unwrap()
                .players[0]
                .length,
            None
        );
        let mut missing_position = wire;
        missing_position["players"][0]
            .as_object_mut()
            .unwrap()
            .remove("position");
        assert!(serde_json::from_value::<Request>(missing_position).is_err());
        assert!(serde_json::from_slice::<Request>(b"{\"requestId\":1,\"players\":[],\"streams\":[],\"action\":{\"kind\":\"seek\",\"position\":1e999}}").is_err());
    }

    #[derive(Default)]
    struct RecordingWriter {
        bytes: Vec<u8>,
        flushes: usize,
    }

    impl Write for RecordingWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            self.flushes += 1;
            Ok(())
        }
    }

    fn responses(writer: &RecordingWriter) -> Vec<Response> {
        writer
            .bytes
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
            .map(|line| serde_json::from_slice(line).unwrap())
            .collect()
    }

    #[test]
    fn stdio_keeps_serving_invalid_lines_with_safe_diagnostics_and_flushes() {
        let valid = serde_json::to_vec(&request(vec![], vec![])).unwrap();
        let mut input = b"{\"requestId\":\"private-token-\\u001b\"}\n\xff\n\n".to_vec();
        input.extend_from_slice(&valid);
        input.extend_from_slice(b"\r\n{\n");
        input.extend_from_slice(&valid); // A final request need not end in LF.
        let mut writer = RecordingWriter::default();
        serve_io(
            BufReader::with_capacity(13, Cursor::new(input)),
            &mut writer,
        )
        .unwrap();
        let output = responses(&writer);
        assert_eq!(output.len(), 6);
        assert_eq!(writer.flushes, 6);
        for index in [0, 1, 2, 4] {
            assert_eq!(output[index].request_id, 0);
            assert!(
                output[index]
                    .error
                    .as_ref()
                    .unwrap()
                    .contains("Invalid JSON request")
            );
            assert!(
                !output[index]
                    .error
                    .as_ref()
                    .unwrap()
                    .contains("private-token")
            );
            assert!(output[index].actions.is_empty());
        }
        for index in [3, 5] {
            assert_eq!(output[index].request_id, 7);
            assert!(output[index].error.is_none());
        }
        assert!(!writer.bytes.contains(&0x1b));
    }

    #[test]
    fn stdio_bounds_lines_drains_oversized_payload_and_resumes() {
        let valid = serde_json::to_vec(&request(vec![], vec![])).unwrap();
        let mut input = valid.clone();
        input.resize(MAX_REQUEST_BYTES, b' ');
        input.push(b'\n');
        input.extend(std::iter::repeat_n(b'x', MAX_REQUEST_BYTES + 1));
        input.push(b'\n');
        input.extend_from_slice(&valid);
        input.push(b'\n');
        let mut writer = RecordingWriter::default();
        serve_io(
            BufReader::with_capacity(4096, Cursor::new(input)),
            &mut writer,
        )
        .unwrap();
        let output = responses(&writer);
        assert_eq!(writer.flushes, 3);
        assert!(output[0].error.is_none());
        assert_eq!(output[1].request_id, 0);
        assert!(output[1].error.as_ref().unwrap().contains("1 MiB"));
        assert_eq!(output[2].request_id, 7);
        assert!(output[2].error.is_none());
    }

    #[test]
    fn stdio_handles_empty_input_and_oversized_final_line() {
        let mut writer = RecordingWriter::default();
        serve_io(Cursor::new(b""), &mut writer).unwrap();
        assert!(writer.bytes.is_empty());
        serve_io(Cursor::new(vec![b'x'; MAX_REQUEST_BYTES + 1]), &mut writer).unwrap();
        assert_eq!(responses(&writer).len(), 1);
        assert_eq!(writer.flushes, 1);
    }
}
