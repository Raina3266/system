use std::fmt;
use std::str::FromStr;

use nmrs::{DeviceState, Network, SecurityFeatures, WiredDevice};

use crate::{AppError, AppResult};

/// Characters that fit on one line of the Rofi message widget.
///
/// Rofi sizes that widget from the wrapped Pango line count, and no theme
/// property caps it, so anything longer silently turns the status row into two
/// lines and shrinks the list below it. 375px window minus 2x(5px margin + 1px
/// border + 12px padding) leaves 339px, which holds 32 cells of JetBrains Mono
/// at 13pt/96dpi. Keep in sync with niri/themes/rofi-network.rasi.
pub const MESSAGE_COLUMNS: usize = 32;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Mode {
    Wifi,
    Ethernet,
}

impl Mode {
    pub fn name(self) -> &'static str {
        match self {
            Self::Wifi => "wifi",
            Self::Ethernet => "ethernet",
        }
    }

    /// The mode switcher's tab label. Rofi ties the input bar's prompt widget
    /// to the same string, so the input bar drops the `prompt` widget entirely
    /// and shows a static filter glyph instead — see the `inputbar` block in
    /// rofi-network.rasi.
    pub fn prompt(self) -> &'static str {
        match self {
            Self::Wifi => "󰤨 Wi-Fi",
            Self::Ethernet => "󰈀 Ethernet",
        }
    }
}

impl FromStr for Mode {
    type Err = AppError;

    fn from_str(value: &str) -> AppResult<Self> {
        match value.to_ascii_lowercase().as_str() {
            "wifi" | "wi-fi" => Ok(Self::Wifi),
            "ethernet" | "wired" => Ok(Self::Ethernet),
            _ => Err(std::io::Error::other(format!("unknown network mode {value:?}")).into()),
        }
    }
}

#[derive(Clone, Debug)]
pub struct SavedWifi {
    pub uuid: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SecurityKind {
    Open,
    Personal,
    Enterprise,
    EnhancedOpen,
    Legacy,
}

#[derive(Clone, Debug)]
pub struct WifiEntry {
    pub key: String,
    pub network: Network,
    pub saved: Option<SavedWifi>,
    pub connected: bool,
    pub connecting: bool,
}

impl WifiEntry {
    pub fn ssid(&self) -> &str {
        &self.network.ssid
    }

    pub fn interface(&self) -> &str {
        &self.network.device
    }

    pub fn strength(&self) -> u8 {
        self.network.strength.unwrap_or(0)
    }

    pub fn is_saved(&self) -> bool {
        self.saved.is_some()
    }

    pub fn security_kind(&self) -> SecurityKind {
        security_kind(&self.network.security_features)
    }

    pub fn security_label(&self) -> &'static str {
        security_label(&self.network.security_features)
    }

    pub fn row_label(&self) -> String {
        format!(
            "{}  {}",
            signal_icon(self.strength()),
            truncate(&self.network.ssid, 30)
        )
    }

    pub fn message_label(&self) -> String {
        let state = if self.connected {
            "connected"
        } else if self.connecting {
            "connecting"
        } else if self.is_saved() {
            "saved"
        } else {
            "available"
        };
        // Spend the leftover columns on the SSID so the security and state stay
        // visible when `message_line` clamps the row.
        let suffix = format!(" · {} · {}", self.security_label(), state);
        let room = MESSAGE_COLUMNS.saturating_sub(suffix.chars().count());
        format!("{}{suffix}", truncate(self.ssid(), room))
    }
}

#[derive(Clone, Debug)]
pub struct EthernetEntry {
    pub key: String,
    pub device: WiredDevice,
}

impl EthernetEntry {
    pub fn connected(&self) -> bool {
        matches!(&self.device.state, DeviceState::Activated)
    }

    pub fn connecting(&self) -> bool {
        self.device.state.is_transitional()
    }

    pub fn row_label(&self) -> String {
        let address = self
            .device
            .ip4_address
            .as_deref()
            .unwrap_or("No local IPv4");
        let state = if self.connected() {
            "Connected"
        } else if self.connecting() {
            "Connecting"
        } else {
            "Disconnected"
        };
        format!(
            "󰈀  {:<18} {:<24} {state}",
            truncate(&self.device.interface, 18),
            truncate(address, 24)
        )
    }

    pub fn message_label(&self) -> String {
        let address = self.device.ip4_address.as_deref().unwrap_or("no IPv4");
        let state = if self.connected() {
            "connected"
        } else if self.connecting() {
            "connecting"
        } else {
            "disconnected"
        };
        // Column budget, see WifiEntry::message_label.
        let suffix = format!(" · {address} · {state}");
        let room = MESSAGE_COLUMNS.saturating_sub(suffix.chars().count());
        format!("{}{suffix}", truncate(&self.device.interface, room))
    }
}

#[derive(Default)]
pub struct Snapshot {
    pub wifi: Vec<WifiEntry>,
    pub ethernet: Vec<EthernetEntry>,
}

pub struct InfoContent {
    pub id: u64,
    pub details: String,
    pub qr_payload: Option<String>,
}

pub fn security_kind(features: &SecurityFeatures) -> SecurityKind {
    if features.is_enterprise() {
        SecurityKind::Enterprise
    } else if features.owe || features.owe_transition_mode {
        SecurityKind::EnhancedOpen
    } else if features.sae || features.psk {
        SecurityKind::Personal
    } else if features.wep40 || features.wep104 || features.privacy {
        SecurityKind::Legacy
    } else {
        SecurityKind::Open
    }
}

pub fn security_label(features: &SecurityFeatures) -> &'static str {
    if features.eap_suite_b_192 {
        "WPA3 Enterprise"
    } else if features.eap && features.sae {
        "WPA2/WPA3 Enterprise"
    } else if features.eap {
        "WPA2 Enterprise"
    } else if features.sae && features.psk {
        "WPA2/WPA3"
    } else if features.sae {
        "WPA3"
    } else if features.owe || features.owe_transition_mode {
        "WPA3 OWE"
    } else if features.psk && features.ccmp && features.tkip {
        "WPA/WPA2"
    } else if features.psk && features.ccmp {
        "WPA2"
    } else if features.psk {
        "WPA"
    } else if features.wep40 || features.wep104 || features.privacy {
        "WEP"
    } else {
        "Open"
    }
}

pub fn signal_icon(strength: u8) -> &'static str {
    match strength {
        76..=u8::MAX => "󰤨",
        51..=75 => "󰤥",
        26..=50 => "󰤢",
        1..=25 => "󰤟",
        0 => "󰤯",
    }
}

/// Fold `text` onto the one line the Rofi message widget can render.
///
/// Rofi hands the message straight to Pango, which both honours embedded
/// newlines and word-wraps overlong text, and every extra line it produces is
/// taken out of the list above it.
pub fn message_line(text: &str) -> String {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    truncate(&collapsed, MESSAGE_COLUMNS)
}

pub fn stable_id(value: &str) -> u64 {
    // FNV-1a is stable across processes, unlike a randomized map hasher.
    value
        .as_bytes()
        .iter()
        .fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
        })
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

fn truncate(value: &str, maximum: usize) -> String {
    let mut characters = value.chars();
    let mut result: String = characters.by_ref().take(maximum).collect();
    if characters.next().is_some() {
        result.pop();
        result.push('…');
    }
    result
}

impl fmt::Display for Mode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.name())
    }
}

#[cfg(test)]
mod tests {
    use nmrs::SecurityFeatures;

    use super::*;

    #[test]
    fn security_names_distinguish_wpa2_and_wpa3() {
        let mut mixed = SecurityFeatures::default();
        mixed.privacy = true;
        mixed.psk = true;
        mixed.sae = true;
        mixed.ccmp = true;
        assert_eq!(security_label(&mixed), "WPA2/WPA3");

        let mut enterprise = SecurityFeatures::default();
        enterprise.privacy = true;
        enterprise.eap_suite_b_192 = true;
        enterprise.ccmp = true;
        assert_eq!(security_label(&enterprise), "WPA3 Enterprise");
    }

    #[test]
    fn messages_are_folded_onto_a_single_line() {
        assert_eq!(
            message_line("Cannot scan:\n  no Wi-Fi device"),
            "Cannot scan: no Wi-Fi device"
        );

        let clamped = message_line("Network information is open in the preview panel.");
        assert_eq!(clamped.chars().count(), MESSAGE_COLUMNS);
        assert!(clamped.ends_with('…'));
    }

    #[test]
    fn stable_ids_are_process_independent() {
        assert_eq!(stable_id("wifi:776c616e30:486f6d65"), 0x0ba2_adda_2787_7ea5);
    }

    #[test]
    fn keys_can_contain_any_utf8_ssid_without_shell_metacharacters() {
        assert_eq!(
            hex_encode("Home; Wi-Fi 🛜"),
            "486f6d653b2057692d466920f09f9b9c"
        );
    }
}
