use std::io::{self, Write};

use crate::AppResult;
use crate::model::{AudioEntry, BluetoothEntry, Devices, Mode, single_line};

use super::UiState;

const RECORD_SEPARATOR: u8 = 0x1e;
const UNIT_SEPARATOR: u8 = 0x1f;
/// Characters retained for the message panel. Rofi wraps them across several
/// visual lines; the limit only stops an unusually long D-Bus error from
/// taking over the whole menu.
const MESSAGE_MAX_CHARS: usize = 120;
const REFRESH_ACTION: &str = "kb-custom-5";
/// Idle seconds between refresh ticks. Must never be 0 on any tab — see
/// `write_refresh_theme`.
const REFRESH_DELAY: u8 = 2;
/// A pending connect wants its result sooner than the steady-state tick.
const PENDING_REFRESH_DELAY: u8 = 1;

pub(super) fn render(
    mode: Mode,
    mut state: UiState,
    selected_key: Option<&str>,
    devices: &Devices,
) -> AppResult<()> {
    let selected_row = selected_key
        .and_then(|key| devices.position(key))
        .or_else(|| (!devices.is_empty()).then_some(0));
    let message = match mode {
        Mode::Bluetooth => {
            let selected_message = selected_row.and_then(|index| devices.message_label(index));
            let raw = state
                .message
                .clone()
                .or(selected_message)
                .unwrap_or_default();
            // Flatten backend-inserted line breaks, then let Rofi wrap the
            // message naturally inside the panel.
            single_line(&raw, MESSAGE_MAX_CHARS)
        }
        Mode::Output | Mode::Input => String::new(),
    };

    let mut output = Vec::new();
    write_headers(&mut output, mode, &mut state, &message, selected_row);
    match devices {
        Devices::Bluetooth(entries) if entries.is_empty() => {
            write_empty_row(&mut output, empty_label(mode))?;
        }
        Devices::Bluetooth(entries) => {
            for entry in entries {
                write_bluetooth_row(&mut output, entry)?;
            }
        }
        Devices::Audio(entries) if entries.is_empty() => {
            write_empty_row(&mut output, empty_label(mode))?;
        }
        Devices::Audio(entries) => {
            for entry in entries {
                write_audio_row(&mut output, entry)?;
            }
        }
    }
    io::stdout().write_all(&output)?;
    Ok(())
}

fn empty_label(mode: Mode) -> &'static str {
    match mode {
        Mode::Bluetooth => "No Bluetooth devices found",
        Mode::Output => "No audio outputs found",
        Mode::Input => "No audio inputs found",
    }
}

fn write_headers(
    output: &mut Vec<u8>,
    mode: Mode,
    state: &mut UiState,
    message: &str,
    selected_row: Option<usize>,
) {
    if !state.initialized {
        output.push(0);
        output.extend_from_slice(b"delim");
        output.push(UNIT_SEPARATOR);
        output.push(RECORD_SEPARATOR);
        output.push(b'\n');
        state.initialized = true;
    }
    write_header(output, "prompt", mode.prompt());
    write_header(output, "message", message);
    // While a pairing code is awaited the filter box is the code entry; allow
    // custom input so Enter submits the typed text (Rofi custom-input, RETV=2).
    write_header(
        output,
        "no-custom",
        if state.code_for.is_some() {
            "false"
        } else {
            "true"
        },
    );
    write_header(output, "use-hot-keys", "true");
    write_header(output, "keep-selection", "true");
    write_refresh_theme(output, mode, state);
    if let Some(selected_row) = selected_row {
        write_header(output, "new-selection", &selected_row.to_string());
    }
    write_header(output, "data", &state.encode());
}

/// Arm Rofi's idle `timeout` action, the only hook a script mode has to refresh
/// itself without user input. Device lists go stale the moment a Bluetooth
/// headset finishes connecting (its PulseAudio sink appears 1–3s after BlueZ)
/// and Rofi never re-runs a script mode on its own, so without this tick a
/// newly connected device stays invisible until the menu is reopened.
///
/// Every tab must render a *non-zero* delay, including the ones with nothing of
/// their own to refresh. Rofi reads this delay in `rofi_view_set_user_timeout`,
/// which runs in exactly two places (view.c): once in `rofi_view_create`, and
/// at the *top* of `rofi_view_trigger_action`, before the action is dispatched
/// — so the value that arms the timer is the one the previous render left
/// behind, and it only arms at all when that value is greater than zero.
///
/// A tab rendering 0 therefore does not merely skip its own tick: it disarms
/// the timer for whichever tab the user opens next. That is what kept a device
/// added on the Bluetooth tab from appearing after a switch to Output or Input
/// — the switch armed from Bluetooth's 0, and the delay Output then wrote was
/// never read again, because nothing re-reads it until the next user action.
pub(super) fn write_refresh_theme(output: &mut Vec<u8>, mode: Mode, state: &UiState) {
    let delay = match mode {
        Mode::Bluetooth if state.pending_connect.is_some() => PENDING_REFRESH_DELAY,
        _ => REFRESH_DELAY,
    };
    let row_height = match mode {
        Mode::Bluetooth => 1,
        Mode::Output | Mode::Input => 2,
    };
    write_header(
        output,
        "theme",
        &format!(
            "configuration {{ eh: {row_height}; \
             timeout {{ delay: {delay}; action: \"{REFRESH_ACTION}\"; }} }}"
        ),
    );
}

pub(super) fn write_bluetooth_row(output: &mut Vec<u8>, entry: &BluetoothEntry) -> io::Result<()> {
    write!(output, "{}", entry.key)?;
    let mut first = true;
    write_row_option(output, &mut first, "display", &entry.row_label());
    write_row_option(output, &mut first, "info", &entry.key);
    write_row_option(
        output,
        &mut first,
        "meta",
        &format!("{} {}", entry.name, entry.address),
    );
    // urgent renders cyan and active renders white in rofi-audio.rasi;
    // everything else stays dim.
    if entry.connected {
        write_row_option(output, &mut first, "urgent", "true");
    } else if entry.paired {
        write_row_option(output, &mut first, "active", "true");
    }
    output.push(RECORD_SEPARATOR);
    Ok(())
}

pub(super) fn write_audio_row(output: &mut Vec<u8>, entry: &AudioEntry) -> io::Result<()> {
    write!(output, "{}", entry.key)?;
    let mut first = true;
    write_row_option(output, &mut first, "display", &entry.row_label());
    write_row_option(output, &mut first, "info", &entry.key);
    write_row_option(
        output,
        &mut first,
        "meta",
        &format!("{} {}", entry.description, entry.name),
    );
    if entry.default {
        write_row_option(output, &mut first, "urgent", "true");
    } else {
        write_row_option(output, &mut first, "active", "true");
    }
    output.push(RECORD_SEPARATOR);
    Ok(())
}

fn write_empty_row(output: &mut Vec<u8>, label: &str) -> io::Result<()> {
    write!(output, "empty")?;
    let mut first = true;
    write_row_option(output, &mut first, "display", label);
    write_row_option(output, &mut first, "nonselectable", "true");
    write_row_option(output, &mut first, "permanent", "true");
    output.push(RECORD_SEPARATOR);
    Ok(())
}

fn write_header(output: &mut Vec<u8>, key: &str, value: &str) {
    output.push(0);
    output.extend_from_slice(key.as_bytes());
    output.push(UNIT_SEPARATOR);
    output.extend_from_slice(sanitize_record_value(value).as_bytes());
    output.push(RECORD_SEPARATOR);
}

pub(super) fn write_row_option(output: &mut Vec<u8>, first: &mut bool, key: &str, value: &str) {
    output.push(if *first { 0 } else { UNIT_SEPARATOR });
    *first = false;
    output.extend_from_slice(key.as_bytes());
    output.push(UNIT_SEPARATOR);
    output.extend_from_slice(sanitize_option_value(value).as_bytes());
}

fn sanitize_record_value(value: &str) -> String {
    value
        .replace('\0', "␀")
        .replace(char::from(RECORD_SEPARATOR), "\n")
}

fn sanitize_option_value(value: &str) -> String {
    sanitize_record_value(value).replace(char::from(UNIT_SEPARATOR), " ")
}
