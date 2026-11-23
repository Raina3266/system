use super::*;
use clap::error::ErrorKind;

#[test]
fn cli_accepts_no_arguments_and_handles_help_without_starting_backend() {
    assert!(Cli::try_parse_from(["network-control"]).is_ok());
    for argument in ["-h", "--help"] {
        assert_eq!(
            Cli::try_parse_from(["network-control", argument])
                .unwrap_err()
                .kind(),
            ErrorKind::DisplayHelp
        );
    }
    assert_eq!(
        Cli::try_parse_from(["network-control", "--version"])
            .unwrap_err()
            .kind(),
        ErrorKind::DisplayVersion
    );
    assert!(Cli::try_parse_from(["network-control", "unexpected"]).is_err());
}

// Expand inside engine to retain private access and engine::tests paths.
macro_rules! engine_tests {
    () => {
        mod tests {
            use super::{Engine, MAX_OPERATIONS, REQUEST_TIME};
            use std::cell::{Cell, RefCell};
            use std::collections::{BTreeMap, VecDeque};
            use std::io::{BufRead, BufReader, Read, Write};
            use std::os::fd::AsRawFd;
            use std::os::unix::net::UnixStream;
            use std::path::PathBuf;
            use std::process::{Child, Command, Stdio};
            use std::sync::mpsc;
            use std::thread;
            use std::time::{Duration, Instant};

            use dbus::arg::{PropMap, RefArg, Variant};
            use dbus::channel::{Channel, Sender};
            use dbus::{Message, MessageType, Path};
            use serde_json::{Value, json};

            use crate::bus::{
                ACTIVE, AP_INTERFACE, CONNECTION, DBUS, DBUS_PATH, DEVICE, Managed, NM, READ_TIME, ROOT,
                Settings, WIRELESS, call, clone_settings, dbus_path, insert, settings_json, text,
            };
            use crate::connection::{SecretGuard, new_connection, set_password};
            use crate::model::{
                EAP, OWE, OWE_TM, PSK, Profile, SAE, SECURITY, SUITE_B, Security, WIFI, ap_compatible,
                device_compatible,
            };
            use crate::protocol::REQUEST_LIMIT;
            use crate::ssid::{ssid_convert, ssid_encodings, ssid_text};
            use crate::transport::{Input, OUTPUT_LIMIT, Output, run_loop};

            const SETTINGS_PATH: &str = "/org/freedesktop/NetworkManager/Settings";

            const D: &str = "/org/freedesktop/NetworkManager/Devices/1";
            const P: &str = "/org/freedesktop/NetworkManager/Settings/1";
            const P2: &str = "/org/freedesktop/NetworkManager/Settings/2";
            const A: &str = "/org/freedesktop/NetworkManager/AccessPoint/1";
            const A2: &str = "/org/freedesktop/NetworkManager/AccessPoint/2";
            const C: &str = "/org/freedesktop/NetworkManager/ActiveConnection/1";
            const IP: &str = "/org/freedesktop/NetworkManager/IP4Config/1";
            const UUID: &str = "00000000-0000-4000-8000-000000000001";
            const UUID2: &str = "00000000-0000-4000-8000-000000000002";
            const PASSWORD: &str = "fixture-only-never-log-this-password";

            #[derive(Default)]
            struct FakeBus {
                serial: Cell<u32>,
                messages: RefCell<VecDeque<Message>>,
            }
            impl Sender for FakeBus {
                fn send(&self, mut message: Message) -> Result<u32, ()> {
                    let serial = self.serial.get() + 1;
                    self.serial.set(serial);
                    message.set_serial(serial);
                    self.messages.borrow_mut().push_back(message);
                    Ok(serial)
                }
            }
            impl FakeBus {
                fn pop(&self) -> Option<Message> {
                    self.messages.borrow_mut().pop_front()
                }
            }
            fn member(message: &Message) -> String {
                message
                    .member()
                    .map(|member| member.to_string())
                    .unwrap_or_default()
            }
            fn put<T: RefArg + 'static>(
                objects: &mut Managed,
                path: &str,
                interface: &str,
                name: &str,
                value: T,
            ) {
                objects
                    .entry(dbus_path(path).unwrap())
                    .or_default()
                    .entry(interface.to_owned())
                    .or_default()
                    .insert(name.to_owned(), Variant(Box::new(value)));
            }
            fn clone_managed(objects: &Managed) -> Managed {
                objects
                    .iter()
                    .map(|(path, interfaces)| (path.clone(), clone_settings(interfaces)))
                    .collect()
            }
            fn saved(uuid: &str, ssid: &[u8], security: Security) -> Settings {
                let mut settings = Settings::new();
                insert(
                    &mut settings,
                    "connection",
                    "id",
                    "Duplicate display ID".to_owned(),
                );
                insert(&mut settings, "connection", "uuid", uuid.to_owned());
                insert(&mut settings, "connection", "type", WIFI.to_owned());
                insert(
                    &mut settings,
                    "connection",
                    "interface-name",
                    "wlan-test".to_owned(),
                );
                insert(&mut settings, WIFI, "ssid", ssid.to_vec());
                insert(&mut settings, WIFI, "mode", "infrastructure".to_owned());
                insert(&mut settings, WIFI, "hidden", true);
                if security != Security::Open {
                    insert(
                        &mut settings,
                        SECURITY,
                        "key-mgmt",
                        match security {
                            Security::Wep => "none",
                            Security::Psk => "wpa-psk",
                            Security::Sae => "sae",
                            Security::Owe => "owe",
                            Security::Enterprise => "wpa-eap",
                            _ => "unsupported",
                        }
                        .to_owned(),
                    );
                }
                settings
            }
            fn basic_objects() -> Managed {
                let mut objects = Managed::new();
                put(
                    &mut objects,
                    ROOT,
                    NM,
                    "Devices",
                    vec![dbus_path(D).unwrap()],
                );
                put(&mut objects, ROOT, NM, "WirelessEnabled", true);
                put(&mut objects, ROOT, NM, "WirelessHardwareEnabled", true);
                put(
                    &mut objects,
                    SETTINGS_PATH,
                    "org.freedesktop.NetworkManager.Settings",
                    "Connections",
                    vec![dbus_path(P).unwrap()],
                );
                put(&mut objects, D, DEVICE, "Interface", "wlan-test".to_owned());
                put(&mut objects, D, DEVICE, "DeviceType", 2u32);
                put(&mut objects, D, DEVICE, "State", 30u32);
                put(&mut objects, D, DEVICE, "StateReason", (30u32, 0u32));
                put(
                    &mut objects,
                    D,
                    DEVICE,
                    "ActiveConnection",
                    dbus_path("/").unwrap(),
                );
                put(&mut objects, D, DEVICE, "Ip4Config", dbus_path(IP).unwrap());
                put(
                    &mut objects,
                    D,
                    DEVICE,
                    "Ip6Config",
                    dbus_path("/").unwrap(),
                );
                put(
                    &mut objects,
                    D,
                    WIRELESS,
                    "AccessPoints",
                    Vec::<Path<'static>>::new(),
                );
                put(
                    &mut objects,
                    D,
                    WIRELESS,
                    "ActiveAccessPoint",
                    dbus_path("/").unwrap(),
                );
                put(
                    &mut objects,
                    D,
                    WIRELESS,
                    "HwAddress",
                    "02:00:00:00:00:01".to_owned(),
                );
                put(
                    &mut objects,
                    D,
                    WIRELESS,
                    "PermHwAddress",
                    "02:00:00:00:00:01".to_owned(),
                );
                put(&mut objects, D, WIRELESS, "WirelessCapabilities", 0x3fffu32);
                put(&mut objects, D, WIRELESS, "Bitrate", 866700u32);
                put(&mut objects, P, CONNECTION, "VersionId", 7u64);
                put(
                    &mut objects,
                    IP,
                    "org.freedesktop.NetworkManager.IP4Config",
                    "Gateway",
                    "192.0.2.1".to_owned(),
                );
                let mut address = PropMap::new();
                address.insert(
                    "address".to_owned(),
                    Variant(Box::new("192.0.2.77".to_owned())),
                );
                address.insert("prefix".to_owned(), Variant(Box::new(24u32)));
                put(
                    &mut objects,
                    IP,
                    "org.freedesktop.NetworkManager.IP4Config",
                    "AddressData",
                    vec![address],
                );
                let mut dns = PropMap::new();
                dns.insert(
                    "address".to_owned(),
                    Variant(Box::new("192.0.2.53".to_owned())),
                );
                put(
                    &mut objects,
                    IP,
                    "org.freedesktop.NetworkManager.IP4Config",
                    "NameserverData",
                    vec![dns],
                );
                objects
            }
            fn add_ap(objects: &mut Managed, path: &str, ssid: &[u8], security: Security, strength: u8) {
                put(objects, path, AP_INTERFACE, "Ssid", ssid.to_vec());
                put(objects, path, AP_INTERFACE, "Strength", strength);
                put(
                    objects,
                    path,
                    AP_INTERFACE,
                    "HwAddress",
                    if path == A {
                        "02:00:00:00:01:01"
                    } else {
                        "02:00:00:00:01:02"
                    }
                    .to_owned(),
                );
                put(objects, path, AP_INTERFACE, "Frequency", 6115u32);
                put(objects, path, AP_INTERFACE, "Mode", 2u32);
                put(
                    objects,
                    path,
                    AP_INTERFACE,
                    "Flags",
                    if security == Security::Open { 0u32 } else { 1 },
                );
                put(objects, path, AP_INTERFACE, "WpaFlags", 0u32);
                put(
                    objects,
                    path,
                    AP_INTERFACE,
                    "RsnFlags",
                    match security {
                        Security::Psk => PSK,
                        Security::Sae => SAE | PSK,
                        Security::Enterprise => EAP,
                        Security::Owe => OWE,
                        Security::Unknown => 8,
                        _ => 0,
                    },
                );
            }
            fn bind(
                objects: &mut Managed,
                profile: &str,
                uuid: &str,
                ap: &str,
                device_state: u32,
                active_state: u32,
            ) {
                put(objects, C, ACTIVE, "Uuid", uuid.to_owned());
                put(objects, C, ACTIVE, "Id", "Active profile".to_owned());
                put(objects, C, ACTIVE, "Type", WIFI.to_owned());
                put(
                    objects,
                    C,
                    ACTIVE,
                    "Connection",
                    dbus_path(profile).unwrap(),
                );
                put(objects, C, ACTIVE, "State", active_state);
                put(objects, C, ACTIVE, "Devices", vec![dbus_path(D).unwrap()]);
                put(objects, C, ACTIVE, "Ip4Config", dbus_path(IP).unwrap());
                put(objects, C, ACTIVE, "Ip6Config", dbus_path("/").unwrap());
                put(
                    objects,
                    D,
                    DEVICE,
                    "ActiveConnection",
                    dbus_path(C).unwrap(),
                );
                put(objects, D, DEVICE, "State", device_state);
                put(
                    objects,
                    D,
                    WIRELESS,
                    "ActiveAccessPoint",
                    dbus_path(ap).unwrap(),
                );
            }
            struct Fixture {
                now: Instant,
                engine: Engine,
                bus: FakeBus,
                objects: Managed,
                settings: BTreeMap<String, Settings>,
                calls: VecDeque<Message>,
            }
            impl Fixture {
                fn new(security: Security) -> Self {
                    let now = Instant::now();
                    let mut engine = Engine::new(now);
                    engine.set_owner(":1.42".to_owned(), now);
                    let mut fixture = Self {
                        now,
                        engine,
                        bus: FakeBus::default(),
                        objects: basic_objects(),
                        settings: BTreeMap::from([(P.to_owned(), saved(UUID, b"fixture", security))]),
                        calls: VecDeque::new(),
                    };
                    fixture.pump();
                    fixture
                }
                fn pump(&mut self) {
                    for _ in 0..32 {
                        self.engine.tick(&self.bus, self.now);
                        while let Some(message) = self.bus.pop() {
                            let reply = match member(&message).as_str() {
                                "GetManagedObjects" => Some(
                                    message
                                        .method_return()
                                        .append1(clone_managed(&self.objects)),
                                ),
                                "GetSettings" => Some(message.method_return().append1(clone_settings(
                                    &self.settings[&message.path().unwrap().to_string()],
                                ))),
                                _ => {
                                    self.calls.push_back(message);
                                    None
                                }
                            };
                            if let Some(reply) = reply {
                                self.engine.receive(&self.bus, reply, self.now);
                            }
                        }
                    }
                }
                fn refresh(&mut self) {
                    self.engine.mark_dirty(self.now);
                    self.pump();
                }
                fn request(&mut self, id: u32, method: &str, args: Value) {
                    self.engine.enqueue(
                        json!({ "id": id, "method": method, "args": args, "expectedStamp": self.engine.last_state["rows"].as_array().and_then(|rows| rows.iter().find(|row| row["key"] == args[0])).map(|row| &row["selectionStamp"]) })
                            .to_string()
                            .as_bytes(),
                        self.now,
                    );
                    self.pump();
                }
                fn key(&self) -> String {
                    self.engine
                        .entries
                        .iter()
                        .find(|entry| entry.ssid == b"fixture")
                        .unwrap()
                        .key
                        .clone()
                }
                fn take_call(&mut self, expected: &str) -> Message {
                    let message = self
                        .calls
                        .pop_front()
                        .unwrap_or_else(|| panic!("missing {expected} fixture call"));
                    assert_eq!(member(&message), expected);
                    message
                }
                fn reply(&mut self, reply: Message) {
                    self.engine.receive(&self.bus, reply, self.now);
                    self.pump();
                }
                fn result(&self, id: u32) -> Option<&Value> {
                    self.engine.output.iter().find(|value| value["id"] == id)
                }
                fn detect(&mut self, security: Security) {
                    add_ap(&mut self.objects, A, b"fixture", security, 60);
                    put(
                        &mut self.objects,
                        D,
                        WIRELESS,
                        "AccessPoints",
                        vec![dbus_path(A).unwrap()],
                    );
                    self.refresh();
                }
            }

            #[test]
            fn grouping_preserves_raw_ssid_security_device_and_active_ap() {
                let mut f = Fixture::new(Security::Psk);
                f.detect(Security::Sae);
                assert_eq!(f.engine.entries.len(), 1);
                assert_eq!(f.engine.entries[0].security, Security::Sae);
                assert_eq!(
                    f.engine.entries[0].profiles.len(),
                    1,
                    "PSK transition compatibility"
                );
                let key = f.key();
                add_ap(&mut f.objects, A2, b"fixture", Security::Sae, 95);
                put(
                    &mut f.objects,
                    D,
                    WIRELESS,
                    "AccessPoints",
                    vec![dbus_path(A).unwrap(), dbus_path(A2).unwrap()],
                );
                bind(&mut f.objects, P, UUID, A, 100, 2);
                f.refresh();
                let row = &f.engine.last_state["rows"][0];
                assert_eq!(row["key"], key);
                assert_eq!(row["strength"], 60);
                assert_eq!(row["connected"], true);
                let details = &f.engine.last_state["details"][&key];
                assert_eq!(details["frequency"], "6115 MHz");
                assert_eq!(details["channel"], "33");
                assert_eq!(details["rate"], "866.7 Mbit/s");
                let raw1 = "/org/freedesktop/NetworkManager/AccessPoint/3";
                let raw2 = "/org/freedesktop/NetworkManager/AccessPoint/4";
                add_ap(&mut f.objects, raw1, &[0xff], Security::Open, 50);
                add_ap(&mut f.objects, raw2, &[0xfe], Security::Open, 50);
                put(
                    &mut f.objects,
                    D,
                    WIRELESS,
                    "AccessPoints",
                    vec![
                        dbus_path(A).unwrap(),
                        dbus_path(raw1).unwrap(),
                        dbus_path(raw2).unwrap(),
                    ],
                );
                f.refresh();
                let raw: Vec<_> = f
                    .engine
                    .entries
                    .iter()
                    .filter(|entry| entry.ssid.len() == 1)
                    .collect();
                assert_eq!(raw.len(), 2);
                assert_ne!(raw[0].key, raw[1].key);
            }
            #[test]
            fn saved_detection_keeps_stamps_and_distinguishes_null_from_zero() {
                let mut f = Fixture::new(Security::Psk);
                let key = f.key();
                let row = f.engine.last_state["rows"][0].clone();
                assert!(row["strength"].is_null());
                f.detect(Security::Psk);
                put(&mut f.objects, A, AP_INTERFACE, "Strength", 0u8);
                f.refresh();
                let detected = &f.engine.last_state["rows"][0];
                assert_eq!(detected["key"], key);
                assert_eq!(detected["strength"], 0);
                assert_eq!(row["savedInfoStamp"], detected["savedInfoStamp"]);
                insert(
                    f.settings.get_mut(P).unwrap(),
                    "connection",
                    "id",
                    "Renamed".to_owned(),
                );
                f.refresh();
                assert_eq!(
                    row["profileStamp"],
                    f.engine.last_state["rows"][0]["profileStamp"]
                );
                assert_eq!(
                    row["savedInfoStamp"],
                    f.engine.last_state["rows"][0]["savedInfoStamp"]
                );
                f.engine.incarnations.remove(P);
                f.refresh();
                assert_ne!(
                    row["savedInfoStamp"],
                    f.engine.last_state["rows"][0]["savedInfoStamp"]
                );
            }
            #[test]
            fn security_labels_cover_beacons_and_saved_protocols() {
                for (wpa, rsn, privacy, expected_type, label) in [
                    (0, 0, 0, Security::Open, "Open"),
                    (0, 0, 1, Security::Wep, "WEP"),
                    (PSK, 0, 1, Security::Psk, "WPA"),
                    (PSK, PSK, 1, Security::Psk, "WPA2"),
                    (0, PSK | SAE, 1, Security::Sae, "WPA3"),
                    (EAP, 0, 1, Security::Enterprise, "WPA Enterprise"),
                    (0, EAP, 1, Security::Enterprise, "WPA2 Enterprise"),
                    (0, SUITE_B, 1, Security::Enterprise, "WPA3 Enterprise"),
                    (0, OWE_TM, 0, Security::Owe, "Enhanced Open (OWE)"),
                    (0, 8, 1, Security::Unknown, "Unknown"),
                ] {
                    let mut f = Fixture::new(Security::Psk);
                    f.settings.clear();
                    f.objects.remove(&dbus_path(P).unwrap());
                    f.detect(Security::Open);
                    put(&mut f.objects, A, AP_INTERFACE, "WpaFlags", wpa);
                    put(&mut f.objects, A, AP_INTERFACE, "RsnFlags", rsn);
                    put(&mut f.objects, A, AP_INTERFACE, "Flags", privacy);
                    f.refresh();
                    assert_eq!(f.engine.entries[0].security, expected_type);
                    assert_eq!(f.engine.last_state["rows"][0]["security"], label);
                }
                let mut f = Fixture::new(Security::Enterprise);
                insert(
                    f.settings.get_mut(P).unwrap(),
                    SECURITY,
                    "key-mgmt",
                    "ieee8021x".to_owned(),
                );
                f.detect(Security::Wep);
                assert_eq!(f.engine.last_state["rows"][0]["security"], "Dynamic WEP");
                insert(
                    f.settings.get_mut(P).unwrap(),
                    SECURITY,
                    "auth-alg",
                    "leap".to_owned(),
                );
                f.refresh();
                assert_eq!(f.engine.last_state["rows"][0]["security"], "LEAP");
                let mut profile = Profile::parse(
                    P.to_owned(),
                    1,
                    saved(UUID, b"fixture", Security::Psk),
                    &json!({}),
                );
                insert(
                    &mut profile.settings,
                    SECURITY,
                    "proto",
                    vec!["rsn".to_owned()],
                );
                profile.value = settings_json(&profile.settings);
                assert_eq!(profile.label(), "WPA2");
            }
            #[test]
            fn compatibility_honors_ssid_bssid_mode_band_protocol_cipher_and_device_binding() {
                let mut f = Fixture::new(Security::Psk);
                f.detect(Security::Sae);
                let ap = f.engine.snapshot.devices[0].aps[0].clone();
                for (section, field, value) in [
                    (WIFI, "bssid", json!([2, 0, 0, 0, 99, 99])),
                    (WIFI, "ssid", json!([1, 2])),
                    (WIFI, "mode", json!("adhoc")),
                    (WIFI, "band", json!("bg")),
                    (WIFI, "channel", json!(1)),
                    (SECURITY, "proto", json!(["wpa"])),
                    (SECURITY, "pairwise", json!(["tkip"])),
                    (SECURITY, "group", json!(["wep40"])),
                ] {
                    let mut profile = Profile::parse(
                        P.to_owned(),
                        1,
                        saved(UUID, b"fixture", Security::Psk),
                        &json!({}),
                    );
                    profile.value[section][field] = value;
                    if field == "ssid" {
                        profile.ssid = vec![1, 2];
                    }
                    assert!(!ap_compatible(&ap, &profile), "restriction {field}");
                }
                let mut profile = Profile::parse(
                    P.to_owned(),
                    1,
                    saved(UUID, b"fixture", Security::Psk),
                    &json!({}),
                );
                profile.value["connection"]["interface-name"] = json!("different-radio");
                assert!(!device_compatible(&f.engine.snapshot.devices[0], &profile));
                profile.value["connection"]["interface-name"] = json!("wlan-test");
                profile.value[WIFI]["mac-address"] = json!([1, 2, 3, 4, 5, 6]);
                assert!(!device_compatible(&f.engine.snapshot.devices[0], &profile));
            }
            #[test]
            fn offline_details_never_leak_another_connections_live_properties() {
                let mut f = Fixture::new(Security::Psk);
                f.detect(Security::Psk);
                let other = "/org/freedesktop/NetworkManager/AccessPoint/9";
                add_ap(&mut f.objects, other, b"other", Security::Psk, 80);
                f.settings
                    .insert(P2.to_owned(), saved(UUID2, b"other", Security::Psk));
                put(&mut f.objects, P2, CONNECTION, "VersionId", 1u64);
                bind(&mut f.objects, P2, UUID2, other, 100, 2);
                f.refresh();
                let details = &f.engine.last_state["details"][f.key()];
                assert_eq!(details["identity"], Value::Null);
                assert_eq!(details["profileUuid"], UUID);
                assert_eq!(details.as_object().unwrap().len(), 5);
                for field in [
                    "ipv4",
                    "ipv6",
                    "dns",
                    "gateway4",
                    "mac",
                    "bssid",
                    "activePath",
                    "frequency",
                ] {
                    assert!(details[field].is_null());
                }
                let active = f
                    .engine
                    .entries
                    .iter()
                    .find(|entry| entry.ssid == b"other")
                    .unwrap();
                assert_eq!(
                    active.details(&f.engine.snapshot)["ipv4"],
                    json!(["192.0.2.77/24"])
                );
                put(
                    &mut f.objects,
                    C,
                    ACTIVE,
                    "Ip4Config",
                    dbus_path("/").unwrap(),
                );
                f.refresh();
                let active = f
                    .engine
                    .entries
                    .iter()
                    .find(|entry| entry.ssid == b"other")
                    .unwrap();
                assert_eq!(active.details(&f.engine.snapshot)["ipv4"], json!([]));
            }
            #[test]
            fn connection_construction_and_password_rules_match_native_shapes() {
                for security in [
                    Security::Open,
                    Security::Wep,
                    Security::Psk,
                    Security::Sae,
                    Security::Owe,
                ] {
                    let mut f = Fixture::new(security);
                    f.detect(security);
                    let password = if security == Security::Wep {
                        Some("abcde")
                    } else if security.password() {
                        Some(PASSWORD)
                    } else {
                        None
                    };
                    let (uuid, settings) =
                        new_connection(&f.engine.snapshot, &f.engine.entries[0], password).unwrap();
                    let value = settings_json(&settings);
                    assert_eq!(uuid.len(), 36);
                    assert_eq!(value["connection"]["uuid"], uuid);
                    assert_eq!(value["connection"]["interface-name"], "wlan-test");
                    assert_eq!(value["connection"]["autoconnect"], true);
                    assert_eq!(value["ipv4"]["method"], "auto");
                    assert_eq!(value["ipv6"]["method"], "auto");
                    assert!(value[WIFI]["bssid"].is_null());
                    assert_eq!(value[WIFI]["ssid"], json!(b"fixture".to_vec()));
                    if matches!(security, Security::Sae | Security::Owe) {
                        assert_eq!(value[SECURITY]["pmf"], 3);
                    }
                }
                for (security, password, success) in [
                    (Security::Wep, "0123456789", true),
                    (Security::Wep, "passphrase", true),
                    (Security::Psk, "short", false),
                    (Security::Sae, "x", true),
                    (Security::Sae, PASSWORD, true),
                    (Security::Psk, "bad\0secret", false),
                ] {
                    let mut settings = saved(UUID, b"fixture", security);
                    insert(&mut settings, SECURITY, "wep-tx-keyidx", 2u32);
                    assert_eq!(set_password(&mut settings, password).is_ok(), success);
                    if success && security == Security::Wep {
                        assert_eq!(settings_json(&settings)[SECURITY]["wep-key2"], password);
                    }
                }
                let mut settings = saved(UUID, b"fixture", Security::Sae);
                assert!(set_password(&mut settings, &"x".repeat(100)).is_ok());
                let mut settings = saved(UUID, b"fixture", Security::Psk);
                assert!(set_password(&mut settings, &"a".repeat(64)).is_ok());
                assert!(set_password(&mut settings, &"g".repeat(64)).is_err());
            }
            #[test]
            fn descriptors_are_native_and_ambiguous_credentials_are_not_authorized() {
                let mut f = Fixture::new(Security::Psk);
                let row = &f.engine.last_state["rows"][0];
                assert_eq!(row["canActivate"], true);
                assert_eq!(row["canShare"], true);
                assert_eq!(row["acceptsPassword"], true);
                assert_eq!(row["requiresPassword"], false);
                assert_eq!(row["canDisconnect"], false);
                assert_eq!(row["canForget"], true);
                f.settings
                    .insert(P2.to_owned(), saved(UUID2, b"fixture", Security::Psk));
                put(&mut f.objects, P2, CONNECTION, "VersionId", 1u64);
                f.refresh();
                assert_eq!(f.engine.last_state["rows"][0]["acceptsPassword"], false);
                let key = f.key();
                f.request(
                    1,
                    "activate",
                    json!([key, { "profileUuid": UUID2, "password": PASSWORD }]),
                );
                assert!(
                    f.result(1).unwrap()["error"]["message"]
                        .as_str()
                        .unwrap()
                        .contains("Multiple")
                );
                assert!(f.calls.is_empty());
                f.settings
                    .insert(P2.to_owned(), saved(UUID, b"fixture", Security::Psk));
                f.refresh();
                assert_eq!(f.engine.last_state["rows"][0]["canShare"], false);
            }
            #[test]
            fn saved_secret_guards_allow_revision_rename_and_detection_but_reject_context_changes() {
                let mut f = Fixture::new(Security::Wep);
                let key = f.key();
                let guard = SecretGuard::capture(
                    &f.engine.snapshot,
                    &f.engine.entries,
                    &key,
                    None,
                    Some(UUID),
                    &f.engine.busy,
                )
                .unwrap();
                put(&mut f.objects, P, CONNECTION, "VersionId", 8u64);
                insert(
                    f.settings.get_mut(P).unwrap(),
                    "connection",
                    "id",
                    "Renamed".to_owned(),
                );
                f.detect(Security::Wep);
                guard
                    .validate(&f.engine.snapshot, &f.engine.entries, &f.engine.busy)
                    .unwrap();
                type Change = fn(&mut Fixture);
                let changes: &[Change] = &[
                    |f| {
                        f.engine.snapshot.profiles[0].incarnation += 1;
                    },
                    |f| {
                        f.engine.snapshot.devices[0].incarnation += 1;
                    },
                    |f| {
                        f.engine.snapshot.devices[0].iface = "replacement".to_owned();
                    },
                    |f| {
                        f.engine.snapshot.wifi_enabled = false;
                    },
                    |f| {
                        f.engine.snapshot.wifi_hardware_enabled = false;
                    },
                    |f| {
                        f.engine.snapshot.profiles[0].uuid = UUID2.to_owned();
                    },
                    |f| {
                        f.engine.snapshot.profiles[0].value[WIFI]["hidden"] = json!(false);
                    },
                    |f| {
                        f.engine.snapshot.profiles[0].value[SECURITY]["wep-tx-keyidx"] = json!(2);
                    },
                    |f| {
                        f.engine.snapshot.profiles[0].value["connection"]["interface-name"] = json!("other");
                    },
                    |f| {
                        f.engine.snapshot.profiles[0].ssid = b"different".to_vec();
                    },
                    |f| {
                        f.engine.snapshot.profiles.clear();
                    },
                ];
                for change in changes {
                    let mut f = Fixture::new(Security::Wep);
                    let key = f.key();
                    let guard = SecretGuard::capture(
                        &f.engine.snapshot,
                        &f.engine.entries,
                        &key,
                        None,
                        Some(UUID),
                        &f.engine.busy,
                    )
                    .unwrap();
                    change(&mut f);
                    assert!(
                        guard
                            .validate(&f.engine.snapshot, &f.engine.entries, &f.engine.busy)
                            .is_err()
                    );
                }
            }
            #[test]
            fn secrets_are_explicit_exact_uuid_local_and_guarded_after_reply() {
                let mut f = Fixture::new(Security::Psk);
                f.settings
                    .insert(P2.to_owned(), saved(UUID2, b"fixture", Security::Psk));
                put(&mut f.objects, P2, CONNECTION, "VersionId", 1u64);
                f.refresh();
                assert!(f.calls.is_empty());
                let key = f.key();
                f.request(1, "readSavedSecret", json!([key, UUID2]));
                let message = f.take_call("GetSecrets");
                assert_eq!(message.path().unwrap().to_string(), P2);
                assert_eq!(message.read1::<String>().unwrap(), SECURITY);
                put(&mut f.objects, P2, CONNECTION, "VersionId", 2u64);
                let mut secrets = Settings::new();
                insert(&mut secrets, SECURITY, "psk", PASSWORD.to_owned());
                f.reply(message.method_return().append1(secrets));
                assert_eq!(f.result(1).unwrap()["result"]["password"], PASSWORD);
                assert!(!f.engine.last_state.to_string().contains(PASSWORD));
                assert!(!f.engine.last_state.to_string().contains("psk-flags"));
                f.request(2, "readSavedSecret", json!([f.key(), UUID]));
                let message = f.take_call("GetSecrets");
                insert(f.settings.get_mut(P).unwrap(), WIFI, "hidden", false);
                let mut secrets = Settings::new();
                insert(&mut secrets, SECURITY, "psk", PASSWORD.to_owned());
                f.reply(message.method_return().append1(secrets));
                assert!(f.result(2).unwrap().get("error").is_some());
                assert!(!f.result(2).unwrap().to_string().contains(PASSWORD));
                let mut open = Fixture::new(Security::Open);
                open.request(3, "readSavedSecret", json!([open.key(), UUID]));
                assert!(open.calls.is_empty());
                assert_eq!(
                    open.result(3).unwrap()["result"],
                    json!({ "password": "", "authentication": "nopass", "hidden": true })
                );
            }
            #[test]
            fn connected_reads_pin_complete_identity_and_profile_incarnation() {
                let mut f = Fixture::new(Security::Psk);
                f.detect(Security::Psk);
                bind(&mut f.objects, P, UUID, A, 100, 2);
                f.refresh();
                let key = f.key();
                let identity = f.engine.entries[0].identity(&f.engine.snapshot).unwrap();
                f.request(1, "readSecret", json!([key, identity]));
                let message = f.take_call("GetSecrets");
                put(
                    &mut f.objects,
                    A,
                    AP_INTERFACE,
                    "HwAddress",
                    "02:00:00:00:99:99".to_owned(),
                );
                let mut secrets = Settings::new();
                insert(&mut secrets, SECURITY, "psk", PASSWORD.to_owned());
                f.reply(message.method_return().append1(secrets));
                assert!(
                    f.result(1).unwrap()["error"]["message"]
                        .as_str()
                        .unwrap()
                        .contains("active connection changed")
                );
                assert!(!f.result(1).unwrap().to_string().contains(PASSWORD));
            }
            #[test]
            fn cancellation_timeout_and_eof_never_continue_a_native_request() {
                let mut f = Fixture::new(Security::Psk);
                f.detect(Security::Psk);
                f.request(1, "activate", json!([f.key(), { "password": PASSWORD }]));
                let message = f.take_call("Update2");
                let (settings, flags, args) = message.read3::<Settings, u32, PropMap>().unwrap();
                assert_eq!(flags, 1);
                assert_eq!(args["version-id"].0.as_u64(), Some(7));
                assert_eq!(settings_json(&settings)[SECURITY]["psk"], PASSWORD);
                f.engine
                    .enqueue(br#"{"method":"cancel","args":[1]}"#, f.now);
                f.reply(message.method_return().append1(PropMap::new()));
                assert_eq!(f.result(1).unwrap()["error"]["code"], "cancelled");
                assert!(f.calls.is_empty());
                assert!(f.engine.busy.is_empty());
                f.request(2, "readSavedSecret", json!([f.key(), UUID]));
                let message = f.take_call("GetSecrets");
                f.now += REQUEST_TIME;
                f.pump();
                assert_eq!(f.result(2).unwrap()["error"]["code"], "timeout");
                let mut secrets = Settings::new();
                insert(&mut secrets, SECURITY, "psk", PASSWORD.to_owned());
                f.reply(message.method_return().append1(secrets));
                assert!(!f.result(2).unwrap().to_string().contains(PASSWORD));
                f.engine.stop();
                let sent = f.bus.serial.get();
                f.engine.tick(&f.bus, f.now + REQUEST_TIME);
                f.engine
                    .enqueue(br#"{"id":9,"method":"toggleWifi","args":[]}"#, f.now);
                assert_eq!(f.bus.serial.get(), sent);
                assert!(f.engine.output.is_empty());
            }
            #[test]
            fn activation_waits_for_exact_binding_and_ignores_previous_failed_state() {
                let mut f = Fixture::new(Security::Psk);
                f.detect(Security::Psk);
                put(&mut f.objects, D, DEVICE, "State", 120u32);
                f.refresh();
                f.request(1, "activate", json!([f.key(), { "profileUuid": UUID }]));
                let message = f.take_call("ActivateConnection");
                let (profile, device, specific) = message
                    .read3::<Path<'static>, Path<'static>, Path<'static>>()
                    .unwrap();
                assert_eq!(&*profile, P);
                assert_eq!(&*device, D);
                assert_eq!(&*specific, A);
                assert_eq!(message.destination().unwrap().to_string(), ":1.42");
                bind(&mut f.objects, P, UUID, A, 120, 1);
                put(
                    &mut f.objects,
                    D,
                    DEVICE,
                    "ActiveConnection",
                    dbus_path("/").unwrap(),
                );
                f.reply(message.method_return().append1(dbus_path(C).unwrap()));
                assert!(f.result(1).is_none());
                put(&mut f.objects, C, ACTIVE, "State", 2u32);
                put(&mut f.objects, D, DEVICE, "State", 100u32);
                f.refresh();
                assert!(f.result(1).is_none());
                put(
                    &mut f.objects,
                    D,
                    DEVICE,
                    "ActiveConnection",
                    dbus_path(C).unwrap(),
                );
                f.refresh();
                assert_eq!(f.result(1).unwrap()["result"], Value::Null);
                assert!(f.engine.busy.is_empty());
            }
            #[test]
            fn activation_failure_authentication_and_uuid_replacement_are_not_success() {
                for replacement in [false, true] {
                    let mut f = Fixture::new(Security::Psk);
                    f.detect(Security::Psk);
                    f.request(1, "activate", json!([f.key(), {}]));
                    let message = f.take_call("ActivateConnection");
                    bind(
                        &mut f.objects,
                        P,
                        if replacement { UUID2 } else { UUID },
                        A,
                        50,
                        if replacement { 2 } else { 4 },
                    );
                    f.engine.reasons.insert(C.to_owned(), 9);
                    f.reply(message.method_return().append1(dbus_path(C).unwrap()));
                    let error = &f.result(1).unwrap()["error"];
                    assert_eq!(
                        error["code"],
                        if replacement {
                            "network_error"
                        } else {
                            "authentication"
                        }
                    );
                    assert!(f.calls.is_empty());
                }
            }
            #[test]
            fn guarded_profile_updates_cannot_activate_a_replacement() {
                let mut f = Fixture::new(Security::Psk);
                f.detect(Security::Psk);
                f.request(1, "activate", json!([f.key(), { "password": PASSWORD }]));
                let message = f.take_call("Update2");
                f.engine.incarnations.remove(P);
                f.reply(message.method_return().append1(PropMap::new()));
                assert!(f.result(1).unwrap().get("error").is_some());
                assert!(f.calls.is_empty());
            }
            #[test]
            fn closing_after_disconnect_or_first_delete_stops_forget() {
                for connected in [false, true] {
                    let mut f = Fixture::new(Security::Psk);
                    f.detect(Security::Psk);
                    f.settings
                        .insert(P2.to_owned(), saved(UUID2, b"fixture", Security::Psk));
                    put(&mut f.objects, P2, CONNECTION, "VersionId", 1u64);
                    if connected {
                        bind(&mut f.objects, P, UUID, A, 100, 2);
                    }
                    f.refresh();
                    f.request(1, "forget", json!([f.key()]));
                    let sent = f.take_call(if connected { "Disconnect" } else { "Delete" });
                    f.engine
                        .enqueue(br#"{"method":"setOpened","args":[false]}"#, f.now);
                    f.reply(sent.method_return());
                    assert_eq!(f.result(1).unwrap()["error"]["code"], "cancelled");
                    assert!(f.calls.is_empty());
                }
            }
            #[test]
            fn stale_confirmation_or_authentication_context_never_dispatches() {
                for change in ["replacement", "addition", "authentication"] {
                    let mut f = Fixture::new(Security::Psk);
                    let stamp = f.engine.last_state["rows"][0]["selectionStamp"].clone();
                    match change {
                        "replacement" => {
                            f.engine.incarnations.remove(P);
                        }
                        "addition" => {
                            f.settings
                                .insert(P2.to_owned(), saved(UUID2, b"fixture", Security::Psk));
                            put(&mut f.objects, P2, CONNECTION, "VersionId", 1u64);
                        }
                        _ => insert(f.settings.get_mut(P).unwrap(), SECURITY, "pmf", 3u32),
                    }
                    f.refresh();
                    assert_ne!(stamp, f.engine.last_state["rows"][0]["selectionStamp"]);
                    let key = f.key();
                    for (method, args) in [
                        ("forget", json!([key])),
                        ("activate", json!([key, {"password": PASSWORD}])),
                        ("readSavedSecret", json!([key, UUID])),
                    ] {
                        f.engine.enqueue(
                            json!({"id":1,"method":method,"args":args,"expectedStamp":stamp})
                                .to_string()
                                .as_bytes(),
                            f.now,
                        );
                        assert!(f.engine.operations.is_empty());
                        assert!(f.result(1).unwrap().get("error").is_some());
                        f.pump();
                        assert!(f.calls.is_empty());
                    }
                }
            }
            #[test]
            fn forget_deletes_all_original_profiles_sequentially_without_retargeting() {
                let mut f = Fixture::new(Security::Psk);
                f.detect(Security::Psk);
                f.settings
                    .insert(P2.to_owned(), saved(UUID2, b"fixture", Security::Psk));
                put(&mut f.objects, P2, CONNECTION, "VersionId", 1u64);
                bind(&mut f.objects, P, UUID, A, 100, 2);
                f.refresh();
                f.request(1, "forget", json!([f.key()]));
                let disconnect = f.take_call("Disconnect");
                put(&mut f.objects, D, DEVICE, "State", 30u32);
                put(
                    &mut f.objects,
                    D,
                    DEVICE,
                    "ActiveConnection",
                    dbus_path("/").unwrap(),
                );
                f.reply(disconnect.method_return());
                let deletion = f.take_call("Delete");
                assert_eq!(deletion.path().unwrap().to_string(), P);
                f.objects.remove(&dbus_path(P).unwrap());
                f.settings.remove(P);
                f.reply(deletion.method_return());
                let deletion = f.take_call("Delete");
                assert_eq!(deletion.path().unwrap().to_string(), P2);
                f.objects.remove(&dbus_path(P2).unwrap());
                f.settings.remove(P2);
                f.reply(deletion.method_return());
                assert!(f.result(1).unwrap().get("result").is_some());
                assert!(f.calls.is_empty());
                let mut f = Fixture::new(Security::Psk);
                f.request(2, "forget", json!([f.key()]));
                let deletion = f.take_call("Delete");
                f.engine.incarnations.remove(P);
                f.reply(deletion.method_return());
                assert!(f.result(2).unwrap().get("error").is_some());
                assert!(f.calls.is_empty());
            }
            #[test]
            fn scans_are_immediate_periodic_open_only_nonoverlapping_and_radio_guarded() {
                let mut f = Fixture::new(Security::Psk);
                assert!(f.calls.is_empty());
                f.engine
                    .enqueue(br#"{"method":"setOpened","args":[true]}"#, f.now);
                f.pump();
                let first = f.take_call("RequestScan");
                f.now += Duration::from_secs(1);
                f.pump();
                assert!(f.calls.is_empty());
                f.reply(first.method_return());
                f.now += Duration::from_secs(13);
                f.pump();
                assert!(f.calls.is_empty());
                f.now += Duration::from_secs(1);
                f.pump();
                let second = f.take_call("RequestScan");
                f.engine
                    .enqueue(br#"{"method":"setOpened","args":[false]}"#, f.now);
                f.reply(second.method_return());
                f.now += Duration::from_secs(45);
                f.pump();
                assert!(f.calls.is_empty());
                assert!(f.engine.scans.is_empty());
                put(&mut f.objects, ROOT, NM, "WirelessHardwareEnabled", false);
                f.refresh();
                f.engine
                    .enqueue(br#"{"method":"setOpened","args":[true]}"#, f.now);
                f.pump();
                assert!(f.calls.is_empty());
                f.request(1, "toggleWifi", json!([]));
                assert!(
                    f.result(1).unwrap()["error"]["message"]
                        .as_str()
                        .unwrap()
                        .contains("hardware")
                );
                assert!(f.calls.is_empty());
            }
            #[test]
            fn raw_errors_and_malformed_requests_are_secret_safe_and_queues_are_bounded() {
                let mut f = Fixture::new(Security::Psk);
                f.request(1, "readSavedSecret", json!([f.key(), UUID]));
                let message = f.take_call("GetSecrets");
                let name = "org.freedesktop.NetworkManager.SecretAgent.NoSecrets".into();
                let body = std::ffi::CString::new(PASSWORD).unwrap();
                f.reply(message.error(&name, &body));
                assert_eq!(f.result(1).unwrap()["error"]["code"], "authentication");
                assert!(!f.result(1).unwrap().to_string().contains(PASSWORD));
                for method in ["spawn", "refresh", "getIdentity", "details", "Delete"] {
                    f.request(2, method, json!([PASSWORD]));
                    assert_eq!(f.result(2).unwrap()["error"]["code"], "invalid_request");
                }
                let mut input = Input::default();
                input.feed(&vec![b'x'; REQUEST_LIMIT + 100], &mut f.engine, f.now);
                assert!(input.line.len() <= REQUEST_LIMIT);
                input.feed(b"\n", &mut f.engine, f.now);
                assert_eq!(
                    f.engine.output.back().unwrap()["error"]["code"],
                    "invalid_request"
                );
                f.engine.output.clear();
                for id in 10..10 + MAX_OPERATIONS as u32 + 5 {
                    f.engine.enqueue(
                        json!({ "id": id, "method": "readSavedSecret", "args": [f.key(), UUID], "expectedStamp": f.engine.last_state["rows"][0]["selectionStamp"] })
                            .to_string()
                            .as_bytes(),
                        f.now,
                    );
                }
                assert_eq!(f.engine.operations.len(), MAX_OPERATIONS);
                let mut output = Output::default();
                assert!(output.push(json!({"type":"state","rows":[1]})));
                assert!(output.push(json!({"type":"state","rows":[2]})));
                assert_eq!(output.replies.len(), 0);
                assert!(output.state.is_some());
                assert!(!output.push(json!({"id":1,"result":"x".repeat(OUTPUT_LIMIT)})));
            }

            #[test]
            fn forgetting_an_offline_profile_does_not_disconnect_an_unrelated_live_network() {
                let mut f = Fixture::new(Security::Psk);
                f.detect(Security::Psk);
                let other = "/org/freedesktop/NetworkManager/AccessPoint/9";
                add_ap(&mut f.objects, other, b"other", Security::Psk, 80);
                f.settings
                    .insert(P2.to_owned(), saved(UUID2, b"other", Security::Psk));
                put(&mut f.objects, P2, CONNECTION, "VersionId", 1u64);
                bind(&mut f.objects, P2, UUID2, other, 100, 2);
                f.refresh();
                f.request(1, "forget", json!([f.key()]));
                let deletion = f.take_call("Delete");
                assert_eq!(deletion.path().unwrap().to_string(), P);
                f.objects.remove(&dbus_path(P).unwrap());
                f.settings.remove(P);
                f.reply(deletion.method_return());
                assert!(f.result(1).unwrap().get("result").is_some());
                assert!(f.calls.is_empty());
                let live = f
                    .engine
                    .entries
                    .iter()
                    .find(|entry| entry.ssid == b"other")
                    .unwrap();
                assert_eq!(
                    live.identity(&f.engine.snapshot).unwrap().profile_uuid,
                    UUID2
                );
            }
            #[test]
            fn mixed_old_ap_and_new_active_profile_cannot_publish_the_new_networks_ip_details() {
                let mut f = Fixture::new(Security::Psk);
                f.detect(Security::Psk);
                f.settings
                    .insert(P2.to_owned(), saved(UUID2, b"other", Security::Psk));
                put(&mut f.objects, P2, CONNECTION, "VersionId", 1u64);
                bind(&mut f.objects, P2, UUID2, A, 100, 2);
                f.refresh();
                let key = f.key();
                let row = f.engine.last_state["rows"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|row| row["key"] == key)
                    .unwrap();
                assert_eq!(row["connected"], false);
                assert!(f.engine.last_state["identities"][&key].is_null());
                let details = &f.engine.last_state["details"][&key];
                assert_eq!(details["profileUuid"], UUID);
                assert!(details["ipv4"].is_null());
                assert!(details["gateway4"].is_null());
                assert!(details["activePath"].is_null());
                f.request(1, "forget", json!([key]));
                let deletion = f.take_call("Delete");
                assert_eq!(deletion.path().unwrap().to_string(), P);
                assert!(
                    f.calls.is_empty(),
                    "an unrelated active profile must not be disconnected"
                );
            }
            #[test]
            fn newly_constructed_connection_activation_binds_the_returned_profile_and_uuid() {
                let mut f = Fixture::new(Security::Open);
                f.settings.clear();
                f.objects.remove(&dbus_path(P).unwrap());
                f.detect(Security::Open);
                let key = f.key();
                assert_eq!(f.engine.last_state["rows"][0]["requiresPassword"], false);
                assert!(
                    f.engine.last_state["details"]
                        .as_object()
                        .unwrap()
                        .is_empty(),
                    "unsaved details are absent, not null"
                );
                f.request(1, "activate", json!([key, {}]));
                let message = f.take_call("AddAndActivateConnection");
                let (settings, device, ap) = message
                    .read3::<Settings, Path<'static>, Path<'static>>()
                    .unwrap();
                let uuid = text(&settings_json(&settings)["connection"], "uuid");
                assert_eq!(&*device, D);
                assert_eq!(&*ap, A);
                f.settings.insert(P.to_owned(), settings);
                put(&mut f.objects, P, CONNECTION, "VersionId", 1u64);
                bind(&mut f.objects, P, &uuid, A, 40, 1);
                f.reply(
                    message
                        .method_return()
                        .append2(dbus_path(P).unwrap(), dbus_path(C).unwrap()),
                );
                assert!(f.result(1).is_none());
                bind(&mut f.objects, P, &uuid, A, 100, 2);
                f.refresh();
                assert!(f.result(1).unwrap().get("result").is_some());
                assert_eq!(f.engine.last_state["identities"][key]["profileUuid"], uuid);
            }
            #[test]
            fn saved_wep_retrieval_uses_only_the_selected_transmit_key() {
                let mut f = Fixture::new(Security::Wep);
                insert(
                    f.settings.get_mut(P).unwrap(),
                    SECURITY,
                    "wep-tx-keyidx",
                    2u32,
                );
                f.refresh();
                f.request(1, "readSavedSecret", json!([f.key(), UUID]));
                let message = f.take_call("GetSecrets");
                let mut secrets = Settings::new();
                insert(&mut secrets, SECURITY, "wep-key0", "wrong-key".to_owned());
                insert(&mut secrets, SECURITY, "wep-key2", "abcde".to_owned());
                f.reply(message.method_return().append1(secrets));
                assert_eq!(
                    f.result(1).unwrap()["result"],
                    json!({ "password":"abcde", "authentication":"WEP", "hidden":true })
                );
                assert!(!f.engine.last_state.to_string().contains("abcde"));
            }
            #[test]
            fn owner_replacement_cancels_without_replaying_a_secret_request() {
                let mut f = Fixture::new(Security::Psk);
                f.request(1, "readSavedSecret", json!([f.key(), UUID]));
                let message = f.take_call("GetSecrets");
                f.engine.set_owner(":1.43".to_owned(), f.now);
                let mut secrets = Settings::new();
                insert(&mut secrets, SECURITY, "psk", PASSWORD.to_owned());
                f.reply(message.method_return().append1(secrets));
                assert_eq!(f.result(1).unwrap()["error"]["code"], "unavailable");
                assert!(f.calls.is_empty());
                assert!(!f.result(1).unwrap().to_string().contains(PASSWORD));
            }
            #[test]
            fn ssid_display_uses_native_encoding_hints_without_losing_raw_identity() {
                assert_eq!(ssid_convert(&[0xe9], "iso-8859-1").unwrap(), "é");
                assert_eq!(ssid_encodings("ru_RU.UTF-8")[0], "koi8-r");
                assert_eq!(ssid_encodings("zh_TW.UTF-8")[0], "big5");
                assert_eq!(ssid_text("Coffee ☕".as_bytes()), "Coffee ☕");
                assert_eq!(ssid_text(b"hidden\0suffix"), "hidden");
            }

            struct PrivateBus {
                child: Child,
                address: String,
                config: PathBuf,
            }
            impl PrivateBus {
                fn new() -> Self {
                    static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
                    let config = std::env::temp_dir().join(format!(
                        "network-dbus-test-{}-{}.conf",
                        std::process::id(),
                        SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                    ));
                    // A private fixture must not depend on /etc session policy or
                    // activate services from the host desktop.
                    std::fs::write(
                        &config,
                        r#"<busconfig>
                    <type>session</type>
                    <listen>unix:tmpdir=/tmp</listen>
                    <auth>EXTERNAL</auth>
                    <policy context="default">
                        <allow send_destination="*"/>
                        <allow receive_sender="*"/>
                        <allow own="*"/>
                    </policy>
                </busconfig>"#,
                    )
                    .unwrap();
                    let mut child = Command::new("dbus-daemon")
                        .arg("--config-file")
                        .arg(&config)
                        .args(["--nofork", "--nopidfile", "--print-address=1"])
                        .stdout(Stdio::piped())
                        .stderr(Stdio::inherit())
                        .spawn()
                        .expect("private-bus fixture needs dbus-daemon");
                    let stdout = child.stdout.take().unwrap();
                    let (sender, receiver) = mpsc::sync_channel(1);
                    thread::spawn(move || {
                        let mut address = String::new();
                        let _ = BufReader::new(stdout).read_line(&mut address);
                        let _ = sender.send(address);
                    });
                    let mut bus = Self {
                        child,
                        address: String::new(),
                        config,
                    };
                    bus.address = receiver
                        .recv_timeout(Duration::from_secs(2))
                        .expect("private daemon startup deadline")
                        .trim()
                        .to_owned();
                    assert!(
                        bus.address.starts_with("unix:"),
                        "private dbus-daemon did not publish a Unix address; status: {:?}",
                        bus.child.try_wait()
                    );
                    bus
                }
                fn connect(&self) -> Channel {
                    let mut channel = Channel::open_private(&self.address).unwrap();
                    channel.register().unwrap();
                    channel
                }
            }
            impl Drop for PrivateBus {
                fn drop(&mut self) {
                    let _ = self.child.kill();
                    let _ = self.child.wait();
                    let _ = std::fs::remove_file(&self.config);
                }
            }
            fn read_frame(reader: &mut BufReader<UnixStream>) -> Value {
                let mut line = String::new();
                assert!(reader.read_line(&mut line).expect("JSONL read deadline") > 0);
                serde_json::from_str(&line).expect("complete JSONL frame")
            }
            fn read_until(reader: &mut BufReader<UnixStream>, predicate: impl Fn(&Value) -> bool) -> Value {
                for _ in 0..64 {
                    let frame = read_frame(reader);
                    if predicate(&frame) {
                        return frame;
                    }
                }
                panic!("fixture frame limit");
            }
            #[test]
            fn private_dbus_jsonl_fixture_stays_responsive_to_cancel_late_reply_and_eof() {
                enum Control {
                    Reply,
                    Stop,
                }
                let private = PrivateBus::new();
                let exporter = private.connect();
                let reply = exporter
                    .send_with_reply_and_block(
                        call(DBUS, DBUS_PATH, DBUS, "RequestName")
                            .unwrap()
                            .append2(NM, 0u32),
                        READ_TIME,
                    )
                    .unwrap();
                assert_eq!(reply.read1::<u32>().unwrap(), 1);
                let (control_tx, control_rx) = mpsc::channel();
                let (observed_tx, observed_rx) = mpsc::channel();
                let exporter_thread = thread::spawn(move || {
                    let objects = basic_objects();
                    let settings = saved(UUID, b"fixture", Security::Psk);
                    let mut held = Vec::<Message>::new();
                    let mut methods = Vec::new();
                    let started = Instant::now();
                    while started.elapsed() < Duration::from_secs(12) {
                        while let Ok(control) = control_rx.try_recv() {
                            match control {
                                Control::Stop => return methods,
                                Control::Reply => {
                                    for message in held.drain(..) {
                                        let mut secrets = Settings::new();
                                        insert(&mut secrets, SECURITY, "psk", PASSWORD.to_owned());
                                        exporter
                                            .send(message.method_return().append1(secrets))
                                            .unwrap();
                                    }
                                }
                            }
                        }
                        exporter
                            .read_write(Some(Duration::from_millis(10)))
                            .unwrap();
                        for _ in 0..64 {
                            let Some(message) = exporter.pop_message() else {
                                break;
                            };
                            if message.msg_type() != MessageType::MethodCall {
                                continue;
                            }
                            let method = member(&message);
                            methods.push(method.clone());
                            let response = match method.as_str() {
                                "GetManagedObjects" => {
                                    Some(message.method_return().append1(clone_managed(&objects)))
                                }
                                "GetSettings" => {
                                    Some(message.method_return().append1(clone_settings(&settings)))
                                }
                                "GetSecrets" => {
                                    held.push(message);
                                    observed_tx.send("GetSecrets").unwrap();
                                    None
                                }
                                _ => {
                                    panic!("unexpected private fixture mutation {method}");
                                }
                            };
                            if let Some(response) = response {
                                exporter.send(response).unwrap();
                            }
                        }
                    }
                    panic!("private exporter fixture deadline");
                });
                let (mut writer, service_input) = UnixStream::pair().unwrap();
                let (service_output, reader) = UnixStream::pair().unwrap();
                reader
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut reader = BufReader::new(reader);
                let (connect_tx, connect_rx) = mpsc::channel();
                connect_tx.send(Some(private.connect())).unwrap();
                let (done_tx, done_rx) = mpsc::channel();
                let runtime_thread = thread::spawn(move || {
                    let status = run_loop(
                        service_input.as_raw_fd(),
                        service_output.as_raw_fd(),
                        connect_rx,
                    );
                    done_tx.send(status).unwrap();
                });
                let state = read_until(&mut reader, |frame| {
                    frame["type"] == "state" && frame["available"] == true
                });
                let row = &state["rows"][0];
                let key = row["key"].as_str().unwrap().to_owned();
                assert_eq!(row["canShare"], true);
                assert!(row["selectionStamp"].is_string());
                writer
                    .write_all(
                        json!({ "id": 1, "method": "readSavedSecret", "args": [key, UUID], "expectedStamp": row["selectionStamp"] })
                            .to_string()
                            .as_bytes(),
                    )
                    .unwrap();
                writer.write_all(b"\n").unwrap();
                assert_eq!(
                    observed_rx.recv_timeout(Duration::from_secs(2)).unwrap(),
                    "GetSecrets"
                );
                let cancelled_at = Instant::now();
                writer
                    .write_all(b"{\"method\":\"cancel\",\"args\":[1]}\n")
                    .unwrap();
                let cancellation = read_until(&mut reader, |frame| frame["id"] == 1);
                assert_eq!(cancellation["error"]["code"], "cancelled");
                assert!(cancelled_at.elapsed() < Duration::from_secs(1));
                control_tx.send(Control::Reply).unwrap();
                writer
                    .write_all(b"{\"id\":20,\"method\":\"invalid\",\"args\":[]}\n")
                    .unwrap();
                let fence = read_until(&mut reader, |frame| {
                    assert!(!frame.to_string().contains(PASSWORD));
                    frame["id"] == 20
                });
                assert_eq!(fence["error"]["code"], "invalid_request");
                writer
                    .write_all(
                        json!({ "id": 2, "method": "readSavedSecret", "args": [key, UUID], "expectedStamp": row["selectionStamp"] })
                            .to_string()
                            .as_bytes(),
                    )
                    .unwrap();
                writer.write_all(b"\n").unwrap();
                assert_eq!(
                    observed_rx.recv_timeout(Duration::from_secs(2)).unwrap(),
                    "GetSecrets"
                );
                drop(writer);
                assert_eq!(
                    done_rx
                        .recv_timeout(Duration::from_secs(1))
                        .expect("EOF must not wait on GetSecrets"),
                    0
                );
                runtime_thread.join().unwrap();
                let mut remainder = String::new();
                reader.read_to_string(&mut remainder).unwrap();
                assert!(!remainder.contains(PASSWORD));
                control_tx.send(Control::Stop).unwrap();
                let methods = exporter_thread.join().unwrap();
                assert_eq!(
                    methods
                        .iter()
                        .filter(|method| *method == "GetSecrets")
                        .count(),
                    2
                );
                assert!(methods.iter().all(|method| matches!(
                    method.as_str(),
                    "GetManagedObjects" | "GetSettings" | "GetSecrets"
                )));
            }
            #[test]
            fn pending_bus_authentication_does_not_block_eof() {
                let (writer, service_input) = UnixStream::pair().unwrap();
                let (service_output, _reader) = UnixStream::pair().unwrap();
                let (_connect_tx, connect_rx) = mpsc::channel();
                let (done_tx, done_rx) = mpsc::channel();
                let worker = thread::spawn(move || {
                    done_tx
                        .send(run_loop(
                            service_input.as_raw_fd(),
                            service_output.as_raw_fd(),
                            connect_rx,
                        ))
                        .unwrap();
                });
                drop(writer);
                assert_eq!(done_rx.recv_timeout(Duration::from_secs(1)).unwrap(), 0);
                worker.join().unwrap();
            }
        }
    };
}
pub(crate) use engine_tests;
