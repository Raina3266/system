//! All package tests: pure policy, request-local agents, batteries, and real CLI.

use audio_control::AppResult;

// Compile production modules here so missing-capability helpers stay private.
#[path = "audio.rs"]
pub mod audio;
#[path = "bluetooth.rs"]
pub mod bluetooth;
#[path = "devices.rs"]
pub mod devices;
#[path = "model.rs"]
pub mod model;
#[path = "policy.rs"]
pub mod policy;

mod fixtures {
    use crate::{
        AppResult,
        audio::{Action, Backend, State},
        devices::Metadata,
        model::{AudioKind, Snapshot},
    };
    use serde_json::{Value, json};

    pub const CARD: &str = "alsa_card.test";
    pub const SPEAKER: &str = "[Out] Speaker";
    pub const HEADPHONES: &str = "[Out] Headphones";

    pub fn node(id: u32, name: &str, kind: &str, device: Option<u32>) -> Value {
        json!({"id": id, "serial": (id * 10).to_string(), "name": name,
            "description": "Alder Lake PCH-P High Definition Audio Controller Analog Stereo",
            "kind": kind, "cardId": device.map(|_| 7), "cardSerial": if device.is_some() {"70"} else {""},
            "profileDevice": device, "routes": [], "routeName": "wrong-card-wide-route"})
    }

    pub fn native_routes(kind: &str) -> Value {
        let dump = dump();
        json!(dump[0]["info"]["params"]["EnumRoute"].as_array().unwrap().iter()
            .filter(|route| route["direction"] == kind).map(|route| json!({
                "index": route["index"], "name": route["name"], "description": route["description"],
                "available": route["available"] != "no"
            })).collect::<Vec<_>>())
    }

    pub fn snapshot() -> Snapshot {
        let mut speaker = node(10, "speaker-sink", "output", Some(3));
        speaker["routes"] = native_routes("Output");
        let mut mic = node(12, "microphone", "input", Some(5));
        mic["routes"] = native_routes("Input");
        let mut virtual_node = node(20, "virtual", "output", None);
        virtual_node["description"] = "Virtual Output".into();
        serde_json::from_value(json!({
            "cards": [{"id":7,"serial":"70","name":CARD,"activeProfile":2,
                "profiles":[{"index":0},{"index":1},{"index":2},{"index":3}]}],
            "nodes": [speaker, mic, virtual_node],
            "defaultOutput":{"id":10,"serial":"100"}, "defaultInput":{"id":12,"serial":"120"}
        }))
        .unwrap()
    }

    pub fn dump() -> Value {
        let profile = |index, name, sinks, sources, priority| {
            json!({
                "index": index, "name":name,"description":"IDENTICAL DESCRIPTION, NEVER AN IDENTITY",
                "available":"yes","priority":priority,
                "classes":[2,["Audio/Source",2,"card.profile.devices",sources],
                    ["Audio/Sink",4,"card.profile.devices",sinks]]
            })
        };
        let route = |index, name, label, direction, profiles, devices, available| {
            json!({
                "index":index,"name":name,"description":label,"direction":direction,
                "profiles":profiles,"devices":devices,"available":available
            })
        };
        json!([{"id":7,"type":"PipeWire:Interface:Device","info":{
            "props":{"object.serial":70,"device.name":CARD,"device.description":"DO NOT USE RAW INVENTORY"},
            "params":{
                "EnumProfile":[
                    {"index":0,"name":"off","priority":0,"available":"yes","classes":[0]},
                    profile(1,"headphones",json!([0,1,2,6]),json!([4,5]),10300),
                    profile(2,"speakers",json!([0,1,2,3]),json!([4,5]),10200),
                    {"index":3,"name":"pro-audio","priority":1,"available":"unknown",
                        "classes":[2,["Audio/Source",1,"card.profile.devices",[11]],
                            ["Audio/Sink",1,"card.profile.devices",[10]]]}
                ],
                "EnumRoute":[
                    route(0,"[Out] HDMI3","HDMI / DisplayPort 3","Output",json!([2,1]),json!([0]),"no"),
                    route(1,"[Out] HDMI2","HDMI / DisplayPort 2","Output",json!([2,1]),json!([1]),"yes"),
                    route(2,"[Out] HDMI1","HDMI / DisplayPort 1","Output",json!([2,1]),json!([2]),"yes"),
                    route(3,SPEAKER,"Speaker","Output",json!([2]),json!([3]),"unknown"),
                    route(4,"[In] Mic2","Microphone jack","Input",json!([2,1]),json!([4]),"no"),
                    route(5,"[In] Mic1","Internal microphone","Input",json!([2,1]),json!([5]),"unknown"),
                    route(6,HEADPHONES,"Headphones","Output",json!([1]),json!([6]),"yes")
                ],
                "Route":[{"index":3,"device":3,"profile":2},{"index":5,"device":5,"profile":2}]
            }
        }}])
    }

    pub fn state(snapshot: Snapshot, dump: &Value) -> State {
        snapshot.validate().unwrap();
        let metadata = Metadata::from_dump(&snapshot, AudioKind::Output, dump).unwrap();
        State { snapshot, metadata }
    }

    pub fn initial() -> State {
        state(snapshot(), &dump())
    }

    pub fn offline() -> Snapshot {
        serde_json::from_value(json!({"cards":[],"nodes":[{
            "id":20,"serial":"200","name":"virtual","description":"Virtual Output",
            "kind":"output","cardId":null,"cardSerial":"","profileDevice":null,"routes":[],"routeName":""
        }],"defaultOutput":null,"defaultInput":null})).unwrap()
    }

    pub struct Fake {
        pub snapshot: Snapshot,
        pub dump: Value,
        pub events: Vec<Action>,
        pub delay: usize,
        pub never_appears: bool,
        pub reject_profile: bool,
        pub reject_route: bool,
        pub reject_default: bool,
        pub ignore_route: bool,
        pub external_change: bool,
        pub reject_restore: bool,
    }

    impl Default for Fake {
        fn default() -> Self {
            Self {
                snapshot: snapshot(),
                dump: dump(),
                events: Vec::new(),
                delay: 0,
                never_appears: false,
                reject_profile: false,
                reject_route: false,
                reject_default: false,
                ignore_route: false,
                external_change: false,
                reject_restore: false,
            }
        }
    }

    impl Backend for Fake {
        fn observe(&mut self) -> AppResult<State> {
            let profile = self.snapshot.cards.first().map(|card| card.active_profile);
            if profile == Some(1) && self.external_change {
                self.snapshot.cards[0].active_profile = 3;
            } else if let Some(profile @ (1 | 2)) = profile {
                let name = if profile == 1 {
                    "headphone-sink"
                } else {
                    "speaker-sink"
                };
                if !self.snapshot.nodes.iter().any(|node| node.name == name) {
                    if self.delay > 0 {
                        self.delay -= 1;
                    } else if profile == 2 || !self.never_appears {
                        let mut value = node(
                            if profile == 1 { 11 } else { 10 },
                            name,
                            "output",
                            Some(if profile == 1 { 6 } else { 3 }),
                        );
                        value["routes"] = native_routes("Output");
                        if profile == 2 {
                            value["serial"] = "101".into();
                        }
                        self.snapshot.nodes.push(serde_json::from_value(value)?);
                    }
                }
            }
            Ok(state(self.snapshot.clone(), &self.dump))
        }

        fn action(&mut self, action: Action) -> AppResult<()> {
            self.events.push(action.clone());
            match action {
                Action::Profile { card, profile } => {
                    assert_eq!(card, self.snapshot.cards[0].identity());
                    if (profile == 1 && self.reject_profile)
                        || (profile == 2 && self.reject_restore)
                    {
                        return Err("Profile rejected".into());
                    }
                    self.snapshot.cards[0].active_profile = profile;
                    self.snapshot.nodes.retain(|node| {
                        node.name != "speaker-sink" && node.name != "headphone-sink"
                    });
                    if self
                        .snapshot
                        .default_output
                        .as_ref()
                        .is_some_and(|identity| self.snapshot.node(identity).is_err())
                    {
                        self.snapshot.default_output = None;
                    }
                }
                Action::Route { node, route } => {
                    if self.reject_route {
                        return Err("Route rejected".into());
                    }
                    let endpoint = self.snapshot.node(&node)?.clone();
                    if !self.ignore_route {
                        let profile = self.snapshot.cards[0].active_profile;
                        let routes = self.dump[0]["info"]["params"]["Route"]
                            .as_array_mut()
                            .unwrap();
                        routes.retain(|route| {
                            route["device"].as_u64() != endpoint.profile_device.map(u64::from)
                        });
                        routes.push(json!({"index":route,"device":endpoint.profile_device,"profile":profile}));
                    }
                }
                Action::Default { node } => {
                    let endpoint = self.snapshot.node(&node)?;
                    if self.reject_default && endpoint.name == "headphone-sink" {
                        return Err("Default rejected".into());
                    }
                    match endpoint.kind {
                        AudioKind::Output => self.snapshot.default_output = Some(node),
                        AudioKind::Input => self.snapshot.default_input = Some(node),
                    }
                }
            }
            Ok(())
        }
        fn pause(&mut self) {}
    }
}

mod labels {
    use crate::model::*;
    #[test]
    fn identifiers_encode_arbitrary_utf8_without_shell_syntax() {
        assert_eq!(hex_encode("Raina's 🎧"), "5261696e61277320f09f8ea7");
    }
    #[test]
    fn compact_labels_strip_only_known_boilerplate() {
        for (name, expected) in [
            ("Built-in Audio Analog Stereo", "Built-in Audio"),
            (
                "GA104 High Definition Audio Controller Digital Stereo (HDMI 2)",
                "GA104 (HDMI 2)",
            ),
            ("Jabra Evolve 65 Analog Stereo", "Jabra Evolve 65"),
            ("WH-1000XM4", "WH-1000XM4"),
            ("Scarlett 2i2 USB", "Scarlett 2i2 USB"),
            ("Beach House Speaker", "Beach House Speaker"),
            ("Family Room Sonos", "Family Room Sonos"),
        ] {
            assert_eq!(short_device_name(name, None), expected);
        }
        assert_eq!(
            short_device_name(
                "Family 17h/19h/20h HD Audio Controller Analog Stereo",
                Some("Speakers")
            ),
            "Speakers"
        );
        assert_eq!(
            short_device_name("Family 17h/19h/20h HD Audio Controller", None),
            "Family 17h/19h/20h HD Audio Controller"
        );
        assert!(is_pci_family("17h/19h/20h"));
        assert!(!is_pci_family("Beach"));
    }
    #[test]
    fn labels_flatten_unicode_separators_and_clip_by_characters() {
        assert_eq!(
            single_line("one\r\ntwo\u{0085}three\u{2028}four\u{2029}five", 40),
            "one two three four five"
        );
        assert_eq!(single_line(&"a".repeat(60), 10), "aaaaaaaaa…");
        assert_eq!(truncate("Raina's Bluetooth Headphones", 12), "Raina's Blu…");
        assert_eq!(single_line("  spaced  ", 40), "spaced");
    }
}

mod metadata {
    use super::fixtures::*;
    use crate::{devices::Metadata, model::AudioKind};
    use serde_json::json;
    #[test]
    fn parses_actual_spa_classes_and_native_indexes_not_descriptions_or_ordinals() {
        let state = initial();
        let data = state.metadata.cards.get(&7).unwrap();
        assert_eq!(data.profiles[1].index, 1);
        assert_eq!(data.profiles[1].sources, [4, 5]);
        assert_eq!(data.profiles[1].sinks, [0, 1, 2, 6]);
        assert_eq!(data.routes[6].profiles, [1]);
        assert_eq!(data.routes[6].devices, [6]);
    }
    #[test]
    fn raw_card_identity_must_match_all_three_canonical_fields() {
        for (field, value) in [
            ("object.serial", json!(71)),
            ("device.name", json!("alsa_card.other")),
        ] {
            let mut raw = dump();
            raw[0]["info"]["props"][field] = value;
            assert!(
                Metadata::from_dump(&snapshot(), AudioKind::Output, &raw)
                    .unwrap_err()
                    .to_string()
                    .contains("stale")
            );
        }
        let mut raw = dump();
        raw[0]["id"] = 8.into();
        assert!(Metadata::from_dump(&snapshot(), AudioKind::Output, &raw).is_err());
        let mut raw = dump();
        raw[0]["info"]["props"]["object.serial"] = "70".into();
        assert!(Metadata::from_dump(&snapshot(), AudioKind::Output, &raw).is_ok());
    }
    #[test]
    fn ambiguous_or_malformed_associations_are_rejected() {
        let mut raw = dump();
        let duplicate = raw[0].clone();
        raw.as_array_mut().unwrap().push(duplicate);
        assert!(Metadata::from_dump(&snapshot(), AudioKind::Output, &raw).is_err());
        let mut raw = dump();
        raw[0]["info"]["params"]["EnumProfile"][1]["classes"][0] = 9.into();
        assert!(Metadata::from_dump(&snapshot(), AudioKind::Output, &raw).is_err());
        let mut raw = dump();
        raw[0]["info"]["params"]["Route"]
            .as_array_mut()
            .unwrap()
            .push(json!({"index":6,"device":3,"profile":2}));
        assert!(Metadata::from_dump(&snapshot(), AudioKind::Output, &raw).is_err());
    }
    #[test]
    fn raw_nodes_and_profile_state_do_not_supply_native_inventory() {
        let mut raw = dump();
        raw.as_array_mut().unwrap().push(json!({"id":99,"type":"PipeWire:Interface:Node","info":{"props":{"node.name":"ghost"}}}));
        raw[0]["info"]["params"]["Profile"] = json!([{"index":1}]);
        let state = state(snapshot(), &raw);
        let rows = crate::audio::rows(&state, AudioKind::Output).unwrap();
        assert!(
            !rows
                .iter()
                .any(|row| row.name == "ghost" || row.description.contains("RAW INVENTORY"))
        );
        assert!(
            rows.iter()
                .find(|row| row.port_name == SPEAKER)
                .unwrap()
                .node_id
                .is_some()
        );
        assert!(
            rows.iter()
                .find(|row| row.port_name == HEADPHONES)
                .unwrap()
                .node_id
                .is_none()
        );
    }
    #[test]
    fn canonical_snapshot_rejects_duplicate_or_stale_native_identities() {
        let mut snapshot = snapshot();
        snapshot.cards[0].serial = "71".into();
        assert!(snapshot.validate().is_err());
        let mut snapshot = super::fixtures::snapshot();
        snapshot.nodes.push(snapshot.nodes[0].clone());
        assert!(snapshot.validate().is_err());
    }
}

mod rows {
    use super::fixtures::*;
    use crate::{
        audio::{self, physical_key},
        model::{AudioKind, single_line},
    };
    use serde_json::json;
    use std::collections::HashMap;
    #[test]
    fn physical_keys_survive_profile_and_endpoint_replacement() {
        let before = audio::rows(&initial(), AudioKind::Output).unwrap();
        let mut fake = Fake::default();
        let card = fake.snapshot.cards[0].identity();
        crate::audio::Backend::action(&mut fake, audio::Action::Profile { card, profile: 1 })
            .unwrap();
        let after = audio::rows(
            &crate::audio::Backend::observe(&mut fake).unwrap(),
            AudioKind::Output,
        )
        .unwrap();
        for port in [SPEAKER, HEADPHONES] {
            let before = before.iter().find(|row| row.port_name == port).unwrap();
            let after = after.iter().find(|row| row.port_name == port).unwrap();
            assert_eq!(before.key, after.key);
            assert_eq!(before.key, physical_key(CARD, port));
            assert_ne!(before.node_id.is_some(), after.node_id.is_some());
        }
    }
    #[test]
    fn active_port_uses_endpoint_device_and_profile_not_card_wide_route_name() {
        let mut snapshot = snapshot();
        let mut hdmi = node(30, "hdmi", "output", Some(1));
        hdmi["routes"] = native_routes("Output");
        snapshot.nodes.push(serde_json::from_value(hdmi).unwrap());
        let mut raw = dump();
        raw[0]["info"]["params"]["Route"]
            .as_array_mut()
            .unwrap()
            .push(json!({"index":1,"device":1,"profile":2}));
        let rows = audio::rows(&state(snapshot, &raw), AudioKind::Output).unwrap();
        assert!(
            rows.iter()
                .find(|row| row.port_name == SPEAKER)
                .unwrap()
                .port_active
        );
        assert!(
            rows.iter()
                .find(|row| row.port_name == "[Out] HDMI2")
                .unwrap()
                .port_active
        );
        assert!(
            !rows
                .iter()
                .find(|row| row.port_name == HEADPHONES)
                .unwrap()
                .port_active
        );
    }
    #[test]
    fn inactive_rows_and_live_rows_bind_canonical_identities_without_native_state_fields() {
        let rows = audio::rows(&initial(), AudioKind::Output).unwrap();
        let inactive = rows.iter().find(|row| row.port_name == HEADPHONES).unwrap();
        assert_eq!(inactive.node_id, None);
        assert!(inactive.name.is_empty());
        assert_eq!(inactive.card_id, Some(7));
        assert_eq!(inactive.card_serial, "70");
        let live = rows.iter().find(|row| row.port_name == SPEAKER).unwrap();
        assert_eq!(live.node_id, Some(10));
        assert_eq!(live.node_serial, "100");
        for row in rows {
            let json = serde_json::to_value(row).unwrap();
            for field in ["default", "volume", "muted", "mute"] {
                assert!(json.get(field).is_none());
            }
        }
    }
    #[test]
    fn sources_are_only_existing_native_sources_and_unavailable_ports_are_hidden() {
        let rows = audio::rows(&initial(), AudioKind::Input).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].label, "Internal microphone");
        assert_eq!(rows[0].node_id, Some(12));
        assert!(rows[0].port_active);
        let outputs = audio::rows(&initial(), AudioKind::Output).unwrap();
        assert!(!outputs.iter().any(|row| row.port_name == "[Out] HDMI3"));
        let mut raw = dump();
        raw[0]["info"]["params"]["EnumProfile"][1]["available"] = "no".into();
        assert!(
            !audio::rows(&state(snapshot(), &raw), AudioKind::Output)
                .unwrap()
                .iter()
                .any(|row| row.port_name == HEADPHONES)
        );
    }
    #[test]
    fn inactive_labels_prefer_native_route_descriptions_where_exposed() {
        let mut raw = dump();
        raw[0]["info"]["params"]["EnumRoute"][6]["description"] = "stale raw label".into();
        let rows = audio::rows(&state(snapshot(), &raw), AudioKind::Output).unwrap();
        assert_eq!(
            rows.iter()
                .find(|row| row.port_name == HEADPHONES)
                .unwrap()
                .label,
            "Headphones"
        );
    }
    #[test]
    fn native_unavailable_routes_are_not_resurrected_as_inactive_rows() {
        let mut native = snapshot();
        for node in &mut native.nodes {
            for route in &mut node.routes {
                if route.name == SPEAKER || route.name == HEADPHONES {
                    route.available = false;
                }
            }
        }
        let rows = audio::rows(&state(native, &dump()), AudioKind::Output).unwrap();
        assert!(
            !rows
                .iter()
                .any(|row| row.port_name == SPEAKER || row.port_name == HEADPHONES)
        );
    }
    #[test]
    fn missing_supplemental_routes_yield_native_device_only_choices() {
        let mut raw = dump();
        raw[0]["info"]["params"]["EnumRoute"] = json!([]);
        let rows = audio::rows(&state(snapshot(), &raw), AudioKind::Output).unwrap();
        let row = rows.iter().find(|row| row.name == "speaker-sink").unwrap();
        assert_eq!(row.key, "sink:737065616b65722d73696e6b");
        assert!(!row.port);
        assert_eq!(row.route_id, None);
        let snapshot = offline();
        let state = audio::State::load(snapshot, AudioKind::Output).unwrap();
        assert_eq!(
            audio::rows(&state, AudioKind::Output).unwrap()[0].label,
            "Virtual Output"
        );
    }
    #[test]
    fn identical_node_names_and_ambiguous_ports_never_merge_live_targets() {
        let mut snapshot = snapshot();
        let mut duplicate = snapshot.nodes[0].clone();
        duplicate.id = 31;
        duplicate.serial = "310".into();
        snapshot.nodes.push(duplicate);
        let rows = audio::rows(&state(snapshot, &dump()), AudioKind::Output).unwrap();
        let speakers: Vec<_> = rows.iter().filter(|row| row.port_name == SPEAKER).collect();
        assert_eq!(speakers.len(), 2);
        assert_ne!(speakers[0].key, speakers[1].key);
        assert!(speakers.iter().all(|row| row.key.starts_with("sink:")));
        assert_ne!(
            physical_key(CARD, SPEAKER),
            physical_key("alsa_card.other", SPEAKER)
        );
    }
    #[test]
    fn duplicate_labels_get_hardware_context_then_short_distinguishing_numbers() {
        let mut rows = audio::rows(&initial(), AudioKind::Output).unwrap();
        rows.retain(|row| row.port_name == SPEAKER);
        let mut other = rows[0].clone();
        other.card = "alsa_card.usb".into();
        other.key = "other".into();
        rows.push(other);
        audio::clarify_selection_labels(
            &mut rows,
            &HashMap::from([
                (CARD.into(), "Built-in Audio".into()),
                ("alsa_card.usb".into(), "USB Audio".into()),
            ]),
        );
        assert_eq!(rows[0].label, "Speaker — Built-in Audio");
        assert_eq!(rows[1].label, "Speaker — USB Audio");
        for row in &mut rows {
            row.label = "Long identical model name ".repeat(10);
        }
        audio::clarify_selection_labels(&mut rows, &HashMap::new());
        assert_ne!(
            single_line(&rows[0].label, 48),
            single_line(&rows[1].label, 48)
        );
        assert!(rows.iter().all(|row| row.label.starts_with('#')));
    }
}

mod transactions {
    use super::fixtures::*;
    use crate::{
        audio::{self, Action},
        model::{AudioKind, Identity},
    };
    use serde_json::json;
    fn activate(fake: &mut Fake) -> crate::AppResult<()> {
        let initial = state(fake.snapshot.clone(), &fake.dump);
        audio::select(
            fake,
            &initial,
            AudioKind::Output,
            &audio::physical_key(CARD, HEADPHONES),
        )
    }
    #[test]
    fn profile_retention_preserves_microphone_nodes_and_ports_not_just_counts() {
        let state = initial();
        let card = &state.snapshot.cards[0];
        let route = &state.metadata.cards[&7].routes[6];
        assert_eq!(audio::profile_for(&state, card, route).unwrap().index, 1);
        for mode in 0..3 {
            let mut state = initial();
            match mode {
                0 => state.metadata.cards.get_mut(&7).unwrap().profiles[1].sources = vec![5],
                1 => state.metadata.cards.get_mut(&7).unwrap().routes[4].profiles = vec![2],
                _ => state.metadata.cards.get_mut(&7).unwrap().profiles[1].sources = vec![40, 50],
            }
            assert!(
                audio::profile_for(
                    &state,
                    &state.snapshot.cards[0],
                    &state.metadata.cards[&7].routes[6]
                )
                .is_none()
            );
        }
    }
    #[test]
    fn native_addressability_and_alsa_only_policy_are_mandatory() {
        let mut state = initial();
        state.snapshot.cards[0]
            .profiles
            .retain(|profile| profile.index != 1);
        assert!(
            audio::profile_for(
                &state,
                &state.snapshot.cards[0],
                &state.metadata.cards[&7].routes[6]
            )
            .is_none()
        );
        let mut state = initial();
        state.snapshot.cards[0].name = "bluez_card.headset".into();
        assert!(
            audio::profile_for(
                &state,
                &state.snapshot.cards[0],
                &state.metadata.cards[&7].routes[6]
            )
            .is_none()
        );
    }
    #[test]
    fn retention_of_hdmi_ports_beats_higher_profile_priority() {
        let mut state = initial();
        let data = state.metadata.cards.get_mut(&7).unwrap();
        let mut minimal = data.profiles[1].clone();
        minimal.index = 4;
        minimal.name = "minimal".into();
        minimal.sinks = vec![6];
        minimal.priority = 999999;
        data.profiles.push(minimal);
        for port in &mut data.routes {
            if port.kind == AudioKind::Input || port.name == HEADPHONES {
                port.profiles.push(4);
            }
        }
        state.snapshot.cards[0]
            .profiles
            .push(crate::model::NativeProfile { index: 4 });
        assert_eq!(
            audio::profile_for(
                &state,
                &state.snapshot.cards[0],
                &state.metadata.cards[&7].routes[6]
            )
            .unwrap()
            .index,
            1
        );
    }
    #[test]
    fn delayed_endpoint_is_re_resolved_then_route_is_verified_before_default() {
        let mut fake = Fake {
            delay: 3,
            ..Default::default()
        };
        activate(&mut fake).unwrap();
        assert_eq!(
            fake.events,
            [
                Action::Profile {
                    card: Identity {
                        id: 7,
                        serial: "70".into()
                    },
                    profile: 1
                },
                Action::Route {
                    node: Identity {
                        id: 11,
                        serial: "110".into()
                    },
                    route: 6
                },
                Action::Default {
                    node: Identity {
                        id: 11,
                        serial: "110".into()
                    }
                },
            ]
        );
        assert_eq!(fake.snapshot.default_output.as_ref().unwrap().id, 11);
    }
    #[test]
    fn live_port_changes_do_not_trigger_profile_changes() {
        let mut fake = Fake::default();
        fake.dump[0]["info"]["params"]["Route"] = json!([]);
        let initial = state(fake.snapshot.clone(), &fake.dump);
        audio::select(
            &mut fake,
            &initial,
            AudioKind::Output,
            &audio::physical_key(CARD, SPEAKER),
        )
        .unwrap();
        assert!(matches!(fake.events[0], Action::Route { route: 3, .. }));
        assert!(matches!(fake.events[1], Action::Default { .. }));
        assert_eq!(fake.events.len(), 2);
    }
    #[test]
    fn native_route_index_zero_is_a_real_action_target() {
        let mut fake = Fake::default();
        fake.snapshot.nodes[0].profile_device = Some(0);
        fake.snapshot.nodes[0].routes[0].available = true;
        fake.dump[0]["info"]["params"]["EnumRoute"][0]["available"] = "yes".into();
        fake.dump[0]["info"]["params"]["Route"] = json!([]);
        let initial = state(fake.snapshot.clone(), &fake.dump);
        audio::select(
            &mut fake,
            &initial,
            AudioKind::Output,
            &audio::physical_key(CARD, "[Out] HDMI3"),
        )
        .unwrap();
        assert!(matches!(fake.events[0], Action::Route { route: 0, .. }));
        assert!(
            fake.events
                .iter()
                .all(|action| !matches!(action, Action::Profile { .. }))
        );
    }
    #[test]
    fn unplugged_removed_or_replaced_targets_never_issue_actions() {
        let mut fake = Fake::default();
        fake.dump[0]["info"]["params"]["EnumRoute"][6]["available"] = "no".into();
        for node in &mut fake.snapshot.nodes {
            for route in &mut node.routes {
                if route.name == HEADPHONES {
                    route.available = false;
                }
            }
        }
        assert!(activate(&mut fake).is_err());
        assert!(fake.events.is_empty());
        let initial = initial();
        let mut fake = Fake::default();
        fake.snapshot.nodes[0].serial = "999".into();
        fake.snapshot.default_output = Some(fake.snapshot.nodes[0].identity());
        assert!(
            audio::select(
                &mut fake,
                &initial,
                AudioKind::Output,
                &audio::physical_key(CARD, SPEAKER)
            )
            .is_err()
        );
        assert!(fake.events.is_empty());
        let mut fake = Fake::default();
        fake.snapshot.cards[0].serial = "71".into();
        for node in &mut fake.snapshot.nodes {
            if node.card_id == Some(7) {
                node.card_serial = "71".into();
            }
        }
        fake.dump[0]["info"]["props"]["object.serial"] = 71.into();
        assert!(
            audio::select(
                &mut fake,
                &initial,
                AudioKind::Output,
                &audio::physical_key(CARD, HEADPHONES)
            )
            .is_err()
        );
        assert!(fake.events.is_empty());
    }
    #[test]
    fn rejected_profile_never_requests_routes_or_defaults() {
        let mut fake = Fake {
            reject_profile: true,
            ..Default::default()
        };
        assert!(
            activate(&mut fake)
                .unwrap_err()
                .to_string()
                .contains("Profile rejected")
        );
        assert_eq!(fake.events.len(), 1);
        assert_eq!(fake.snapshot.cards[0].active_profile, 2);
    }
    #[test]
    fn failed_or_unacknowledged_followup_restores_profile_and_fresh_default_identity() {
        for mode in 0..4 {
            let mut fake = Fake::default();
            match mode {
                0 => fake.never_appears = true,
                1 => fake.reject_route = true,
                2 => fake.reject_default = true,
                _ => fake.ignore_route = true,
            }
            let error = activate(&mut fake).unwrap_err().to_string();
            assert!(error.contains("Previous profile restored"), "{error}");
            assert_eq!(fake.snapshot.cards[0].active_profile, 2);
            assert_eq!(
                fake.snapshot.default_output,
                Some(Identity {
                    id: 10,
                    serial: "101".into()
                })
            );
            assert!(fake.events.ends_with(&[
                Action::Profile {
                    card: Identity {
                        id: 7,
                        serial: "70".into()
                    },
                    profile: 2
                },
                Action::Default {
                    node: Identity {
                        id: 10,
                        serial: "101".into()
                    }
                }
            ]));
            if mode == 3 {
                assert!(
                    !fake
                        .events
                        .iter()
                        .any(|action| matches!(action,Action::Default{node} if node.id==11))
                );
            }
        }
    }
    #[test]
    fn rollback_restores_virtual_default_without_inventing_a_card() {
        let mut fake = Fake {
            never_appears: true,
            ..Default::default()
        };
        fake.snapshot.default_output = Some(Identity {
            id: 20,
            serial: "200".into(),
        });
        assert!(
            activate(&mut fake)
                .unwrap_err()
                .to_string()
                .contains("Previous profile restored")
        );
        assert_eq!(
            fake.snapshot.default_output,
            Some(Identity {
                id: 20,
                serial: "200".into()
            })
        );
    }
    #[test]
    fn rollback_does_not_overwrite_external_profile_or_conceal_recovery_failure() {
        let mut fake = Fake {
            external_change: true,
            ..Default::default()
        };
        let error = activate(&mut fake).unwrap_err().to_string();
        assert!(error.contains("changed elsewhere"), "{error}");
        assert_eq!(fake.events.len(), 1);
        assert_eq!(fake.snapshot.cards[0].active_profile, 3);
        let mut fake = Fake {
            reject_route: true,
            reject_restore: true,
            ..Default::default()
        };
        let error = activate(&mut fake).unwrap_err().to_string();
        assert!(
            error.contains("Could not restore: Profile rejected"),
            "{error}"
        );
    }
    #[test]
    fn route_acknowledgement_never_accepts_replacement_unplug_or_another_device_profile() {
        struct Interleaved {
            inner: Fake,
            mode: u8,
            fired: bool,
        }
        impl crate::audio::Backend for Interleaved {
            fn observe(&mut self) -> crate::AppResult<crate::audio::State> {
                if !self.fired && matches!(self.inner.events.last(), Some(Action::Route { .. })) {
                    self.fired = true;
                    match self.mode {
                        0 => {
                            self.inner
                                .snapshot
                                .nodes
                                .iter_mut()
                                .find(|node| node.name == "headphone-sink")
                                .unwrap()
                                .serial = "999".into()
                        }
                        1 => {
                            for node in &mut self.inner.snapshot.nodes {
                                for route in &mut node.routes {
                                    if route.name == HEADPHONES {
                                        route.available = false;
                                    }
                                }
                            }
                        }
                        2 => self.inner.snapshot.cards[0].active_profile = 3,
                        _ => {
                            let routes = self.inner.dump[0]["info"]["params"]["Route"]
                                .as_array_mut()
                                .unwrap();
                            for route in routes.iter_mut().filter(|route| route["index"] == 6) {
                                route["device"] = 42.into();
                                route["profile"] = 0.into();
                            }
                        }
                    }
                }
                crate::audio::Backend::observe(&mut self.inner)
            }
            fn action(&mut self, action: Action) -> crate::AppResult<()> {
                crate::audio::Backend::action(&mut self.inner, action)
            }
            fn pause(&mut self) {}
        }
        for mode in 0..4 {
            let mut backend = Interleaved {
                inner: Fake::default(),
                mode,
                fired: false,
            };
            let error = audio::select(
                &mut backend,
                &initial(),
                AudioKind::Output,
                &audio::physical_key(CARD, HEADPHONES),
            )
            .unwrap_err()
            .to_string();
            assert!(backend.fired);
            assert!(
                !backend
                    .inner
                    .events
                    .iter()
                    .any(|action| matches!(action, Action::Default { node } if node.id == 11))
            );
            if mode == 2 {
                assert!(error.contains("changed elsewhere"), "{error}");
                assert_eq!(backend.inner.snapshot.cards[0].active_profile, 3);
                assert!(
                    !backend
                        .inner
                        .events
                        .iter()
                        .any(|action| matches!(action, Action::Profile { profile: 2, .. }))
                );
            } else {
                assert!(error.contains("Previous profile restored"), "{error}");
            }
        }
    }
    #[test]
    fn source_port_action_is_native_rpc_and_never_autoswitches_profiles() {
        let mut fake = Fake::default();
        fake.dump[0]["info"]["params"]["Route"] = json!([]);
        let initial = state(fake.snapshot.clone(), &fake.dump);
        let key = audio::rows(&initial, AudioKind::Input).unwrap()[0]
            .key
            .clone();
        audio::select(&mut fake, &initial, AudioKind::Input, &key).unwrap();
        assert!(matches!(fake.events[0], Action::Route { route: 5, .. }));
        assert!(
            fake.events
                .iter()
                .all(|action| !matches!(action, Action::Profile { .. }))
        );
    }
}

mod agents_and_batteries {
    use crate::{bluetooth::battery_provider::*, bluetooth::*, model::CodeKind};
    #[test]
    fn pairing_targets_are_exact_adapter_and_address_not_names_or_paths() {
        assert!(pairing_target("hci0", "AA:BB:CC:DD:EE:FF").is_ok());
        assert!(pairing_target("hci12", "aa:bb:cc:dd:ee:ff").is_ok());
        for adapter in [
            "",
            "hci",
            "default",
            "../hci0",
            "/org/bluez/hci0",
            "hci0/other",
        ] {
            assert!(pairing_target(adapter, "AA:BB:CC:DD:EE:FF").is_err());
        }
        for address in [
            "Keyboard",
            "AABBCCDDEEFF",
            "AA-BB-CC-DD-EE-FF",
            "AA:BB:CC:DD:EE:GG",
        ] {
            assert!(pairing_target("hci0", address).is_err());
        }
    }
    #[test]
    fn numeric_comparison_requires_explicit_yes() {
        for answer in ["yes", "YES", " Yes\n"] {
            assert!(validate_answer(CodeKind::Confirmation, answer).is_ok());
        }
        for answer in ["no", "NO", "", " ", "y", "true", "042311", "yes please"] {
            assert!(validate_answer(CodeKind::Confirmation, answer).is_err());
        }
    }
    #[test]
    fn comparison_prompts_preserve_six_digits_and_pin_answers_remain_text() {
        for (passkey, digits) in [(0, "000000"), (42311, "042311"), (999999, "999999")] {
            let request = confirmation_request("AA:BB:CC:DD:EE:FF".into(), passkey);
            assert_eq!(request.kind, CodeKind::Confirmation);
            assert_eq!(
                request.message,
                format!(
                    "Does passkey {digits} match the one shown on AA:BB:CC:DD:EE:FF? Type yes or no."
                )
            );
        }
        assert_eq!(validate_answer(CodeKind::Pin, " aB12\n").unwrap(), "aB12");
        assert_eq!(
            validate_answer(CodeKind::Passkey, "042311\n").unwrap(),
            "042311"
        );
        assert!(validate_answer(CodeKind::Pin, "").is_err());
    }
    #[test]
    fn bluetooth_addresses_normalize_but_receiver_serials_do_not() {
        for address in ["c8:1a:7b:31:09:df", "C8-1A-7B-31-09-DF"] {
            assert_eq!(normalize_address(address).as_deref(), Some("C81A7B3109DF"));
        }
        for serial in ["4516LGN8", ""] {
            assert_eq!(normalize_address(serial), None);
        }
    }
    #[test]
    fn battery_levels_and_provider_paths_are_preserved() {
        assert_eq!(level_percentage("Full\n"), Some(100));
        assert_eq!(level_percentage("critical"), Some(5));
        assert_eq!(level_percentage("unknown"), None);
        assert_eq!(
            battery_path("C81A7B3109DF").unwrap().as_str(),
            "/bt_battery/hidpp/C81A7B3109DF"
        );
    }
    #[test]
    fn only_logitech_hid_devices_are_preempted() {
        let hid = vec![HID_SERVICE_UUID.to_owned()];
        assert!(is_logitech_hid(Some("usb:v046DpB023d0015"), &hid));
        assert!(!is_logitech_hid(Some("usb:v046DpB040d0015"), &[]));
        assert!(!is_logitech_hid(Some("usb:v8087p0026d0015"), &hid));
        assert!(!is_logitech_hid(None, &hid));
    }
}

mod protocol_and_cli {
    use super::fixtures::*;
    use crate::audio::{Action, Backend};
    use serde_json::{Value, json};
    use std::{
        fs,
        io::{BufRead, BufReader, Cursor, Write},
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
    struct DumpFixture {
        directory: PathBuf,
        executable: PathBuf,
    }
    impl DumpFixture {
        fn new(dump: &Value) -> Self {
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
        fn update(&self, dump: &Value) {
            fs::write(self.directory.join("dump.json"), format!("{dump}\n")).unwrap();
        }
    }
    impl Drop for DumpFixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.directory);
        }
    }

    #[test]
    fn help_lists_only_policy_explicit_pairing_and_battery_provider() {
        let output = run(&["--help"]);
        assert!(output.status.success());
        let help = String::from_utf8(output.stdout).unwrap();
        for name in [
            "audio-control policy",
            "audio-control bluetooth-pair <adapter> <address>",
            "audio-control battery-provider",
        ] {
            assert!(help.contains(name));
        }
        for name in [
            "status",
            "bluetooth-power",
            "bluetooth-devices",
            "bluetooth-scan",
            "set-default",
            "audio-control devices",
        ] {
            assert!(!help.contains(name));
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
                    .contains(&format!("unknown command {name:?}"))
            );
        }
    }
    #[test]
    fn strict_cli_arity_and_pairing_identity_validation_precede_bus_access() {
        for args in [
            vec!["policy", "extra"],
            vec!["battery-provider", "extra"],
            vec!["bluetooth-pair"],
            vec!["bluetooth-pair", "hci0"],
            vec!["bluetooth-pair", "default", "AA:BB:CC:DD:EE:FF"],
            vec!["bluetooth-pair", "hci0", "Keyboard"],
        ] {
            let output = run(&args);
            assert_eq!(output.status.code(), Some(2));
            assert!(output.stdout.is_empty());
            assert!(!String::from_utf8(output.stderr).unwrap().contains("D-Bus"));
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
    fn bounded_lines_and_strict_request_schemas_are_enforced() {
        assert!(
            crate::policy::read_line(&mut Cursor::new(vec![
                b'x';
                crate::policy::MAX_LINE_BYTES + 1
            ]))
            .is_err()
        );
        assert!(crate::policy::read_line(&mut Cursor::new(b"{}".to_vec())).is_err());
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
    fn oversized_result_is_a_typed_failure_not_unframed_stdout_or_silent_exit() {
        let mut snapshot = offline();
        let template = snapshot.nodes[0].clone();
        snapshot.nodes = (1000..1500)
            .map(|id| {
                let mut node = template.clone();
                node.id = id;
                node.serial = id.to_string();
                node.name = format!("virtual-{id}");
                node.description = format!("Output {id} {}", "x".repeat(1000));
                node
            })
            .collect();
        let request = format!("{}\n", request(snapshot, None));
        assert!(request.len() < crate::policy::MAX_LINE_BYTES);
        let mut output = Vec::new();
        crate::policy::serve(Cursor::new(request.into_bytes()), &mut output).unwrap();
        let result: Value = serde_json::from_slice(&output).unwrap();
        assert_eq!(result["ok"], false);
        assert!(
            result["error"]
                .as_str()
                .unwrap()
                .contains("response exceeds")
        );
    }
    #[test]
    fn oversized_raw_dump_is_killed_and_reported_through_the_protocol() {
        let fixture = DumpFixture::new(&Value::String(
            "x".repeat(crate::devices::MAX_DUMP_BYTES as usize + 1),
        ));
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
