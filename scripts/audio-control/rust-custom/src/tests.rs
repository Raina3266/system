//! Real CLI and resident-protocol integration tests.

use audio_control::{AppResult, audio, devices, model, policy};

#[path = "test_fixtures.rs"]
pub mod fixtures;

mod resident_protocol {
    use super::fixtures::resident::{Fake, simple};
    use crate::{model::AudioKind, policy::panel::Native};
    use serde_json::{Value, json};
    use std::{
        collections::HashMap,
        io::{BufRead, BufReader, Write},
        process::{Child, ChildStdin, Command, Stdio},
        sync::mpsc,
        thread,
        time::{Duration, Instant},
    };

    struct Resident {
        child: Child,
        input: ChildStdin,
        output: mpsc::Receiver<Value>,
        fake: Fake,
        held: HashMap<String, Value>,
        id: u64,
        model: Value,
    }
    impl Resident {
        fn new(fake: Fake) -> Self {
            Self::with_dump(fake, std::path::Path::new("/nonexistent/pw-dump"))
        }
        fn with_dump(fake: Fake, dump: &std::path::Path) -> Self {
            let mut child = Command::new(env!("CARGO_BIN_EXE_audio-control"))
                .arg("panel")
                .env("DBUS_SYSTEM_BUS_ADDRESS", "disabled:")
                .env("AUDIO_CONTROL_PW_DUMP", dump)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .unwrap();
            let input = child.stdin.take().unwrap();
            let stdout = child.stdout.take().unwrap();
            let (sender, output) = mpsc::channel();
            thread::spawn(move || {
                for line in BufReader::new(stdout).lines() {
                    if sender
                        .send(serde_json::from_str(&line.unwrap()).unwrap())
                        .is_err()
                    {
                        break;
                    }
                }
            });
            let mut resident = Self {
                child,
                input,
                output,
                fake,
                held: HashMap::new(),
                id: 0,
                model: Value::Null,
            };
            resident.snapshot();
            resident
        }
        fn send(&mut self, value: Value) {
            writeln!(self.input, "{value}").unwrap();
            self.input.flush().unwrap();
        }
        fn snapshot(&mut self) {
            self.send(json!({"type":"snapshot","snapshot":self.fake.state}));
        }
        fn intent(&mut self, command: &str, fields: Value) {
            let mut value = json!({"type":"intent","command":command});
            for (key, item) in fields.as_object().unwrap() {
                value[key] = item.clone();
            }
            self.send(value);
        }
        fn step(&mut self, event: Value) {
            match event["type"].as_str().unwrap() {
                "model" => self.model = event["model"].clone(),
                "snapshot" | "action" => {
                    assert_eq!(event["id"], self.id + 1);
                    self.id += 1;
                    let result = if event["type"] == "snapshot" {
                        Ok(self.fake.state.clone())
                    } else {
                        let key = event["target"].as_str().unwrap();
                        let operation = event["operation"].as_str().unwrap();
                        let absent = self.fake.state.object(key).is_err();
                        if absent && let Some(object) = self.held.get(key) {
                            self.fake.state.objects.push(object.clone());
                        }
                        let result = self.fake.execute(
                            operation,
                            key,
                            event["args"].clone(),
                            serde_json::from_value(event["guards"].clone()).unwrap(),
                            event["cleanup"] == true,
                        );
                        if result.is_ok() {
                            if operation == "retain" {
                                self.held.insert(
                                    key.into(),
                                    self.fake.state.object(key).unwrap().clone(),
                                );
                            } else if operation == "release" {
                                self.held.remove(key);
                            }
                        }
                        if absent {
                            self.fake
                                .state
                                .objects
                                .retain(|object| object["key"] != key);
                        }
                        result
                    };
                    self.send(match result {Ok(_)=>json!({"type":"reply","id":event["id"],"ok":true,"snapshot":self.fake.state}),Err(error)=>json!({"type":"reply","id":event["id"],"ok":false,"error":error.to_string(),"snapshot":self.fake.state})});
                }
                other => panic!("unexpected event {other}: {event}"),
            }
        }
        fn until(&mut self, check: impl Fn(&Self) -> bool) {
            let deadline = Instant::now() + Duration::from_secs(6);
            while !check(self) {
                let event = self
                    .output
                    .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                    .expect("resident progress timed out");
                self.step(event);
            }
        }
        fn quiet(&mut self, milliseconds: u64) {
            let deadline = Instant::now() + Duration::from_millis(milliseconds);
            while let Ok(event) = self
                .output
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            {
                self.step(event);
                if Instant::now() >= deadline {
                    break;
                }
            }
        }
        fn ready(&mut self) {
            self.until(|r| {
                r.model["output"]["loading"] == false && r.model["input"]["loading"] == false
            });
        }
        fn actions(&self, operation: &str) -> usize {
            self.fake
                .actions
                .iter()
                .filter(|action| action.0 == operation)
                .count()
        }
        fn close(&mut self) {
            self.send(json!({"type":"shutdown"}));
            loop {
                let event = self.output.recv_timeout(Duration::from_secs(7)).unwrap();
                if event["type"] == "closed" {
                    break;
                }
                self.step(event);
            }
            assert!(self.child.wait().unwrap().success());
        }
    }
    impl Drop for Resident {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
    fn bluetooth() -> Fake {
        let mut state = simple(AudioKind::Output);
        state.objects.extend([json!({"key":"adapter","type":"adapter","path":"/org/bluez/hci0","powered":true,"discovering":true}),json!({"key":"buds","type":"device","address":"AA:BB:CC:DD:EE:01","adapter":"/org/bluez/hci0","name":"Headset","alias":"","connected":false,"paired":true,"trusted":false,"battery":0.5})]);
        state.adapter = "adapter".into();
        Fake::new(state)
    }

    #[test]
    fn resident_native_selection_is_end_to_end_and_clears_playback_only_after_default_ack() {
        let mut native = simple(AudioKind::Output);
        native.objects.push(json!({"key":"Chrome","type":"stream","id":40,"serial":"400","targetSerial":100,"name":"Chrome"}));
        let mut resident = Resident::new(Fake::new(native));
        resident.ready();
        let key = resident.model["output"]["choices"][0]["key"].clone();
        resident.intent("select", json!({"kind":"output","key":key}));
        resident.until(|r| r.actions("follow") == 1 && r.model["output"]["busy"] == false);
        let operations: Vec<_> = resident
            .fake
            .actions
            .iter()
            .map(|action| action.0.as_str())
            .collect();
        assert_eq!(operations, ["default", "follow"]);
        assert_eq!(
            resident.fake.state.object("Chrome").unwrap()["targetSerial"],
            -1
        );
        resident.close();
    }

    #[test]
    fn resident_physical_profile_selection_and_rollback_use_guarded_native_rpcs_end_to_end() {
        use super::{fixtures, fixtures::resident::native, protocol_and_cli::DumpFixture};
        use crate::audio::{Action, Backend};
        for (fail_route, external, final_race) in [
            (false, false, false),
            (true, false, false),
            (true, true, false),
            (true, false, true),
        ] {
            let mut legacy = fixtures::Fake::default();
            let fixture = std::sync::Arc::new(DumpFixture::new(&legacy.dump));
            let updates = fixture.clone();
            let mut initial = native(legacy.snapshot.clone());
            initial.objects.push(json!({"key":"Chrome","type":"stream","id":40,"serial":"400","name":"Chrome","targetSerial":100}));
            let mut fake = Fake::new(initial);
            if fail_route {
                fake.fail = "route".into();
            }
            fake.hook = Some(Box::new(move |fake, operation| {
                if !matches!(operation, "profile" | "route" | "default") {
                    return;
                }
                let (_, key, args) = fake.actions.last().unwrap();
                let object = fake.state.object(key).unwrap();
                let identity = crate::model::Identity {
                    id: object["id"].as_u64().unwrap() as u32,
                    serial: object["serial"].as_str().unwrap().into(),
                };
                let action = match operation {
                    "profile" => Action::Profile {
                        card: identity,
                        profile: args["profile"].as_i64().unwrap() as i32,
                    },
                    "route" => Action::Route {
                        node: identity,
                        route: args["route"].as_i64().unwrap() as i32,
                    },
                    _ => Action::Default { node: identity },
                };
                if external && matches!(action, Action::Profile { profile: 1, .. }) {
                    legacy.snapshot.default_output = Some(crate::model::Identity {
                        id: 20,
                        serial: "200".into(),
                    });
                }
                legacy.action(action).unwrap();
                let snapshot = legacy.observe().unwrap().snapshot;
                updates.update(&legacy.dump);
                let streams = fake.state.objects("stream").cloned().collect::<Vec<_>>();
                fake.state = native(snapshot);
                fake.state.objects.extend(streams);
            }));
            let mut resident = Resident::with_dump(fake, &fixture.executable);
            resident.ready();
            let key = crate::audio::physical_key(fixtures::CARD, fixtures::HEADPHONES);
            assert!(
                resident.model["output"]["choices"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|row| row["key"] == key)
            );
            resident.intent("select", json!({"kind":"output","key":key}));
            if final_race {
                loop {
                    let event = resident
                        .output
                        .recv_timeout(Duration::from_secs(7))
                        .unwrap();
                    if event["operation"] == "default" {
                        assert_eq!(resident.actions("profile"), 2);
                        resident.fake.state.output_epoch.push('x');
                        resident.step(event);
                        break;
                    }
                    resident.step(event);
                }
            }
            resident.until(|r| r.actions("profile") > 0 && r.model["output"]["busy"] == false);
            if fail_route {
                assert!(
                    resident.model["output"]["error"]
                        .as_str()
                        .unwrap()
                        .contains(if final_race {
                            "Native object changed"
                        } else {
                            "Previous profile restored"
                        })
                );
                if final_race {
                    assert_eq!(resident.actions("default"), 0);
                }
                assert_eq!(resident.actions("follow"), 0);
                assert_eq!(resident.actions("profile"), 2);
                if external {
                    assert_eq!(resident.actions("default"), 0);
                    assert_eq!(resident.fake.state.default_output, "node:20:200");
                }
                assert_eq!(
                    resident.fake.state.object("Chrome").unwrap()["targetSerial"],
                    100
                );
            } else {
                assert_eq!(resident.model["output"]["error"], "");
                assert_eq!(resident.actions("follow"), 1);
                assert_eq!(
                    resident.fake.state.object("Chrome").unwrap()["targetSerial"],
                    -1
                );
            }
            resident.close();
        }
    }

    #[test]
    fn volume_and_mute_queues_are_rust_serialized_and_do_not_reset_playback() {
        let mut resident = Resident::new(Fake::new(simple(AudioKind::Output)));
        resident.ready();
        let key = resident.model["output"]["nodeKey"].clone();
        for value in [0.1, 0.2, 1.5] {
            resident.intent("volume", json!({"kind":"output","key":key,"value":value}));
        }
        resident.until(|r| r.actions("volume") == 3);
        assert_eq!(
            resident
                .fake
                .actions
                .iter()
                .filter(|action| action.0 == "volume")
                .map(|action| action.2["volume"].clone())
                .collect::<Vec<_>>(),
            [json!(0.1), json!(0.2), json!(1.5)]
        );
        assert_eq!(resident.actions("follow"), 0);
        resident.intent("mute", json!({"kind":"output","key":key}));
        resident.intent("mute", json!({"kind":"output","key":key}));
        resident.until(|r| r.actions("mute") == 2);
        assert_eq!(
            resident
                .fake
                .actions
                .iter()
                .filter(|action| action.0 == "mute")
                .map(|action| action.2["muted"].clone())
                .collect::<Vec<_>>(),
            [json!(true), json!(false)]
        );
        resident.close();
    }

    #[test]
    fn discovery_owns_a_session_even_if_another_client_is_scanning_and_follows_tab_power_and_shutdown()
     {
        let mut resident = Resident::new(bluetooth());
        resident.ready();
        assert_eq!(resident.actions("startDiscovery"), 0);
        resident.fake.state.opened = true;
        resident.fake.state.tab = 0;
        resident.snapshot();
        resident.until(|r| r.actions("startDiscovery") == 1);
        resident.fake.state.tab = 1;
        resident.snapshot();
        resident.until(|r| r.actions("stopDiscovery") == 1);
        resident.fake.state.tab = 0;
        resident.snapshot();
        resident.until(|r| r.actions("startDiscovery") == 2);
        resident.intent("bluetoothPower", json!({}));
        resident.until(|r| r.actions("power") == 1);
        resident.quiet(100);
        assert_eq!(resident.actions("stopDiscovery"), 2);
        assert_eq!(resident.actions("startDiscovery"), 2);
        assert_eq!(resident.model["bluetooth"]["powered"], false);
        resident
            .fake
            .state
            .objects
            .iter_mut()
            .find(|object| object["key"] == "adapter")
            .unwrap()["powered"] = true.into();
        resident.snapshot();
        resident.until(|r| r.actions("startDiscovery") == 3);
        resident.close();
        assert_eq!(resident.actions("stopDiscovery"), 3);
        assert!(resident.held.is_empty());
    }

    #[test]
    fn failed_discovery_start_does_not_spin_or_stop_an_unowned_session_and_can_retry() {
        let mut fake = bluetooth();
        fake.fail = "startDiscovery".into();
        fake.state.opened = true;
        fake.state.tab = 0;
        let mut resident = Resident::new(fake);
        resident.until(|r| {
            r.model["bluetooth"]["error"]
                .as_str()
                .is_some_and(|error| error.contains("rejected"))
        });
        resident.quiet(100);
        assert_eq!(resident.actions("startDiscovery"), 1);
        assert_eq!(resident.actions("stopDiscovery"), 0);
        resident.fake.state.opened = false;
        resident.snapshot();
        resident.quiet(50);
        assert_eq!(resident.actions("stopDiscovery"), 0);
        resident.fake.fail.clear();
        resident.fake.state.opened = true;
        resident.snapshot();
        resident.until(|r| r.actions("startDiscovery") == 2);
        resident.close();
        assert_eq!(resident.actions("stopDiscovery"), 1);
    }

    #[test]
    fn failed_discovery_stop_retains_ownership_for_later_cleanup_without_spinning() {
        let mut fake = bluetooth();
        fake.state.opened = true;
        fake.state.tab = 0;
        let mut resident = Resident::new(fake);
        resident.until(|r| r.actions("startDiscovery") == 1);
        resident.fake.fail = "stopDiscovery".into();
        resident.fake.state.opened = false;
        resident.snapshot();
        resident.until(|r| {
            r.model["bluetooth"]["error"]
                .as_str()
                .is_some_and(|error| error.contains("rejected"))
        });
        resident.quiet(100);
        assert_eq!(resident.actions("stopDiscovery"), 1);
        assert!(!resident.held.is_empty());
        resident.fake.fail.clear();
        resident.intent("active", json!({}));
        resident.until(|r| r.actions("stopDiscovery") == 2);
        resident.close();
        assert!(resident.held.is_empty());
    }

    #[test]
    fn adapter_replacement_stops_only_the_owned_old_adapter_and_starts_the_current_one() {
        for fail_stop in [false, true] {
            let mut fake = bluetooth();
            fake.state.opened = true;
            fake.state.tab = 0;
            let mut resident = Resident::new(fake);
            resident.until(|r| r.actions("startDiscovery") == 1);
            resident
                .fake
                .state
                .objects
                .retain(|object| object["key"] != "adapter");
            resident.fake.state.objects.push(json!({"key":"replacement","type":"adapter","path":"/org/bluez/hci12","powered":true,"discovering":false}));
            resident.fake.state.adapter = "replacement".into();
            if fail_stop {
                resident.fake.fail = "stopDiscovery".into();
            }
            resident.snapshot();
            resident.until(|r| r.actions("startDiscovery") == 2);
            assert!(
                resident
                    .fake
                    .actions
                    .iter()
                    .any(|action| action.0 == "stopDiscovery" && action.1 == "adapter")
            );
            assert_eq!(
                resident
                    .fake
                    .actions
                    .iter()
                    .filter(|action| action.0 == "startDiscovery")
                    .map(|action| action.1.as_str())
                    .collect::<Vec<_>>(),
                ["adapter", "replacement"]
            );
            assert_eq!(resident.held.contains_key("adapter"), fail_stop);
            assert!(resident.held.contains_key("replacement"));
            resident.fake.fail.clear();
            resident.close();
            let stops: Vec<_> = resident
                .fake
                .actions
                .iter()
                .filter(|a| a.0 == "stopDiscovery")
                .map(|a| a.1.as_str())
                .collect();
            if fail_stop {
                assert_eq!(stops, ["adapter", "adapter", "replacement"]);
            } else {
                assert_eq!(stops, ["adapter", "replacement"]);
            }
            assert!(resident.held.is_empty());
        }
    }

    #[test]
    fn bluetooth_device_semantic_authorization_and_forget_run_only_in_rust() {
        let mut resident = Resident::new(bluetooth());
        resident.ready();
        resident.intent("device", json!({"key":"buds"}));
        resident.until(|r| {
            r.actions("connect") == 1 && r.model["bluetooth"]["devices"][0]["busy"] == false
        });
        resident.intent("forget", json!({"key":"buds"}));
        resident.until(|r| r.actions("forget") == 1);
        resident
            .fake
            .state
            .objects
            .iter_mut()
            .find(|object| object["key"] == "buds")
            .unwrap()["paired"] = false.into();
        resident.snapshot();
        resident.intent("forget", json!({"key":"buds"}));
        resident.quiet(75);
        assert_eq!(resident.actions("forget"), 1);
        resident.intent("device", json!({"key":"missing"}));
        resident.until(|r| {
            !r.model["bluetooth"]["error"]
                .as_str()
                .unwrap_or_default()
                .is_empty()
        });
        resident.close();
    }

    #[test]
    fn cancellation_after_retaining_a_pair_target_prevents_trust_and_connect_and_releases_the_handle()
     {
        let mut fake = bluetooth();
        fake.state.opened = true;
        fake.state.tab = 0;
        fake.state
            .objects
            .iter_mut()
            .find(|object| object["key"] == "buds")
            .unwrap()["paired"] = false.into();
        let mut resident = Resident::new(fake);
        resident.ready();
        resident.intent("device", json!({"key":"buds"}));
        loop {
            let event = resident
                .output
                .recv_timeout(Duration::from_secs(5))
                .unwrap();
            if event["type"] == "action"
                && event["operation"] == "retain"
                && event["target"] == "buds"
            {
                resident.fake.state.opened = false;
                resident.snapshot();
                resident.step(event);
                break;
            }
            resident.step(event);
        }
        resident
            .until(|r| r.actions("cancelPairing") == 1 && r.model["bluetooth"]["pairing"] == false);
        resident.quiet(100);
        assert_eq!(resident.actions("trust"), 0);
        assert_eq!(resident.actions("connect"), 0);
        assert!(resident.model["bluetooth"]["pairingMessage"].is_null());
        resident.close();
        assert!(resident.held.is_empty());
    }

    #[test]
    fn slow_native_bluetooth_completion_is_not_a_six_second_rpc_failure() {
        let mut resident = Resident::new(bluetooth());
        resident.ready();
        resident.intent("device", json!({"key":"buds"}));
        let action = loop {
            let event = resident
                .output
                .recv_timeout(Duration::from_secs(5))
                .unwrap();
            if event["type"] == "action" && event["operation"] == "connect" {
                assert_eq!(event["id"], resident.id + 1);
                resident.id += 1;
                break event;
            }
            resident.step(event);
        };
        resident.quiet(6100);
        assert!(resident.child.try_wait().unwrap().is_none());
        assert_eq!(resident.model["bluetooth"]["devices"][0]["busy"], true);
        resident.send(
            json!({"type":"reply","id":action["id"],"ok":true,"snapshot":resident.fake.state}),
        );
        resident.until(|r| r.model["bluetooth"]["devices"][0]["busy"] == false);
        assert_eq!(resident.model["bluetooth"]["error"], "");
        resident.close();
    }

    #[test]
    fn late_removed_device_failure_cannot_leak_into_a_replacement_with_the_same_address() {
        let mut resident = Resident::new(bluetooth());
        resident.ready();
        resident.intent("device", json!({"key":"buds"}));
        let old = loop {
            let event = resident
                .output
                .recv_timeout(Duration::from_secs(5))
                .unwrap();
            if event["type"] == "action" && event["operation"] == "connect" {
                assert_eq!(event["id"], resident.id + 1);
                resident.id += 1;
                break event;
            }
            resident.step(event);
        };
        resident
            .fake
            .state
            .objects
            .iter_mut()
            .find(|object| object["key"] == "buds")
            .unwrap()["key"] = "new-buds".into();
        resident.snapshot();
        resident.intent("device", json!({"key":"new-buds"}));
        let current = loop {
            let event = resident
                .output
                .recv_timeout(Duration::from_secs(5))
                .unwrap();
            if event["type"] == "action" && event["operation"] == "connect" {
                assert_eq!(event["id"], resident.id + 1);
                resident.id += 1;
                break event;
            }
            resident.step(event);
        };
        resident.send(json!({"type":"reply","id":old["id"],"ok":false,"error":"Old connection failed","snapshot":resident.fake.state}));
        resident.quiet(75);
        assert_eq!(resident.model["bluetooth"]["error"], "");
        assert_eq!(resident.model["bluetooth"]["devices"][0]["key"], "new-buds");
        assert_eq!(resident.model["bluetooth"]["devices"][0]["busy"], true);
        resident.send(
            json!({"type":"reply","id":current["id"],"ok":true,"snapshot":resident.fake.state}),
        );
        resident.until(|r| r.model["bluetooth"]["devices"][0]["busy"] == false);
        resident.close();
    }

    #[test]
    fn rust_metadata_polling_is_visible_audio_only_and_default_controls_do_not_reset_pins() {
        let mut resident = Resident::new(Fake::new(simple(AudioKind::Output)));
        resident.ready();
        let start = Instant::now();
        resident.quiet(200);
        assert!(resident.fake.actions.is_empty());
        resident.fake.state.opened = true;
        resident.fake.state.tab = 1;
        resident.snapshot();
        resident.intent("active", json!({}));
        let mut reloads = 0;
        while start.elapsed() < Duration::from_millis(2300) {
            let event = resident
                .output
                .recv_timeout(Duration::from_millis(2400).saturating_sub(start.elapsed()))
                .unwrap();
            if event["type"] == "model" && event["model"]["output"]["loading"] == true {
                reloads += 1;
            }
            resident.step(event);
            if reloads >= 2 {
                break;
            }
        }
        assert_eq!(
            reloads, 2,
            "opening refreshes immediately, then polls at two seconds"
        );
        resident.fake.state.opened = false;
        resident.snapshot();
        resident.quiet(2100);
        assert_eq!(resident.model["output"]["loading"], false);
        assert!(resident.fake.actions.is_empty());
        resident.close();
    }

    #[test]
    fn malformed_resident_intents_report_errors_without_starting_native_actions() {
        let mut resident = Resident::new(Fake::new(simple(AudioKind::Output)));
        resident.ready();
        resident.intent("volume", json!({"kind":"output","key":"stale","value":0.5}));
        resident.until(|r| {
            r.model["output"]["error"]
                .as_str()
                .is_some_and(|error| error.contains("changed"))
        });
        resident.intent(
            "volume",
            json!({"kind":"output","key":"stale","value":0.5,"answer":"unexpected"}),
        );
        resident.until(|r| {
            r.model["output"]["error"]
                .as_str()
                .is_some_and(|error| error.contains("fields"))
        });
        assert!(resident.fake.actions.is_empty());
        resident.close();
    }
}

mod protocol_and_cli {
    use super::fixtures::*;
    use crate::audio::{Action, Backend};
    use serde_json::{Value, json};
    use std::{
        fs,
        io::{BufRead, BufReader, Write},
        os::unix::fs::PermissionsExt,
        path::PathBuf,
        process::{Command, Output, Stdio},
        sync::atomic::{AtomicU64, Ordering},
    };

    fn command(args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_audio-control"));
        command
            .args(args)
            .env_clear()
            .env("DBUS_SYSTEM_BUS_ADDRESS", "disabled:")
            .env("AUDIO_CONTROL_PW_DUMP", "/nonexistent/pw-dump");
        command
    }
    fn run(args: &[&str]) -> Output {
        command(args).output().unwrap()
    }
    fn request(snapshot: crate::model::Snapshot, key: Option<String>) -> Value {
        json!({"command":if key.is_some(){"select"}else{"rows"},"kind":"output","key":key,"snapshot":snapshot})
    }
    fn policy(request: Value) -> Value {
        let mut child = command(&["policy"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        writeln!(child.stdin.take().unwrap(), "{request}").unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success(), "{output:?}");
        assert!(output.stderr.is_empty(), "{output:?}");
        serde_json::from_slice(&output.stdout).unwrap()
    }
    pub struct DumpFixture {
        directory: PathBuf,
        pub executable: PathBuf,
    }
    impl DumpFixture {
        pub fn new(dump: &Value) -> Self {
            static SERIAL: AtomicU64 = AtomicU64::new(0);
            let directory = std::env::temp_dir().join(format!(
                "audio-policy-{}-{}",
                std::process::id(),
                SERIAL.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&directory).unwrap();
            let executable = directory.join("pw-dump");
            let data = directory.join("dump.json");
            let path = data.to_str().unwrap().replace('\'', "'\\''");
            // Stream large fixtures directly: Bash's line reader can spend the
            // entire dump timeout assembling the oversized-output test's line.
            // Resolve cat before command() clears the subprocess environment.
            let cat = std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
                .map(|directory| directory.join("cat"))
                .find(|path| path.is_file())
                .expect("the test environment must provide coreutils cat");
            let cat = cat.to_str().unwrap().replace('\'', "'\\''");
            fs::write(
                &executable,
                format!(
                    "#!/bin/sh\n[ \"$#\" -eq 1 ] && [ \"$1\" = -N ] || exit 8\nexec '{cat}' '{path}'\n"
                ),
            )
            .unwrap();
            fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
            let fixture = Self {
                directory,
                executable,
            };
            fixture.update(dump);
            fixture
        }
        pub fn update(&self, dump: &Value) {
            fs::write(self.directory.join("dump.next"), format!("{dump}\n")).unwrap();
            fs::rename(
                self.directory.join("dump.next"),
                self.directory.join("dump.json"),
            )
            .unwrap();
        }
    }
    impl Drop for DumpFixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.directory);
        }
    }

    #[test]
    fn help_succeeds_without_arguments_and_lists_only_current_commands() {
        for args in [vec![], vec!["--help"], vec!["-h"], vec!["help"]] {
            let output = run(&args);
            assert!(output.status.success(), "{args:?}: {output:?}");
            assert!(output.stderr.is_empty(), "{args:?}: {output:?}");
            let help = String::from_utf8(output.stdout).unwrap();
            assert!(help.contains("Usage: audio-control"));
            for name in [
                "panel",
                "policy",
                "bluetooth-pair",
                "battery-provider",
                "JSON",
            ] {
                assert!(help.contains(name), "{name}: {help}");
            }
            for name in [
                "status",
                "bluetooth-power",
                "bluetooth-devices",
                "bluetooth-scan",
                "set-default",
                "stream-move",
            ] {
                assert!(!help.contains(name), "{name}: {help}");
            }
        }
    }
    #[test]
    fn subcommand_help_never_reads_stdin_or_accesses_services() {
        for name in ["panel", "policy", "bluetooth-pair", "battery-provider"] {
            for args in [vec![name, "--help"], vec!["help", name]] {
                let output = run(&args);
                assert!(output.status.success(), "{args:?}: {output:?}");
                assert!(output.stderr.is_empty(), "{args:?}: {output:?}");
                let help = String::from_utf8(output.stdout).unwrap();
                assert!(
                    help.contains(&format!("Usage: audio-control {name}")),
                    "{help}"
                );
                if name == "bluetooth-pair" {
                    assert!(help.contains("<adapter> <address>"), "{help}");
                }
            }
        }
    }
    #[test]
    fn retired_commands_are_rejected_before_service_access() {
        for name in [
            "status",
            "bluetooth-power",
            "bluetooth-devices",
            "bluetooth-scan",
            "devices",
            "set-default",
            "stream-move",
        ] {
            let output = run(&[name]);
            assert_eq!(output.status.code(), Some(2));
            assert!(output.stdout.is_empty());
            assert!(
                String::from_utf8(output.stderr)
                    .unwrap()
                    .contains(&format!("unrecognized subcommand '{name}'"))
            );
        }
    }
    #[test]
    fn strict_cli_arity_and_pairing_identity_validation_precede_bus_access() {
        for args in [
            vec!["panel", "extra"],
            vec!["policy", "extra"],
            vec!["battery-provider", "extra"],
            vec!["bluetooth-pair"],
            vec!["bluetooth-pair", "hci0"],
            vec!["bluetooth-pair", "hci0", "AA:BB:CC:DD:EE:FF", "extra"],
            vec!["bluetooth-pair", "default", "AA:BB:CC:DD:EE:FF"],
            vec!["bluetooth-pair", "hci0", "Keyboard"],
            vec!["bluetooth-pair", "", "AA:BB:CC:DD:EE:FF"],
            vec!["bluetooth-pair", "hci0", ""],
            vec!["--unknown"],
            vec!["policy", "--unknown"],
            vec!["help", "retired"],
        ] {
            let output = run(&args);
            assert_eq!(output.status.code(), Some(2), "{args:?}: {output:?}");
            assert!(output.stdout.is_empty(), "{args:?}: {output:?}");
            let error = String::from_utf8(output.stderr).unwrap();
            assert!(!error.is_empty(), "{args:?}");
            assert!(!error.contains("D-Bus"), "{args:?}: {error}");
        }
    }
    #[test]
    fn offline_rows_skip_raw_query_and_emit_only_camel_case_supplemental_fields() {
        let result = policy(request(offline(), None));
        assert_eq!(result["type"], "result");
        assert_eq!(result["rows"].as_array().unwrap().len(), 1);
        let row = &result["rows"][0];
        assert_eq!(row["key"], "sink:7669727475616c");
        assert_eq!(row["nodeSerial"], "200");
        assert_eq!(row["cardId"], Value::Null);
        assert_eq!(row["portActive"], false);
        for key in ["default", "volume", "mute", "node_id"] {
            assert!(row.get(key).is_none());
        }
    }
    #[test]
    fn strict_request_schemas_are_enforced() {
        let mut value = request(offline(), None);
        value["command"] = "status".into();
        assert_eq!(policy(value)["ok"], false);
        let mut value = request(offline(), None);
        value["snapshot"]["volume"] = 1.into();
        assert_eq!(policy(value)["ok"], false);
        let mut value = request(offline(), None);
        value["kind"] = "playback".into();
        assert_eq!(policy(value)["ok"], false);
    }
    #[test]
    fn oversized_raw_dump_is_killed_and_reported_through_the_protocol() {
        let fixture = DumpFixture::new(&Value::String("x".repeat(8 * 1024 * 1024 + 1)));
        let mut child = command(&["policy"])
            .env("AUDIO_CONTROL_PW_DUMP", &fixture.executable)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        writeln!(child.stdin.take().unwrap(), "{}", request(snapshot(), None)).unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success());
        let result: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(
            result,
            json!({"type":"result", "ok":false, "error":"pw-dump output exceeds 8 MiB"}),
            "expected the dump byte-limit failure, got: {result}"
        );
    }
    #[test]
    fn fake_dump_rows_resolve_inactive_physical_choices_without_raw_node_inventory() {
        let fixture = DumpFixture::new(&dump());
        let mut child = command(&["policy"])
            .env("AUDIO_CONTROL_PW_DUMP", &fixture.executable)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        writeln!(child.stdin.take().unwrap(), "{}", request(snapshot(), None)).unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success());
        let result: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert!(
            result["rows"]
                .as_array()
                .unwrap()
                .iter()
                .any(|row| row["portName"] == HEADPHONES && row["nodeId"].is_null())
        );
    }
    #[test]
    fn mixed_alsa_bluez_rows_succeed_through_the_policy_protocol_for_both_kinds() {
        let (native, raw) = mixed_alsa_bluez();
        let fixture = DumpFixture::new(&raw);
        for (kind, id, alsa_id) in [("output", 30, 10), ("input", 31, 12)] {
            let mut request = request(native.clone(), None);
            request["kind"] = kind.into();
            let mut child = command(&["policy"])
                .env("AUDIO_CONTROL_PW_DUMP", &fixture.executable)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            writeln!(child.stdin.take().unwrap(), "{request}").unwrap();
            let output = child.wait_with_output().unwrap();
            assert!(output.status.success(), "{output:?}");
            assert!(output.stderr.is_empty(), "{output:?}");
            let result: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(result["type"], "result");
            assert!(result.get("error").is_none(), "{result}");
            let rows = result["rows"].as_array().unwrap();
            let row = rows.iter().find(|row| row["nodeId"] == id).unwrap();
            assert_eq!(row["cardId"], 8);
            assert_eq!(row["port"], false);
            assert!(row["routeId"].is_null());
            assert!(rows.iter().any(|row| row["nodeId"] == alsa_id));
        }
    }
    fn drive(mut fake: Fake) -> (Value, Vec<Action>) {
        let fixture = DumpFixture::new(&fake.dump);
        let mut child = command(&["policy"])
            .env("AUDIO_CONTROL_PW_DUMP", &fixture.executable)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut input = child.stdin.take().unwrap();
        let mut output = BufReader::new(child.stdout.take().unwrap());
        writeln!(
            input,
            "{}",
            request(
                fake.snapshot.clone(),
                Some(crate::audio::physical_key(CARD, HEADPHONES))
            )
        )
        .unwrap();
        let mut expected_id = 1;
        let result = loop {
            let mut line = String::new();
            assert!(output.read_line(&mut line).unwrap() > 0);
            let event: Value = serde_json::from_str(&line).unwrap();
            if event["type"] == "result" {
                break event;
            }
            assert_eq!(event["id"], expected_id);
            expected_id += 1;
            let reply = match event["type"].as_str().unwrap() {
                "snapshot" => {
                    let state = fake.observe().unwrap();
                    fixture.update(&fake.dump);
                    json!({"id":event["id"],"snapshot":state.snapshot})
                }
                "action" => {
                    let identity = |key: &str| serde_json::from_value(event[key].clone()).unwrap();
                    let action = match event["operation"].as_str().unwrap() {
                        "profile" => Action::Profile {
                            card: identity("card"),
                            profile: event["profile"].as_i64().unwrap() as i32,
                        },
                        "route" => Action::Route {
                            node: identity("node"),
                            route: event["route"].as_i64().unwrap() as i32,
                        },
                        "default" => Action::Default {
                            node: identity("node"),
                        },
                        other => panic!("unexpected action {other}"),
                    };
                    match fake.action(action) {
                        Ok(()) => {
                            fixture.update(&fake.dump);
                            json!({"id":event["id"],"ok":true})
                        }
                        Err(error) => {
                            json!({"id":event["id"],"ok":false,"error":error.to_string()})
                        }
                    }
                }
                other => panic!("unexpected protocol output {other}"),
            };
            writeln!(input, "{reply}").unwrap();
            input.flush().unwrap();
        };
        drop(input);
        let exit = child.wait_with_output().unwrap();
        assert!(exit.status.success());
        assert!(exit.stderr.is_empty(), "{exit:?}");
        (result, fake.events)
    }
    #[test]
    fn real_binary_profile_transaction_uses_only_typed_native_rpcs() {
        let (result, actions) = drive(Fake {
            delay: 2,
            ..Default::default()
        });
        assert_eq!(result, json!({"type":"result","ok":true}));
        assert!(matches!(
            actions.as_slice(),
            [
                Action::Profile { profile: 1, .. },
                Action::Route { route: 6, .. },
                Action::Default { .. }
            ]
        ));
    }
    #[test]
    fn real_binary_rolls_back_via_native_rpcs_on_followup_failure() {
        let (result, actions) = drive(Fake {
            reject_default: true,
            ..Default::default()
        });
        assert_eq!(result["ok"], false);
        assert!(
            result["error"]
                .as_str()
                .unwrap()
                .contains("Previous profile restored")
        );
        assert!(matches!(
            actions[actions.len() - 2],
            Action::Profile { profile: 2, .. }
        ));
        assert!(matches!(actions.last().unwrap(),Action::Default{node} if node.serial=="101"));
    }
    #[test]
    fn rpc_id_mismatch_fails_without_any_native_action() {
        let mut child = command(&["policy"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut input = child.stdin.take().unwrap();
        let mut output = BufReader::new(child.stdout.take().unwrap());
        writeln!(
            input,
            "{}",
            request(offline(), Some("sink:7669727475616c".into()))
        )
        .unwrap();
        let mut line = String::new();
        output.read_line(&mut line).unwrap();
        let event: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(event["type"], "snapshot");
        writeln!(input, "{}", json!({"id":999,"snapshot":offline()})).unwrap();
        input.flush().unwrap();
        line.clear();
        output.read_line(&mut line).unwrap();
        let result: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(result["type"], "result");
        assert_eq!(result["ok"], false);
        assert!(result["error"].as_str().unwrap().contains("wrong RPC id"));
        drop(input);
        assert!(child.wait().unwrap().success());
    }
    #[test]
    fn stale_raw_identity_produces_typed_error_and_no_actions() {
        let mut raw = dump();
        raw[0]["info"]["props"]["object.serial"] = 999.into();
        let fixture = DumpFixture::new(&raw);
        let mut child = command(&["policy"])
            .env("AUDIO_CONTROL_PW_DUMP", &fixture.executable)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        writeln!(
            child.stdin.take().unwrap(),
            "{}",
            request(
                snapshot(),
                Some(crate::audio::physical_key(CARD, HEADPHONES))
            )
        )
        .unwrap();
        let result: Value =
            serde_json::from_slice(&child.wait_with_output().unwrap().stdout).unwrap();
        assert_eq!(result["ok"], false);
        assert!(result["error"].as_str().unwrap().contains("stale"));
    }
}
