//! Network snapshots, compatibility, reconciliation, identities, and state JSON.

use std::collections::{BTreeMap, BTreeSet};
use std::hash::{Hash, Hasher};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::bus::{
    ACTIVE, AP_INTERFACE, CONNECTION, DEVICE, ETHERNET, Failure, NM, NetResult, Objects, ROOT,
    Settings, WIRELESS, boolean, bytes, hex, number, settings_json, strings, strip_secrets, text,
};
use crate::ssid::ssid_text;

pub(crate) const WIFI: &str = "802-11-wireless";
pub(crate) const WIRED: &str = "802-3-ethernet";
pub(crate) const SECURITY: &str = "802-11-wireless-security";
pub(crate) const PSK: u32 = 0x100;
pub(crate) const EAP: u32 = 0x200;
pub(crate) const SAE: u32 = 0x400;
pub(crate) const OWE: u32 = 0x800;
pub(crate) const OWE_TM: u32 = 0x1000;
pub(crate) const SUITE_B: u32 = 0x2000;
const MAX_DEVICES: usize = 64;

fn object<'a>(objects: &'a Objects, path: &str, interface: &str) -> &'a Value {
    objects
        .get(path)
        .and_then(|interfaces| interfaces.get(interface))
        .unwrap_or(&Value::Null)
}
pub(crate) fn non_root(path: &str) -> bool {
    !path.is_empty() && path != "/"
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Security {
    Open,
    Wep,
    Psk,
    Sae,
    Owe,
    Enterprise,
    Unknown,
}
impl Security {
    fn name(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Wep => "wep",
            Self::Psk => "psk",
            Self::Sae => "sae",
            Self::Owe => "owe",
            Self::Enterprise => "enterprise",
            Self::Unknown => "unknown",
        }
    }
    fn label(self) -> &'static str {
        match self {
            Self::Open => "Open",
            Self::Wep => "WEP",
            Self::Psk => "WPA/WPA2",
            Self::Sae => "WPA3",
            Self::Owe => "Enhanced Open (OWE)",
            Self::Enterprise => "Enterprise",
            Self::Unknown => "Unknown",
        }
    }
    pub(crate) fn password(self) -> bool {
        matches!(self, Self::Wep | Self::Psk | Self::Sae)
    }
    fn shareable(self) -> bool {
        self == Self::Open || self.password()
    }
}
#[derive(Clone, Debug)]
pub(crate) struct AccessPoint {
    pub(crate) path: String,
    ssid: Vec<u8>,
    bssid: String,
    flags: u32,
    wpa: u32,
    rsn: u32,
    strength: u32,
    frequency: u32,
    pub(crate) mode: u32,
}
impl AccessPoint {
    fn parse(path: &str, properties: &Value) -> Self {
        Self {
            path: path.to_owned(),
            ssid: bytes(properties, "Ssid"),
            bssid: text(properties, "HwAddress"),
            flags: number(properties, "Flags"),
            wpa: number(properties, "WpaFlags"),
            rsn: number(properties, "RsnFlags"),
            strength: number(properties, "Strength").min(100),
            frequency: number(properties, "Frequency"),
            mode: number(properties, "Mode"),
        }
    }
    fn security(&self) -> Security {
        let flags = self.wpa | self.rsn;
        if flags & (EAP | SUITE_B) != 0 {
            Security::Enterprise
        } else if flags & SAE != 0 {
            Security::Sae
        } else if flags & PSK != 0 {
            Security::Psk
        } else if flags & (OWE | OWE_TM) != 0 {
            Security::Owe
        } else if flags != 0 {
            Security::Unknown
        } else if self.flags & 1 != 0 {
            Security::Wep
        } else {
            Security::Open
        }
    }
}
pub(crate) struct Profile {
    pub(crate) path: String,
    pub(crate) incarnation: u64,
    pub(crate) settings: Settings,
    pub(crate) value: Value,
    pub(crate) version: Option<u64>,
    pub(crate) uuid: String,
    id: String,
    pub(crate) kind: String,
    pub(crate) ssid: Vec<u8>,
    pub(crate) security: Security,
}
impl Profile {
    pub(crate) fn parse(
        path: String,
        incarnation: u64,
        mut settings: Settings,
        properties: &Value,
    ) -> Self {
        strip_secrets(&mut settings);
        let value = settings_json(&settings);
        let security = if value.get("802-1x").is_some() {
            Security::Enterprise
        } else if value.get(SECURITY).is_none() {
            Security::Open
        } else {
            match value[SECURITY]["key-mgmt"].as_str().unwrap_or("") {
                "none" => Security::Wep,
                "wpa-psk" | "wpa-none" => Security::Psk,
                "sae" => Security::Sae,
                "owe" => Security::Owe,
                "ieee8021x" | "wpa-eap" | "wpa-eap-suite-b-192" => Security::Enterprise,
                _ => Security::Unknown,
            }
        };
        Self {
            uuid: text(&value["connection"], "uuid"),
            id: text(&value["connection"], "id"),
            kind: text(&value["connection"], "type"),
            ssid: bytes(&value[WIFI], "ssid"),
            version: properties["VersionId"].as_u64(),
            path,
            incarnation,
            settings,
            value,
            security,
        }
    }
    pub(crate) fn hidden(&self) -> bool {
        boolean(&self.value[WIFI], "hidden")
    }
    pub(crate) fn key_index(&self) -> u32 {
        number(&self.value[SECURITY], "wep-tx-keyidx")
    }
    pub(crate) fn label(&self) -> String {
        let setting = &self.value[SECURITY];
        let protocols = strings(setting, "proto");
        let protocol = if protocols.iter().any(|proto| proto == "rsn")
            && !protocols.iter().any(|proto| proto == "wpa")
        {
            "WPA2"
        } else if protocols.iter().any(|proto| proto == "wpa")
            && !protocols.iter().any(|proto| proto == "rsn")
        {
            "WPA"
        } else {
            "WPA/WPA2"
        };
        match setting["key-mgmt"].as_str().unwrap_or("") {
            "wpa-none" => "WPA".to_owned(),
            "wpa-psk" => protocol.to_owned(),
            "wpa-eap" => format!("{protocol} Enterprise"),
            "wpa-eap-suite-b-192" => "WPA3 Enterprise".to_owned(),
            "ieee8021x" => if setting["auth-alg"] == "leap" {
                "LEAP"
            } else {
                "Dynamic WEP"
            }
            .to_owned(),
            _ => self.security.label().to_owned(),
        }
    }
    pub(crate) fn context(&self) -> Value {
        authentication_context(&self.value)
    }
}
pub(crate) fn authentication_context(value: &Value) -> Value {
    let mut context = value.clone();
    if let Some(connection) = context["connection"].as_object_mut() {
        for field in [
            "id",
            "timestamp",
            "autoconnect",
            "autoconnect-priority",
            "autoconnect-retries",
        ] {
            connection.remove(field);
        }
    }
    if let Some(security) = context[SECURITY].as_object_mut() {
        for field in [
            "psk",
            "psk-flags",
            "wep-key0",
            "wep-key1",
            "wep-key2",
            "wep-key3",
            "wep-key-flags",
            "leap-password",
            "leap-password-flags",
        ] {
            security.remove(field);
        }
    }
    context
}
#[derive(Clone, Debug)]
pub(crate) struct ActiveConnection {
    pub(crate) path: String,
    pub(crate) uuid: String,
    id: String,
    pub(crate) kind: String,
    pub(crate) profile: String,
    pub(crate) state: u32,
    pub(crate) reason: u32,
    pub(crate) devices: Vec<String>,
    ip4: String,
    ip6: String,
}
impl ActiveConnection {
    fn parse(path: &str, value: &Value, reason: u32) -> Self {
        Self {
            path: path.to_owned(),
            uuid: text(value, "Uuid"),
            id: text(value, "Id"),
            kind: text(value, "Type"),
            profile: text(value, "Connection"),
            state: number(value, "State"),
            reason,
            devices: strings(value, "Devices"),
            ip4: text(value, "Ip4Config"),
            ip6: text(value, "Ip6Config"),
        }
    }
}
#[derive(Clone, Debug)]
pub(crate) struct DeviceState {
    pub(crate) path: String,
    pub(crate) incarnation: u64,
    pub(crate) iface: String,
    pub(crate) wireless: bool,
    pub(crate) state: u32,
    pub(crate) reason: u32,
    pub(crate) active: Option<ActiveConnection>,
    active_ap: String,
    pub(crate) aps: Vec<AccessPoint>,
    link: Value,
    ip4: Value,
    ip6: Value,
}
impl DeviceState {
    pub(crate) fn changing(&self) -> bool {
        (40..100).contains(&self.state)
    }
}
#[derive(Default)]
pub(crate) struct Snapshot {
    pub(crate) owner: String,
    pub(crate) wifi_enabled: bool,
    pub(crate) wifi_hardware_enabled: bool,
    pub(crate) devices: Vec<DeviceState>,
    pub(crate) profiles: Vec<Profile>,
    pub(crate) active: BTreeMap<String, ActiveConnection>,
}
impl Snapshot {
    pub(crate) fn build(
        owner: String,
        objects: &Objects,
        mut settings: BTreeMap<String, Settings>,
        incarnations: &BTreeMap<String, u64>,
        reasons: &BTreeMap<String, u32>,
    ) -> NetResult<Self> {
        let manager = object(objects, ROOT, NM);
        if manager.is_null() {
            return Err(Failure::unavailable());
        }
        let mut result = Self {
            owner,
            wifi_enabled: boolean(manager, "WirelessEnabled"),
            wifi_hardware_enabled: boolean(manager, "WirelessHardwareEnabled"),
            ..Self::default()
        };
        for (path, interfaces) in objects {
            if let Some(value) = interfaces.get(ACTIVE) {
                result.active.insert(
                    path.clone(),
                    ActiveConnection::parse(path, value, *reasons.get(path).unwrap_or(&0)),
                );
            }
            if interfaces.contains_key(CONNECTION)
                && let Some(settings) = settings.remove(path)
            {
                result.profiles.push(Profile::parse(
                    path.clone(),
                    *incarnations.get(path).unwrap_or(&0),
                    settings,
                    object(objects, path, CONNECTION),
                ));
            }
        }
        let paths = strings(manager, "Devices");
        if paths.len() > MAX_DEVICES {
            return Err(Failure::unavailable());
        }
        for path in paths {
            let properties = object(objects, &path, DEVICE);
            let kind = number(properties, "DeviceType");
            if !matches!(kind, 1 | 2) {
                continue;
            }
            let wireless = kind == 2;
            let link = object(objects, &path, if wireless { WIRELESS } else { ETHERNET });
            if properties.is_null() || link.is_null() {
                continue;
            }
            let active_path = text(properties, "ActiveConnection");
            let active = result.active.get(&active_path).cloned();
            let active_ap = text(link, "ActiveAccessPoint");
            let mut aps = Vec::new();
            if wireless {
                let mut paths = strings(link, "AccessPoints");
                if non_root(&active_ap) && !paths.contains(&active_ap) {
                    paths.push(active_ap.clone());
                }
                for ap in paths {
                    let value = object(objects, &ap, AP_INTERFACE);
                    if !value.is_null() {
                        aps.push(AccessPoint::parse(&ap, value));
                    }
                }
            }
            let ip = |field, interface| {
                let path = text(properties, field);
                // Device and ActiveConnection can temporarily describe different
                // requests. Never publish the device's unrelated IP configuration.
                if active.as_ref().is_some_and(|active| {
                    path.as_str()
                        == if field == "Ip4Config" {
                            active.ip4.as_str()
                        } else {
                            active.ip6.as_str()
                        }
                }) {
                    object(objects, &path, interface).clone()
                } else {
                    Value::Null
                }
            };
            result.devices.push(DeviceState {
                path: path.clone(),
                incarnation: *incarnations.get(&path).unwrap_or(&0),
                iface: text(properties, "Interface"),
                wireless,
                state: number(properties, "State"),
                reason: properties["StateReason"]
                    .get(1)
                    .and_then(Value::as_u64)
                    .unwrap_or(0) as u32,
                active: active.clone(),
                active_ap,
                aps,
                link: link.clone(),
                ip4: ip("Ip4Config", "org.freedesktop.NetworkManager.IP4Config"),
                ip6: ip("Ip6Config", "org.freedesktop.NetworkManager.IP6Config"),
            });
        }
        Ok(result)
    }
    pub(crate) fn unique_profile(&self, uuid: &str) -> Option<&Profile> {
        let mut profiles = self.profiles.iter().filter(|profile| profile.uuid == uuid);
        let profile = profiles.next()?;
        if profiles.next().is_some() {
            None
        } else {
            Some(profile)
        }
    }
}

fn normalize_mac(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_hexdigit())
        .flat_map(char::to_lowercase)
        .collect()
}
fn setting_mac(value: &Value, field: &str) -> String {
    value[field]
        .as_str()
        .map(normalize_mac)
        .unwrap_or_else(|| hex(&bytes(value, field)))
}
pub(crate) fn device_compatible(device: &DeviceState, profile: &Profile) -> bool {
    if profile.kind != if device.wireless { WIFI } else { WIRED } || profile.uuid.is_empty() {
        return false;
    }
    let connection = &profile.value["connection"];
    let iface = text(connection, "interface-name");
    if !iface.is_empty() && iface != device.iface {
        return false;
    }
    let setting = &profile.value[if device.wireless { WIFI } else { WIRED }];
    let permanent = text(&device.link, "PermHwAddress");
    let actual = normalize_mac(if permanent.is_empty() {
        device.link["HwAddress"].as_str().unwrap_or("")
    } else {
        &permanent
    });
    let mac = setting_mac(setting, "mac-address");
    if !mac.is_empty() && mac != actual {
        return false;
    }
    for field in ["mac-address-blacklist", "mac-address-denylist"] {
        if strings(setting, field)
            .iter()
            .any(|mac| normalize_mac(mac) == actual)
        {
            return false;
        }
    }
    if !device.wireless {
        let subchannels = strings(setting, "s390-subchannels");
        if !subchannels.is_empty() && subchannels != strings(&device.link, "S390Subchannels") {
            return false;
        }
    } else {
        let caps = number(&device.link, "WirelessCapabilities");
        let required = match setting["mode"].as_str().unwrap_or("infrastructure") {
            "infrastructure" => 0,
            "adhoc" => 0x80,
            "ap" => 0x40,
            "mesh" => 0x1000,
            _ => return false,
        };
        if required != 0 && caps & required == 0 {
            return false;
        }
        let security = &profile.value[SECURITY];
        if matches!(
            security["key-mgmt"].as_str(),
            Some("wpa-psk" | "wpa-eap" | "wpa-eap-suite-b-192")
        ) {
            if caps & 0x3c == 0 {
                return false;
            }
            let protocols = strings(security, "proto");
            if protocols.iter().any(|proto| proto == "rsn")
                && !protocols.iter().any(|proto| proto == "wpa")
                && caps & 0x28 == 0
            {
                return false;
            }
        }
    }
    true
}
fn frequency_channel(frequency: u32) -> u32 {
    match frequency {
        2484 => 14,
        2412..=2472 if (frequency - 2407).is_multiple_of(5) => (frequency - 2407) / 5,
        4910..=4980 if (frequency - 4000).is_multiple_of(5) => (frequency - 4000) / 5,
        5000..=5895 if (frequency - 5000).is_multiple_of(5) => (frequency - 5000) / 5,
        5935 => 2,
        5955..=7115 if (frequency - 5950).is_multiple_of(5) => (frequency - 5950) / 5,
        58320..=69120 if (frequency - 56160).is_multiple_of(2160) => (frequency - 56160) / 2160,
        _ => 0,
    }
}
pub(crate) fn ap_compatible(ap: &AccessPoint, profile: &Profile) -> bool {
    let wireless = &profile.value[WIFI];
    if profile.kind != WIFI || profile.ssid != ap.ssid {
        return false;
    }
    let bssid = setting_mac(wireless, "bssid");
    if !bssid.is_empty() && bssid != normalize_mac(&ap.bssid) {
        return false;
    }
    if ap.bssid.is_empty() || ap.mode == 0 {
        return false;
    }
    let mode = wireless["mode"].as_str().unwrap_or("infrastructure");
    if mode == "ap" {
        return false;
    }
    if ap.mode
        != match mode {
            "infrastructure" => 2,
            "adhoc" => 1,
            "ap" => 3,
            "mesh" => 4,
            _ => return false,
        }
    {
        return false;
    }
    let band = wireless["band"].as_str().unwrap_or("");
    if !match band {
        "" => true,
        "bg" => (2400..2500).contains(&ap.frequency),
        "a" => (4900..5900).contains(&ap.frequency),
        "6GHz" | "6ghz" => (5925..=7125).contains(&ap.frequency),
        _ => false,
    } {
        return false;
    }
    let channel = number(wireless, "channel");
    if channel != 0 && channel != frequency_channel(ap.frequency) {
        return false;
    }
    let security = &profile.value[SECURITY];
    let key = security["key-mgmt"].as_str().unwrap_or("");
    if key.is_empty() {
        return profile.security == Security::Open
            && (ap.wpa == OWE_TM
                || ap.rsn == OWE_TM
                || (ap.flags & 1 == 0 && ap.wpa == 0 && ap.rsn == 0));
    }
    if key == "none" {
        return ap.flags & 1 != 0 && ap.wpa == 0 && ap.rsn == 0;
    }
    if key == "ieee8021x" {
        if ap.flags & 1 == 0 {
            return false;
        }
        if ap.wpa == 0 {
            return true;
        }
        let ciphers_match = |field: &str, shift: u32| {
            let allowed = strings(security, field);
            allowed.is_empty()
                || allowed.iter().any(|cipher| {
                    ap.wpa
                        & (match cipher.as_str() {
                            "wep40" => 1u32,
                            "wep104" => 2,
                            _ => 0,
                        } << shift)
                        != 0
                })
        };
        return ap.wpa & EAP != 0
            && ap.wpa & 3 != 0
            && ap.wpa & 0x30 != 0
            && ciphers_match("pairwise", 0)
            && ciphers_match("group", 4);
    }
    let protocols = strings(security, "proto");
    let pairwise = strings(security, "pairwise");
    let group = strings(security, "group");
    let required = match key {
        "wpa-psk" | "wpa-none" => PSK,
        "sae" => SAE,
        "owe" => OWE | OWE_TM,
        "wpa-eap" => EAP,
        "wpa-eap-suite-b-192" => SUITE_B,
        _ => return false,
    };
    let ciphers = |flags: u32, names: &[String], group: bool| {
        names.is_empty()
            || names.iter().any(|name| {
                let bit = match name.as_str() {
                    "wep40" => 1,
                    "wep104" => 2,
                    "tkip" => 4,
                    "ccmp" => 8,
                    _ => 0,
                };
                flags & if group { bit << 4 } else { bit } != 0
            })
    };
    [("wpa", ap.wpa), ("rsn", ap.rsn)]
        .iter()
        .any(|(protocol, flags)| {
            (protocols.is_empty() || protocols.iter().any(|allowed| allowed == protocol))
                && (!matches!(key, "sae" | "owe" | "wpa-eap-suite-b-192") || *protocol == "rsn")
                && (number(security, "pmf") != 3 || *protocol == "rsn")
                && flags & required != 0
                && ciphers(*flags, &pairwise, false)
                && ciphers(*flags, &group, true)
        })
}

#[derive(Clone, Debug)]
pub(crate) struct Entry {
    pub(crate) key: String,
    pub(crate) device: usize,
    pub(crate) ssid: Vec<u8>,
    pub(crate) security: Security,
    aps: Vec<usize>,
    pub(crate) profiles: Vec<usize>,
}
fn entry_key(device: &DeviceState, ssid: &[u8], security: Security) -> String {
    if device.wireless {
        json!([device.iface, "wifi", hex(ssid), security.name()]).to_string()
    } else {
        json!([device.iface, "wired"]).to_string()
    }
}
pub(crate) fn reconcile(snapshot: &Snapshot) -> Vec<Entry> {
    let mut entries = BTreeMap::<String, Entry>::new();
    for (index, device) in snapshot.devices.iter().enumerate() {
        let mut add = |ssid: &[u8], security| {
            let key = entry_key(device, ssid, security);
            entries.entry(key.clone()).or_insert_with(|| Entry {
                key,
                device: index,
                ssid: ssid.to_vec(),
                security,
                aps: Vec::new(),
                profiles: Vec::new(),
            });
        };
        if !device.wireless {
            add(&[], Security::Open);
        }
        for (ap_index, ap) in device.aps.iter().enumerate() {
            let fallback = device.active.as_ref().and_then(|active| {
                snapshot
                    .profiles
                    .iter()
                    .find(|profile| profile.path == active.profile)
            });
            let ssid = if ap.ssid.is_empty() && ap.path == device.active_ap {
                fallback.map_or(ap.ssid.as_slice(), |profile| profile.ssid.as_slice())
            } else {
                &ap.ssid
            };
            if ssid.is_empty() {
                continue;
            }
            let key = entry_key(device, ssid, ap.security());
            entries
                .entry(key.clone())
                .or_insert_with(|| Entry {
                    key,
                    device: index,
                    ssid: ssid.to_vec(),
                    security: ap.security(),
                    aps: Vec::new(),
                    profiles: Vec::new(),
                })
                .aps
                .push(ap_index);
        }
        for (profile_index, profile) in snapshot.profiles.iter().enumerate() {
            if !device_compatible(device, profile) || (device.wireless && profile.ssid.is_empty()) {
                continue;
            }
            let matches: Vec<String> = entries
                .values()
                .filter(|entry| {
                    entry.device == index
                        && (!device.wireless
                            || (entry.ssid == profile.ssid
                                && entry
                                    .aps
                                    .iter()
                                    .any(|ap| ap_compatible(&device.aps[*ap], profile))))
                })
                .map(|entry| entry.key.clone())
                .collect();
            if matches.is_empty() {
                let ssid = if device.wireless {
                    profile.ssid.as_slice()
                } else {
                    &[]
                };
                let security = if device.wireless {
                    profile.security
                } else {
                    Security::Open
                };
                let key = entry_key(device, ssid, security);
                entries
                    .entry(key.clone())
                    .or_insert_with(|| Entry {
                        key,
                        device: index,
                        ssid: ssid.to_vec(),
                        security,
                        aps: Vec::new(),
                        profiles: Vec::new(),
                    })
                    .profiles
                    .push(profile_index);
            } else {
                for key in matches {
                    entries.get_mut(&key).unwrap().profiles.push(profile_index);
                }
            }
        }
    }
    for entry in entries.values_mut() {
        let active_uuid = snapshot.devices[entry.device]
            .active
            .as_ref()
            .map(|active| active.uuid.as_str())
            .unwrap_or("");
        entry.profiles.sort_by(|left, right| {
            let left = &snapshot.profiles[*left];
            let right = &snapshot.profiles[*right];
            (right.uuid == active_uuid)
                .cmp(&(left.uuid == active_uuid))
                .then_with(|| {
                    right.value["connection"]["timestamp"]
                        .as_u64()
                        .unwrap_or(0)
                        .cmp(&left.value["connection"]["timestamp"].as_u64().unwrap_or(0))
                })
                .then_with(|| left.uuid.cmp(&right.uuid))
                .then_with(|| left.path.cmp(&right.path))
        });
    }
    entries.into_values().collect()
}
impl Entry {
    pub(crate) fn targets(&self, snapshot: &Snapshot) -> bool {
        let device = &snapshot.devices[self.device];
        let Some(active) = &device.active else {
            return false;
        };
        if !device.wireless {
            return active.kind == WIRED;
        }
        // AP and ActiveConnection notifications are independently ordered.
        // A matching old AP must not authorize acting on a newer profile.
        if !snapshot
            .unique_profile(&active.uuid)
            .is_some_and(|profile| {
                profile.path == active.profile && profile.kind == WIFI && profile.ssid == self.ssid
            })
        {
            return false;
        }
        if let Some(ap) = device
            .aps
            .iter()
            .find(|ap| ap.path == device.active_ap && !ap.ssid.is_empty())
        {
            return self.ssid == ap.ssid && self.security == ap.security();
        }
        self.profiles
            .iter()
            .any(|profile| snapshot.profiles[*profile].uuid == active.uuid)
    }
    pub(crate) fn connected(&self, snapshot: &Snapshot) -> bool {
        let device = &snapshot.devices[self.device];
        self.targets(snapshot)
            && device.state == 100
            && device.active.as_ref().is_some_and(|active| {
                active.state == 2
                    && active.devices.contains(&device.path)
                    && (!device.wireless
                        || snapshot
                            .unique_profile(&active.uuid)
                            .is_some_and(|profile| {
                                profile.path == active.profile
                                    && profile.kind == WIFI
                                    && profile.ssid == self.ssid
                                    && device_compatible(device, profile)
                                    && device
                                        .aps
                                        .iter()
                                        .find(|ap| ap.path == device.active_ap)
                                        .is_none_or(|ap| {
                                            ap.ssid.is_empty() || ap_compatible(ap, profile)
                                        })
                            }))
            })
    }
    pub(crate) fn changing(&self, snapshot: &Snapshot) -> bool {
        let device = &snapshot.devices[self.device];
        self.targets(snapshot) && (device.changing() || device.state == 110)
    }
    pub(crate) fn best_ap<'a>(
        &self,
        snapshot: &'a Snapshot,
        profile: Option<&Profile>,
    ) -> Option<&'a AccessPoint> {
        let device = &snapshot.devices[self.device];
        self.aps
            .iter()
            .map(|index| &device.aps[*index])
            .filter(|ap| profile.is_none_or(|profile| ap_compatible(ap, profile)))
            .min_by(|left, right| {
                (right.path == device.active_ap)
                    .cmp(&(left.path == device.active_ap))
                    .then_with(|| right.strength.cmp(&left.strength))
                    .then_with(|| left.path.cmp(&right.path))
            })
    }
    pub(crate) fn saved_identity(&self, snapshot: &Snapshot) -> String {
        let device = &snapshot.devices[self.device];
        let mut profiles: Vec<_> = self
            .profiles
            .iter()
            .map(|index| {
                let profile = &snapshot.profiles[*index];
                (
                    profile.uuid.clone(),
                    profile.path.clone(),
                    profile.incarnation,
                )
            })
            .collect();
        profiles.sort();
        json!([snapshot.owner, device.path, device.incarnation, profiles]).to_string()
    }
    fn security_label(&self, snapshot: &Snapshot, ap: Option<&AccessPoint>) -> String {
        let device = &snapshot.devices[self.device];
        let profiles: Vec<_> = self
            .profiles
            .iter()
            .map(|index| &snapshot.profiles[*index])
            .filter(|profile| ap.is_none_or(|ap| ap_compatible(ap, profile)))
            .collect();
        let labels: BTreeSet<_> = profiles.iter().map(|profile| profile.label()).collect();
        let saved_label = profiles
            .iter()
            .find(|profile| {
                device
                    .active
                    .as_ref()
                    .is_some_and(|active| active.uuid == profile.uuid)
            })
            .map(|profile| profile.label())
            .or_else(|| {
                if labels.len() == 1 {
                    labels.first().cloned()
                } else {
                    None
                }
            });
        let Some(ap) = ap else {
            return saved_label.unwrap_or_else(|| self.security.label().to_owned());
        };
        match ap.security() {
            Security::Psk => if ap.rsn & PSK != 0 { "WPA2" } else { "WPA" }.to_owned(),
            Security::Enterprise => if ap.rsn & SUITE_B != 0 {
                "WPA3 Enterprise"
            } else if ap.rsn & EAP != 0 {
                "WPA2 Enterprise"
            } else if ap.wpa & EAP != 0 {
                "WPA Enterprise"
            } else {
                "Enterprise"
            }
            .to_owned(),
            Security::Wep => saved_label
                .filter(|label| matches!(label.as_str(), "Dynamic WEP" | "LEAP"))
                .unwrap_or_else(|| "WEP".to_owned()),
            security => security.label().to_owned(),
        }
    }
    pub(crate) fn identity(&self, snapshot: &Snapshot) -> Option<Identity> {
        if !self.connected(snapshot) {
            return None;
        }
        let device = &snapshot.devices[self.device];
        let active = device.active.as_ref()?;
        Some(Identity {
            interface: device.iface.clone(),
            ssid: if device.wireless {
                ssid_text(&self.ssid)
            } else {
                String::new()
            },
            profile_uuid: active.uuid.clone(),
            bssid: device
                .aps
                .iter()
                .find(|ap| device.wireless && ap.path == device.active_ap)
                .map(|ap| ap.bssid.clone())
                .unwrap_or_default(),
            active_path: active.path.clone(),
        })
    }
    pub(crate) fn details(&self, snapshot: &Snapshot) -> Value {
        let device = &snapshot.devices[self.device];
        let Some(identity) = self.identity(snapshot) else {
            return self.profiles.first().map(|index| { let profile = &snapshot.profiles[*index]; json!({
                "ssid": if device.wireless { ssid_text(&self.ssid) } else { String::new() }, "interface": device.iface,
                "profileId": profile.id, "profileUuid": profile.uuid, "identity": null,
            }) }).unwrap_or(Value::Null);
        };
        let addresses = |config: &Value| {
            config["AddressData"]
                .as_array()
                .map(|addresses| {
                    addresses
                        .iter()
                        .filter_map(|address| {
                            Some(format!(
                                "{}/{}",
                                address["address"].as_str()?,
                                address["prefix"].as_u64()?
                            ))
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        };
        let nameservers = |config: &Value| {
            let mut values: Vec<String> = config["NameserverData"]
                .as_array()
                .map(|servers| {
                    servers
                        .iter()
                        .filter_map(|server| server["address"].as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default();
            if values.is_empty()
                && let Some(servers) = config["Nameservers"].as_array()
            {
                for server in servers {
                    if let Some(server) = server.as_u64() {
                        values.push(
                            std::net::Ipv4Addr::from((server as u32).to_ne_bytes()).to_string(),
                        );
                    } else if let Some(octets) = server.as_array() {
                        let bytes: Vec<u8> = octets
                            .iter()
                            .filter_map(|value| {
                                value.as_u64().and_then(|byte| u8::try_from(byte).ok())
                            })
                            .collect();
                        if let Ok(bytes) = <[u8; 16]>::try_from(bytes) {
                            values.push(std::net::Ipv6Addr::from(bytes).to_string());
                        }
                    }
                }
            }
            values
        };
        let mut dns = nameservers(&device.ip4);
        dns.extend(nameservers(&device.ip6));
        let mut seen = BTreeSet::new();
        dns.retain(|address| seen.insert(address.clone()));
        let ap = device
            .aps
            .iter()
            .find(|ap| device.wireless && ap.path == device.active_ap);
        let frequency = ap.map_or(0, |ap| ap.frequency);
        let rate = number(
            &device.link,
            if device.wireless { "Bitrate" } else { "Speed" },
        ) as f64
            / if device.wireless { 1000.0 } else { 1.0 };
        let mut details = serde_json::to_value(&identity).unwrap();
        let fields = json!({ "identity": identity, "profileId": device.active.as_ref().map(|active| &active.id),
            "ipv4": addresses(&device.ip4), "ipv6": addresses(&device.ip6),
            "prefix": device.ip4["AddressData"].get(0).and_then(|address| address.get("prefix")).cloned().unwrap_or(json!("")),
            "gateway4": text(&device.ip4, "Gateway"), "gateway6": text(&device.ip6, "Gateway"), "dns": dns,
            "mac": text(&device.link, "HwAddress"),
            "frequency": if frequency == 0 { String::new() } else { format!("{frequency} MHz") },
            "band": if frequency == 0 { "" } else if frequency >= 59400 { "60 GHz" } else if frequency >= 5925 { "6 GHz" } else if frequency >= 4900 { "5 GHz" } else { "2.4 GHz" },
            "channel": if frequency == 0 { String::new() } else { frequency_channel(frequency).to_string() },
            "mode": ap.map_or("", |ap| match ap.mode { 1 => "Ad-Hoc", 2 => "Infrastructure", 3 => "Access point", 4 => "Mesh", _ => "Unknown" }),
            "rate": if rate == 0.0 { String::new() } else { format!("{} Mbit/s", (rate * 10.0).round() / 10.0) },
        });
        details
            .as_object_mut()
            .unwrap()
            .extend(fields.as_object().unwrap().clone());
        details
    }
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Identity {
    interface: String,
    ssid: String,
    pub(crate) profile_uuid: String,
    bssid: String,
    active_path: String,
}

pub(crate) fn profile_shareable(
    snapshot: &Snapshot,
    entry: &Entry,
    profile: &Profile,
    busy: bool,
) -> bool {
    let device = &snapshot.devices[entry.device];
    device.wireless
        && entry.security.shareable()
        && profile.security.shareable()
        && profile.kind == WIFI
        && device_compatible(device, profile)
        && snapshot
            .unique_profile(&profile.uuid)
            .is_some_and(|current| current.path == profile.path)
        && !entry.changing(snapshot)
        && !busy
}
pub(crate) fn state_json(
    snapshot: &Snapshot,
    entries: &[Entry],
    busy: &BTreeMap<String, String>,
) -> Value {
    let mut identities = serde_json::Map::new();
    let mut details = serde_json::Map::new();
    let rows: Vec<_> = entries.iter().map(|entry| {
        let device = &snapshot.devices[entry.device];
        let connected = entry.connected(snapshot);
        let device_busy = busy.contains_key(&device.path);
        let changing = entry.changing(snapshot) || busy.get(&device.path).is_some_and(|key| key == &entry.key);
        let known = !entry.profiles.is_empty();
        let ap = entry.best_ap(snapshot, None);
        let radios = !device.wireless || (snapshot.wifi_enabled && snapshot.wifi_hardware_enabled);
        let supports_creation = !device.wireless || (ap.is_some_and(|ap| ap.mode == 2) && entry.security != Security::Enterprise && entry.security != Security::Unknown);
        let can_activate = !connected && !changing && !device_busy && radios && (known || supports_creation);
        let accepts_password = device.wireless && entry.security.password() && entry.profiles.len() <= 1
            && entry.profiles.first().is_none_or(|index| snapshot.profiles[*index].security.password());
        let profiles: Vec<_> = entry.profiles.iter().map(|index| {
            let profile = &snapshot.profiles[*index];
            let selected = !connected || device.active.as_ref().is_some_and(|active| active.profile == profile.path && active.uuid == profile.uuid);
            json!({ "id": profile.id, "uuid": profile.uuid, "canShare": selected && profile_shareable(snapshot, entry, profile, device_busy) })
        }).collect();
        let can_share = profiles.iter().any(|profile| profile["canShare"] == true);
        let mut uuids: Vec<_> = entry.profiles.iter().map(|index| snapshot.profiles[*index].uuid.clone()).collect(); uuids.sort();
        let profile_stamp = json!(uuids).to_string();
        let saved_identity = entry.saved_identity(snapshot);
        let saved_stamp = if known && !connected { json!([entry.key, device.iface,
            if device.wireless { ssid_text(&entry.ssid) } else { String::new() }, device.wireless, entry.security,
            changing, profile_stamp, saved_identity]).to_string() } else { String::new() };
        let credential_stamp = json!([entry.key, saved_identity, profile_stamp, accepts_password,
            snapshot.wifi_enabled, snapshot.wifi_hardware_enabled]).to_string();
        identities.insert(entry.key.clone(), json!(entry.identity(snapshot)));
        let information = entry.details(snapshot);
        if information.is_object() { details.insert(entry.key.clone(), information); }
        let mut contexts: Vec<_> = entry.profiles.iter().map(|index| {
            let profile = &snapshot.profiles[*index]; (profile.uuid.clone(), profile.context().to_string())
        }).collect();
        contexts.sort();
        let mut stamp = std::collections::hash_map::DefaultHasher::new();
        (entry.key.as_str(), saved_identity.as_str(), contexts, snapshot.wifi_enabled, snapshot.wifi_hardware_enabled).hash(&mut stamp);
        let selection_stamp = format!("{:016x}", stamp.finish());
        json!({ "key": entry.key, "name": if device.wireless { ssid_text(&entry.ssid) } else { device.iface.clone() },
            "ssid": if device.wireless { ssid_text(&entry.ssid) } else { String::new() }, "interface": device.iface,
            "kind": if device.wireless { "WI-FI" } else { "WIRED" }, "wireless": device.wireless,
            "connected": connected, "changing": changing, "known": known,
            "strength": if device.wireless { ap.map(|ap| ap.strength) } else { Some(0) },
            "security": if device.wireless { entry.security_label(snapshot, ap) } else { "Wired".to_owned() },
            "securityType": entry.security, "profiles": profiles, "savedIdentity": saved_identity,
            "canActivate": can_activate, "canDisconnect": connected && !changing && !device_busy,
            "canForget": known && !changing && !device_busy, "canShare": can_share,
            "canShowInfo": connected || known, "acceptsPassword": accepts_password,
            "requiresPassword": !known && accepts_password, "profileStamp": profile_stamp,
            "savedInfoStamp": saved_stamp, "credentialStamp": credential_stamp, "selectionStamp": selection_stamp,
        })
    }).collect();
    json!({ "type": "state", "rows": rows, "available": !snapshot.owner.is_empty() && !snapshot.devices.is_empty(),
        "wifiAvailable": !snapshot.owner.is_empty() && snapshot.devices.iter().any(|device| device.wireless),
        "wifiEnabled": snapshot.wifi_enabled, "wifiHardwareEnabled": snapshot.wifi_hardware_enabled,
        "identities": identities, "details": details })
}
