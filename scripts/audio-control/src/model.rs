#[derive(Clone, Copy, Debug, Eq, PartialEq)]
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

#[derive(Clone, Debug)]
pub struct BluetoothEntry {
    /// Colon-separated upper-case address, as BlueZ renders it.
    pub address: String,
    pub name: String,
    /// False when BlueZ has resolved no name and `name` is just the address.
    pub named: bool,

    pub connected: bool,
    pub paired: bool,
    pub battery: Option<u8>,
}

impl BluetoothEntry {
    /// Connected first, then paired, then everything discovery turned up.
    pub fn rank(&self) -> u8 {
        if self.connected {
            0
        } else if self.paired {
            1
        } else {
            2
        }
    }
}

#[derive(Clone, Debug)]
pub struct AudioEntry {
    pub key: String,
    pub kind: AudioKind,
    /// PulseAudio node name, or empty for a card port whose profile is inactive.
    pub name: String,
    /// Stable ALSA card name for Output ports, including inactive-profile rows.
    /// Together with `port` it survives sink replacement during a profile switch.
    pub card: Option<String>,
    /// Full PulseAudio description for the device tooltip.
    pub description: String,
    /// What the row displays: `description` with the boilerplate stripped.
    pub label: String,
    pub volume: u8,
    pub muted: bool,
    pub default: bool,
    /// A physical port selected by this row. None for device-only Waybar rows.
    pub port: Option<String>,
}

impl AudioEntry {
    pub fn volume_icon(&self) -> &'static str {
        match (self.kind, self.muted) {
            (AudioKind::Input, true) => "󰍭",
            (AudioKind::Input, false) => "󰍬",
            (AudioKind::Output, true) => "󰝟",
            (AudioKind::Output, false) => output_icon(self.volume),
        }
    }
}

pub fn output_icon(volume: u8) -> &'static str {
    match volume {
        0 => "󰕿",
        1..=50 => "󰖀",
        _ => "󰕾",
    }
}

/// Boilerplate that appears in almost every PulseAudio description and
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

/// Turns a PulseAudio description into something that fits a menu row.
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

pub fn json_escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            character if character.is_control() => {
                use std::fmt::Write as _;
                let _ = write!(escaped, "\\u{:04x}", u32::from(character));
            }
            character => escaped.push(character),
        }
    }
    escaped
}
