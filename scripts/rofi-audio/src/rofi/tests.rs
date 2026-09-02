use crate::model::{AudioEntry, AudioKind, BluetoothEntry, CodeKind, Mode, hex_encode};

use super::mixer::apply_chosen_default;
use super::pairing::{UiState, address_from_key};
use super::render::{write_audio_row, write_bluetooth_row, write_refresh_theme, write_row_option};

#[test]
fn ui_state_round_trips_unicode_messages() {
    let mut state = UiState {
        initialized: true,
        ..Default::default()
    };
    state.set_message("Cannot connect to Café 🎧");
    let decoded = UiState::parse(Some(state.encode()));
    assert!(decoded.initialized);
    assert_eq!(decoded.message, state.message);
}

#[test]
fn ui_state_round_trips_a_pending_pairing_code_prompt() {
    let state = UiState {
        initialized: true,
        message: Some("Please type in the 6-digit passkey for Café 🎧.".to_owned()),
        pending_connect: Some("bt:4141".to_owned()),
        code_for: Some("bt:4141".to_owned()),
        code_kind: Some(CodeKind::Passkey),
        ..Default::default()
    };
    let decoded = UiState::parse(Some(state.encode()));
    assert_eq!(decoded.message, state.message);
    assert_eq!(decoded.pending_connect, state.pending_connect);
    assert_eq!(decoded.code_for, state.code_for);
    assert_eq!(decoded.code_kind, Some(CodeKind::Passkey));
}

#[test]
fn finished_connects_drop_the_pending_and_await_state() {
    let mut state = UiState {
        initialized: true,
        message: None,
        pending_connect: Some("bt:4141".to_owned()),
        code_for: Some("bt:4141".to_owned()),
        code_kind: Some(CodeKind::Pin),
        ..Default::default()
    };
    state.pending_connect = None;
    state.clear_code_prompt();
    let encoded = state.encode();
    assert!(!encoded.contains("pending="));
    assert!(!encoded.contains("await="));
    assert!(!encoded.contains("kind="));
}

/// Rofi arms the idle timer from the delay left by the *previous* render
/// (view.c calls `rofi_view_set_user_timeout` at the top of
/// `rofi_view_trigger_action`, before dispatching), and only when that
/// delay is greater than zero. So a tab that renders 0 disarms the tick for
/// the tab the user switches to next — which is how a device added on the
/// Bluetooth tab stayed missing from Output and Input.
#[test]
fn every_tab_leaves_the_next_one_a_running_refresh_tick() {
    for mode in [Mode::Bluetooth, Mode::Output, Mode::Input, Mode::Playback] {
        for pending in [None, Some("bt:4141".to_owned())] {
            let state = UiState {
                pending_connect: pending,
                ..Default::default()
            };
            let mut output = Vec::new();
            write_refresh_theme(&mut output, mode, &state);
            let rendered = String::from_utf8_lossy(&output).into_owned();
            assert!(
                !rendered.contains("delay: 0;"),
                "{mode} renders a disarmed tick: {rendered}"
            );
        }
    }
}

#[test]
fn every_tab_uses_compact_single_line_rows() {
    let render = |mode| {
        let mut output = Vec::new();
        write_refresh_theme(&mut output, mode, &UiState::default());
        String::from_utf8_lossy(&output).into_owned()
    };

    for mode in [Mode::Bluetooth, Mode::Output, Mode::Input, Mode::Playback] {
        assert!(render(mode).contains("configuration { eh: 1;"));
    }
}

#[test]
fn device_keys_survive_the_round_trip_to_a_bluez_address() {
    let key = format!("bt:{}", hex_encode("AA:BB:CC:DD:EE:FF"));
    assert_eq!(address_from_key(&key).as_deref(), Some("AA:BB:CC:DD:EE:FF"));
    assert_eq!(address_from_key("sink:4141"), None);
}

#[test]
fn script_rows_use_one_metadata_nul() {
    let mut row = b"key".to_vec();
    let mut first = true;
    write_row_option(&mut row, &mut first, "display", "Visible");
    write_row_option(&mut row, &mut first, "info", "key");
    assert_eq!(row.iter().filter(|byte| **byte == 0).count(), 1);
}

/// The `prompt` header names the mode-switcher tab, so it has to carry the
/// tab's text. The input bar hides the prompt widget instead of shortening
/// this string — see the `inputbar` block in rofi-audio.rasi.
#[test]
fn prompts_name_their_tab() {
    assert_eq!(Mode::Bluetooth.prompt(), "󰂯 Bluetooth");
    assert_eq!(Mode::Output.prompt(), "󰕾 Output");
    assert_eq!(Mode::Input.prompt(), "󰍬 Input");
    assert_eq!(Mode::Playback.prompt(), "󰐊 Playback");
}

#[test]
fn picker_state_round_trips_without_persisting_selection_overrides() {
    use crate::model::Picker;
    for picker in [
        Picker::Route("playback:7:3:123".into()),
        Picker::Route("playback:cafésemi;colon".into()),
    ] {
        let state = UiState {
            picker: Some(picker.clone()),
            selection: Some("chosen".into()),
            ..Default::default()
        };
        let decoded = UiState::parse(Some(state.encode()));
        assert_eq!(decoded.picker, Some(picker));
        assert!(decoded.selection.is_none());
    }
    assert!(
        UiState::parse(Some("route=not-hex".into()))
            .picker
            .is_none()
    );
    assert!(
        UiState::parse(Some(format!("port={}", hex_encode("source:old"))))
            .picker
            .is_none()
    );
}

#[test]
fn empty_audio_tabs_keep_refresh_and_hotkeys_alive() {
    use super::render::render_output;
    use crate::model::Devices;
    for mode in [Mode::Output, Mode::Input, Mode::Playback] {
        let rows = if mode.is_stream() {
            Devices::Streams(Vec::new())
        } else {
            Devices::Audio(Vec::new())
        };
        let rendered =
            String::from_utf8(render_output(mode, UiState::default(), None, &rows).unwrap())
                .unwrap();
        assert!(rendered.contains("no-custom\x1ffalse"));
        assert!(rendered.contains("use-hot-keys\x1ftrue"));
        assert!(rendered.contains("nonselectable\x1ftrue"));
        assert!(rendered.contains("keep-filter\x1ffalse"));
    }
}

#[test]
fn audio_messages_are_hidden_when_idle_but_errors_are_escaped_and_visible() {
    use super::render::render_output;
    use crate::model::Devices;
    let rows = Devices::Audio(Vec::new());
    let idle = render_output(Mode::Output, UiState::default(), None, &rows).unwrap();
    assert!(String::from_utf8_lossy(&idle).contains("message\x1f\x1e"));
    let state = UiState {
        message: Some("A&B <microphone> failed".into()),
        ..Default::default()
    };
    let rendered = render_output(Mode::Input, state, None, &rows).unwrap();
    assert!(String::from_utf8_lossy(&rendered).contains("A&amp;B &lt;microphone&gt; failed"));
}

#[test]
fn every_tab_escapes_and_prevents_wrapping_of_multiline_messages() {
    use super::render::render_output;
    use crate::model::Devices;
    for mode in [Mode::Bluetooth, Mode::Output, Mode::Input, Mode::Playback] {
        let rows = match mode {
            Mode::Bluetooth => Devices::Bluetooth(Vec::new()),
            Mode::Playback => Devices::Streams(Vec::new()),
            _ => Devices::Audio(Vec::new()),
        };
        let state = UiState {
            message: Some("<b>A&B</b>\r\n第二行\u{2028}Third line".into()),
            ..Default::default()
        };
        let output = String::from_utf8(render_output(mode, state, None, &rows).unwrap()).unwrap();
        let message = output
            .split('\x1e')
            .find_map(|record| record.strip_prefix("\0message\x1f"))
            .unwrap();
        assert_eq!(
            message,
            "<span allow_breaks=\"false\">&lt;b&gt;A&amp;B&lt;/b&gt; 第二行 Third line</span>"
        );
        let empty =
            String::from_utf8(render_output(mode, UiState::default(), None, &rows).unwrap())
                .unwrap();
        assert!(empty.contains("\0message\x1f\x1e"));
    }
}

#[test]
fn long_wide_route_prompts_remain_bounded_and_cannot_wrap() {
    use super::render::render_output;
    use crate::model::{ChoiceList, Devices};
    let rows = Devices::Choices(ChoiceList {
        title: "界".repeat(100),
        entries: Vec::new(),
    });
    let output =
        String::from_utf8(render_output(Mode::Playback, UiState::default(), None, &rows).unwrap())
            .unwrap();
    let message = output
        .split('\x1e')
        .find_map(|record| record.strip_prefix("\0message\x1f"))
        .unwrap();
    assert_eq!(
        message,
        format!("<span allow_breaks=\"false\">{}…</span>", "界".repeat(53))
    );
}

#[test]
fn route_picker_back_is_permanent_and_disabled_choices_cannot_be_activated() {
    use super::render::render_output;
    use crate::model::{ChoiceEntry, ChoiceList, Devices};
    let rows = Devices::Choices(ChoiceList {
        title: "Output for Firefox".into(),
        entries: vec![
            ChoiceEntry {
                key: "sink:00".into(),
                label: "Speakers".into(),
                description: "Built-in Audio Speakers".into(),
                active: true,
                enabled: true,
            },
            ChoiceEntry {
                key: "sink:01".into(),
                label: "Headphones (unplugged)".into(),
                description: "Built-in Audio Headphones (unplugged)".into(),
                active: false,
                enabled: false,
            },
        ],
    });
    let state = UiState {
        selection: Some("sink:00".into()),
        ..Default::default()
    };
    let bytes = render_output(Mode::Playback, state, Some("stream"), &rows).unwrap();
    let rendered = String::from_utf8_lossy(&bytes);
    assert!(rendered.contains("new-selection\x1f1"));
    let back = rendered
        .split('\x1e')
        .find(|r| r.starts_with("back\0"))
        .unwrap();
    assert!(back.contains("permanent\x1ftrue"));
    let unplugged = rendered
        .split('\x1e')
        .find(|r| r.starts_with("sink:01\0"))
        .unwrap();
    assert!(unplugged.contains("nonselectable\x1ftrue"));
}

#[test]
fn playback_picker_displays_compact_names_but_keeps_full_search_metadata() {
    use super::render::render_output;
    use crate::model::{AudioEntry, AudioKind, ChoiceEntry, ChoiceList, Devices, hex_encode};
    let choices = ChoiceList {
        title: "Output for Google Chrome".into(),
        entries: ["Speakers", "HDMI / DisplayPort 1", "HDMI / DisplayPort 2"]
            .into_iter()
            .enumerate()
            .map(|(index, label)| {
                let name = format!("alsa_output.device{index}");
                ChoiceEntry::route(
                    AudioEntry {
                        key: format!("sink:{}", hex_encode(&name)),
                        kind: AudioKind::Output,
                        name,
                        description: format!(
                            "Alder Lake PCH-P High Definition Audio Controller {label}"
                        ),
                        label: label.into(),
                        volume: 65,
                        muted: false,
                        default: index == 0,
                        port: None,
                    },
                    "alsa_output.device1",
                )
            })
            .collect(),
    };
    assert_eq!(choices.entries.len(), 3);
    assert!(!choices.entries[0].active);
    assert!(choices.entries[1].active);
    assert!(!choices.entries[2].active);
    let devices = Devices::Choices(choices.clone());
    let output = String::from_utf8(
        render_output(Mode::Playback, UiState::default(), None, &devices).unwrap(),
    )
    .unwrap();
    for choice in &choices.entries {
        let record = output
            .split('\x1e')
            .find(|record| record.starts_with(&format!("{}\0", choice.key)))
            .unwrap();
        let marker = if choice.active { "✓ " } else { "" };
        assert!(record.contains(&format!("display\x1f{marker}{}\x1f", choice.label)));
        assert!(record.contains(&format!("meta\x1f{}", choice.description)));
        assert_eq!(record.contains("urgent\x1ftrue"), choice.active);
        assert!(!choice.key.contains(":port:"));
    }
}

#[test]
fn stream_rows_keep_full_search_metadata_and_single_line_labels() {
    use super::render::write_stream_row;
    use crate::model::StreamEntry;
    let mut entry = StreamEntry {
        key: "playback:7:3:123".into(),
        index: 7,
        application: "Firefox".into(),
        name: "A long title\nwith two lines".into(),
        device_name: "alsa_output.test".into(),
        device_label: "USB headphones".into(),
        volume: Some(125),
        muted: false,
        corked: false,
    };
    assert!(!entry.row_label().contains('\n'));
    assert!(entry.row_label().contains("125%"));
    assert!(
        entry
            .row_label()
            .contains("Firefox: A long title with two lines")
    );
    assert!(!entry.row_label().contains('→'));
    assert!(!entry.row_label().contains("USB headphones"));
    for (muted, corked) in [(false, false), (true, false), (false, true)] {
        entry.muted = muted;
        entry.corked = corked;
        let mut output = Vec::new();
        write_stream_row(&mut output, &entry).unwrap();
        let row = String::from_utf8_lossy(&output);
        assert!(row.contains("alsa_output.test"));
        assert!(row.contains("USB headphones"));
        assert_eq!(row.contains("active\x1ftrue"), !muted && !corked);
        assert_eq!(output.iter().filter(|b| **b == 0).count(), 1);
    }
    entry.volume = None;
    assert!(entry.row_label().contains('—'));
}

#[test]
fn generic_playback_titles_are_hidden_without_changing_stream_identity() {
    use super::render::write_stream_row;
    use crate::model::StreamEntry;
    let mut entry = StreamEntry {
        key: "playback:7:3:123".into(),
        index: 7,
        application: "Google Chrome".into(),
        name: "Playback".into(),
        device_name: "alsa_output.test".into(),
        device_label: "Alder Lake PCH-P Speaker".into(),
        volume: Some(100),
        muted: false,
        corked: false,
    };
    for name in [
        "Playback",
        " playback ",
        "",
        "Google Chrome",
        "google chrome",
    ] {
        entry.name = name.into();
        assert_eq!(entry.row_label(), "󰐊 100%  Google Chrome");
        let mut output = Vec::new();
        write_stream_row(&mut output, &entry).unwrap();
        let output = String::from_utf8(output).unwrap();
        assert!(output.starts_with("playback:7:3:123\0"));
        assert!(output.contains("Alder Lake PCH-P Speaker"));
    }
    // Actual titles still distinguish multiple streams belonging to one app.
    entry.name = "Concert recording".into();
    assert_eq!(
        entry.row_label(),
        "󰐊 100%  Google Chrome: Concert recording"
    );
}

#[test]
fn connected_devices_render_urgent_and_paired_ones_render_active() {
    let entry = |connected, paired| BluetoothEntry {
        key: "bt:4141".to_owned(),
        address: "AA:BB:CC:DD:EE:FF".to_owned(),
        name: "WH-1000XM4".to_owned(),
        named: true,
        icon: None,
        connected,
        paired,
        battery: None,
    };
    let render = |entry: BluetoothEntry| {
        let mut output = Vec::new();
        write_bluetooth_row(&mut output, &entry).unwrap();
        String::from_utf8_lossy(&output).into_owned()
    };
    assert!(render(entry(true, true)).contains("urgent"));
    assert!(render(entry(false, true)).contains("active"));
    let discovered = render(entry(false, false));
    assert!(!discovered.contains("urgent") && !discovered.contains("active"));
}

#[test]
fn activating_a_device_marks_it_default_even_if_the_server_lags() {
    let entry = |name: &str, default| AudioEntry {
        key: format!("sink:{}", hex_encode(name)),
        kind: AudioKind::Output,
        name: name.to_owned(),
        description: name.to_owned(),
        label: name.to_owned(),
        port: None,
        volume: 50,
        muted: false,
        default,
    };
    // pipewire-pulse can still report the old default right after
    // accepting the change, so both rows come back stale.
    let mut entries = vec![entry("speakers", true), entry("headset", false)];
    apply_chosen_default(&mut entries, &format!("sink:{}", hex_encode("headset")));
    assert!(!entries[0].default);
    assert!(entries[1].default);
}

#[test]
fn selecting_a_port_marks_only_that_row_default_even_when_ports_share_a_device() {
    let speakers = AudioEntry {
        key: "sink:00:port:01".into(),
        kind: AudioKind::Output,
        name: "built-in".into(),
        description: "Built-in Audio — Speakers".into(),
        label: "Built-in Audio — Speakers".into(),
        port: Some("speakers".into()),
        volume: 60,
        muted: false,
        default: true,
    };
    let headphones = AudioEntry {
        key: "sink:00:port:02".into(),
        port: Some("headphones".into()),
        description: "Built-in Audio — Headphones".into(),
        label: "Built-in Audio — Headphones".into(),
        default: false,
        ..speakers.clone()
    };
    let mut entries = vec![speakers, headphones];
    apply_chosen_default(&mut entries, "sink:00:port:02");
    assert!(!entries[0].default);
    assert!(entries[1].default);
    let mut rendered = Vec::new();
    write_audio_row(&mut rendered, &entries[1]).unwrap();
    let row = String::from_utf8_lossy(&rendered);
    assert!(row.contains("Built-in Audio — Headphones"));
    assert!(row.contains("headphones"));
    assert!(row.contains("urgent\x1ftrue"));
}

#[test]
fn the_default_audio_device_renders_urgent_and_the_rest_render_active() {
    let entry = |default| AudioEntry {
        key: "sink:4141".to_owned(),
        kind: AudioKind::Output,
        name: "alsa_output.pci".to_owned(),
        description: "Built-in Audio Analog Stereo".to_owned(),
        label: "Built-in Audio".to_owned(),
        port: None,
        volume: 60,
        muted: false,
        default,
    };
    let render = |entry: AudioEntry| {
        let mut output = Vec::new();
        write_audio_row(&mut output, &entry).unwrap();
        String::from_utf8_lossy(&output).into_owned()
    };
    assert!(render(entry(true)).contains("urgent"));
    assert!(render(entry(false)).contains("active"));
}
