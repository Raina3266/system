use crate::model::{AudioEntry, AudioKind, BluetoothEntry, CodeKind, Mode, hex_encode};

use super::apply_chosen_default;
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
    for mode in [Mode::Bluetooth, Mode::Output, Mode::Input] {
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
        volume: 50,
        muted: false,
        default,
    };
    // pipewire-pulse can still report the old default right after
    // accepting the change, so both rows come back stale.
    let mut entries = vec![entry("speakers", true), entry("headset", false)];
    apply_chosen_default(&mut entries, "headset");
    assert!(!entries[0].default);
    assert!(entries[1].default);
}

#[test]
fn the_default_audio_device_renders_urgent_and_the_rest_render_active() {
    let entry = |default| AudioEntry {
        key: "sink:4141".to_owned(),
        kind: AudioKind::Output,
        name: "alsa_output.pci".to_owned(),
        description: "Built-in Audio Analog Stereo".to_owned(),
        label: "Built-in Audio".to_owned(),
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
