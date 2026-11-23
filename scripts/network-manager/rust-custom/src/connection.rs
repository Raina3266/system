//! Request-local credentials, native profile construction, and operation guards.

use std::collections::BTreeMap;

use dbus::arg::PropMap;
use serde_json::{Value, json};

use crate::bus::{Failure, NetResult, Settings, hex, insert, number, settings_json};
use crate::model::{
    DeviceState, Entry, Identity, Profile, SECURITY, Security, Snapshot, WIFI, WIRED,
    profile_shareable,
};
use crate::ssid::ssid_text;

pub(crate) struct Secret(pub(crate) String);
impl Drop for Secret {
    fn drop(&mut self) {
        // Request-local credentials are not part of the reconciled cache.
        unsafe {
            self.0.as_bytes_mut().fill(0);
        }
    }
}
pub(crate) fn set_password(settings: &mut Settings, password: &str) -> NetResult<()> {
    if password.is_empty() || password.contains('\0') {
        return Err(Failure::authentication("Enter a valid network password."));
    }
    let value = settings_json(settings);
    let security = value[SECURITY]["key-mgmt"].as_str().unwrap_or("");
    if value.get("802-1x").is_some() {
        return Err(Failure::new(
            "Configure authentication for this network in a network connection editor.",
        ));
    }
    match security {
        "none" => {
            let raw = (matches!(password.len(), 5 | 13)
                && password.bytes().all(|byte| (0x20..=0x7e).contains(&byte)))
                || (matches!(password.len(), 10 | 26)
                    && password.bytes().all(|byte| byte.is_ascii_hexdigit()));
            if !raw && password.len() > 64 {
                return Err(Failure::authentication(
                    "Enter a valid WEP key or passphrase.",
                ));
            }
            let index = number(&value[SECURITY], "wep-tx-keyidx");
            if index > 3 {
                return Err(Failure::authentication("Enter a valid network password."));
            }
            insert(
                settings,
                SECURITY,
                "wep-key-type",
                if raw { 1u32 } else { 2u32 },
            );
            insert(
                settings,
                SECURITY,
                &format!("wep-key{index}"),
                password.to_owned(),
            );
            insert(settings, SECURITY, "wep-key-flags", 0u32);
        }
        "wpa-psk" | "wpa-none" | "sae" => {
            if security != "sae"
                && !((8..=63).contains(&password.len())
                    || (password.len() == 64
                        && password.bytes().all(|byte| byte.is_ascii_hexdigit())))
            {
                return Err(Failure::authentication(
                    "Enter a valid WPA password (8–63 characters or 64 hexadecimal digits).",
                ));
            }
            insert(settings, SECURITY, "psk", password.to_owned());
            insert(settings, SECURITY, "psk-flags", 0u32);
        }
        _ => {
            return Err(Failure::new(
                "Configure authentication for this network in a network connection editor.",
            ));
        }
    }
    Ok(())
}
fn random_uuid() -> NetResult<String> {
    let mut bytes = [0u8; 16];
    if unsafe { libc::getrandom(bytes.as_mut_ptr().cast(), bytes.len(), libc::GRND_NONBLOCK) }
        != bytes.len() as isize
    {
        return Err(Failure::new(
            "The connection could not be started. Try again.",
        ));
    }
    bytes[6] = (bytes[6] & 15) | 0x40;
    bytes[8] = (bytes[8] & 63) | 0x80;
    let value = hex(&bytes);
    Ok(format!(
        "{}-{}-{}-{}-{}",
        &value[..8],
        &value[8..12],
        &value[12..16],
        &value[16..20],
        &value[20..]
    ))
}
pub(crate) fn new_connection(
    snapshot: &Snapshot,
    entry: &Entry,
    password: Option<&str>,
) -> NetResult<(String, Settings)> {
    let device = &snapshot.devices[entry.device];
    let mut settings = Settings::new();
    let uuid = random_uuid()?;
    insert(
        &mut settings,
        "connection",
        "id",
        if device.wireless {
            ssid_text(&entry.ssid)
        } else {
            format!("Wired {}", device.iface)
        },
    );
    insert(&mut settings, "connection", "uuid", uuid.clone());
    insert(
        &mut settings,
        "connection",
        "type",
        if device.wireless { WIFI } else { WIRED }.to_owned(),
    );
    insert(
        &mut settings,
        "connection",
        "interface-name",
        device.iface.clone(),
    );
    insert(&mut settings, "connection", "autoconnect", true);
    insert(&mut settings, "ipv4", "method", "auto".to_owned());
    insert(&mut settings, "ipv6", "method", "auto".to_owned());
    if !device.wireless {
        if password.is_some() {
            return Err(Failure::new(
                "Configure wired authentication in a network connection editor.",
            ));
        }
        settings.insert(WIRED.to_owned(), PropMap::new());
    } else {
        let ap = entry.best_ap(snapshot, None).ok_or_else(|| {
            Failure::new("This network is no longer in range. Refresh and try again.")
        })?;
        if ap.mode != 2 {
            return Err(Failure::new(
                "Configure this network mode in a network connection editor.",
            ));
        }
        if entry.ssid.is_empty() || entry.ssid.len() > 32 {
            return Err(Failure::new(
                "The network settings are not supported. Configure a supported saved profile first.",
            ));
        }
        insert(&mut settings, WIFI, "ssid", entry.ssid.clone());
        insert(&mut settings, WIFI, "mode", "infrastructure".to_owned());
        match entry.security {
            Security::Open => (),
            Security::Wep => {
                insert(&mut settings, SECURITY, "key-mgmt", "none".to_owned());
                insert(&mut settings, SECURITY, "auth-alg", "open".to_owned());
            }
            Security::Psk => insert(&mut settings, SECURITY, "key-mgmt", "wpa-psk".to_owned()),
            Security::Sae | Security::Owe => {
                insert(
                    &mut settings,
                    SECURITY,
                    "key-mgmt",
                    if entry.security == Security::Sae {
                        "sae"
                    } else {
                        "owe"
                    }
                    .to_owned(),
                );
                insert(&mut settings, SECURITY, "pmf", 3u32);
            }
            _ => {
                return Err(Failure::new(
                    "Create a suitable saved authentication profile before connecting.",
                ));
            }
        }
        if entry.security.password() {
            set_password(
                &mut settings,
                password.ok_or_else(|| {
                    Failure::authentication("A password is required for this network.")
                })?,
            )?;
        } else if password.is_some() {
            return Err(Failure::new(
                "This network does not accept an inline password. Configure its saved profile in a network connection editor.",
            ));
        }
    }
    Ok((uuid, settings))
}

#[derive(Clone, Debug)]
pub(crate) struct ProfileGuard {
    pub(crate) path: String,
    incarnation: u64,
    pub(crate) uuid: String,
    pub(crate) context: Value,
}
impl ProfileGuard {
    pub(crate) fn capture(profile: &Profile) -> Self {
        Self {
            path: profile.path.clone(),
            incarnation: profile.incarnation,
            uuid: profile.uuid.clone(),
            context: profile.context(),
        }
    }
    pub(crate) fn validate<'a>(&self, snapshot: &'a Snapshot) -> NetResult<&'a Profile> {
        snapshot
            .unique_profile(&self.uuid)
            .filter(|profile| {
                profile.path == self.path
                    && profile.incarnation == self.incarnation
                    && profile.context() == self.context
            })
            .ok_or_else(Failure::changed)
    }
}
#[derive(Clone, Debug)]
pub(crate) struct EntryGuard {
    pub(crate) key: String,
    owner: String,
    pub(crate) device: String,
    incarnation: u64,
    iface: String,
    pub(crate) wireless: bool,
    pub(crate) ssid: Vec<u8>,
    security: Security,
    pub(crate) profiles: Vec<ProfileGuard>,
    active: Option<String>,
    pub(crate) active_uuid: Option<String>,
}
impl EntryGuard {
    pub(crate) fn capture(entry: &Entry, snapshot: &Snapshot) -> Self {
        let device = &snapshot.devices[entry.device];
        Self {
            key: entry.key.clone(),
            owner: snapshot.owner.clone(),
            device: device.path.clone(),
            incarnation: device.incarnation,
            iface: device.iface.clone(),
            wireless: device.wireless,
            ssid: entry.ssid.clone(),
            security: entry.security,
            profiles: entry
                .profiles
                .iter()
                .map(|index| ProfileGuard::capture(&snapshot.profiles[*index]))
                .collect(),
            active: device.active.as_ref().map(|active| active.path.clone()),
            active_uuid: device.active.as_ref().map(|active| active.uuid.clone()),
        }
    }
    pub(crate) fn entry<'a>(
        &self,
        snapshot: &Snapshot,
        entries: &'a [Entry],
    ) -> NetResult<&'a Entry> {
        let entry = entries
            .iter()
            .find(|entry| entry.key == self.key)
            .ok_or_else(Failure::changed)?;
        let device = &snapshot.devices[entry.device];
        if snapshot.owner != self.owner
            || device.path != self.device
            || device.incarnation != self.incarnation
            || device.iface != self.iface
            || device.wireless != self.wireless
            || entry.ssid != self.ssid
            || entry.security != self.security
        {
            return Err(Failure::changed());
        }
        Ok(entry)
    }
    pub(crate) fn device<'a>(&self, snapshot: &'a Snapshot) -> NetResult<&'a DeviceState> {
        snapshot
            .devices
            .iter()
            .find(|device| {
                snapshot.owner == self.owner
                    && device.path == self.device
                    && device.incarnation == self.incarnation
                    && device.iface == self.iface
                    && device.wireless == self.wireless
            })
            .ok_or_else(Failure::unavailable)
    }
    pub(crate) fn current_active(&self, snapshot: &Snapshot) -> NetResult<()> {
        let current = self
            .device(snapshot)?
            .active
            .as_ref()
            .map(|active| &active.path);
        if current.is_some()
            && (current != self.active.as_ref()
                || self
                    .device(snapshot)?
                    .active
                    .as_ref()
                    .map(|active| &active.uuid)
                    != self.active_uuid.as_ref())
        {
            return Err(Failure::changed());
        }
        Ok(())
    }
}
#[derive(Clone, Debug)]
pub(crate) struct SecretGuard {
    entry: EntryGuard,
    pub(crate) profile: ProfileGuard,
    identity: Option<Identity>,
    saved_identity: String,
    radios: (bool, bool),
    pub(crate) security: Security,
    hidden: bool,
    pub(crate) key_index: u32,
}
impl SecretGuard {
    pub(crate) fn capture(
        snapshot: &Snapshot,
        entries: &[Entry],
        key: &str,
        expected: Option<Identity>,
        uuid: Option<&str>,
        busy: &BTreeMap<String, String>,
    ) -> NetResult<Self> {
        let entry = entries
            .iter()
            .find(|entry| entry.key == key)
            .ok_or_else(Failure::changed)?;
        let device = &snapshot.devices[entry.device];
        let profile = if let Some(expected) = &expected {
            if entry.identity(snapshot).as_ref() != Some(expected) {
                return Err(Failure::new(
                    "The active connection changed. Reopen network information and try again.",
                ));
            }
            let active = device.active.as_ref().ok_or_else(Failure::changed)?;
            snapshot
                .unique_profile(&expected.profile_uuid)
                .filter(|profile| profile.path == active.profile)
        } else {
            uuid.filter(|uuid| !uuid.is_empty())
                .and_then(|uuid| snapshot.unique_profile(uuid))
                .filter(|profile| {
                    entry
                        .profiles
                        .iter()
                        .any(|index| snapshot.profiles[*index].path == profile.path)
                })
        }
        .ok_or_else(|| Failure::new("The selected saved profile is no longer available."))?;
        if !profile_shareable(snapshot, entry, profile, busy.contains_key(&device.path)) {
            return Err(Failure::new(
                "Sharing credentials is not supported for this network.",
            ));
        }
        let guard = Self {
            entry: EntryGuard::capture(entry, snapshot),
            profile: ProfileGuard::capture(profile),
            identity: expected,
            saved_identity: entry.saved_identity(snapshot),
            radios: (snapshot.wifi_enabled, snapshot.wifi_hardware_enabled),
            security: profile.security,
            hidden: profile.hidden(),
            key_index: profile.key_index(),
        };
        guard.validate(snapshot, entries, busy)?;
        Ok(guard)
    }
    pub(crate) fn validate(
        &self,
        snapshot: &Snapshot,
        entries: &[Entry],
        busy: &BTreeMap<String, String>,
    ) -> NetResult<()> {
        let entry = self.entry.entry(snapshot, entries)?;
        let device = &snapshot.devices[entry.device];
        let profile = self.profile.validate(snapshot)?;
        if !profile_shareable(snapshot, entry, profile, busy.contains_key(&device.path))
            || profile.ssid != entry.ssid
            || profile.security != self.security
            || profile.hidden() != self.hidden
            || profile.key_index() != self.key_index
            || self.radios != (snapshot.wifi_enabled, snapshot.wifi_hardware_enabled)
        {
            return Err(Failure::changed());
        }
        if let Some(identity) = &self.identity {
            if entry.identity(snapshot).as_ref() != Some(identity)
                || !device.active.as_ref().is_some_and(|active| {
                    active.profile == self.profile.path && active.uuid == self.profile.uuid
                })
            {
                return Err(Failure::new(
                    "The active connection changed. Reopen network information and try again.",
                ));
            }
        } else if entry.connected(snapshot)
            || entry.changing(snapshot)
            || entry.identity(snapshot).is_some()
            || entry.saved_identity(snapshot) != self.saved_identity
        {
            return Err(Failure::changed());
        }
        Ok(())
    }
    pub(crate) fn credentials(&self, password: &str) -> Value {
        json!({ "password": password, "authentication": if self.security == Security::Open { "nopass" }
            else if self.security == Security::Wep { "WEP" } else { "WPA" }, "hidden": self.hidden })
    }
}
