use std::fmt;
use std::str::FromStr;

use crate::{AppError, AppResult};

/// Each tab is a separate script mode, so Rofi keeps a
/// private `ROFI_DATA` per tab and the mode switcher renders them as buttons.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Mode {
    Bluetooth,
    Output,
    Input,
    Playback,
}

impl Mode {
    pub fn name(self) -> &'static str {
        match self {
            Self::Bluetooth => "bluetooth",
            Self::Output => "output",
            Self::Input => "input",
            Self::Playback => "playback",
        }
    }

    /// The mode switcher's tab label. Rofi ties the input bar's prompt widget
    /// to the same string (`script.c` assigns the `prompt` header to
    /// `sw->display_name`, and `rofi_view_update_prompt` reads it back), so
    /// the input bar drops the `prompt` widget entirely and shows a static
    /// filter glyph instead — see the `inputbar` block in rofi-audio.rasi.
    pub fn prompt(self) -> &'static str {
        match self {
            Self::Bluetooth => "󰂯 Bluetooth",
            Self::Output => "󰕾 Output",
            Self::Input => "󰍬 Input",
            Self::Playback => "󰐊 Playback",
        }
    }

    pub fn audio_kind(self) -> Option<AudioKind> {
        match self {
            Self::Bluetooth => None,
            Self::Output | Self::Playback => Some(AudioKind::Output),
            Self::Input => Some(AudioKind::Input),
        }
    }

    pub fn is_stream(self) -> bool {
        matches!(self, Self::Playback)
    }
}

impl FromStr for Mode {
    type Err = AppError;

    fn from_str(value: &str) -> AppResult<Self> {
        match value.to_ascii_lowercase().as_str() {
            "bluetooth" | "bt" => Ok(Self::Bluetooth),
            "output" | "sink" | "outputs" => Ok(Self::Output),
            "input" | "source" | "inputs" => Ok(Self::Input),
            "playback" => Ok(Self::Playback),
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
    pub fn maximum(self) -> i16 {
        match self {
            Self::Output => 150,
            Self::Input => 100,
        }
    }

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
    /// False when BlueZ has resolved no name and `name` is just the address.
    pub named: bool,
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
            truncate(&self.name, 24)
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
        // The renderer flattens this text and wraps it in no-break markup.
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
    /// PulseAudio node name, or empty for a card port whose profile is inactive.
    pub name: String,
    /// Stable ALSA card name for Output ports, including inactive-profile rows.
    /// Together with `port` it survives sink replacement during a profile switch.
    pub card: Option<String>,
    /// Full PulseAudio description. Kept for the row's `meta`, so filtering
    /// still matches the long name even though the row shows the short one.
    pub description: String,
    /// What the row displays: `description` with the boilerplate stripped.
    pub label: String,
    pub volume: u8,
    pub muted: bool,
    pub default: bool,
    /// A physical port selected by this row. None for device-only rows used
    /// by Waybar and the per-app routing list.
    pub port: Option<String>,
}

impl AudioEntry {
    pub fn inactive(&self) -> bool {
        self.card.is_some() && self.name.is_empty()
    }

    pub fn row_label(&self) -> String {
        // Keep every device on one compact line: icon, volume, then name.
        if self.inactive() {
            return format!("󰕾   —   {}", single_line(&self.label, 48));
        }
        format!(
            "{} {:>3}%  {}",
            self.volume_icon(),
            self.volume,
            single_line(&self.label, 48)
        )
    }

    pub fn message_label(&self) -> String {
        if self.inactive() {
            return format!("{} · select to enable", self.label);
        }
        let state = if self.default { "default" } else { "available" };
        let mute = if self.muted { " · muted" } else { "" };
        format!("{} · {}%{mute} · {state}", self.label, self.volume)
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

/// One live playback stream, not an MPRIS player. Several streams
/// from the same application remain separate and are identified by `key`.
#[derive(Clone, Debug)]
pub struct StreamEntry {
    pub key: String,
    pub index: u32,
    pub application: String,
    pub name: String,
    pub device_name: String,
    pub device_label: String,
    pub volume: Option<u8>,
    pub muted: bool,
    pub corked: bool,
}

impl StreamEntry {
    pub fn row_label(&self) -> String {
        let icon = match (self.muted, self.corked) {
            (true, _) => "󰝟",
            (false, true) => "󰏤",
            (false, false) => "󰐊",
        };
        let volume = self
            .volume
            .map(|v| format!("{v:>3}%"))
            .unwrap_or_else(|| "  — ".into());
        let name = self.name.trim();
        let title = if name.is_empty()
            || name.eq_ignore_ascii_case(&self.application)
            || name.eq_ignore_ascii_case("Playback")
        {
            self.application.clone()
        } else {
            format!("{}: {name}", self.application)
        };
        // The destination remains in search metadata and the route picker.
        // Spend the row's space on the application and meaningful stream title.
        format!("{icon} {volume}  {}", single_line(&title, 48))
    }
}

/// A routing sub-menu stays in the current tab; the target is a stream key,
/// never its display label or row number.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Picker {
    Route(String),
}

impl Picker {
    pub fn target(&self) -> &str {
        match self {
            Self::Route(key) => key,
        }
    }
}

pub const BACK_KEY: &str = "back";

#[derive(Clone, Debug)]
pub struct ChoiceEntry {
    pub key: String,
    pub label: String,
    /// Full device description for filtering, separate from the compact label.
    pub description: String,
    pub active: bool,
    pub enabled: bool,
}

impl ChoiceEntry {
    pub fn route(device: AudioEntry, current_device: &str) -> Self {
        Self {
            active: device.name == current_device,
            key: device.key,
            label: device.label,
            description: device.description,
            enabled: true,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ChoiceList {
    pub title: String,
    pub entries: Vec<ChoiceEntry>,
}

/// One script invocation only ever renders one tab, so only that tab's devices
/// are collected. Bluetooth reads BlueZ, the audio tabs read PulseAudio, and
/// neither backend has to be reachable for the other tab to work.
pub enum Devices {
    Bluetooth(Vec<BluetoothEntry>),
    Audio(Vec<AudioEntry>),
    Streams(Vec<StreamEntry>),
    Choices(ChoiceList),
}

impl Devices {
    pub fn is_empty(&self) -> bool {
        match self {
            Self::Bluetooth(entries) => entries.is_empty(),
            Self::Audio(entries) => entries.is_empty(),
            Self::Streams(entries) => entries.is_empty(),
            // Every picker has a Back row, even when all targets disappear.
            Self::Choices(_) => false,
        }
    }

    pub fn position(&self, key: &str) -> Option<usize> {
        match self {
            Self::Bluetooth(entries) => entries.iter().position(|entry| entry.key == key),
            Self::Audio(entries) => entries.iter().position(|entry| entry.key == key),
            Self::Streams(entries) => entries.iter().position(|entry| entry.key == key),
            Self::Choices(choices) => {
                if key == BACK_KEY {
                    Some(0)
                } else {
                    choices
                        .entries
                        .iter()
                        .position(|entry| entry.key == key)
                        .map(|i| i + 1)
                }
            }
        }
    }

    pub fn message_label(&self, index: usize) -> Option<String> {
        match self {
            Self::Bluetooth(entries) => entries.get(index).map(BluetoothEntry::message_label),
            Self::Audio(entries) => entries.get(index).map(AudioEntry::message_label),
            Self::Streams(_) => None,
            Self::Choices(choices) => Some(choices.title.clone()),
        }
    }

    pub fn bluetooth(&self, key: &str) -> Option<&BluetoothEntry> {
        match self {
            Self::Bluetooth(entries) => entries.iter().find(|entry| entry.key == key),
            _ => None,
        }
    }

    pub fn audio(&self, key: &str) -> Option<&AudioEntry> {
        match self {
            Self::Audio(entries) => entries.iter().find(|entry| entry.key == key),
            _ => None,
        }
    }

    pub fn stream(&self, key: &str) -> Option<&StreamEntry> {
        match self {
            Self::Streams(entries) => entries.iter().find(|entry| entry.key == key),
            _ => None,
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
    // 10% buckets so the glyph tracks the number beside it; the old 20% buckets
    // drew, e.g., the fully-solid `mdi-battery` for all of 90..=100 and
    // `mdi-battery-70` for 70..=89, so 90% looked full and 89% looked 70%.
    match percentage {
        100 => "󰁹",
        90..=99 => "󰂂",
        80..=89 => "󰂁",
        70..=79 => "󰂀",
        60..=69 => "󰁿",
        50..=59 => "󰁾",
        40..=49 => "󰁽",
        30..=39 => "󰁼",
        20..=29 => "󰁻",
        10..=19 => "󰁺",
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
/// "GA104 (HDMI 2)". When nothing identifying survives — which is what happens
/// to the bare on-board controller, "Family 17h/19h/20h HD Audio Controller
/// Analog Stereo" — the active port's name ("Speakers", "Headphones") is a far
/// better label than the wreckage, so it wins instead.
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

/// A PCI family code: two or three hex digits followed by `h`, optionally
/// slash-joined ("17h", "17h/19h/20h"). Deliberately narrow — requiring a
/// leading digit and at most three of them keeps ordinary words that happen to
/// end in `h` and spell out in hex, such as "Beach", out of the filter.
fn is_pci_family(token: &str) -> bool {
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

pub fn hex_decode(value: &str) -> Option<String> {
    if !value.len().is_multiple_of(2) {
        return None;
    }
    let bytes = value
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
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
        for mode in [Mode::Bluetooth, Mode::Output, Mode::Input, Mode::Playback] {
            assert_eq!(mode.name().parse::<Mode>().unwrap(), mode);
        }
    }

    #[test]
    fn recording_mode_is_not_supported() {
        assert!("recording".parse::<Mode>().is_err());
        assert!("Recording".parse::<Mode>().is_err());
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
            card: None,
            description: "Built-in Audio Analog Stereo".to_owned(),
            label: "Built-in Audio".to_owned(),
            port: None,
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
            card: None,
            description: "Webcam Mic".to_owned(),
            label: "Webcam Mic".to_owned(),
            port: None,
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
            named: true,
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
    fn pulseaudio_boilerplate_is_stripped_from_row_labels() {
        assert_eq!(
            short_device_name("Built-in Audio Analog Stereo", None),
            "Built-in Audio"
        );
        assert_eq!(
            short_device_name(
                "GA104 High Definition Audio Controller Digital Stereo (HDMI 2)",
                None
            ),
            "GA104 (HDMI 2)"
        );
        assert_eq!(
            short_device_name("Jabra Evolve 65 Analog Stereo", Some("Headset")),
            "Jabra Evolve 65"
        );
    }

    #[test]
    fn a_description_that_is_all_boilerplate_falls_back_to_the_port() {
        // The on-board controller's description identifies the chipset and
        // nothing else, so the port name is what the user actually recognises.
        assert_eq!(
            short_device_name(
                "Family 17h/19h/20h HD Audio Controller Analog Stereo",
                Some("Speakers")
            ),
            "Speakers"
        );
        // With no port to fall back on, the original is better than nothing.
        assert_eq!(
            short_device_name("Family 17h/19h/20h HD Audio Controller", None),
            "Family 17h/19h/20h HD Audio Controller"
        );
    }

    #[test]
    fn names_without_boilerplate_are_left_alone() {
        assert_eq!(short_device_name("WH-1000XM4", None), "WH-1000XM4");
        assert_eq!(
            short_device_name("Scarlett 2i2 USB", Some("Line In")),
            "Scarlett 2i2 USB"
        );
    }

    #[test]
    fn ordinary_words_are_not_mistaken_for_pci_family_codes() {
        // "Beac" is four valid hex digits, so a loose rule would eat this.
        assert_eq!(
            short_device_name("Beach House Speaker", None),
            "Beach House Speaker"
        );
        // "Family" only goes when a family code follows it.
        assert_eq!(
            short_device_name("Family Room Sonos", None),
            "Family Room Sonos"
        );
        assert!(is_pci_family("17h"));
        assert!(is_pci_family("17h/19h/20h"));
        assert!(!is_pci_family("Beach"));
        assert!(!is_pci_family("Family"));
    }

    #[test]
    fn messages_are_flattened_and_clipped_to_a_single_line() {
        assert_eq!(
            single_line("First line\nSecond line", 40),
            "First line Second line"
        );
        assert_eq!(single_line("a".repeat(60).as_str(), 10), "aaaaaaaaa…");
        assert_eq!(single_line("  spaced  ", 40), "spaced");
        assert_eq!(
            single_line("one\r\ntwo\u{0085}three\u{2028}four\u{2029}five", 40),
            "one two three four five"
        );
    }

    #[test]
    fn long_device_names_are_ellipsized_rather_than_wrapped() {
        assert_eq!(truncate("Raina's Bluetooth Headphones", 12), "Raina's Blu…");
        assert_eq!(truncate("Short", 12), "Short");
    }
}
