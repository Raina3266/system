//! White-box tests for the production audio library.

#[path = "test_fixtures.rs"]
mod fixtures;

mod resident_policy {
    use super::fixtures::{self, resident::*};
    use crate::{
        audio::{self, SelectionPlan},
        model::{AudioKind, Choice, NativeSnapshot, object_key},
        policy::panel::{self, Native},
    };
    use serde_json::{Value, json};
    use std::collections::HashSet;

    fn row(native: &NativeSnapshot, kind: AudioKind) -> Choice {
        let mut value = audio::choices(native, kind, &[], &[]).unwrap().remove(0);
        value.as_object_mut().unwrap().remove("default");
        serde_json::from_value(value).unwrap()
    }
    fn stream(key: &str, target: i32) -> Value {
        json!({"key":key,"type":"stream","id":40,"serial":"400","name":key,"targetSerial":target})
    }
    #[test]
    fn reliability_counts_only_same_kind_and_exact_card_binding() {
        let mut snapshot = fixtures::snapshot();
        assert!(audio::routes_reliable(&snapshot, &snapshot.nodes[0]));
        let mut second = snapshot.nodes[0].clone();
        second.id = 99;
        second.serial = "999".into();
        snapshot.nodes.push(second);
        assert!(!audio::routes_reliable(&snapshot, &snapshot.nodes[0]));
        assert!(audio::routes_reliable(&snapshot, &snapshot.nodes[1]));
        assert!(audio::routes_reliable(&snapshot, &snapshot.nodes[2]));
        snapshot.nodes.last_mut().unwrap().card_serial = "replaced".into();
        assert!(audio::routes_reliable(&snapshot, &snapshot.nodes[0]));
    }

    #[test]
    fn selection_plan_preserves_zero_and_rejects_stale_ambiguous_or_unavailable_native_routes() {
        let mut initial = simple(AudioKind::Output);
        let mut target = row(&initial, AudioKind::Output);
        let route =
            json!({"index":0,"name":"headphones","description":"Headphones","available":true});
        initial.objects[0]["routes"] = json!([route]);
        target.port = true;
        target.port_name = "headphones".into();
        target.route_id = Some(0);
        assert!(matches!(
            audio::selection_plan(&initial.audio().unwrap(), &target).unwrap(),
            SelectionPlan::Native { route: Some(0), .. }
        ));
        for mode in [
            "removed",
            "serial",
            "index",
            "name",
            "unavailable",
            "duplicate",
        ] {
            let mut state = initial.clone();
            match mode {
                "removed" => state.objects.clear(),
                "serial" => state.objects[0]["serial"] = "999".into(),
                "index" => state.objects[0]["routes"][0]["index"] = 2.into(),
                "name" => state.objects[0]["routes"][0]["name"] = "other".into(),
                "unavailable" => state.objects[0]["routes"][0]["available"] = false.into(),
                _ => state.objects[0]["routes"] = json!([route, route]),
            }
            assert!(
                audio::selection_plan(&state.audio().unwrap(), &target).is_err(),
                "{mode}"
            );
        }
        let mut snapshot = initial.audio().unwrap();
        snapshot.nodes[0].card_id = Some(7);
        snapshot.nodes[0].card_serial = "70".into();
        let mut duplicate = snapshot.nodes[0].clone();
        duplicate.id = 21;
        snapshot.nodes.push(duplicate);
        assert_eq!(
            audio::selection_plan(&snapshot, &target).unwrap(),
            SelectionPlan::Policy
        );
        target.port = false;
        target.route_id = None;
        assert!(matches!(
            audio::selection_plan(&snapshot, &target).unwrap(),
            SelectionPlan::Native { route: None, .. }
        ));
        target.node_id = None;
        target.port = true;
        target.card_id = Some(7);
        assert_eq!(
            audio::selection_plan(&snapshot, &target).unwrap(),
            SelectionPlan::Policy
        );
    }

    #[test]
    fn choice_reconciliation_keeps_order_native_defaults_and_intentional_omissions() {
        let mut state = fixtures::initial();
        state.snapshot.nodes[0].route_name = fixtures::SPEAKER.into();
        let native = native(state.snapshot.clone());
        let entries = audio::rows(&state, AudioKind::Output).unwrap();
        let rows = audio::choices(&native, AudioKind::Output, &entries, &[]).unwrap();
        assert!(
            rows.iter()
                .any(|row| row["nodeId"] == 10 && row["default"] == true)
        );
        assert!(
            rows.iter()
                .all(|row| row["nodeId"] == 10 || row["default"] == false)
        );
        let keys: Vec<_> = rows.iter().map(|row| &row["key"]).collect();
        assert_eq!(
            keys,
            entries
                .iter()
                .map(|row| &row.key)
                .map(|key| rows.iter().find(|row| row["key"] == *key).unwrap())
                .map(|row| &row["key"])
                .collect::<Vec<_>>()
        );
        let resolved = native
            .audio()
            .unwrap()
            .nodes
            .iter()
            .filter(|node| node.kind == AudioKind::Output)
            .map(|node| node.identity())
            .collect::<Vec<_>>();
        assert!(
            audio::choices(&native, AudioKind::Output, &[], &resolved)
                .unwrap()
                .is_empty()
        );
        let fallback = audio::choices(&native, AudioKind::Output, &[], &[]).unwrap();
        assert_eq!(fallback.len(), 2);
        assert!(
            fallback
                .iter()
                .all(|row| row["key"].as_str().unwrap().starts_with("native:"))
        );
        let mut ghosts = entries.clone();
        for row in &mut ghosts {
            row.node_serial = "999".into();
            row.card_serial = "999".into();
        }
        assert_eq!(
            audio::choices(&native, AudioKind::Output, &ghosts, &[]).unwrap(),
            fallback
        );
        let mut absent = native.clone();
        absent
            .objects
            .retain(|object| object["type"] != "card" && object["cardId"].is_null());
        assert!(
            audio::choices(&absent, AudioKind::Output, &entries, &[])
                .unwrap()
                .iter()
                .all(|row| !row["nodeId"].is_null())
        );
    }

    #[test]
    fn ambiguous_card_getter_uses_only_endpoint_port_active_but_never_selects_a_non_native_default()
    {
        let mut state = fixtures::initial();
        let mut second = state.snapshot.nodes[0].clone();
        second.id = 11;
        second.serial = "110".into();
        second.name = "second".into();
        state.snapshot.nodes.push(second);
        let mut entries = audio::rows(&state, AudioKind::Output).unwrap();
        for row in &mut entries {
            row.port_active = row.node_id == Some(11);
        }
        let native = native(state.snapshot.clone());
        assert!(
            audio::choices(&native, AudioKind::Output, &entries, &[])
                .unwrap()
                .iter()
                .all(|row| row["default"] == false)
        );
        entries
            .iter_mut()
            .filter(|row| row.node_id == Some(10))
            .for_each(|row| row.port_active = true);
        assert!(
            audio::choices(&native, AudioKind::Output, &entries, &[])
                .unwrap()
                .iter()
                .any(|row| row["nodeId"] == 10 && row["default"] == true)
        );
    }

    #[test]
    fn volumes_are_finite_unquantized_clamped_per_kind_and_guard_the_default_epoch() {
        for kind in [AudioKind::Output, AudioKind::Input] {
            let initial = simple(kind);
            let key = initial.epoch(kind);
            for (value, expected) in [
                (-5.0, 0.0),
                (10.0, if kind == AudioKind::Output { 1.5 } else { 1.0 }),
                (0.12345, 0.12345),
            ] {
                let (action, guards) =
                    panel::volume_action(&initial, kind, key, Some(value)).unwrap();
                assert_eq!(action["args"]["volume"], expected);
                assert_eq!(guards.len(), 1);
            }
            for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
                assert!(panel::volume_action(&initial, kind, key, Some(value)).is_err());
            }
            for stale in ["", "old", "other-kind"] {
                assert!(panel::volume_action(&initial, kind, stale, None).is_err());
            }
            let (action, guards) = panel::volume_action(&initial, kind, key, None).unwrap();
            assert_eq!(action["args"]["muted"], true);
            let mut fake = Fake::new(initial.clone());
            if kind == AudioKind::Output {
                fake.state.output_epoch.push('x');
            } else {
                fake.state.input_epoch.push('x');
            }
            assert!(
                fake.execute(
                    "mute",
                    object_key(&action["node"]),
                    action["args"].clone(),
                    guards,
                    false
                )
                .is_err()
            );
            assert!(fake.actions.is_empty());
        }
    }

    #[test]
    fn native_route_and_default_transactions_wait_preserve_index_zero_and_never_switch_profiles() {
        for kind in [AudioKind::Output, AudioKind::Input] {
            for routed in [false, true] {
                let mut initial = simple(kind);
                let mut target = row(&initial, kind);
                if routed {
                    initial.objects[0]["routes"] =
                        json!([{"index":0,"name":"port","description":"Port","available":true}]);
                    target.port = true;
                    target.port_name = "port".into();
                    target.route_id = Some(0);
                }
                let mut fake = Fake::new(initial.clone());
                panel::native_select(&mut fake, &initial, kind, &target).unwrap();
                assert_eq!(
                    fake.actions
                        .iter()
                        .map(|action| action.0.as_str())
                        .collect::<Vec<_>>(),
                    if routed {
                        vec!["route", "default"]
                    } else {
                        vec!["default"]
                    }
                );
                assert_eq!(fake.polls, if routed { 2 } else { 1 });
            }
        }
    }

    #[test]
    fn rejected_or_unacknowledged_native_changes_never_default_or_clear_stream_pins() {
        for mode in [
            "route-error",
            "route-timeout",
            "default-error",
            "default-timeout",
            "replacement",
        ] {
            let mut initial = simple(AudioKind::Output);
            let mut target = row(&initial, AudioKind::Output);
            initial.objects[0]["routes"] =
                json!([{"index":0,"name":"port","description":"Port","available":true}]);
            target.port = true;
            target.port_name = "port".into();
            target.route_id = Some(0);
            initial.objects.push(stream("Chrome", 100));
            initial.default_output.clear();
            let mut fake = Fake::new(initial.clone());
            if mode.ends_with("error") {
                fake.fail = mode.split('-').next().unwrap().into();
            } else if mode.ends_with("timeout") {
                fake.ignore = mode.split('-').next().unwrap().into();
            }
            if mode == "replacement" {
                fake.hook = Some(Box::new(|fake, operation| {
                    if operation == "pause" {
                        fake.state.objects[0]["key"] = "replacement".into();
                    }
                }));
            }
            assert!(
                panel::native_select(&mut fake, &initial, AudioKind::Output, &target).is_err(),
                "{mode}"
            );
            assert!(!fake.actions.iter().any(|action| action.0 == "follow"));
            if mode.starts_with("route") || mode == "replacement" {
                assert!(!fake.actions.iter().any(|action| action.0 == "default"));
            }
            assert!(fake.polls <= 32);
        }
    }

    #[test]
    fn follow_default_resets_every_playback_pin_to_null_never_capture_and_only_after_selection_success()
     {
        let mut initial = simple(AudioKind::Output);
        initial.objects.extend([
            stream("Chrome", 100),
            stream("Elisa", 999),
            json!({"key":"capture","type":"device","targetSerial":200}),
        ]);
        let endpoint = initial.objects[0].clone();
        let mut fake = Fake::new(initial.clone());
        panel::follow_output(&mut fake, &endpoint).unwrap();
        assert_eq!(
            fake.actions
                .iter()
                .map(|action| action.1.as_str())
                .collect::<Vec<_>>(),
            ["Chrome", "Elisa"]
        );
        assert!(
            fake.actions
                .iter()
                .all(|action| action.0 == "follow" && action.2 == json!({}))
        );
        assert_eq!(fake.state.object("capture").unwrap()["targetSerial"], 200);
        fake.state.objects.push(stream("New playback", 444));
        panel::follow_output(&mut fake, &endpoint).unwrap();
        assert_eq!(
            fake.state.object("New playback").unwrap()["targetSerial"],
            -1
        );
    }

    #[test]
    fn playback_reset_guards_default_and_lifetime_before_each_write_and_during_convergence() {
        for cause in ["default", "removed", "replacement", "serial"] {
            for during in [false, true] {
                let mut initial = simple(AudioKind::Output);
                initial
                    .objects
                    .extend([stream("first", 100), stream("later", 100)]);
                let endpoint = initial.objects[0].clone();
                let mut fake = Fake::new(initial);
                fake.ignore = if during {
                    "follow".into()
                } else {
                    String::new()
                };
                fake.hook = Some(Box::new(move |fake, operation| {
                    if operation == if during { "pause" } else { "follow" } {
                        match cause {
                            "default" => fake.state.default_output = "elsewhere".into(),
                            "removed" => {
                                fake.state.objects.retain(|object| object["type"] != "node")
                            }
                            "replacement" => fake.state.objects[0]["key"] = "new-object".into(),
                            _ => fake.state.objects[0]["serial"] = "999".into(),
                        }
                    }
                }));
                assert!(
                    panel::follow_output(&mut fake, &endpoint).is_err(),
                    "{cause}/{during}"
                );
                if !during {
                    assert_eq!(fake.actions.len(), 1);
                }
            }
        }
    }

    #[test]
    fn removed_streams_are_skipped_and_no_longer_block_pending_follow_default() {
        for during in [false, true] {
            let mut initial = simple(AudioKind::Output);
            initial
                .objects
                .extend([stream("first", 100), stream("removed", 100)]);
            let endpoint = initial.objects[0].clone();
            let mut fake = Fake::new(initial);
            fake.hook = Some(Box::new(move |fake, operation| {
                if operation == if during { "pause" } else { "follow" } {
                    fake.state
                        .objects
                        .retain(|object| object["key"] != "removed");
                }
            }));
            panel::follow_output(&mut fake, &endpoint).unwrap();
            if !during {
                assert_eq!(fake.actions.len(), 1);
            }
        }
    }

    #[test]
    fn partial_playback_failures_do_not_abandon_other_setters_or_native_convergence() {
        let mut initial = simple(AudioKind::Output);
        initial.objects.extend([
            stream("Chrome", 100),
            stream("Elisa", 200),
            stream("Other", 333),
        ]);
        let endpoint = initial.objects[0].clone();
        let mut fake = Fake::new(initial);
        fake.fail = "Chrome".into();
        fake.ignore = "follow".into();
        fake.hook = Some(Box::new(|fake, operation| {
            if operation == "pause" && fake.polls == 3 {
                fake.object_mut("Elisa")["targetSerial"] = (-1).into();
                fake.object_mut("Other")["targetSerial"] = (-1).into();
            }
        }));
        let error = panel::follow_output(&mut fake, &endpoint)
            .unwrap_err()
            .to_string();
        assert!(error.contains("Chrome") && error.contains("rejected"));
        assert_eq!(fake.actions.len(), 3);
        assert_eq!(fake.polls, 3);
        let mut fake = Fake::new(fake.state);
        fake.ignore = "follow".into();
        fake.object_mut("Elisa")["targetSerial"] = 200.into();
        assert!(
            panel::follow_output(&mut fake, &endpoint)
                .unwrap_err()
                .to_string()
                .contains("playback")
        );
        assert_eq!(fake.polls, 30);
    }

    fn bluetooth_native() -> NativeSnapshot {
        NativeSnapshot {
            objects: vec![
                json!({"key":"adapter","type":"adapter","path":"/org/bluez/hci12","powered":true}),
                json!({"key":"buds","type":"device","address":"AA:BB:CC:DD:EE:01","adapter":"/org/bluez/hci12","name":"Desk buds","alias":"","connected":false,"paired":true,"trusted":false,"battery":1.4}),
            ],
            adapter: "adapter".into(),
            opened: true,
            tab: 0,
            ..Default::default()
        }
    }

    #[test]
    fn automatic_trust_connect_is_rust_ordered_and_cancelled_on_native_or_ui_replacement() {
        let initial = bluetooth_native();
        let device = initial.object("buds").unwrap().clone();
        let mut fake = Fake::new(initial.clone());
        panel::paired_setup(&mut fake, &initial, &device).unwrap();
        assert_eq!(
            fake.actions
                .iter()
                .map(|action| action.0.as_str())
                .collect::<Vec<_>>(),
            ["trust", "connect"]
        );
        for cause in [
            "close",
            "tab",
            "power",
            "adapter",
            "device",
            "cancel",
            "trust-error",
            "connect-error",
        ] {
            let mut fake = Fake::new(initial.clone());
            if cause.ends_with("error") {
                fake.fail = cause.split('-').next().unwrap().into();
            } else {
                fake.hook = Some(Box::new(move |fake, operation| {
                    if operation == "trust" {
                        match cause {
                            "close" => fake.state.opened = false,
                            "tab" => fake.state.tab = 1,
                            "power" => fake.object_mut("adapter")["powered"] = false.into(),
                            "adapter" => fake.state.adapter = "replacement".into(),
                            "device" => fake.object_mut("buds")["key"] = "replacement".into(),
                            _ => fake.state.pairing_epoch += 1,
                        }
                    }
                }));
            }
            assert!(
                panel::paired_setup(&mut fake, &initial, &device).is_err(),
                "{cause}"
            );
            if cause != "connect-error" {
                assert_eq!(fake.actions.len(), 1, "{cause}");
            }
        }
    }

    #[test]
    fn pairing_notices_keep_one_named_status_and_latest_typed_prompt_without_reviving_dismissed_completion()
     {
        use crate::bluetooth::PairingNotice;
        let native = bluetooth_native();
        let device = native.object("buds").unwrap();
        let mut notice = PairingNotice::new(device);
        assert_eq!(notice.text, "Pairing with Desk buds (AA:BB:CC:DD:EE:01)…");
        assert!(notice.prompt.is_empty());
        notice.dismiss(true);
        assert!(notice.dismissed);
        notice.set_prompt("Does passkey 000007 match? Type yes or no.".into(), true);
        assert!(!notice.dismissed);
        assert!(notice.needs_answer);
        notice.dismiss(true);
        assert!(!notice.dismissed, "an unanswered prompt remains reachable");
        notice.set_prompt("Enter PIN:".into(), true);
        assert_eq!(notice.prompt, "Enter PIN:");
        assert!(!notice.prompt.contains("000007"));
        notice.clear_prompt();
        notice.dismiss(true);
        notice.paired(device);
        assert!(notice.dismissed);
        assert_eq!(notice.state, "paired");
        assert!(notice.prompt.is_empty());
        notice.set_prompt("Type 042311 on the device to finish pairing.".into(), false);
        assert!(!notice.dismissed);
        assert!(!notice.needs_answer);
        notice.fail(
            device,
            "progress\naudio-control: Incorrect pairing code for AA:BB:CC:DD:EE:01.\n",
            false,
        );
        assert_eq!(
            notice.text,
            "Pairing failed: Incorrect pairing code for Desk buds (AA:BB:CC:DD:EE:01)."
        );
        assert_eq!(notice.state, "failed");
        assert!(!notice.text.contains('\n'));
        assert!(notice.prompt.is_empty());
        notice.dismiss(false);
        notice.fail(device, "connect failed", true);
        assert!(notice.dismissed);
        assert!(notice.text.contains("but setup failed: connect failed"));
        let mut unnamed = device.clone();
        unnamed["name"] = "".into();
        unnamed["alias"] = "Alias".into();
        assert_eq!(
            PairingNotice::new(&unnamed).text,
            "Pairing with Alias (AA:BB:CC:DD:EE:01)…"
        );
        unnamed["alias"] = "".into();
        assert_eq!(
            PairingNotice::new(&unnamed).text,
            "Pairing with AA:BB:CC:DD:EE:01…"
        );
    }

    #[test]
    fn bluetooth_names_batteries_and_answers_preserve_aliases_leading_zeroes_and_injection_guards()
    {
        let mut native = bluetooth_native();
        let mut device = native.object("buds").unwrap().clone();
        device["name"] = device["address"].clone();
        device["alias"] = " Desk buds ".into();
        assert_eq!(crate::bluetooth::display_name(&device), "Desk buds");
        device["alias"] = "00-11-22-33-44-55".into();
        assert_eq!(crate::bluetooth::display_name(&device), "AA:BB:CC:DD:EE:01");
        let rows = crate::bluetooth::device_rows(&native, &HashSet::from(["buds".into()]));
        assert_eq!(rows[0]["battery"], 1.0);
        assert_eq!(rows[0]["busy"], true);
        native.objects[1]["battery"] = (-1).into();
        assert!(crate::bluetooth::device_rows(&native, &HashSet::new())[0]["battery"].is_null());
        native.objects[1]["adapter"] = "/org/bluez/hci0".into();
        assert!(crate::bluetooth::device_rows(&native, &HashSet::new()).is_empty());
        for answer in ["000007", "  PIN with spaces  ", "yes", "no"] {
            assert_eq!(crate::bluetooth::pairing_answer(answer).unwrap(), answer);
        }
        assert_eq!(
            crate::bluetooth::pairing_answer(&"🎧".repeat(256))
                .unwrap()
                .encode_utf16()
                .count(),
            512
        );
        assert!(crate::bluetooth::pairing_answer(&"🎧".repeat(257)).is_err());
        for answer in ["", " ", "a\nb", "a\rb", "a\0b", &"x".repeat(513)] {
            assert!(crate::bluetooth::pairing_answer(answer).is_err());
        }
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
    use crate::{audio, devices::Metadata, model::AudioKind};
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
    fn mixed_alsa_bluez_profiles_preserve_routes_and_native_endpoint_fallback() {
        for associated in [false, true] {
            let (native, mut raw) = mixed_alsa_bluez();
            if associated {
                for (index, device) in [(0, 1), (1, 0)] {
                    raw[1]["info"]["params"]["EnumRoute"][index]["devices"] = json!([device]);
                    raw[1]["info"]["params"]["Route"][index]["profile"] = 1.into();
                }
            }
            for (kind, id, route) in [(AudioKind::Output, 30, 10), (AudioKind::Input, 31, 11)] {
                native.validate().unwrap();
                let state = audio::State {
                    snapshot: native.clone(),
                    metadata: Metadata::from_dump(&native, kind, &raw).unwrap(),
                };
                let card = &state.snapshot.cards[1];
                let data = &state.metadata.cards[&card.id];
                assert!(
                    data.profiles.is_empty(),
                    "non-ALSA profiles are not policy inputs"
                );
                assert_eq!(data.routes.len(), 2);
                assert_eq!(data.active_routes.len(), if associated { 2 } else { 0 });
                let endpoint = state
                    .snapshot
                    .nodes
                    .iter()
                    .find(|node| node.id == id)
                    .unwrap();
                assert_eq!(
                    state.metadata.routes_for(card, endpoint).len(),
                    usize::from(associated)
                );
                let raw_route = data.routes.iter().find(|port| port.index == route).unwrap();
                assert_eq!(raw_route.kind, kind);
                assert_eq!(raw_route.profiles, [1]);
                assert!(audio::profile_for(&state, card, raw_route).is_none());
                let mut rows = audio::rows(&state, kind).unwrap();
                let row = rows.iter().find(|row| row.node_id == Some(id)).unwrap();
                assert_eq!(row.card_id, Some(8));
                assert_eq!(row.card_serial, "80");
                assert_eq!(row.port, associated);
                assert_eq!(row.route_id, associated.then_some(route));
                assert_eq!(row.port_active, associated);
                assert_eq!(rows.iter().filter(|row| row.card_id == Some(8)).count(), 1);
                rows.retain(|row| row.card_id != Some(8));
                assert_eq!(
                    serde_json::to_value(rows).unwrap(),
                    serde_json::to_value(audio::rows(&initial(), kind).unwrap()).unwrap(),
                    "BlueZ metadata must not disrupt ALSA choices"
                );
            }
        }
    }
    #[test]
    fn alsa_profile_classes_remain_required_and_strict_in_mixed_inventory() {
        for (classes, message) in [
            (None, "missing field `classes`"),
            (Some(json!(null)), "Invalid profile classes"),
            (Some(json!([])), "Missing profile class count"),
            (Some(json!([9])), "Invalid profile class count"),
            (
                Some(json!([1, ["Audio/Sink", 1, "wrong-property", [0]]])),
                "Missing profile device association",
            ),
            (
                Some(json!([1, ["Audio/Sink", 2, "card.profile.devices", [0]]])),
                "Invalid profile node count",
            ),
        ] {
            let (native, mut raw) = mixed_alsa_bluez();
            let profile = raw[0]["info"]["params"]["EnumProfile"][1]
                .as_object_mut()
                .unwrap();
            if let Some(classes) = classes {
                profile.insert("classes".into(), classes);
            } else {
                profile.remove("classes");
            }
            for kind in [AudioKind::Output, AudioKind::Input] {
                let error = Metadata::from_dump(&native, kind, &raw).unwrap_err();
                assert!(error.to_string().contains(message), "{error}");
            }
        }
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
        for (parameter, field, value) in [
            ("EnumProfile", "priority", json!("high")),
            (
                "EnumProfile",
                "classes",
                json!([1, ["Audio/Sink", 1, "card.profile.devices", [-1]]]),
            ),
            ("EnumRoute", "profiles", json!([1.5])),
            ("EnumRoute", "devices", json!([-1])),
            ("Route", "device", json!("3")),
        ] {
            let mut raw = dump();
            raw[0]["info"]["params"][parameter][0][field] = value;
            assert!(Metadata::from_dump(&snapshot(), AudioKind::Output, &raw).is_err());
        }
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
        audio::{self, Action, physical_key},
        model::{AudioKind, Snapshot, hex_encode, single_line},
    };
    use serde_json::{Value, json};
    use std::collections::HashMap;

    fn card_wide_active_routes() -> (Snapshot, Value) {
        let mut native = snapshot();
        for endpoint in &mut native.nodes {
            endpoint.routes.retain(|route| matches!(route.index, 3 | 5));
        }
        // Each endpoint exposes only its direction's card-wide active route.
        for (id, name, device, kind) in [
            (30, "hdmi1", 2, AudioKind::Output),
            (31, "hdmi2", 1, AudioKind::Output),
            (32, "hdmi3", 0, AudioKind::Output),
            (33, "mic2", 4, AudioKind::Input),
        ] {
            let mut endpoint = native
                .nodes
                .iter()
                .find(|node| node.kind == kind)
                .unwrap()
                .clone();
            endpoint.id = id;
            endpoint.serial = (id * 10).to_string();
            endpoint.name = name.into();
            endpoint.profile_device = Some(device);
            native.nodes.push(endpoint);
        }
        let mut raw = dump();
        for index in 0..3 {
            raw[0]["info"]["params"]["EnumRoute"][index]["available"] = "no".into();
        }
        (native, raw)
    }

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
    fn card_wide_active_routes_do_not_resurrect_unavailable_endpoints() {
        let (native, raw) = card_wide_active_routes();
        let state = state(native, &raw);
        let outputs = audio::rows(&state, AudioKind::Output).unwrap();
        assert!(
            outputs
                .iter()
                .all(|row| !matches!(row.node_id, Some(30..=32)))
        );
        assert!(
            !outputs
                .iter()
                .any(|row| row.port_name.starts_with("[Out] HDMI"))
        );
        let speaker = outputs.iter().find(|row| row.node_id == Some(10)).unwrap();
        assert_eq!(speaker.port_name, SPEAKER);
        assert_eq!(speaker.route_id, Some(3));
        assert!(speaker.port_active);
        let inputs = audio::rows(&state, AudioKind::Input).unwrap();
        assert_eq!(inputs.len(), 1);
        assert_eq!(inputs[0].node_id, Some(12));
        assert_eq!(inputs[0].port_name, "[In] Mic1");
        assert_eq!(inputs[0].route_id, Some(5));
        assert!(inputs[0].port_active);
    }
    #[test]
    fn available_associations_without_native_routes_allow_default_only_selection() {
        for (kind, id, index) in [(AudioKind::Output, 30, 2), (AudioKind::Input, 33, 4)] {
            let (native, mut raw) = card_wide_active_routes();
            assert!(
                !audio::rows(&state(native.clone(), &raw), kind)
                    .unwrap()
                    .iter()
                    .any(|row| row.node_id == Some(id))
            );
            raw[0]["info"]["params"]["EnumRoute"][index]["available"] = "yes".into();
            let initial = state(native.clone(), &raw);
            let rows = audio::rows(&initial, kind).unwrap();
            let row = rows.iter().find(|row| row.node_id == Some(id)).unwrap();
            assert!(!row.port);
            assert!(row.port_name.is_empty());
            assert_eq!(row.route_id, None);
            let identity = native
                .nodes
                .iter()
                .find(|node| node.id == id)
                .unwrap()
                .identity();
            let mut fake = Fake {
                snapshot: native,
                dump: raw,
                ..Fake::default()
            };
            audio::select(&mut fake, &initial, kind, &row.key).unwrap();
            assert_eq!(
                fake.events,
                [Action::Default {
                    node: identity.clone()
                }]
            );
            assert_eq!(
                fake.snapshot.default_node(kind).unwrap().identity(),
                identity
            );
        }
    }
    #[test]
    fn device_only_fallback_prefers_matching_native_availability_on_the_card() {
        for (kind, id, index) in [(AudioKind::Output, 30, 2), (AudioKind::Input, 33, 4)] {
            for available in [false, true] {
                let (mut native, mut raw) = card_wide_active_routes();
                let associated = &raw[0]["info"]["params"]["EnumRoute"][index];
                let endpoint = native
                    .nodes
                    .iter_mut()
                    .find(|node| node.kind == kind)
                    .unwrap();
                let mut route = endpoint.routes[0].clone();
                route.index = index as i32;
                route.name = associated["name"].as_str().unwrap().into();
                route.description = associated["description"].as_str().unwrap().into();
                route.available = available;
                endpoint.routes.push(route);
                raw[0]["info"]["params"]["EnumRoute"][index]["available"] =
                    if available { "no" } else { "yes" }.into();
                let rows = audio::rows(&state(native, &raw), kind).unwrap();
                assert_eq!(rows.iter().any(|row| row.node_id == Some(id)), available);
            }
        }
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
    fn bluetooth_routes_use_device_names_instead_of_generic_port_labels() {
        let (mut native, mut raw) = mixed_alsa_bluez();
        for node in native
            .nodes
            .iter_mut()
            .filter(|node| node.card_id == Some(8))
        {
            node.description = "Raina's Pixel Buds Pro".into();
            node.routes[0].description = if node.kind == AudioKind::Output {
                "Headphones"
            } else {
                "Bluetooth"
            }
            .into();
        }
        // Exercise associated native ports, not just device-only fallback rows.
        raw[1]["info"]["params"]["EnumRoute"][0]["devices"] = json!([1]);
        raw[1]["info"]["params"]["EnumRoute"][1]["devices"] = json!([0]);
        let state = state(native, &raw);
        for (kind, id, route) in [(AudioKind::Output, 30, 10), (AudioKind::Input, 31, 11)] {
            let rows = audio::rows(&state, kind).unwrap();
            let row = rows.iter().find(|row| row.node_id == Some(id)).unwrap();
            assert_eq!(row.label, "Raina's Pixel Buds Pro");
            assert!(row.port);
            assert_eq!(row.route_id, Some(route));
            assert_eq!(
                row.key,
                format!(
                    "{}:{}:port:{}",
                    kind.key_prefix(),
                    hex_encode(&row.name),
                    hex_encode(&row.port_name)
                )
            );
        }
        let wired = audio::rows(&state, AudioKind::Output).unwrap();
        assert_eq!(
            wired
                .iter()
                .find(|row| row.port_name == SPEAKER)
                .unwrap()
                .label,
            "Speaker"
        );
        assert_eq!(
            wired
                .iter()
                .find(|row| row.port_name == HEADPHONES)
                .unwrap()
                .label,
            "Headphones"
        );
    }

    #[test]
    fn bluetooth_device_only_rows_preserve_names_and_distinguish_connected_devices() {
        let (mut native, raw) = mixed_alsa_bluez();
        native
            .nodes
            .iter_mut()
            .find(|node| node.id == 30)
            .unwrap()
            .description = "Raina's Pixel Buds Pro".into();
        let mut second = native
            .nodes
            .iter()
            .find(|node| node.id == 30)
            .unwrap()
            .clone();
        second.id = 32;
        second.serial = "320".into();
        second.name = "bluez_output.second".into();
        second.description = "Family Room Analog Stereo 🎧".into();
        second.card_id = None;
        second.card_serial.clear();
        native.nodes.push(second);
        let rows = audio::rows(&state(native, &raw), AudioKind::Output).unwrap();
        for (id, name) in [
            (30, "Raina's Pixel Buds Pro"),
            (32, "Family Room Analog Stereo 🎧"),
        ] {
            let row = rows.iter().find(|row| row.node_id == Some(id)).unwrap();
            assert_eq!(row.label, name);
            assert!(!row.port);
        }
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

mod protocol {
    use super::fixtures::offline;
    use serde_json::{Value, json};
    use std::io::Cursor;

    #[test]
    fn lines_must_be_bounded_and_newline_terminated() {
        assert!(
            crate::policy::read_line(&mut Cursor::new(vec![
                b'x';
                crate::policy::MAX_LINE_BYTES + 1
            ]))
            .is_err()
        );
        assert!(crate::policy::read_line(&mut Cursor::new(b"{}".to_vec())).is_err());
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
        let request = format!(
            "{}\n",
            json!({"command":"rows","kind":"output","snapshot":snapshot})
        );
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
}
