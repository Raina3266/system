use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use crate::bluetooth::Backend;
use crate::model::{
    AudioEntry, BluetoothEntry, CodeKind, Devices, Mode, hex_decode, hex_encode, single_line,
};
use crate::{AppResult, audio, bluetooth};

const RECORD_SEPARATOR: u8 = 0x1e;
const UNIT_SEPARATOR: u8 = 0x1f;
const PRESERVE_SELECTION_ENV: &str = "ROFI_PRESERVE_SELECTION_ON_FILTER";
const CONNECT_RESULT_FILENAME: &str = "rofi-audio-connect-result";
/// How long a script invocation waits for a detached connect before handing
/// control back to Rofi. Long enough for most pairings, short enough that the
/// menu never feels frozen.
const CONNECT_POLL_ATTEMPTS: u32 = 60;
const CONNECT_POLL_INTERVAL: Duration = Duration::from_millis(100);
/// Characters the single-line message panel can show at the window's width.
const MESSAGE_WIDTH: usize = 46;

/// Rofi maps `-kb-custom-N` to `ROFI_RETV` 9 + N. The six action-bar buttons
/// are wired to these in rofi-audio.rasi.
const RETV_ACTIVATE: u8 = 1;
const RETV_CUSTOM_INPUT: u8 = 2;
const RETV_SCAN: u8 = 10;
const RETV_CONNECT: u8 = 11;
const RETV_FORGET: u8 = 12;
const RETV_VOLUME_UP: u8 = 13;
const RETV_VOLUME_DOWN: u8 = 14;
const RETV_CONFIRM: u8 = 15;

#[derive(Clone, Debug, Default)]
struct UiState {
    initialized: bool,
    message: Option<String>,
    /// Row key whose connection is being performed by a detached `connect-bg`
    /// subprocess. While set, the subprocess owns the outcome and writes it to
    /// $XDG_RUNTIME_DIR/rofi-audio-connect-result.
    pending_connect: Option<String>,
    /// Row key we are waiting for the user to type a pairing code for. When
    /// set, the filter input becomes the code entry box: the typed text is
    /// submitted with Enter (Rofi custom-input, RETV=2) and handed to the
    /// pairing agent instead of opening a nested Rofi dialog.
    code_for: Option<String>,
    code_kind: Option<CodeKind>,
}

impl UiState {
    fn parse(value: Option<String>) -> Self {
        let Some(value) = value else {
            return Self::default();
        };
        let mut state = Self::default();
        for part in value.split(';') {
            if part == "init=1" {
                state.initialized = true;
            } else if let Some(message) = part.strip_prefix("msg=") {
                state.message = hex_decode(message);
            } else if let Some(pending) = part.strip_prefix("pending=") {
                state.pending_connect = hex_decode(pending);
            } else if let Some(awaiting) = part.strip_prefix("await=") {
                state.code_for = hex_decode(awaiting);
            } else if let Some(kind) = part.strip_prefix("kind=") {
                state.code_kind = kind.parse().ok();
            }
        }
        state
    }

    fn encode(&self) -> String {
        let mut parts = Vec::with_capacity(5);
        parts.push(format!("init={}", u8::from(self.initialized)));
        if let Some(message) = self.message.as_deref() {
            parts.push(format!("msg={}", hex_encode(message)));
        }
        if let Some(pending) = self.pending_connect.as_deref() {
            parts.push(format!("pending={}", hex_encode(pending)));
        }
        if let Some(awaiting) = self.code_for.as_deref() {
            parts.push(format!("await={}", hex_encode(awaiting)));
        }
        if let Some(kind) = self.code_kind {
            parts.push(format!("kind={}", kind.name()));
        }
        parts.join(";")
    }

    fn set_message(&mut self, message: impl Into<String>) {
        self.message = Some(message.into());
    }

    fn clear_code_prompt(&mut self) {
        self.code_for = None;
        self.code_kind = None;
    }
}

pub fn launch() -> AppResult<()> {
    // A pairing abandoned by closing the menu can leave a prompt behind; drop
    // it so this launch never opens straight into a stale code box.
    bluetooth::cleanup();
    clear_connect_result();
    let executable = env::current_exe()?;
    let executable = executable.to_string_lossy();
    let modes = format!(
        "bluetooth:{executable} script bluetooth,\
         output:{executable} script output,\
         input:{executable} script input"
    );
    let status = Command::new(rofi_binary())
        .env(PRESERVE_SELECTION_ENV, "true")
        .args([
            "-show",
            "bluetooth",
            "-modes",
            &modes,
            // Same strings the script's `prompt` header sets, so a tab reads
            // the same before and after its script has first run.
            "-display-bluetooth",
            "󰂯 Bluetooth",
            "-display-output",
            "󰕾 Output",
            "-display-input",
            "󰍬 Input",
            // Alt+1..Alt+6 are Rofi's defaults for the action-bar buttons;
            // volume also answers to Alt+Up/Alt+Down, which nothing else uses.
            "-kb-custom-4",
            "Alt+4,Alt+Up",
            "-kb-custom-5",
            "Alt+5,Alt+Down",
            "-theme",
        ])
        .arg(theme_path()?)
        .status()?;
    if !status.success() && status.code() != Some(1) {
        return Err(io::Error::other(format!("rofi exited with {status}")).into());
    }
    Ok(())
}

pub async fn run_script(mode: Mode) -> AppResult<()> {
    let retv = env::var("ROFI_RETV")
        .ok()
        .and_then(|value| value.parse::<u8>().ok())
        .unwrap_or(0);
    let selected_key = env::var("ROFI_INFO").ok();
    let mut state = UiState::parse(env::var("ROFI_DATA").ok());
    // Feedback belongs to the action that produced it. Rofi only re-runs the
    // script on an action, so clearing here keeps a stale "Connected to …"
    // from outliving the keypress that caused it.
    if retv != 0 {
        state.message = None;
    }
    // Pull anything the detached connect left behind before acting, so a
    // finished pairing or a pending code prompt is visible right away.
    consume_connect_result(&mut state);
    consume_pair_request(&mut state);

    let devices = match mode.audio_kind() {
        Some(kind) => run_audio(kind, retv, selected_key.as_deref(), &mut state),
        None => run_bluetooth(retv, selected_key.as_deref(), &mut state).await,
    };
    render(mode, state, selected_key.as_deref(), &devices)
}

// ---------------------------------------------------------------------------
// Bluetooth tab
// ---------------------------------------------------------------------------

async fn run_bluetooth(retv: u8, selected_key: Option<&str>, state: &mut UiState) -> Devices {
    let backend = match Backend::new().await {
        Ok(backend) => backend,
        Err(error) => {
            // The raw D-Bus error runs to several lines; the panel has one.
            eprintln!("rofi-audio: cannot reach BlueZ: {error}");
            state.set_message("Bluetooth service is unavailable.");
            return Devices::Bluetooth(Vec::new());
        }
    };
    // Only the actions that operate on the highlighted row need to know the
    // state before they run. Listing devices costs a D-Bus round trip per
    // device, so scan and the plain redraws skip straight to the final list.
    let acts_on_row = matches!(
        retv,
        RETV_ACTIVATE | RETV_CONNECT | RETV_CONFIRM | RETV_FORGET
    );
    let before = if acts_on_row {
        match backend.snapshot().await {
            Ok(entries) => Devices::Bluetooth(entries),
            Err(error) => {
                state.set_message(format!("Cannot list Bluetooth devices: {error}"));
                return Devices::Bluetooth(Vec::new());
            }
        }
    } else {
        Devices::Bluetooth(Vec::new())
    };

    match retv {
        RETV_CUSTOM_INPUT if state.code_for.is_some() => submit_code(state),
        RETV_ACTIVATE | RETV_CONNECT | RETV_CONFIRM => {
            // Enter on the row we are waiting on means "submit the code I just
            // typed"; anything else drops the prompt and acts on the new row.
            if state.code_for.is_some() && state.code_for.as_deref() == selected_key {
                submit_code(state);
            } else {
                abandon_code_prompt(state);
                activate_bluetooth(&backend, &before, selected_key, state).await;
            }
        }
        RETV_SCAN => {
            abandon_code_prompt(state);
            scan(&backend, state).await;
        }
        RETV_FORGET => {
            abandon_code_prompt(state);
            forget(&backend, &before, selected_key, state).await;
        }
        RETV_VOLUME_UP | RETV_VOLUME_DOWN => {
            state.set_message("Volume applies to the Output and Input tabs.");
        }
        _ => {}
    }

    match backend.snapshot().await {
        Ok(entries) => Devices::Bluetooth(entries),
        Err(error) => {
            state.set_message(format!("Cannot list Bluetooth devices: {error}"));
            Devices::Bluetooth(Vec::new())
        }
    }
}

async fn activate_bluetooth(
    backend: &Backend,
    before: &Devices,
    selected_key: Option<&str>,
    state: &mut UiState,
) {
    let Some(entry) = selected_key.and_then(|key| before.bluetooth(key)) else {
        state.set_message("Select a Bluetooth device first.");
        return;
    };
    if entry.connected {
        match backend.disconnect(entry).await {
            Ok(()) => state.set_message(format!("Disconnected from {}.", entry.name)),
            Err(error) => {
                state.set_message(format!("Cannot disconnect {}: {error}", entry.name));
            }
        }
        return;
    }
    if let Err(error) = backend.power_on().await {
        state.set_message(format!("Cannot power on the Bluetooth adapter: {error}"));
        return;
    }
    // Detached connect, so Rofi can paint "Connecting…" immediately and the
    // pairing agent can outlive this invocation while the user types a code.
    spawn_connect_background(&entry.key);
    state.pending_connect = Some(entry.key.clone());
    state.set_message(if entry.paired {
        format!("Connecting to {}…", entry.name)
    } else {
        format!("Pairing with {}…", entry.name)
    });
    wait_for_connect(state);
}

async fn scan(backend: &Backend, state: &mut UiState) {
    if let Err(error) = backend.power_on().await {
        state.set_message(format!("Cannot power on the Bluetooth adapter: {error}"));
        return;
    }
    match backend.scan().await {
        Ok(()) => state.set_message("Scan complete."),
        Err(error) => state.set_message(format!("Cannot scan for devices: {error}")),
    }
}

async fn forget(
    backend: &Backend,
    before: &Devices,
    selected_key: Option<&str>,
    state: &mut UiState,
) {
    let Some(entry) = selected_key.and_then(|key| before.bluetooth(key)) else {
        state.set_message("Select a Bluetooth device first.");
        return;
    };
    if !entry.paired {
        state.set_message(format!("{} is not paired.", entry.name));
        return;
    }
    match backend.forget(entry).await {
        Ok(()) => state.set_message(format!(
            "Forgot {}. Connecting again will pair from scratch.",
            entry.name
        )),
        Err(error) => state.set_message(format!("Cannot forget {}: {error}", entry.name)),
    }
}

/// The user moved on without answering an open code prompt. Tell the waiting
/// agent so the abandoned pairing fails now instead of holding BlueZ for the
/// full two-minute timeout.
fn abandon_code_prompt(state: &mut UiState) {
    if state.code_for.is_none() {
        return;
    }
    state.clear_code_prompt();
    bluetooth::answer_request(None);
}

/// Hands the typed pairing code to the agent running inside `connect-bg`.
fn submit_code(state: &mut UiState) {
    // Rofi passes the filter text as ROFI_INPUT on custom-input submit
    // (RETV=2) and on row actions taken while the code box is open.
    let input = env::var("ROFI_INPUT").unwrap_or_default();
    let code = input.trim();
    if code.is_empty() {
        state.set_message("Type the pairing code, then press Enter.");
        return;
    }
    if state.code_kind == Some(CodeKind::Passkey)
        && !(code.len() <= 6 && code.chars().all(|character| character.is_ascii_digit()))
    {
        state.set_message("The passkey is a number of up to 6 digits.");
        return;
    }
    state.clear_code_prompt();
    bluetooth::answer_request(Some(code));
    state.set_message("Pairing…");
    wait_for_connect(state);
}

/// Fire-and-forget `connect-bg` subprocess. Detached (the handle is dropped)
/// so this script can exit and let Rofi render right away.
fn spawn_connect_background(key: &str) {
    let Ok(executable) = env::current_exe() else {
        return;
    };
    let _ = Command::new(executable)
        .args(["connect-bg", key])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}

/// Detached connect handler: pairs when needed and connects, then writes the
/// outcome to the result file for the next script invocation to surface.
pub async fn run_connect_bg(key: &str) -> AppResult<()> {
    let (name, outcome) = attempt_connect(key).await;
    let message = match &outcome {
        Ok(()) => format!("Connected to {name}."),
        Err(error) => bluetooth::error_message(&name, error.as_ref()),
    };
    // A failed pairing can leave its unanswered prompt behind.
    bluetooth::clear_request();
    write_connect_result(key, outcome.is_ok(), &message)?;
    Ok(())
}

async fn attempt_connect(key: &str) -> (String, AppResult<()>) {
    let Some(address) = address_from_key(key) else {
        let error = io::Error::other(format!("invalid device key {key:?}"));
        return ("the selected device".to_owned(), Err(error.into()));
    };
    let backend = match Backend::new().await {
        Ok(backend) => backend,
        Err(error) => return (address, Err(error)),
    };
    let name = backend.name_of(&address).await;
    let device = match backend.device(&address) {
        Ok(device) => device,
        Err(error) => return (name, Err(error)),
    };
    let outcome = backend.pair_and_connect(&device).await;
    (name, outcome)
}

fn address_from_key(key: &str) -> Option<String> {
    hex_decode(key.strip_prefix("bt:")?)
}

/// Rofi script mode has no auto-refresh directive, so one invocation can only
/// paint one frame. Polling here lets a quick connect show its result
/// immediately; a slow one falls through with `pending_connect` still set and
/// is picked up by the next keypress.
fn wait_for_connect(state: &mut UiState) {
    if state.pending_connect.is_none() {
        return;
    }
    for _ in 0..CONNECT_POLL_ATTEMPTS {
        std::thread::sleep(CONNECT_POLL_INTERVAL);
        consume_connect_result(state);
        if state.pending_connect.is_none() {
            return;
        }
        // A code prompt needs the user, so stop waiting and paint the box.
        if consume_pair_request(state) {
            return;
        }
    }
}

/// Moves a prompt raised by the pairing agent into the UI. Returns true when
/// the prompt needs the user to type something.
fn consume_pair_request(state: &mut UiState) -> bool {
    let Some(request) = bluetooth::take_request() else {
        return false;
    };
    state.set_message(request.message);
    match request.kind {
        Some(kind) => {
            state.code_for = Some(format!("bt:{}", hex_encode(&request.address)));
            state.code_kind = Some(kind);
            true
        }
        // Informational: a code to type on the device, not here.
        None => false,
    }
}

fn result_file_path() -> Option<PathBuf> {
    let runtime = env::var_os("XDG_RUNTIME_DIR")?;
    Some(PathBuf::from(runtime).join(CONNECT_RESULT_FILENAME))
}

/// Atomically write the connect outcome so a concurrent reader never sees a
/// partial file: temp file + rename.
fn write_connect_result(key: &str, ok: bool, message: &str) -> io::Result<()> {
    let path = result_file_path().ok_or_else(|| io::Error::other("XDG_RUNTIME_DIR is not set"))?;
    let content = format!(
        "{}\n{}\n{}\n",
        hex_encode(key),
        if ok { "ok" } else { "err" },
        hex_encode(message),
    );
    let temporary = path.with_extension("tmp");
    fs::write(&temporary, &content)?;
    fs::rename(&temporary, &path)?;
    Ok(())
}

fn clear_connect_result() {
    if let Some(path) = result_file_path() {
        let _ = fs::remove_file(path);
    }
}

/// If the `connect-bg` subprocess has finished, pull its outcome into the
/// message widget and clear `pending_connect`. Stale results (a different key,
/// or no pending connection) are discarded so they never clobber the UI.
fn consume_connect_result(state: &mut UiState) {
    let Some(path) = result_file_path() else {
        return;
    };
    let Ok(contents) = fs::read_to_string(&path) else {
        return;
    };
    // Always remove once read; the outcome is single-use.
    let _ = fs::remove_file(&path);
    let mut lines = contents.lines();
    let (Some(hex_key), Some(status), Some(hex_message)) =
        (lines.next(), lines.next(), lines.next())
    else {
        return;
    };
    let Some(key) = hex_decode(hex_key) else {
        return;
    };
    if state.pending_connect.as_deref() == Some(key.as_str()) {
        state.pending_connect = None;
        state.clear_code_prompt();
        if (status == "ok" || status == "err")
            && let Some(message) = hex_decode(hex_message)
        {
            state.set_message(message);
        }
    }
}

// ---------------------------------------------------------------------------
// Output and Input tabs
// ---------------------------------------------------------------------------

fn run_audio(
    kind: crate::model::AudioKind,
    retv: u8,
    selected_key: Option<&str>,
    state: &mut UiState,
) -> Devices {
    let before = match audio::snapshot(kind) {
        Ok(entries) => Devices::Audio(entries),
        Err(error) => {
            eprintln!("rofi-audio: cannot reach PulseAudio: {error}");
            state.set_message("Audio service is unavailable.");
            return Devices::Audio(Vec::new());
        }
    };

    let mut chosen_default = None;
    match retv {
        RETV_ACTIVATE | RETV_CONFIRM | RETV_CONNECT => {
            chosen_default = set_default(&before, selected_key, state);
        }
        RETV_VOLUME_UP => nudge_volume(&before, selected_key, state, audio::STEP),
        RETV_VOLUME_DOWN => nudge_volume(&before, selected_key, state, -audio::STEP),
        RETV_SCAN => state.set_message("Scan applies to the Bluetooth tab."),
        RETV_FORGET => state.set_message("Forget applies to the Bluetooth tab."),
        _ => {}
    }

    let mut after = match audio::snapshot(kind) {
        Ok(entries) => entries,
        Err(_) => return before,
    };
    if let Some(chosen) = chosen_default.as_deref() {
        apply_chosen_default(&mut after, chosen);
    }
    Devices::Audio(after)
}

/// Moves the "default" mark onto the device the user just confirmed.
///
/// PulseAudio acknowledges `set_default_device` before the change is visible
/// to a follow-up query: pipewire-pulse routes it through PipeWire's metadata
/// and applies it asynchronously, so the snapshot taken microseconds later can
/// still name the old default and the row would redraw in the wrong colour.
/// The server accepted the change, so this render trusts that over the stale
/// read; the next render reads the settled value anyway.
fn apply_chosen_default(entries: &mut [AudioEntry], chosen: &str) {
    for entry in entries {
        entry.default = entry.name == chosen;
    }
}

/// Returns the PulseAudio node name that is now the default, when it changed.
fn set_default(
    before: &Devices,
    selected_key: Option<&str>,
    state: &mut UiState,
) -> Option<String> {
    let Some(entry) = selected_key.and_then(|key| before.audio(key)) else {
        state.set_message("Select a device first.");
        return None;
    };
    if entry.default {
        state.set_message(format!("{} is already the default.", entry.label));
        return None;
    }
    match audio::set_default(entry) {
        Ok(()) => {
            state.set_message(format!(
                "{} is now the default {}.",
                entry.label,
                entry.kind.noun()
            ));
            Some(entry.name.clone())
        }
        Err(error) => {
            state.set_message(format!("Cannot select {}: {error}", entry.label));
            None
        }
    }
}

fn nudge_volume(before: &Devices, selected_key: Option<&str>, state: &mut UiState, delta: i16) {
    let Some(entry) = selected_key.and_then(|key| before.audio(key)) else {
        state.set_message("Select a device first.");
        return;
    };
    // On success the re-rendered row already shows the new level, so the
    // message widget is left to fall back to the selected row's status line.
    if let Err(error) = audio::nudge_volume(entry, delta) {
        state.set_message(format!(
            "Cannot change the volume of {}: {error}",
            entry.label
        ));
    }
}

// ---------------------------------------------------------------------------
// Rofi script-mode protocol
// ---------------------------------------------------------------------------

fn render(
    mode: Mode,
    mut state: UiState,
    selected_key: Option<&str>,
    devices: &Devices,
) -> AppResult<()> {
    let selected_row = selected_key
        .and_then(|key| devices.position(key))
        .or_else(|| (!devices.is_empty()).then_some(0));
    // Only Bluetooth falls back to describing the highlighted row: it carries
    // the address and the pairing state, which the row itself has no space
    // for. An Output or Input row already shows its volume and name, so
    // repeating them below the list says nothing, and leaving the message
    // empty lets Rofi drop the panel until an action has something to report.
    let selected_message = match mode {
        Mode::Bluetooth => selected_row.and_then(|index| devices.message_label(index)),
        Mode::Output | Mode::Input => None,
    };
    let message = state
        .message
        .clone()
        .or(selected_message)
        .unwrap_or_default();
    // The panel is one line high; anything longer is clipped rather than
    // silently cut off mid-glyph by the widget.
    let message = single_line(&message, MESSAGE_WIDTH);

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
    if let Some(selected_row) = selected_row {
        write_header(output, "new-selection", &selected_row.to_string());
    }
    write_header(output, "data", &state.encode());
}

fn write_bluetooth_row(output: &mut Vec<u8>, entry: &BluetoothEntry) -> io::Result<()> {
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

fn write_audio_row(output: &mut Vec<u8>, entry: &AudioEntry) -> io::Result<()> {
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

fn write_row_option(output: &mut Vec<u8>, first: &mut bool, key: &str, value: &str) {
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

fn rofi_binary() -> PathBuf {
    env::var_os("ROFI_AUDIO_ROFI")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new("rofi").to_path_buf())
}

fn theme_path() -> AppResult<PathBuf> {
    if let Some(path) = env::var_os("ROFI_AUDIO_THEME") {
        return Ok(PathBuf::from(path));
    }
    if let Some(config) = env::var_os("XDG_CONFIG_HOME") {
        return Ok(PathBuf::from(config).join("rofi").join("rofi-audio.rasi"));
    }
    let home = env::var_os("HOME").ok_or_else(|| io::Error::other("HOME is not set"))?;
    Ok(PathBuf::from(home).join(".config/rofi/rofi-audio.rasi"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::AudioKind;

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
    fn confirming_a_device_marks_it_default_even_if_the_server_lags() {
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
}
