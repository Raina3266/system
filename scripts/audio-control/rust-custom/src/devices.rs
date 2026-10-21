//! Read-only supplementation for fields the pinned Astal model discards.
//!
//! Only canonical Astal cards select dump objects. Dump nodes, defaults, volume,
//! mute, card descriptions, and the raw active Profile are never inventory/state inputs.

use std::{
    collections::{HashMap, HashSet},
    env, io,
    io::Read,
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use serde::Deserialize;
use serde_json::Value;

use crate::{
    AppResult,
    model::{AudioKind, Card, Node, Snapshot, validate_text},
};

pub(crate) const MAX_DUMP_BYTES: u64 = 8 * 1024 * 1024;
const DUMP_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Clone, Debug, Default)]
pub struct Metadata {
    pub(crate) cards: HashMap<u32, CardData>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct CardData {
    pub profiles: Vec<Profile>,
    pub routes: Vec<Route>,
    pub active_routes: Vec<ActiveRoute>,
}

#[derive(Clone, Debug)]
pub(crate) struct Profile {
    pub index: i32,
    pub name: String,
    pub available: bool,
    pub priority: i32,
    pub sinks: Vec<u32>,
    pub sources: Vec<u32>,
}

#[derive(Clone, Debug)]
pub(crate) struct Route {
    pub index: i32,
    pub name: String,
    pub description: String,
    pub kind: AudioKind,
    pub available: bool,
    pub profiles: Vec<i32>,
    pub devices: Vec<u32>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct ActiveRoute {
    pub index: i32,
    pub device: u32,
    pub profile: i32,
}

fn relevant(card: &Card, snapshot: &Snapshot, kind: AudioKind) -> bool {
    (kind == AudioKind::Output && card.name.starts_with("alsa_card."))
        || snapshot.nodes.iter().any(|node| {
            node.kind == kind && node.card_id == Some(card.id) && node.profile_device.is_some()
        })
}

impl Metadata {
    pub fn load(snapshot: &Snapshot, kind: AudioKind) -> AppResult<Self> {
        if !snapshot
            .cards
            .iter()
            .any(|card| relevant(card, snapshot, kind))
        {
            return Ok(Self::default());
        }
        Self::from_dump(snapshot, kind, &read_dump()?)
    }

    pub fn from_dump(snapshot: &Snapshot, kind: AudioKind, dump: &Value) -> AppResult<Self> {
        let objects = dump
            .as_array()
            .ok_or("pw-dump did not return an object array")?;
        let mut cards = HashMap::new();
        for card in snapshot
            .cards
            .iter()
            .filter(|card| relevant(card, snapshot, kind))
        {
            let mut matches = objects
                .iter()
                .filter(|object| object["id"].as_u64() == Some(u64::from(card.id)));
            let object = matches
                .next()
                .ok_or("Astal card is absent from pw-dump; refresh native state")?;
            if matches.next().is_some() || object["type"] != "PipeWire:Interface:Device" {
                return Err("Ambiguous or non-device pw-dump card identity".into());
            }
            let props = &object["info"]["props"];
            let serial = match &props["object.serial"] {
                Value::String(serial) => Some(serial.clone()),
                Value::Number(serial) => serial.as_u64().map(|serial| serial.to_string()),
                _ => None,
            };
            if serial.as_deref() != Some(&card.serial)
                || props["device.name"].as_str() != Some(&card.name)
            {
                return Err(
                    "Astal card serial/name is stale relative to pw-dump; refresh native state"
                        .into(),
                );
            }
            cards.insert(card.id, CardData::parse(&object["info"]["params"])?);
        }
        Ok(Self { cards })
    }

    pub(crate) fn card(&self, card: &Card) -> Option<&CardData> {
        self.cards.get(&card.id)
    }

    pub(crate) fn routes_for<'a>(&'a self, card: &Card, node: &Node) -> Vec<&'a Route> {
        let Some(device) = node.profile_device else {
            return Vec::new();
        };
        self.card(card)
            .into_iter()
            .flat_map(|data| &data.routes)
            .filter(|route| {
                route.kind == node.kind
                    && route.profiles.contains(&card.active_profile)
                    && route.devices.contains(&device)
            })
            .collect()
    }

    pub(crate) fn port_active(&self, card: &Card, node: &Node, route: &Route) -> bool {
        self.card(card).is_some_and(|data| {
            data.active_routes.iter().any(|active| {
                active.index == route.index
                    && Some(active.device) == node.profile_device
                    && active.profile == card.active_profile
            })
        })
    }
}

impl CardData {
    fn parse(params: &Value) -> AppResult<Self> {
        let mut data = Self::default();
        let mut indexes = HashSet::new();
        let mut names = HashSet::new();
        for value in array(params, "EnumProfile")? {
            #[derive(Deserialize)]
            struct RawProfile {
                index: i32,
                name: String,
                #[serde(default)]
                priority: i32,
                available: String,
                classes: Value,
            }
            let raw: RawProfile = serde_json::from_value(value.clone())?;
            validate_text(&raw.name)?;
            if raw.index < 0
                || raw.name.is_empty()
                || !indexes.insert(raw.index)
                || !names.insert(raw.name.clone())
            {
                return Err("Invalid or ambiguous EnumProfile identity".into());
            }
            let (sinks, sources) = parse_classes(&raw.classes)?;
            data.profiles.push(Profile {
                index: raw.index,
                name: raw.name,
                available: available(&raw.available)?,
                priority: raw.priority,
                sinks,
                sources,
            });
        }
        indexes.clear();
        names.clear();
        for value in array(params, "EnumRoute")? {
            #[derive(Deserialize)]
            struct RawRoute {
                index: i32,
                name: String,
                description: String,
                direction: String,
                available: String,
                #[serde(default)]
                profiles: Vec<i32>,
                #[serde(default)]
                devices: Vec<u32>,
            }
            let raw: RawRoute = serde_json::from_value(value.clone())?;
            validate_text(&raw.name)?;
            validate_text(&raw.description)?;
            let kind = match raw.direction.as_str() {
                "Input" => AudioKind::Input,
                "Output" => AudioKind::Output,
                _ => return Err("Unknown EnumRoute direction".into()),
            };
            if raw.index < 0
                || raw.name.is_empty()
                || !indexes.insert(raw.index)
                || !names.insert(format!("{}:{}", kind.key_prefix(), raw.name))
                || raw.profiles.iter().any(|index| *index < 0)
            {
                return Err("Invalid or ambiguous EnumRoute identity".into());
            }
            data.routes.push(Route {
                index: raw.index,
                name: raw.name,
                description: raw.description,
                kind,
                available: available(&raw.available)?,
                profiles: raw.profiles,
                devices: raw.devices,
            });
        }
        let mut active_devices = HashSet::new();
        for value in array(params, "Route")? {
            // A route without the endpoint/profile association cannot acknowledge
            // an endpoint write. In particular, never use the card-wide last route.
            if value.get("device").is_none() || value.get("profile").is_none() {
                continue;
            }
            let active: ActiveRoute = serde_json::from_value(value.clone())?;
            if !active_devices.insert((active.device, active.profile)) {
                return Err("Ambiguous per-endpoint active Route state".into());
            }
            data.active_routes.push(active);
        }
        Ok(data)
    }
}

fn array<'a>(params: &'a Value, key: &str) -> AppResult<&'a [Value]> {
    match params.get(key) {
        None => Ok(&[]),
        Some(Value::Array(array)) if array.len() <= 4096 => Ok(array),
        _ => Err(format!("Invalid or oversized {key} parameters").into()),
    }
}

fn available(value: &str) -> AppResult<bool> {
    match value {
        "yes" | "unknown" => Ok(true),
        "no" => Ok(false),
        _ => Err("Unknown SPA availability".into()),
    }
}

fn parse_classes(value: &Value) -> AppResult<(Vec<u32>, Vec<u32>)> {
    let classes = value.as_array().ok_or("Invalid profile classes")?;
    let count = classes
        .first()
        .and_then(Value::as_u64)
        .ok_or("Missing profile class count")?;
    if count != (classes.len() - 1) as u64 {
        return Err("Invalid profile class count".into());
    }
    let (mut sinks, mut sources) = (Vec::new(), Vec::new());
    for class in &classes[1..] {
        let class = class.as_array().ok_or("Invalid profile class")?;
        if class.len() != 4 {
            return Err("Invalid profile class fields".into());
        }
        let target = match class[0].as_str() {
            Some("Audio/Sink") => &mut sinks,
            Some("Audio/Source") => &mut sources,
            _ => continue,
        };
        if class[2] != "card.profile.devices" {
            return Err("Missing profile device association".into());
        }
        let devices: Vec<u32> = serde_json::from_value(class[3].clone())?;
        if class[1].as_u64() != Some(devices.len() as u64) {
            return Err("Invalid profile node count".into());
        }
        target.extend(devices);
        let unique: HashSet<_> = target.iter().collect();
        if unique.len() != target.len() {
            return Err("Ambiguous profile device indexes".into());
        }
    }
    Ok((sinks, sources))
}

fn read_dump() -> AppResult<Value> {
    let executable = env::var_os("AUDIO_CONTROL_PW_DUMP").unwrap_or_else(|| "pw-dump".into());
    let mut child = Command::new(executable)
        .arg("-N")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let stdout = child.stdout.take().ok_or("pw-dump stdout pipe is absent")?;
    let (sender, receiver) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = stdout
            .take(MAX_DUMP_BYTES + 1)
            .read_to_end(&mut bytes)
            .map(|_| bytes);
        let _ = sender.send(result);
    });
    let deadline = Instant::now() + DUMP_TIMEOUT;
    let result = (|| -> AppResult<Value> {
        let bytes = receiver
            .recv_timeout(DUMP_TIMEOUT)
            .map_err(|_| io::Error::other("pw-dump timed out"))??;
        if bytes.len() as u64 > MAX_DUMP_BYTES {
            return Err("pw-dump output exceeds 8 MiB".into());
        }
        loop {
            if let Some(status) = child.try_wait()? {
                if !status.success() {
                    return Err(format!("pw-dump failed: {status}").into());
                }
                return Ok(serde_json::from_slice(&bytes)?);
            }
            if Instant::now() >= deadline {
                return Err("pw-dump timed out".into());
            }
            thread::sleep(Duration::from_millis(5));
        }
    })();
    if result.is_err() {
        let _ = child.kill();
    }
    let _ = child.wait();
    result
}
