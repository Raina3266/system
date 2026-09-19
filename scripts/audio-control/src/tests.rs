//! Every test in the script, one mod per feature and one test per sub-feature.

mod model {
    use crate::model::*;

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

mod audio {
    use std::{collections::HashMap, io};

    use libpulse_binding::def::PortAvailable;
    use libpulse_binding::volume::{ChannelVolumes, Volume};
    use pulsectl::controllers::types::DevicePortInfo;

    use crate::AppResult;
    use crate::audio::*;
    use crate::model::{AudioEntry, AudioKind, hex_encode};

    fn device(kind: AudioKind) -> AudioEntry {
        AudioEntry {
            key: format!("{}:{}", kind.key_prefix(), hex_encode("built-in")),
            kind,
            name: "built-in".into(),
            card: None,
            description: "Built-in Audio Analog Stereo".into(),
            label: "Built-in Audio".into(),
            volume: 65,
            muted: true,
            default: true,
            port: None,
        }
    }

    fn port(name: &str, label: &str, available: PortAvailable) -> DevicePortInfo {
        DevicePortInfo {
            name: Some(name.into()),
            description: Some(label.into()),
            priority: 100,
            available,
        }
    }

    #[test]
    fn output_and_input_ports_are_distinct_rows_with_only_the_active_default_marked() {
        for (kind, labels) in [
            (AudioKind::Output, ["Speakers", "Headphones"]),
            (AudioKind::Input, ["Internal microphone", "Microphone jack"]),
        ] {
            let ports = [
                port("internal", labels[0], PortAvailable::Yes),
                port("jack", labels[1], PortAvailable::Yes),
            ];
            let rows = port_rows(device(kind), &ports, Some("jack"));
            assert_eq!(rows.len(), 2);
            assert_ne!(rows[0].key, rows[1].key);
            assert_eq!(rows[0].port.as_deref(), Some("internal"));
            assert_eq!(rows[1].port.as_deref(), Some("jack"));
            assert!(!rows[0].default);
            assert!(rows[1].default);
            for (row, label) in rows.iter().zip(labels) {
                assert_eq!(row.name, "built-in");
                assert_eq!(row.label, label);
                assert!(row.description.contains(label));
                assert_eq!(row.volume, 65);
                assert!(row.muted);
            }
            let mut other = device(kind);
            other.default = false;
            assert!(
                port_rows(other, &ports, Some("jack"))
                    .iter()
                    .all(|r| !r.default)
            );
            assert!(
                port_rows(device(kind), &ports, None)
                    .iter()
                    .all(|r| !r.default)
            );
        }
    }

    #[test]
    fn unavailable_ports_are_hidden_but_unknown_availability_is_allowed() {
        let ports = [
            port("speaker", "Speakers", PortAvailable::Unknown),
            port("jack", "Headphones", PortAvailable::No),
        ];
        let rows = port_rows(device(AudioKind::Output), &ports, Some("speaker"));
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].port.as_deref(), Some("speaker"));
        // Do not add a generic row that would bypass unavailable-port checks.
        assert!(port_rows(device(AudioKind::Output), &ports[1..], Some("jack")).is_empty());
    }

    #[test]
    fn devices_without_named_ports_keep_a_single_device_row() {
        let base = device(AudioKind::Output);
        let mut unnamed = port("", "", PortAvailable::Unknown);
        unnamed.name = None;
        for ports in [
            vec![],
            vec![unnamed],
            vec![port("", "", PortAvailable::Unknown)],
        ] {
            let rows = port_rows(base.clone(), &ports, None);
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].key, base.key);
            assert_eq!(rows[0].label, base.label);
            assert!(rows[0].port.is_none());
            assert!(rows[0].default);
        }
    }

    #[test]
    fn port_identity_uses_names_not_labels_and_survives_active_port_changes() {
        let ports = [
            port("jack:one;🎧", "Headphones", PortAvailable::Yes),
            port("jack:two", "Headphones", PortAvailable::Yes),
        ];
        let before = port_rows(device(AudioKind::Output), &ports, Some("jack:one;🎧"));
        let after = port_rows(device(AudioKind::Output), &ports, Some("jack:two"));
        assert_ne!(before[0].key, before[1].key);
        assert_eq!(before[0].key, after[0].key);
        assert_eq!(before[1].key, after[1].key);
        assert!(before[0].key.is_ascii());
        assert!(!before[0].key.contains(';'));
        let mut other = device(AudioKind::Output);
        other.key = format!("sink:{}", hex_encode("usb"));
        assert_ne!(before[0].key, port_rows(other, &ports, None)[0].key);
        assert_ne!(
            before[0].key,
            port_rows(device(AudioKind::Input), &ports, None)[0].key
        );
    }

    #[test]
    fn routing_choices_mark_only_the_streams_active_port() {
        for on_stream_device in [true, false] {
            let mut base = device(AudioKind::Output);
            base.default = on_stream_device;
            let ports = [
                port("speaker", "Speaker", PortAvailable::Unknown),
                port("jack", "Headphones", PortAvailable::Yes),
            ];
            let choices: Vec<_> = port_rows(base, &ports, Some("jack"))
                .into_iter()
                .map(crate::model::ChoiceEntry::route)
                .collect();
            assert_eq!(choices.len(), 2);
            assert_eq!(choices[0].label, "Speaker");
            assert_eq!(choices[1].label, "Headphones");
            assert!(!choices[0].active);
            assert_eq!(choices[1].active, on_stream_device);
            assert!(choices.iter().all(|choice| choice.enabled));
            assert_ne!(choices[0].key, choices[1].key);
        }
    }

    #[test]
    fn routing_choices_without_ports_keep_the_device_name() {
        let mut base = device(AudioKind::Output);
        base.label = "WH-1000XM4".into();
        let rows = port_rows(base.clone(), &[], None);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].label, "WH-1000XM4");
        assert_eq!(rows[0].key, base.key);
    }

    #[test]
    fn routing_choices_disambiguate_identical_port_names() {
        let mut usb = device(AudioKind::Output);
        usb.name = "usb".into();
        usb.key = format!("sink:{}", hex_encode(&usb.name));
        let ports = [port("speaker", "Speakers", PortAvailable::Unknown)];
        let mut rows: Vec<_> = [device(AudioKind::Output), usb]
            .into_iter()
            .flat_map(|base| port_rows(base, &ports, Some("speaker")))
            .collect();
        clarify_selection_labels(
            &mut rows,
            &HashMap::from([
                ("built-in".into(), "Built-in Audio".into()),
                ("usb".into(), "USB Audio".into()),
            ]),
        );
        assert_eq!(rows[0].label, "Speakers — Built-in Audio");
        assert_eq!(rows[1].label, "Speakers — USB Audio");
        assert!(
            rows.iter()
                .all(|row| row.port.as_deref() == Some("speaker"))
        );
        assert_ne!(rows[0].key, rows[1].key);
    }

    #[test]
    fn long_hardware_names_do_not_hide_the_port() {
        let mut base = device(AudioKind::Output);
        base.description = "An extremely long descriptive device name for a sound card".into();
        let ports = [port("jack", "Headphones", PortAvailable::Yes)];
        let rows = port_rows(base, &ports, Some("jack"));
        assert_eq!(rows[0].label, "Headphones");
        assert!(rows[0].row_label().ends_with("Headphones"));
    }

    #[test]
    fn hdmi_port_numbers_survive_without_repeated_chipset_names() {
        let mut base = device(AudioKind::Output);
        base.description = "Alder Lake PCH-P HDMI / DisplayPort".into();
        let ports = [
            port("hdmi-1", "HDMI / DisplayPort 1", PortAvailable::Yes),
            port("hdmi-2", "HDMI / DisplayPort 2", PortAvailable::Yes),
        ];
        let mut rows = port_rows(base, &ports, Some("hdmi-1"));
        clarify_selection_labels(&mut rows, &HashMap::new());
        assert_eq!(rows[0].label, "HDMI / DisplayPort 1");
        assert_eq!(rows[1].label, "HDMI / DisplayPort 2");
        assert!(rows[0].description.contains("Alder Lake PCH-P"));
        assert!(rows[0].row_label().ends_with('1'));
        assert!(rows[1].row_label().ends_with('2'));
    }

    #[test]
    fn identical_port_labels_only_add_hardware_context_when_needed() {
        for kind in [AudioKind::Output, AudioKind::Input] {
            let ports = [port("jack", "Headphones", PortAvailable::Yes)];
            let mut usb = device(kind);
            usb.name = "usb".into();
            usb.key = format!("{}:{}", kind.key_prefix(), hex_encode(&usb.name));
            let mut rows = port_rows(device(kind), &ports, Some("jack"));
            rows.extend(port_rows(usb, &ports, Some("jack")));
            let keys: Vec<_> = rows.iter().map(|e| e.key.clone()).collect();
            let names = HashMap::from([
                ("built-in".into(), "Built-in Audio".into()),
                ("usb".into(), "USB Headset".into()),
            ]);
            clarify_selection_labels(&mut rows, &names);
            assert_eq!(rows[0].label, "Headphones — Built-in Audio");
            assert_eq!(rows[1].label, "Headphones — USB Headset");
            assert_eq!(rows.iter().map(|e| e.key.clone()).collect::<Vec<_>>(), keys);
            assert!(rows.iter().all(|e| e.port.as_deref() == Some("jack")));
        }
    }

    #[test]
    fn identical_models_and_clipped_labels_still_have_distinct_rows() {
        let ports = [
            port("jack-1", "Headphones", PortAvailable::Yes),
            port("jack-2", "Headphones", PortAvailable::Yes),
        ];
        let mut rows = port_rows(device(AudioKind::Output), &ports, Some("jack-1"));
        clarify_selection_labels(&mut rows, &HashMap::new());
        assert_eq!(rows[0].label, "#1 Headphones");
        assert_eq!(rows[1].label, "#2 Headphones");
        assert_ne!(rows[0].key, rows[1].key);

        for (i, row) in rows.iter_mut().enumerate() {
            row.label = format!("{} {i}", "Long device name ".repeat(10));
        }
        clarify_selection_labels(&mut rows, &HashMap::new());
        assert_ne!(rows[0].row_label(), rows[1].row_label());
    }

    #[test]
    fn unique_bluetooth_usb_and_virtual_device_names_are_unchanged() {
        let mut rows: Vec<_> = ["WH-1000XM4", "Scarlett 2i2 USB", "Virtual Output"]
            .into_iter()
            .map(|label| AudioEntry {
                label: label.into(),
                ..device(AudioKind::Output)
            })
            .collect();
        clarify_selection_labels(&mut rows, &HashMap::new());
        assert_eq!(rows[0].label, "WH-1000XM4");
        assert_eq!(rows[1].label, "Scarlett 2i2 USB");
        assert_eq!(rows[2].label, "Virtual Output");
    }

    #[test]
    fn selecting_a_missing_or_unplugged_port_is_rejected() {
        let ports = [
            port("speaker", "Speakers", PortAvailable::Unknown),
            port("jack", "Headphones", PortAvailable::No),
        ];
        assert_eq!(available_port(&ports, "speaker").unwrap(), "speaker");
        assert!(available_port(&ports, "jack").is_err());
        assert!(available_port(&ports, "removed").is_err());
    }

    #[test]
    fn percentages_round_trip_through_pulseaudio_volume_units() {
        for level in [0_u8, 5, 33, 50, 66, 100, 125, 150] {
            let mut volumes = ChannelVolumes::default();
            volumes.set(2, from_percent(level));
            assert_eq!(percent(&volumes), level);
        }
    }

    #[test]
    fn normal_volume_is_exactly_one_hundred_percent() {
        let mut volumes = ChannelVolumes::default();
        volumes.set(2, Volume::NORMAL);
        assert_eq!(percent(&volumes), 100);
        assert_eq!(from_percent(100), Volume::NORMAL);
        assert_eq!(from_percent(0), Volume::MUTED);
    }

    #[test]
    fn output_can_amplify_but_input_stays_at_one_hundred() {
        assert_eq!(volume_target(100, AudioKind::Output, STEP), 105);
        assert_eq!(volume_target(148, AudioKind::Output, STEP), 150);
        assert_eq!(volume_target(150, AudioKind::Output, STEP), 150);
        assert_eq!(volume_target(100, AudioKind::Input, STEP), 100);
        assert_eq!(volume_target(97, AudioKind::Input, STEP), 100);
        assert_eq!(volume_target(3, AudioKind::Output, -STEP), 0);
        assert_eq!(volume_target(0, AudioKind::Input, -STEP), 0);
    }

    #[test]
    fn nudges_preserve_existing_channel_balance() {
        let mut volume = ChannelVolumes::default();
        volume.set(2, from_percent(60));
        volume.get_mut()[0] = from_percent(30);
        let adjusted = adjusted_volume(volume, AudioKind::Output, STEP).unwrap();
        assert_eq!(percent(&adjusted), 50);
        assert!((i64::from(adjusted.get()[0].0) * 2 - i64::from(adjusted.get()[1].0)).abs() <= 2);
        let limited = adjusted_volume(adjusted, AudioKind::Output, 200).unwrap();
        assert!(limited.max().0 <= from_percent(150).0);
    }

    #[test]
    fn zero_volume_can_be_raised() {
        let mut volume = ChannelVolumes::default();
        volume.set(2, Volume::MUTED);
        assert_eq!(
            percent(&adjusted_volume(volume, AudioKind::Output, STEP).unwrap()),
            5
        );
    }

    #[test]
    fn stream_keys_separate_clients_and_reused_indices() {
        let key = stream_identity(7, Some(3), "100");
        assert!(key.starts_with("playback:"));
        assert_ne!(key, stream_identity(7, Some(4), "100"));
        assert_ne!(key, stream_identity(7, Some(3), "101"));
    }
    use crate::audio::profiles::*;

    const CARD: &str = "alsa_card.test";
    const SPEAKERS: &str = "HiFi (HDMI1, HDMI2, HDMI3, Mic1, Mic2, Speaker)";
    const HEADPHONES: &str = "HiFi (HDMI1, HDMI2, HDMI3, Headphones, Mic1, Mic2)";

    fn card() -> Card {
        let profile = |name: &str| Profile {
            name: name.into(),
            available: true,
            sinks: 4,
            sources: 2,
            priority: 100,
        };
        let port = |name: &str, output, available, profiles: Vec<String>| Port {
            name: name.into(),
            label: name.trim_start_matches("[Out] ").into(),
            output,
            available,
            profiles,
        };
        let both = vec![SPEAKERS.into(), HEADPHONES.into()];
        Card {
            index: 7,
            name: CARD.into(),
            label: "Alder Lake PCH-P High Definition Audio Controller".into(),
            active: SPEAKERS.into(),
            profiles: vec![profile(SPEAKERS), profile(HEADPHONES)],
            ports: vec![
                port(
                    "[Out] Speaker",
                    true,
                    PortAvailable::Unknown,
                    vec![SPEAKERS.into()],
                ),
                port(
                    "[Out] Headphones",
                    true,
                    PortAvailable::Yes,
                    vec![HEADPHONES.into()],
                ),
                port("[Out] HDMI1", true, PortAvailable::Yes, both.clone()),
                port("[Out] HDMI2", true, PortAvailable::Yes, both.clone()),
                port("[Out] HDMI3", true, PortAvailable::No, both.clone()),
                port("[In] Mic1", false, PortAvailable::Unknown, both.clone()),
                port("[In] Mic2", false, PortAvailable::Yes, both),
            ],
        }
    }

    fn output(card: u32, name: &str, port: &str) -> Output {
        Output {
            card: Some(card),
            name: name.into(),
            ports: vec![port.into()],
        }
    }

    fn row(name: &str, port: &str) -> AudioEntry {
        AudioEntry {
            key: format!("sink:{}:port:{}", hex_encode(name), hex_encode(port)),
            kind: AudioKind::Output,
            name: name.into(),
            card: None,
            description: format!("Alder Lake PCH-P — {port}"),
            label: port.trim_start_matches("[Out] ").into(),
            volume: 60,
            muted: false,
            default: true,
            port: Some(port.into()),
        }
    }

    #[test]
    fn both_outputs_remain_visible_with_stable_keys_across_profile_changes() {
        let mut card = card();
        let mut before = vec![row("speaker-sink", "[Out] Speaker")];
        complete_outputs(
            &[card.clone()],
            &[output(7, "speaker-sink", "[Out] Speaker")],
            &mut before,
        );
        let speaker = before.iter().find(|e| e.label == "Speaker").unwrap();
        let headphone = before.iter().find(|e| e.label == "Headphones").unwrap();
        assert!(!speaker.inactive());
        assert!(headphone.inactive());
        assert_eq!(headphone.row_label(), "󰕾   —   Headphones");
        assert!(!headphone.default);
        assert!(require_live_output(headphone).is_err());
        assert!(require_live_output(speaker).is_ok());
        assert!(
            before
                .iter()
                .all(|e| !e.label.contains("Mic") && !e.label.contains("HDMI3"))
        );
        let speaker_key = speaker.key.clone();
        let headphone_key = headphone.key.clone();

        card.active = HEADPHONES.into();
        card.index = 12; // Indexes can change; only card and port names are identity.
        let mut after = vec![row("different-headphone-sink", "[Out] Headphones")];
        complete_outputs(
            &[card],
            &[output(12, "different-headphone-sink", "[Out] Headphones")],
            &mut after,
        );
        let speaker = after.iter().find(|e| e.label == "Speaker").unwrap();
        let headphone = after.iter().find(|e| e.label == "Headphones").unwrap();
        assert_eq!(speaker.key, speaker_key);
        assert_eq!(headphone.key, headphone_key);
        assert!(speaker.inactive());
        assert!(!headphone.inactive());
        assert!(headphone.default);
        let choices: Vec<_> = after
            .into_iter()
            .map(crate::model::ChoiceEntry::route)
            .collect();
        let speaker = choices
            .iter()
            .find(|choice| choice.label == "Speaker")
            .unwrap();
        assert!(speaker.enabled);
        assert!(!speaker.active);
        assert_eq!(speaker.key, speaker_key);
    }

    #[test]
    fn playback_can_activate_speakers_without_setting_the_default() {
        let mut fake = Fake {
            default: "virtual".into(),
            ..Default::default()
        };
        fake.card.active = HEADPHONES.into();
        activate_with_action(&mut fake, CARD, "[Out] Speaker", |backend, output| {
            backend.events.push(format!("route:{output}"));
            Ok(())
        })
        .unwrap();
        assert_eq!(
            fake.events,
            [
                format!("profile:{SPEAKERS}"),
                "port:speaker-sink:[Out] Speaker".into(),
                "route:speaker-sink".into(),
            ]
        );
        assert_eq!(fake.default, "virtual");
    }

    #[test]
    fn playback_to_a_live_port_does_not_set_profile_or_default() {
        let mut fake = Fake::default();
        activate_with_action(&mut fake, CARD, "[Out] Speaker", |backend, output| {
            backend.events.push(format!("route:{output}"));
            Ok(())
        })
        .unwrap();
        assert_eq!(
            fake.events,
            ["port:speaker-sink:[Out] Speaker", "route:speaker-sink"]
        );
    }

    #[test]
    fn failed_playback_move_restores_the_previous_profile() {
        let mut fake = Fake::default();
        let error = activate_with_action(&mut fake, CARD, "[Out] Headphones", |backend, output| {
            backend.events.push(format!("route:{output}"));
            Err(io::Error::other("Stream ended").into())
        })
        .unwrap_err();
        assert!(error.to_string().contains("Stream ended"));
        assert!(error.to_string().contains("Previous profile restored"));
        assert_eq!(fake.card.active, SPEAKERS);
        assert_eq!(fake.default, "speaker-sink");
    }

    #[test]
    fn disconnected_ports_and_unavailable_profiles_are_not_offered() {
        for unavailable_profile in [false, true] {
            let mut card = card();
            if unavailable_profile {
                card.profiles[1].available = false;
            } else {
                card.ports[1].available = PortAvailable::No;
            }
            let mut rows = Vec::new();
            complete_outputs(&[card], &[], &mut rows);
            assert!(!rows.iter().any(|row| row.label == "Headphones"));
        }
    }

    #[test]
    fn profile_selection_preserves_microphones_and_never_changes_bluetooth_codecs() {
        let mut card = card();
        assert_eq!(profile_for(&card, &card.ports[1]), Some(HEADPHONES));
        assert_eq!(profile_for(&card, &card.ports[2]), Some(SPEAKERS));
        card.profiles[1].sources = 0;
        assert_eq!(profile_for(&card, &card.ports[1]), None);
        card.profiles[1].sources = 2;
        card.ports[5].profiles = vec![SPEAKERS.into()];
        assert_eq!(profile_for(&card, &card.ports[1]), None);
        card.name = "bluez_card.test".into();
        assert_eq!(profile_for(&card, &card.ports[0]), None);
        let mut rows = Vec::new();
        complete_outputs(&[card], &[], &mut rows);
        assert!(rows.is_empty());
    }

    #[test]
    fn profiles_retaining_hdmi_ports_win_over_higher_priority_reduced_profiles() {
        let mut card = card();
        let minimal = "headphones-and-mics-only";
        card.profiles.push(Profile {
            name: minimal.into(),
            available: true,
            sinks: 1,
            sources: 2,
            priority: 999,
        });
        for port in &mut card.ports {
            if port.name == "[Out] Headphones" || !port.output {
                port.profiles.push(minimal.into());
            }
        }
        assert_eq!(profile_for(&card, &card.ports[1]), Some(HEADPHONES));
    }

    #[test]
    fn identical_port_names_on_other_cards_never_supply_the_target() {
        assert!(
            find_output(
                vec![output(8, "wrong", "[Out] Headphones")],
                7,
                "[Out] Headphones"
            )
            .unwrap()
            .is_none()
        );
        assert!(
            find_output(
                vec![
                    output(7, "one", "[Out] Headphones"),
                    output(7, "two", "[Out] Headphones")
                ],
                7,
                "[Out] Headphones"
            )
            .is_err()
        );
        assert_ne!(
            key(CARD, "[Out] Speaker"),
            key("alsa_card.other", "[Out] Speaker")
        );
        assert!(key("alsa_card.🎧;x", "[Out] Jack:1").is_ascii());
    }

    struct Fake {
        card: Card,
        events: Vec<String>,
        default: String,
        delay: usize,
        never_appears: bool,
        reject_profile: bool,
        reject_port: bool,
        reject_default: bool,
        external_change: bool,
    }

    impl Default for Fake {
        fn default() -> Self {
            Self {
                card: card(),
                events: Vec::new(),
                default: "speaker-sink".into(),
                delay: 0,
                never_appears: false,
                reject_profile: false,
                reject_port: false,
                reject_default: false,
                external_change: false,
            }
        }
    }

    impl Backend for Fake {
        fn card(&mut self, name: &str) -> AppResult<Card> {
            assert_eq!(name, CARD);
            Ok(self.card.clone())
        }

        fn outputs(&mut self) -> AppResult<Vec<Output>> {
            let mut outputs = vec![Output {
                card: None,
                name: "virtual".into(),
                ports: Vec::new(),
            }];
            if self.card.active == SPEAKERS {
                outputs.push(output(self.card.index, "speaker-sink", "[Out] Speaker"));
            } else if self.card.active == HEADPHONES {
                if self.external_change {
                    self.card.active = "changed-elsewhere".into();
                } else if self.delay > 0 {
                    self.delay -= 1;
                } else if !self.never_appears {
                    outputs.push(output(
                        self.card.index,
                        "new-headphone-sink",
                        "[Out] Headphones",
                    ));
                }
            }
            Ok(outputs)
        }

        fn default_output(&mut self) -> AppResult<Option<String>> {
            Ok(Some(self.default.clone()))
        }

        fn set_profile(&mut self, _: &str, profile: &str) -> AppResult<()> {
            self.events.push(format!("profile:{profile}"));
            if self.reject_profile {
                return Err(io::Error::other("Profile rejected").into());
            }
            self.card.active = profile.into();
            Ok(())
        }

        fn set_port(&mut self, output: &Output, port: &str) -> AppResult<()> {
            self.events.push(format!("port:{}:{port}", output.name));
            if self.reject_port {
                return Err(io::Error::other("Port rejected").into());
            }
            Ok(())
        }

        fn set_default(&mut self, name: &str) -> AppResult<()> {
            self.events.push(format!("default:{name}"));
            if self.reject_default && name == "new-headphone-sink" {
                return Err(io::Error::other("Default rejected").into());
            }
            self.default = name.into();
            Ok(())
        }

        fn pause(&mut self) {}
    }

    #[test]
    fn switching_waits_for_the_new_sink_then_sets_port_before_default() {
        let mut fake = Fake {
            delay: 3,
            ..Default::default()
        };
        activate_with(&mut fake, CARD, "[Out] Headphones").unwrap();
        assert_eq!(
            fake.events,
            [
                format!("profile:{HEADPHONES}"),
                "port:new-headphone-sink:[Out] Headphones".into(),
                "default:new-headphone-sink".into()
            ]
        );
        assert_eq!(fake.default, "new-headphone-sink");
        fake.events.clear();
        activate_with(&mut fake, CARD, "[Out] Speaker").unwrap();
        assert_eq!(
            fake.events,
            [
                format!("profile:{SPEAKERS}"),
                "port:speaker-sink:[Out] Speaker".into(),
                "default:speaker-sink".into()
            ]
        );
    }

    #[test]
    fn live_outputs_do_not_trigger_profile_changes() {
        let mut fake = Fake::default();
        activate_with(&mut fake, CARD, "[Out] Speaker").unwrap();
        assert_eq!(
            fake.events,
            ["port:speaker-sink:[Out] Speaker", "default:speaker-sink"]
        );
    }

    #[test]
    fn unplugged_or_removed_ports_do_not_trigger_any_mutation() {
        let mut fake = Fake::default();
        fake.card.ports[1].available = PortAvailable::No;
        assert!(activate_with(&mut fake, CARD, "[Out] Headphones").is_err());
        assert!(activate_with(&mut fake, CARD, "removed").is_err());
        assert!(fake.events.is_empty());
    }

    #[test]
    fn rejected_profile_never_attempts_port_or_default_changes() {
        let mut fake = Fake {
            reject_profile: true,
            ..Default::default()
        };
        assert!(activate_with(&mut fake, CARD, "[Out] Headphones").is_err());
        assert_eq!(fake.events, [format!("profile:{HEADPHONES}")]);
        assert_eq!(fake.card.active, SPEAKERS);
    }

    #[test]
    fn timeouts_and_rejected_followup_actions_restore_profile_and_default() {
        for (never_appears, reject_port, reject_default) in [
            (true, false, false),
            (false, true, false),
            (false, false, true),
        ] {
            let mut fake = Fake {
                never_appears,
                reject_port,
                reject_default,
                ..Default::default()
            };
            let error = activate_with(&mut fake, CARD, "[Out] Headphones").unwrap_err();
            assert!(error.to_string().contains("Previous profile restored"));
            assert_eq!(fake.card.active, SPEAKERS);
            assert_eq!(fake.default, "speaker-sink");
            assert!(
                fake.events
                    .ends_with(&[format!("profile:{SPEAKERS}"), "default:speaker-sink".into()])
            );
        }
    }

    #[test]
    fn rollback_can_restore_a_virtual_default_without_a_sound_card() {
        let mut fake = Fake {
            default: "virtual".into(),
            never_appears: true,
            ..Default::default()
        };
        let error = activate_with(&mut fake, CARD, "[Out] Headphones").unwrap_err();
        assert!(error.to_string().contains("Previous profile restored"));
        assert_eq!(fake.default, "virtual");
    }

    #[test]
    fn rollback_does_not_overwrite_a_newer_external_profile_choice() {
        let mut fake = Fake {
            external_change: true,
            ..Default::default()
        };
        let error = activate_with(&mut fake, CARD, "[Out] Headphones").unwrap_err();
        assert!(error.to_string().contains("changed elsewhere"));
        assert_eq!(fake.card.active, "changed-elsewhere");
        assert_eq!(fake.events, [format!("profile:{HEADPHONES}")]);
    }
}

mod bluetooth {
    use crate::bluetooth::*;
    use crate::model::CodeKind;

    #[test]
    fn pair_prompts_round_trip_through_the_wire_format() {
        let encoded = encode_request(&PairRequest {
            kind: Some(CodeKind::Passkey),
            address: "AA:BB:CC:DD:EE:FF".to_owned(),
            message: "Please type in the 6-digit passkey for Café 🎧.".to_owned(),
        });
        let request = decode_request(&encoded).unwrap();
        assert_eq!(request.kind, Some(CodeKind::Passkey));
        assert_eq!(request.address, "AA:BB:CC:DD:EE:FF");
        assert_eq!(
            request.message,
            "Please type in the 6-digit passkey for Café 🎧."
        );
    }

    #[test]
    fn display_prompts_carry_no_code_kind_so_they_never_open_an_input_box() {
        let encoded = encode_request(&PairRequest {
            kind: None,
            address: "AA:BB:CC:DD:EE:FF".to_owned(),
            message: "Type 042311 on the device to finish pairing.".to_owned(),
        });
        assert!(encoded.starts_with("display\n"));
        let request = decode_request(&encoded).unwrap();
        assert_eq!(request.kind, None);
        assert_eq!(
            request.message,
            "Type 042311 on the device to finish pairing."
        );
    }

    #[test]
    fn multi_line_prompts_cannot_break_the_record_layout() {
        let encoded = encode_request(&PairRequest {
            kind: Some(CodeKind::Pin),
            address: "AA:BB:CC:DD:EE:FF".to_owned(),
            message: "First line\nSecond line".to_owned(),
        });
        assert_eq!(encoded.lines().count(), 3);
        assert_eq!(
            decode_request(&encoded).unwrap().message,
            "First line\nSecond line"
        );
    }

    #[test]
    fn answers_and_cancellations_are_distinguishable() {
        assert_eq!(
            decode_response(&encode_response(Some("042311"))),
            Some(Some("042311".to_owned()))
        );
        assert_eq!(decode_response(&encode_response(None)), Some(None));
        assert_eq!(decode_response(""), None);
    }

    #[test]
    fn a_scan_window_is_open_until_its_deadline_passes() {
        assert!(window_is_open("1200\n", 1190));
        assert!(!window_is_open("1200\n", 1200));
        assert!(!window_is_open("1200\n", 1300));
    }

    #[test]
    fn an_unreadable_scan_marker_never_reports_as_scanning() {
        // A scanner killed mid-write, or a truncated file, must not leave the
        // menu claiming a scan is running forever.
        assert!(!window_is_open("", 100));
        assert!(!window_is_open("not-a-deadline", 100));
        assert!(!window_is_open("-5", 100));
    }

    #[test]
    fn truncated_prompts_are_rejected_rather_than_half_applied() {
        assert!(decode_request("passkey\n4141\n").is_none());
        assert!(decode_request("passkey\nnot-hex\n4141\n").is_none());
    }
    use crate::bluetooth::battery_provider::*;

    #[test]
    fn bluetooth_addresses_collapse_to_bare_hex() {
        assert_eq!(
            normalize_address("c8:1a:7b:31:09:df").as_deref(),
            Some("C81A7B3109DF")
        );
        assert_eq!(
            normalize_address("C8-1A-7B-31-09-DF").as_deref(),
            Some("C81A7B3109DF")
        );
    }

    #[test]
    fn receiver_serials_are_not_addresses() {
        assert_eq!(normalize_address("4516LGN8"), None);
        assert_eq!(normalize_address(""), None);
    }

    #[test]
    fn level_words_map_to_coarse_percentages() {
        assert_eq!(level_percentage("Full\n"), Some(100));
        assert_eq!(level_percentage("critical"), Some(5));
        assert_eq!(level_percentage("unknown"), None);
    }

    #[test]
    fn battery_paths_sit_under_the_provider_root() {
        let path = battery_path("C81A7B3109DF").unwrap();
        assert_eq!(path.as_str(), "/bt_battery/hidpp/C81A7B3109DF");
    }

    #[test]
    fn only_logitech_hid_devices_are_pre_empted() {
        let hid = vec![HID_SERVICE_UUID.to_owned()];
        assert!(is_logitech_hid(Some("usb:v046DpB023d0015"), &hid));
        // Logitech, but audio: no HID service, no HID++ battery.
        assert!(!is_logitech_hid(Some("usb:v046DpB040d0015"), &[]));
        // HID, but not Logitech.
        assert!(!is_logitech_hid(Some("usb:v8087p0026d0015"), &hid));
        assert!(!is_logitech_hid(None, &hid));
    }
}

mod waybar {
    use crate::model::{AudioEntry, AudioKind};
    use crate::waybar::*;

    fn output(volume: u8, muted: bool) -> AudioEntry {
        AudioEntry {
            key: "sink:00".to_owned(),
            kind: AudioKind::Output,
            name: "alsa_output.pci".to_owned(),
            card: None,
            description: "Built-in Audio".to_owned(),
            label: "Built-in Audio".to_owned(),
            port: None,
            volume,
            muted,
            default: true,
        }
    }

    #[test]
    fn bluetooth_on_and_off_are_different_glyphs() {
        let off = glyph(Some(&output(60, false)), false, false);
        let on = glyph(Some(&output(60, false)), true, false);
        let on_connected = glyph(Some(&output(60, false)), true, true);
        assert_eq!(off, "󰕾");
        assert_eq!(on, "󰂯");
        assert_eq!(on_connected, "󰂱");
        assert_ne!(off, on);
        assert_ne!(on, on_connected);
    }

    #[test]
    fn with_bluetooth_off_the_glyph_tracks_volume_and_mute() {
        assert_eq!(glyph(Some(&output(0, false)), false, false), "󰕿");
        assert_eq!(glyph(Some(&output(30, false)), false, false), "󰖀");
        assert_eq!(glyph(Some(&output(90, false)), false, false), "󰕾");
        assert_eq!(glyph(Some(&output(90, true)), false, false), "󰝟");
        // No default output at all, and Bluetooth off: nothing to report.
        assert_eq!(glyph(None, false, false), "󰝟");
    }
}

mod wayle {
    use crate::model::{AudioEntry, AudioKind};
    use crate::wayle::*;

    fn entry(default: bool) -> AudioEntry {
        AudioEntry {
            key: "card-output:01:port:02".into(),
            kind: AudioKind::Output,
            name: "alsa_output.pci".into(),
            card: Some("alsa_card.pci".into()),
            description: "Alder Lake Controller — Headphones".into(),
            label: "Headphones".into(),
            volume: 50,
            muted: false,
            default,
            port: Some("[Out] Headphones".into()),
        }
    }

    #[test]
    fn bridge_rows_have_fixed_nul_delimited_fields() {
        let mut bytes = Vec::new();
        write_entries(&mut bytes, &[entry(true), entry(false)]).unwrap();
        let mut fields: Vec<_> = bytes.split(|byte| *byte == 0).collect();
        assert_eq!(fields.pop(), Some(&[][..]));
        assert_eq!(fields.len(), 10);
        assert_eq!(fields[0], b"card-output:01:port:02");
        assert_eq!(fields[1], b"Headphones");
        assert_eq!(fields[4], b"1");
        assert_eq!(fields[9], b"0");
    }

    #[test]
    fn only_device_tabs_are_accepted() {
        assert_eq!(kind("output").unwrap(), AudioKind::Output);
        assert_eq!(kind("input").unwrap(), AudioKind::Input);
        assert!(kind("playback").is_err());
    }
}
