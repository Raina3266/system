use nmrs::{DeviceState, Network, SecurityFeatures, WiredDevice};

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
}

#[derive(Clone, Debug)]
pub struct EthernetEntry {
    pub device: WiredDevice,
}

impl EthernetEntry {
    pub fn connected(&self) -> bool {
        matches!(&self.device.state, DeviceState::Activated)
    }

    pub fn connecting(&self) -> bool {
        self.device.state.is_transitional()
    }
}

#[derive(Default)]
pub struct Snapshot {
    pub wifi: Vec<WifiEntry>,
    pub ethernet: Vec<EthernetEntry>,
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

/// Bars drawn for `strength`, 0 (none) through 4 (full).
///
/// The list sorts on this rather than the raw percentage: the view redraws on a
/// timer, and a percentage drifting a point either way would keep reshuffling
/// rows for a difference nobody can see.
pub fn signal_bars(strength: u8) -> u8 {
    match strength {
        76..=u8::MAX => 4,
        51..=75 => 3,
        26..=50 => 2,
        1..=25 => 1,
        0 => 0,
    }
}

pub fn signal_icon(strength: u8) -> &'static str {
    match signal_bars(strength) {
        4 => "󰤨",
        3 => "󰤥",
        2 => "󰤢",
        1 => "󰤟",
        _ => "󰤯",
    }
}
