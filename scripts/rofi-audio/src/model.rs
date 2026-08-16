use std::fmt;
use std::str::FromStr;

use crate::{AppError, AppResult};

/// The three Rofi tabs. Each one is a separate script mode, so Rofi keeps a
/// private `ROFI_DATA` per tab and the mode switcher renders them as buttons.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Mode {
    Bluetooth,
    Output,
    Input,
}

impl Mode {
    pub fn name(self) -> &'static str {
        match self {
            Self::Bluetooth => "bluetooth",
            Self::Output => "output",
            Self::Input => "input",
        }
    }

    pub fn prompt(self) -> &'static str {
        match self {
            Self::Bluetooth => "󰂯 Bluetooth",
            Self::Output => "󰕾 Output",
            Self::Input => "󰍬 Input",
        }
    }

    pub fn audio_kind(self) -> Option<AudioKind> {
        match self {
            Self::Bluetooth => None,
            Self::Output => Some(AudioKind::Output),
            Self::Input => Some(AudioKind::Input),
        }
    }
}

impl FromStr for Mode {
    type Err = AppError;

    fn from_str(value: &str) -> AppResult<Self> {
        match value.to_ascii_lowercase().as_str() {
            "bluetooth" | "bt" => Ok(Self::Bluetooth),
            "output" | "sink" | "outputs" => Ok(Self::Output),
            "input" | "source" | "inputs" => Ok(Self::Input),
            _ => Err(std::io::Error::other(format!("unknown mode {value:?}")).into()),
        }
    }
}

impl fmt::Display for Mode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.name())
    }
}

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

    pub fn noun(self) -> &'static str {
        match self {
            Self::Output => "output",
            Self::Input => "input",
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
}

impl CodeKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Pin => "pin",
            Self::Passkey => "passkey",
        }
    }

    pub fn prompt(self, device: &str) -> String {
        match self {
            Self::Pin => format!("Please type in the pairing PIN for {device}."),
            Self::Passkey => format!("Please type in the 6-digit passkey for {device}."),
        }
    }
}

impl FromStr for CodeKind {
    type Err = AppError;

    fn from_str(value: &str) -> AppResult<Self> {
        match value {
            "pin" => Ok(Self::Pin),
            "passkey" => Ok(Self::Passkey),
            _ => Err(std::io::Error::other(format!("unknown code kind {value:?}")).into()),
        }
    }
}

#[derive(Clone, Debug)]
pub struct BluetoothEntry {
    pub key: String,
    /// Colon-separated upper-case address, as BlueZ renders it.
    pub address: String,
    pub name: String,
    /// BlueZ `Icon` property, used to pick the row glyph.
    pub icon: Option<String>,
    pub connected: bool,
    pub paired: bool,
    pub battery: Option<u8>,
}

impl BluetoothEntry {
    pub fn row_label(&self) -> String {
        let mut label = format!(
            "{}  {}",
            device_icon(self.icon.as_deref()),
            truncate(&self.name, 26)
        );
        if let Some(battery) = self.battery {
            label.push_str(&format!("  {} {battery}%", battery_icon(battery)));
        }
        label
    }

    pub fn message_label(&self) -> String {
        let state = if self.connected {
            "connected"
        } else if self.paired {
            "paired"
        } else {
            "available"
        };
        // Single-line status row matched by the `lines: 1` message widget in
        // rofi-audio.rasi.
        format!("{} · {} · {state}", self.name, self.address)
    }

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
    /// PulseAudio node name. Stable across renders, unlike the numeric index,
    /// so it is what the row key and every follow-up action are built from.
    pub name: String,
    pub description: String,
    pub volume: u8,
    pub muted: bool,
    pub default: bool,
}

impl AudioEntry {
    pub fn row_label(&self) -> String {
        // Volume first, so the percentages line up in a column and the eye can
        // scan them without reading the device names.
        format!(
            "{} {:>3}%  {}",
            self.volume_icon(),
            self.volume,
            truncate(&self.description, 30)
        )
    }

    pub fn message_label(&self) -> String {
        let state = if self.default { "default" } else { "available" };
        let mute = if self.muted { " · muted" } else { "" };
        format!("{} · {}%{mute} · {state}", self.description, self.volume)
    }

    pub fn volume_icon(&self) -> &'static str {
        match (self.kind, self.muted) {
            (AudioKind::Input, true) => "󰍭",
            (AudioKind::Input, false) => "󰍬",
            (AudioKind::Output, true) => "󰝟",
            (AudioKind::Output, false) => output_icon(self.volume),
        }
    }
}

/// One script invocation only ever renders one tab, so only that tab's devices
/// are collected. Bluetooth reads BlueZ, the audio tabs read PulseAudio, and
/// neither backend has to be reachable for the other tab to work.
pub enum Devices {
    Bluetooth(Vec<BluetoothEntry>),
    Audio(Vec<AudioEntry>),
}

impl Devices {
    pub fn is_empty(&self) -> bool {
        match self {
            Self::Bluetooth(entries) => entries.is_empty(),
            Self::Audio(entries) => entries.is_empty(),
        }
    }

    pub fn position(&self, key: &str) -> Option<usize> {
        match self {
            Self::Bluetooth(entries) => entries.iter().position(|entry| entry.key == key),
            Self::Audio(entries) => entries.iter().position(|entry| entry.key == key),
        }
    }

    pub fn message_label(&self, index: usize) -> Option<String> {
        match self {
            Self::Bluetooth(entries) => entries.get(index).map(BluetoothEntry::message_label),
            Self::Audio(entries) => entries.get(index).map(AudioEntry::message_label),
        }
    }

    pub fn bluetooth(&self, key: &str) -> Option<&BluetoothEntry> {
        match self {
            Self::Bluetooth(entries) => entries.iter().find(|entry| entry.key == key),
            Self::Audio(_) => None,
        }
    }

    pub fn audio(&self, key: &str) -> Option<&AudioEntry> {
        match self {
            Self::Audio(entries) => entries.iter().find(|entry| entry.key == key),
            Self::Bluetooth(_) => None,
        }
    }
}

pub fn device_icon(icon: Option<&str>) -> &'static str {
    match icon.unwrap_or_default() {
        "audio-headset" | "audio-headphones" => "󰋋",
        "audio-speakers" | "audio-card" => "󰓃",
        "input-keyboard" => "󰌌",
        "input-mouse" | "input-tablet" => "󰍽",
        "input-gaming" => "󰊴",
        "phone" => "󰄜",
        "computer" => "󰟀",
        "camera-photo" | "camera-video" => "󰄀",
        "printer" => "󰐪",
        "video-display" => "󰍹",
        _ => "󰂯",
    }
}

pub fn battery_icon(percentage: u8) -> &'static str {
    match percentage {
        90..=u8::MAX => "󰁹",
        70..=89 => "󰂀",
        50..=69 => "󰁾",
        30..=49 => "󰁼",
        10..=29 => "󰁺",
        _ => "󰂎",
    }
}

pub fn output_icon(volume: u8) -> &'static str {
    match volume {
        0 => "󰕿",
        1..=50 => "󰖀",
        _ => "󰕾",
    }
}

pub fn bluetooth_icon(powered: bool, connected: bool) -> &'static str {
    match (powered, connected) {
        (false, _) => "󰂲",
        (true, false) => "󰂯",
        (true, true) => "󰂱",
    }
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

pub fn hex_decode(value: &str) -> Option<String> {
    if !value.len().is_multiple_of(2) {
        return None;
    }
    let bytes = value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let pair = std::str::from_utf8(pair).ok()?;
            u8::from_str_radix(pair, 16).ok()
        })
        .collect::<Option<Vec<_>>>()?;
    String::from_utf8(bytes).ok()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modes_round_trip_through_their_script_argument() {
        for mode in [Mode::Bluetooth, Mode::Output, Mode::Input] {
            assert_eq!(mode.name().parse::<Mode>().unwrap(), mode);
        }
    }

    #[test]
    fn keys_can_carry_any_utf8_name_without_shell_metacharacters() {
        assert_eq!(hex_encode("Raina's 🎧"), "5261696e61277320f09f8ea7");
        assert_eq!(
            hex_decode(&hex_encode("Raina's 🎧")).as_deref(),
            Some("Raina's 🎧")
        );
    }

    #[test]
    fn odd_length_hex_is_rejected_instead_of_truncated() {
        assert_eq!(hex_decode("abc"), None);
    }

    #[test]
    fn audio_rows_put_the_volume_before_the_name() {
        let entry = AudioEntry {
            key: "sink:00".to_owned(),
            kind: AudioKind::Output,
            name: "alsa_output.pci".to_owned(),
            description: "Built-in Audio".to_owned(),
            volume: 45,
            muted: false,
            default: true,
        };
        assert_eq!(entry.row_label(), "󰖀  45%  Built-in Audio");
    }

    #[test]
    fn muted_audio_rows_keep_showing_the_volume_they_will_return_to() {
        let entry = AudioEntry {
            key: "source:01".to_owned(),
            kind: AudioKind::Input,
            name: "alsa_input.pci".to_owned(),
            description: "Webcam Mic".to_owned(),
            volume: 80,
            muted: true,
            default: false,
        };
        assert_eq!(entry.row_label(), "󰍭  80%  Webcam Mic");
    }

    #[test]
    fn bluetooth_ranks_connected_above_paired_above_discovered() {
        let entry = |connected, paired| BluetoothEntry {
            key: "bt:00".to_owned(),
            address: "AA:BB:CC:DD:EE:FF".to_owned(),
            name: "WH-1000XM4".to_owned(),
            icon: Some("audio-headset".to_owned()),
            connected,
            paired,
            battery: None,
        };
        assert_eq!(entry(true, true).rank(), 0);
        assert_eq!(entry(false, true).rank(), 1);
        assert_eq!(entry(false, false).rank(), 2);
    }

    #[test]
    fn long_device_names_are_ellipsized_rather_than_wrapped() {
        assert_eq!(truncate("Raina's Bluetooth Headphones", 12), "Raina's Blu…");
        assert_eq!(truncate("Short", 12), "Short");
    }
}
