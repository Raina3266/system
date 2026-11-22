use std::{collections::HashSet, io};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::AppResult;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AudioKind {
    Output,
    Input,
}

impl AudioKind {
    pub fn key_prefix(self) -> &'static str {
        match self {
            Self::Output => "sink",
            Self::Input => "source",
        }
    }
}

/// What the pairing agent is asking the user to type. BlueZ decides this per
/// device: legacy devices ask for an alphanumeric PIN, Secure Simple Pairing
/// devices ask for a six-digit passkey.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CodeKind {
    Pin,
    Passkey,
    Confirmation,
}

impl CodeKind {
    pub fn prompt(self, device: &str) -> String {
        match self {
            Self::Pin => format!("Please type in the pairing PIN for {device}."),
            Self::Passkey => format!("Please type in the 6-digit passkey for {device}."),
            Self::Confirmation => format!("Confirm pairing with {device}? Type yes or no."),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Identity {
    pub id: u32,
    pub serial: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Snapshot {
    pub cards: Vec<Card>,
    pub nodes: Vec<Node>,
    #[serde(skip)]
    pub output_stamp: (String, String),
    pub default_output: Option<Identity>,
    pub default_input: Option<Identity>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Card {
    pub id: u32,
    pub serial: String,
    pub name: String,
    pub active_profile: i32,
    pub profiles: Vec<NativeProfile>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NativeProfile {
    pub index: i32,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Node {
    pub id: u32,
    pub serial: String,
    pub name: String,
    pub description: String,
    pub kind: AudioKind,
    pub card_id: Option<u32>,
    pub card_serial: String,
    pub profile_device: Option<u32>,
    pub routes: Vec<NativeRoute>,
    pub route_name: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NativeRoute {
    pub index: i32,
    pub name: String,
    pub description: String,
    pub available: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Choice {
    pub key: String,
    pub label: String,
    pub description: String,
    pub name: String,
    pub card: String,
    pub port: bool,
    pub port_name: String,
    pub port_active: bool,
    pub node_id: Option<u32>,
    pub node_serial: String,
    pub card_id: Option<u32>,
    pub card_serial: String,
    pub route_id: Option<i32>,
}

impl Card {
    pub fn identity(&self) -> Identity {
        Identity {
            id: self.id,
            serial: self.serial.clone(),
        }
    }
}

impl Node {
    pub fn identity(&self) -> Identity {
        Identity {
            id: self.id,
            serial: self.serial.clone(),
        }
    }
}

impl Snapshot {
    pub fn card(&self, identity: &Identity) -> AppResult<&Card> {
        self.cards
            .iter()
            .find(|card| card.id == identity.id && card.serial == identity.serial)
            .ok_or_else(|| {
                io::Error::other("The selected sound card disappeared or was replaced").into()
            })
    }

    pub fn node(&self, identity: &Identity) -> AppResult<&Node> {
        self.nodes
            .iter()
            .find(|node| node.id == identity.id && node.serial == identity.serial)
            .ok_or_else(|| {
                io::Error::other("The selected audio endpoint disappeared or was replaced").into()
            })
    }

    pub fn default_node(&self, kind: AudioKind) -> Option<&Node> {
        let identity = match kind {
            AudioKind::Output => &self.default_output,
            AudioKind::Input => &self.default_input,
        };
        identity
            .as_ref()
            .and_then(|identity| self.node(identity).ok())
    }

    pub fn validate(&self) -> AppResult<()> {
        if self.cards.len() > 1024 || self.nodes.len() > 4096 {
            return Err(io::Error::other("Astal snapshot is too large").into());
        }
        let mut ids = HashSet::new();
        let mut names = HashSet::new();
        for card in &self.cards {
            validate_identity(card.id, &card.serial)?;
            validate_text(&card.name)?;
            if card.name.is_empty()
                || !ids.insert(card.id)
                || !names.insert(&card.name)
                || card.profiles.len() > 1024
            {
                return Err(io::Error::other("Invalid or duplicate Astal card").into());
            }
            let mut profiles = HashSet::new();
            if card
                .profiles
                .iter()
                .any(|p| p.index < 0 || !profiles.insert(p.index))
            {
                return Err(io::Error::other("Invalid or duplicate native profile index").into());
            }
        }
        for node in &self.nodes {
            validate_identity(node.id, &node.serial)?;
            for text in [&node.name, &node.description, &node.route_name] {
                validate_text(text)?;
            }
            if !ids.insert(node.id) || node.routes.len() > 1024 {
                return Err(io::Error::other("Invalid or duplicate Astal endpoint").into());
            }
            match node.card_id {
                Some(id) => {
                    self.card(&Identity {
                        id,
                        serial: node.card_serial.clone(),
                    })?;
                }
                None if !node.card_serial.is_empty() => {
                    return Err(
                        io::Error::other("Endpoint has a card serial without a card").into(),
                    );
                }
                None => {}
            }
            let mut routes = HashSet::new();
            for route in &node.routes {
                validate_text(&route.name)?;
                validate_text(&route.description)?;
                if route.index < 0 || !routes.insert(route.index) {
                    return Err(io::Error::other("Invalid or duplicate native route index").into());
                }
            }
        }
        for (kind, identity) in [
            (AudioKind::Output, &self.default_output),
            (AudioKind::Input, &self.default_input),
        ] {
            if let Some(identity) = identity
                && self.node(identity)?.kind != kind
            {
                return Err(io::Error::other("Default endpoint has the wrong audio kind").into());
            }
        }
        Ok(())
    }
}

pub(crate) fn validate_text(text: &str) -> AppResult<()> {
    if text.len() > 8192 || text.contains('\0') {
        return Err(io::Error::other("Invalid or oversized identifier/text").into());
    }
    Ok(())
}

fn validate_identity(id: u32, serial: &str) -> AppResult<()> {
    if id == 0
        || serial.is_empty()
        || !serial.bytes().all(|b| b.is_ascii_digit())
        || serial.parse::<u64>().is_err()
    {
        return Err(io::Error::other("Invalid native object identity").into());
    }
    Ok(())
}

/// Raw Astal objects. Keys identify object lifetimes, not names, addresses, or
/// reusable PipeWire IDs. The executor treats guards as opaque field equality.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NativeSnapshot {
    pub objects: Vec<Value>,
    pub default_output: String,
    pub default_input: String,
    pub output_epoch: String,
    pub input_epoch: String,
    pub pairing_epoch: u64,
    pub adapter: String,
    pub opened: bool,
    pub tab: u8,
}

impl NativeSnapshot {
    pub fn validate(&self) -> AppResult<()> {
        if self.objects.len() > 8192 || self.tab > 1 {
            return Err("Invalid native panel snapshot".into());
        }
        let mut keys = HashSet::new();
        for object in &self.objects {
            let key = object["key"].as_str().ok_or("Missing native object key")?;
            validate_text(key)?;
            if key.is_empty()
                || !keys.insert(key)
                || !matches!(
                    object["type"].as_str(),
                    Some("card" | "node" | "stream" | "adapter" | "device")
                )
            {
                return Err("Invalid or duplicate native object key/type".into());
            }
        }
        self.audio()?.validate()
    }

    pub fn object(&self, key: &str) -> AppResult<&Value> {
        self.objects
            .iter()
            .find(|object| object["key"] == key)
            .ok_or_else(|| "The native object disappeared or was replaced".into())
    }

    pub fn objects(&self, kind: &str) -> impl Iterator<Item = &Value> {
        self.objects
            .iter()
            .filter(move |object| object["type"] == kind)
    }

    pub fn endpoint(&self, identity: &Identity) -> AppResult<&Value> {
        self.objects("node")
            .find(|node| node["id"] == identity.id && node["serial"] == identity.serial)
            .ok_or_else(|| "The selected audio endpoint disappeared or was replaced".into())
    }

    pub fn default_key(&self, kind: AudioKind) -> &str {
        match kind {
            AudioKind::Output => &self.default_output,
            AudioKind::Input => &self.default_input,
        }
    }

    pub fn epoch(&self, kind: AudioKind) -> &str {
        match kind {
            AudioKind::Output => &self.output_epoch,
            AudioKind::Input => &self.input_epoch,
        }
    }

    pub fn audio(&self) -> AppResult<Snapshot> {
        fn fields(object: &Value, names: &[&str]) -> Value {
            Value::Object(
                names
                    .iter()
                    .map(|name| ((*name).into(), object[*name].clone()))
                    .collect(),
            )
        }
        let cards = self
            .objects("card")
            .map(|card| {
                serde_json::from_value(fields(
                    card,
                    &["id", "serial", "name", "activeProfile", "profiles"],
                ))
            })
            .collect::<Result<Vec<Card>, _>>()?;
        let nodes = self
            .objects("node")
            .map(|node| {
                serde_json::from_value(fields(
                    node,
                    &[
                        "id",
                        "serial",
                        "name",
                        "description",
                        "kind",
                        "cardId",
                        "cardSerial",
                        "profileDevice",
                        "routes",
                        "routeName",
                    ],
                ))
            })
            .collect::<Result<Vec<Node>, _>>()?;
        let default = |key: &str| {
            self.object(key)
                .ok()
                .filter(|object| object["type"] == "node")
                .and_then(|object| {
                    Some(Identity {
                        id: object["id"].as_u64()?.try_into().ok()?,
                        serial: object["serial"].as_str()?.into(),
                    })
                })
        };
        Ok(Snapshot {
            cards,
            nodes,
            output_stamp: (self.default_output.clone(), self.output_epoch.clone()),
            default_output: default(&self.default_output),
            default_input: default(&self.default_input),
        })
    }

    pub fn root_guard(&self, fields: &[&str]) -> Value {
        let root = serde_json::to_value(self).expect("serializable native snapshot");
        json!({"key":null,"expect":fields.iter().map(|field| ((*field).to_owned(), root[*field].clone())).collect::<serde_json::Map<_, _>>()})
    }
}

pub fn object_guard(object: &Value, fields: &[&str]) -> Value {
    json!({"key":object["key"],"expect":fields.iter().map(|field| ((*field).to_owned(), object[*field].clone())).collect::<serde_json::Map<_, _>>()})
}

pub fn object_key(object: &Value) -> &str {
    object["key"].as_str().unwrap_or_default()
}

/// Boilerplate that appears in many native audio descriptions and
/// identifies nothing. Longest first, so "Analog Stereo" is removed as a
/// phrase before "Stereo" could eat half of it.
const DESCRIPTION_NOISE: &[&str] = &[
    "High Definition Audio Controller",
    "HD Audio Controller",
    "Audio Controller",
    "Analog Surround 7.1",
    "Analog Surround 5.1",
    "Analog Surround 4.0",
    "Digital Surround 7.1",
    "Digital Surround 5.1",
    "Digital Stereo",
    "Analog Stereo",
    "Digital Mono",
    "Analog Mono",
    "Stereo Duplex",
    "Mono Duplex",
];

/// Turns a native description into something that fits a menu row.
///
/// "GA104 High Definition Audio Controller Digital Stereo (HDMI 2)" becomes
/// "GA104 (HDMI 2)". When nothing identifying survives — as for the bare
/// on-board controller — the active port's name wins instead.
pub fn short_device_name(description: &str, port: Option<&str>) -> String {
    let mut short = description.to_owned();
    for noise in DESCRIPTION_NOISE {
        while let Some(at) = short.find(noise) {
            short.replace_range(at..at + noise.len(), " ");
        }
    }
    // PCI family designations such as "Family 17h/19h/20h" name a chipset
    // generation, not a device. "Family" is only dropped when the code follows
    // it, so a speaker actually called "Family Room" keeps its name.
    let tokens: Vec<&str> = short.split_whitespace().collect();
    let short = tokens
        .iter()
        .enumerate()
        .filter(|(index, token)| {
            let labels_a_family_code = **token == "Family"
                && tokens
                    .get(index + 1)
                    .is_some_and(|next| is_pci_family(next));
            !(is_pci_family(token) || labels_a_family_code)
        })
        .map(|(_, token)| *token)
        .collect::<Vec<_>>()
        .join(" ");
    let short = short.trim_matches(|character: char| {
        character.is_whitespace() || matches!(character, '-' | ',' | ':')
    });
    match (short.is_empty(), port) {
        (true, Some(port)) if !port.is_empty() => port.to_owned(),
        (true, _) => description.to_owned(),
        (false, _) => short.to_owned(),
    }
}

/// A PCI family code: two or three hex digits then `h`, optionally
/// slash-joined ("17h", "17h/19h/20h"). Narrow on purpose, so hex-spellable
/// words like "Beach" stay out.
pub(crate) fn is_pci_family(token: &str) -> bool {
    !token.is_empty()
        && token.split('/').all(|part| {
            let Some(digits) = part.strip_suffix('h') else {
                return false;
            };
            (2..=3).contains(&digits.len())
                && digits.starts_with(|character: char| character.is_ascii_digit())
                && digits
                    .chars()
                    .all(|character| character.is_ascii_hexdigit())
        })
}

/// Collapses embedded line breaks, including Unicode paragraph separators,
/// then clips the text to keep compact labels and messages bounded.
pub fn single_line(value: &str, maximum: usize) -> String {
    let joined = value
        .split(['\n', '\r', '\u{0085}', '\u{2028}', '\u{2029}'])
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    truncate(&joined, maximum)
}

pub fn hex_encode(value: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(value.len() * 2);
    for byte in value.as_bytes() {
        let byte = *byte;
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}

pub fn truncate(value: &str, maximum: usize) -> String {
    let mut characters = value.chars();
    let mut result: String = characters.by_ref().take(maximum).collect();
    if characters.next().is_some() {
        result.pop();
        result.push('…');
    }
    result
}
