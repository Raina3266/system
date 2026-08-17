use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use nmrs::NetworkManager;

use crate::model::{EthernetEntry, Mode, SecurityKind, Snapshot, WifiEntry, message_line};
use crate::{AppResult, network, preview};

const RECORD_SEPARATOR: u8 = 0x1e;
const UNIT_SEPARATOR: u8 = 0x1f;
const WAYLAND_KEYBOARD_MODE_ENV: &str = "ROFI_WAYLAND_KEYBOARD_MODE";
const PRESERVE_SELECTION_ENV: &str = "ROFI_PRESERVE_SELECTION_ON_FILTER";
const CONNECT_RESULT_FILENAME: &str = "rofi-network-connect-result";
/// Keybinding Rofi triggers from the idle `timeout` action to refresh the view
/// while a background connect runs. Rofi reports custom bindings as `10 + N-1`.
const REFRESH_ACTION: &str = "kb-custom-5";
const REFRESH_RETV: u8 = 14;

#[derive(Clone, Debug, Default)]
struct UiState {
    initialized: bool,
    message: Option<String>,
    /// The Wi-Fi key whose connection is being performed by a detached
    /// `connect-bg` subprocess. While set, `message` holds "Connecting to
    /// {ssid}…". The subprocess writes the outcome to a result file that the
    /// next script invocation consumes (see `consume_connect_result`).
    pending_connect: Option<String>,
    /// The Wi-Fi key we are waiting for the user to type a password for.
    /// When set, the filter input becomes a password entry box: the typed
    /// text is submitted with Enter (Rofi custom-input, RETV=2) and consumed
    /// by `submit_password` instead of opening a nested Rofi dialog.
    password_for: Option<String>,
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
                state.password_for = hex_decode(awaiting);
            }
        }
        state
    }

    fn encode(&self) -> String {
        let mut parts = Vec::with_capacity(4);
        parts.push(format!("init={}", u8::from(self.initialized)));
        if let Some(message) = self.message.as_deref() {
            parts.push(format!("msg={}", hex_encode(message)));
        }
        if let Some(pending) = self.pending_connect.as_deref() {
            parts.push(format!("pending={}", hex_encode(pending)));
        }
        if let Some(awaiting) = self.password_for.as_deref() {
            parts.push(format!("await={}", hex_encode(awaiting)));
        }
        parts.join(";")
    }

    fn set_message(&mut self, message: impl Into<String>) {
        self.message = Some(message.into());
    }
}

pub fn launch() -> AppResult<()> {
    let executable = env::current_exe()?;
    let executable = executable.to_string_lossy();
    let socket = preview::session_socket_path()?;
    preview::cleanup(&socket)?;
    let modes = format!("wifi:{executable} script wifi,ethernet:{executable} script ethernet");
    let selection_command = format!(
        "{} preview-selection {{completion}} {{selection-serial}}",
        shell_quote(&executable)
    );
    let mut command = Command::new(rofi_binary());
    command
        .env(WAYLAND_KEYBOARD_MODE_ENV, "on-demand")
        .env(PRESERVE_SELECTION_ENV, "true")
        .env(preview::SOCKET_ENV, &socket)
        .args([
            "-show",
            "wifi",
            "-modes",
            &modes,
            "-display-wifi",
            "󰤨 ",
            "-display-ethernet",
            "󰈀 ",
            "-on-selection-changed",
            &selection_command,
            "-theme",
        ])
        .arg(theme_path()?);

    let status = command.status();
    preview::close_at(&socket);
    preview::cleanup(&socket)?;
    let status = status?;
    if !status.success() && status.code() != Some(1) {
        return Err(io::Error::other(format!("rofi exited with {status}")).into());
    }
    Ok(())
}

pub async fn run_script(manager: &NetworkManager, mode: Mode) -> AppResult<()> {
    let retv = env::var("ROFI_RETV")
        .ok()
        .and_then(|value| value.parse::<u8>().ok())
        .unwrap_or(0);
    let selected_key = env::var("ROFI_INFO").ok();
    let mut state = UiState::parse(env::var("ROFI_DATA").ok());
    // Pull any finished connect-bg outcome into the message before doing
    // anything else, so the user sees "Connected." / the error as soon as
    // they next touch the UI (move selection, type, etc.).
    consume_connect_result(&mut state);
    let before = network::snapshot(manager).await?;

    match retv {
        0 => {}
        // Rofi custom-input submit: the user typed text into the filter box
        // (used as the password while a Wi-Fi is awaiting credentials) and
        // pressed Enter without the text matching any row.
        2 => {
            if state.password_for.is_some() {
                submit_password(&before, &mut state).await
            }
        }
        1 | 11 => {
            // If we are awaiting a password and the user pressed Enter on the
            // awaiting row (or the Connect button while it was selected) the
            // filter text holds the typed password; submit it. Otherwise drop
            // any awaiting state and behave as a normal row/connect click.
            let awaiting = state.password_for.clone();
            if let Some(awaited) = awaiting.as_deref() {
                if Some(awaited) == selected_key.as_deref() {
                    submit_password(&before, &mut state).await
                } else {
                    state.password_for = None;
                    connect_selected(manager, mode, &before, selected_key.as_deref(), &mut state)
                        .await
                }
            } else {
                connect_selected(manager, mode, &before, selected_key.as_deref(), &mut state).await
            }
        }
        10 => {
            state.password_for = None;
            match network::scan(manager).await {
                Ok(()) => state.set_message("Scan complete."),
                Err(error) => state.set_message(format!("Cannot scan: {error}")),
            }
        }
        12 => {
            state.password_for = None;
            forget_selected(manager, mode, &before, selected_key.as_deref(), &mut state).await
        }
        13 => {
            state.password_for = None;
            show_info(manager, mode, &before, selected_key.as_deref(), &mut state).await
        }
        // Refresh tick while a connect runs: `consume_connect_result` above has
        // already folded in the outcome, so there is nothing to do but re-render.
        REFRESH_RETV => {}
        15 => {}
        _ => {}
    }

    render(manager, mode, state, selected_key.as_deref()).await
}

async fn connect_selected(
    manager: &NetworkManager,
    mode: Mode,
    snapshot: &Snapshot,
    selected_key: Option<&str>,
    state: &mut UiState,
) {
    match mode {
        Mode::Wifi => {
            let Some(entry) = selected_key.and_then(|key| find_wifi(snapshot, key)) else {
                state.set_message("Select a Wi-Fi network first.");
                return;
            };
            let was_connected = entry.connected || entry.connecting;
            if !entry.is_saved()
                && entry.security_kind() == SecurityKind::Personal
                && !was_connected
            {
                // Defer the connection: switch the filter input into password
                // mode and let the user type and submit with Enter. This keeps
                // everything inside the same Rofi window so Wayland keyboard
                // focus is never split across a nested process.
                state.password_for = Some(entry.key.clone());
                state.set_message(format!("Password for {}:", entry.ssid()));
                return;
            }

            match network::connect_wifi(manager, entry, None).await {
                Ok(()) if was_connected => {
                    state.set_message(format!("Disconnected from {}.", entry.ssid()));
                }
                Ok(()) => state.set_message(format!("Connected to {}.", entry.ssid())),
                Err(error) => state.set_message(network::connection_error_message(
                    entry.ssid(),
                    error.as_ref(),
                )),
            }
        }
        Mode::Ethernet => {
            let Some(entry) = selected_key.and_then(|key| find_ethernet(snapshot, key)) else {
                state.set_message("Select an Ethernet interface.");
                return;
            };
            let was_connected = entry.connected() || entry.connecting();
            match network::connect_ethernet(entry) {
                Ok(()) if was_connected => {
                    state.set_message(format!("Disconnected {}.", entry.device.interface));
                }
                Ok(()) => state.set_message(format!("Connected {}.", entry.device.interface)),
                Err(error) => {
                    state.set_message(format!("Cannot change {}: {error}", entry.device.interface))
                }
            }
        }
    }
}

async fn submit_password(snapshot: &Snapshot, state: &mut UiState) {
    let Some(key) = state.password_for.clone() else {
        return;
    };
    let Some(entry) = find_wifi(snapshot, &key) else {
        state.password_for = None;
        state.set_message("Network is no longer available.");
        return;
    };
    // Rofi passes the typed filter text via ROFI_INPUT on custom-input submit
    // (RETV=2) and on row/connection-button actions while in awaiting mode.
    let password = env::var("ROFI_INPUT").unwrap_or_default();
    if password.is_empty() {
        // User pressed Enter without typing; stay in awaiting mode.
        state.set_message(format!("Password for {}:", entry.ssid()));
        return;
    }
    state.password_for = None;
    // Hand the connect to a detached subprocess and return immediately. Rofi
    // blocks on this script's stdout, so waiting for the connect here would
    // freeze the whole window instead of showing "Connecting…" -- the frame we
    // render below. `write_refresh_theme` arms Rofi's idle timeout so the
    // outcome replaces this message on its own a second later.
    if !spawn_connect_background(&entry.key, &password) {
        state.set_message(format!("Cannot connect to {}.", entry.ssid()));
        return;
    }
    state.pending_connect = Some(entry.key.clone());
    state.set_message(format!("Connecting to {}…", entry.ssid()));
}

/// Fire-and-forget `connect-bg` subprocess. Detached (we drop the handle) so
/// this script can exit and let rofi render the "Connecting…" frame right
/// away while the connection proceeds in the background.
///
/// Returns whether it started: a connect that never runs would leave the UI
/// waiting on a result file nobody writes.
fn spawn_connect_background(key: &str, password: &str) -> bool {
    let Ok(executable) = env::current_exe() else {
        return false;
    };
    Command::new(executable)
        .args(["connect-bg", &hex_encode(key), &hex_encode(password)])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .is_ok()
}

/// Detached connect handler: performs the actual `connect_wifi` call and
/// writes the outcome (ok/err + message) to the result file so the next
/// rofi script invocation can surface it in the message widget.
pub async fn run_connect_bg(hex_key: &str, hex_password: &str) -> AppResult<()> {
    let key = hex_decode(hex_key).ok_or_else(|| io::Error::other("invalid hex key"))?;
    let password =
        hex_decode(hex_password).ok_or_else(|| io::Error::other("invalid hex password"))?;
    // Every path has to leave a result behind, including the ones that fail
    // before reaching NetworkManager: the UI clears "Connecting…" only when it
    // reads this file, so bailing out early would strand that message on screen.
    let outcome = connect_in_background(&key, password).await;
    let message = match &outcome {
        Ok(message) | Err(message) => message.as_str(),
    };
    write_connect_result(&key, outcome.is_ok(), message)?;
    Ok(())
}

/// The connect itself, with both arms reduced to the sentence to show.
async fn connect_in_background(key: &str, password: String) -> Result<String, String> {
    let manager = NetworkManager::new()
        .await
        .map_err(|error| format!("NetworkManager unavailable: {error}"))?;
    let snapshot = network::snapshot(&manager)
        .await
        .map_err(|error| format!("Cannot list networks: {error}"))?;
    let Some(entry) = find_wifi(&snapshot, key) else {
        return Err("Network is no longer available.".to_owned());
    };
    match network::connect_wifi(&manager, entry, Some(password)).await {
        Ok(()) => Ok(format!("Connected to {}.", entry.ssid())),
        Err(error) => Err(network::connection_error_message(
            entry.ssid(),
            error.as_ref(),
        )),
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
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, &content)?;
    fs::rename(&tmp, &path)?;
    Ok(())
}

/// If the `connect-bg` subprocess has finished, pull its outcome into the
/// message widget and clear `pending_connect`. Stale results (different key,
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
        if status == "ok" || status == "err" {
            if let Some(message) = hex_decode(hex_message) {
                state.set_message(message);
            }
        }
    }
}

async fn forget_selected(
    manager: &NetworkManager,
    mode: Mode,
    snapshot: &Snapshot,
    selected_key: Option<&str>,
    state: &mut UiState,
) {
    if mode == Mode::Ethernet {
        state.set_message("Forget needs a saved network.");
        return;
    }
    let Some(entry) = selected_key.and_then(|key| find_wifi(snapshot, key)) else {
        state.set_message("Select a Wi-Fi network first.");
        return;
    };
    if !entry.is_saved() {
        state.set_message(format!("{} is not saved.", entry.ssid()));
        return;
    }
    match network::forget_wifi(manager, entry).await {
        Ok(()) => state.set_message(format!("Forgot {}.", entry.ssid())),
        Err(error) => state.set_message(format!("Cannot forget {}: {error}", entry.ssid())),
    }
}

async fn show_info(
    manager: &NetworkManager,
    mode: Mode,
    snapshot: &Snapshot,
    selected_key: Option<&str>,
    state: &mut UiState,
) {
    // The info button toggles the preview panel: pressing it while the panel
    // is already open closes it instead of refreshing the same content.
    if preview::is_open() {
        preview::close();
        state.set_message("Closed network information.");
        return;
    }
    let result = match mode {
        Mode::Wifi => match selected_key.and_then(|key| find_wifi(snapshot, key)) {
            Some(entry) => network::wifi_info(manager, entry).await,
            None => Err(io::Error::other("select a Wi-Fi network first").into()),
        },
        Mode::Ethernet => match selected_key.and_then(|key| find_ethernet(snapshot, key)) {
            Some(entry) => Ok(network::ethernet_info(entry)),
            None => Err(io::Error::other("select an Ethernet interface first").into()),
        },
    };
    match result.and_then(|content| preview::show(&content, 0)) {
        Ok(()) => state.set_message("Details open in preview panel."),
        Err(error) => state.set_message(format!("Cannot show details: {error}")),
    }
}

pub async fn update_preview_selection(
    manager: &NetworkManager,
    key: &str,
    serial: u64,
) -> AppResult<()> {
    if !preview::is_open() {
        return Ok(());
    }
    let snapshot = network::snapshot(manager).await?;
    if let Some(entry) = find_wifi(&snapshot, key) {
        let content = network::wifi_info(manager, entry).await?;
        preview::update_if_open(&content, serial)?;
    } else if let Some(entry) = find_ethernet(&snapshot, key) {
        let content = network::ethernet_info(entry);
        preview::update_if_open(&content, serial)?;
    }
    Ok(())
}

async fn render(
    manager: &NetworkManager,
    mode: Mode,
    mut state: UiState,
    selected_key: Option<&str>,
) -> AppResult<()> {
    let snapshot = network::snapshot(manager).await?;
    let selected_row = selected_row(&snapshot, mode, selected_key).or_else(|| match mode {
        Mode::Wifi => (!snapshot.wifi.is_empty()).then_some(0),
        Mode::Ethernet => (!snapshot.ethernet.is_empty()).then_some(0),
    });
    let selected_message = selected_row.and_then(|index| match mode {
        Mode::Wifi => snapshot.wifi.get(index).map(WifiEntry::message_label),
        Mode::Ethernet => snapshot
            .ethernet
            .get(index)
            .map(EthernetEntry::message_label),
    });
    // Single choke point for everything the message widget shows, including
    // error text we do not control, so nothing can wrap it to a second line.
    let message = message_line(
        &state
            .message
            .clone()
            .or(selected_message)
            .unwrap_or_else(|| "No network interfaces available.".to_owned()),
    );

    let mut output = Vec::new();
    write_headers(&mut output, mode, &mut state, &message, selected_row);
    match mode {
        Mode::Wifi if snapshot.wifi.is_empty() => {
            write_empty_row(&mut output, "No Wi-Fi networks found")?
        }
        Mode::Wifi => {
            for entry in &snapshot.wifi {
                write_wifi_row(&mut output, entry)?;
            }
        }
        Mode::Ethernet if snapshot.ethernet.is_empty() => {
            write_empty_row(&mut output, "No Ethernet interfaces found")?
        }
        Mode::Ethernet => {
            for entry in &snapshot.ethernet {
                write_ethernet_row(&mut output, entry)?;
            }
        }
    }
    io::stdout().write_all(&output)?;
    Ok(())
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
    // While awaiting a password the filter box is the password entry; allow
    // custom input so Enter submits the typed text (Rofi custom-input, RETV=2).
    write_header(
        output,
        "no-custom",
        if state.password_for.is_some() {
            "false"
        } else {
            "true"
        },
    );
    write_header(output, "use-hot-keys", "true");
    write_header(output, "keep-selection", "true");
    // Rofi reads this one action late, so it covers a refresh tick landing while
    // the user is still typing: without it the tick would wipe the half-typed
    // password. Off otherwise, so the submitted password does not stay sitting
    // in the filter box.
    write_header(
        output,
        "keep-filter",
        if state.password_for.is_some() {
            "true"
        } else {
            "false"
        },
    );
    write_refresh_theme(output, state);
    if let Some(selected_row) = selected_row {
        write_header(output, "new-selection", &selected_row.to_string());
    }
    write_header(output, "data", &state.encode());
}

/// Arm or disarm Rofi's idle `timeout` action, which is the only hook a script
/// mode has to refresh itself without user input: Rofi re-arms the timer on
/// every action, so once it is ticking each tick re-runs this script and
/// `consume_connect_result` can surface a finished background connect.
///
/// Rofi arms the timer from the theme as it stood *before* the script ran, so
/// arming has to start one frame early -- at the password box, whose next action
/// is the Enter that kicks off the connect.
fn write_refresh_theme(output: &mut Vec<u8>, state: &UiState) {
    let delay = u8::from(state.pending_connect.is_some() || state.password_for.is_some());
    write_header(
        output,
        "theme",
        &format!("configuration {{ timeout {{ delay: {delay}; action: \"{REFRESH_ACTION}\"; }} }}"),
    );
}

fn write_wifi_row(output: &mut Vec<u8>, entry: &WifiEntry) -> io::Result<()> {
    write!(output, "{}", entry.key)?;
    let mut first = true;
    write_row_option(output, &mut first, "display", &entry.row_label());
    write_row_option(output, &mut first, "info", &entry.key);
    write_row_option(
        output,
        &mut first,
        "meta",
        &format!(
            "{} {} {}% {}",
            entry.ssid(),
            entry.interface(),
            entry.strength(),
            entry.security_label()
        ),
    );
    if entry.connected || entry.connecting {
        write_row_option(output, &mut first, "urgent", "true");
    } else if entry.is_saved() {
        write_row_option(output, &mut first, "active", "true");
    }
    output.push(RECORD_SEPARATOR);
    Ok(())
}

fn write_ethernet_row(output: &mut Vec<u8>, entry: &EthernetEntry) -> io::Result<()> {
    write!(output, "{}", entry.key)?;
    let mut first = true;
    write_row_option(output, &mut first, "display", &entry.row_label());
    write_row_option(output, &mut first, "info", &entry.key);
    write_row_option(
        output,
        &mut first,
        "meta",
        &format!(
            "{} {} {}",
            entry.device.interface,
            entry.device.ip4_address.as_deref().unwrap_or_default(),
            entry.device.ip6_address.as_deref().unwrap_or_default()
        ),
    );
    if entry.connected() || entry.connecting() {
        write_row_option(output, &mut first, "urgent", "true");
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

fn selected_row(snapshot: &Snapshot, mode: Mode, selected_key: Option<&str>) -> Option<usize> {
    let key = selected_key?;
    match mode {
        Mode::Wifi => snapshot.wifi.iter().position(|entry| entry.key == key),
        Mode::Ethernet => snapshot.ethernet.iter().position(|entry| entry.key == key),
    }
}

fn find_wifi<'a>(snapshot: &'a Snapshot, key: &str) -> Option<&'a WifiEntry> {
    snapshot.wifi.iter().find(|entry| entry.key == key)
}

fn find_ethernet<'a>(snapshot: &'a Snapshot, key: &str) -> Option<&'a EthernetEntry> {
    snapshot.ethernet.iter().find(|entry| entry.key == key)
}

fn rofi_binary() -> PathBuf {
    env::var_os("ROFI_NETWORK_ROFI")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new("rofi").to_path_buf())
}

fn theme_path() -> AppResult<PathBuf> {
    if let Some(path) = env::var_os("ROFI_NETWORK_THEME") {
        return Ok(PathBuf::from(path));
    }
    if let Some(config) = env::var_os("XDG_CONFIG_HOME") {
        return Ok(PathBuf::from(config).join("rofi").join("network.rasi"));
    }
    let home = env::var_os("HOME").ok_or_else(|| io::Error::other("HOME is not set"))?;
    Ok(PathBuf::from(home).join(".config/rofi/rofi-network.rasi"))
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn hex_encode(value: &str) -> String {
    crate::model::hex_encode(value)
}

fn hex_decode(value: &str) -> Option<String> {
    if !value.len().is_multiple_of(2) {
        return None;
    }
    let bytes = value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let pair = std::str::from_utf8(pair).ok()?;
            u8::from_str_radix(pair, 16).ok()
        })
        .collect::<Option<Vec<_>>>()?;
    String::from_utf8(bytes).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ui_state_round_trips_unicode_messages() {
        let mut state = UiState {
            initialized: true,
            message: None,
            pending_connect: None,
            password_for: None,
        };
        state.set_message("Incorrect password for Café 🛜");
        let decoded = UiState::parse(Some(state.encode()));
        assert!(decoded.initialized);
        assert_eq!(decoded.message, state.message);
        assert_eq!(decoded.password_for, state.password_for);
    }

    #[test]
    fn ui_state_round_trips_password_for_awaiting() {
        let state = UiState {
            initialized: true,
            message: Some("Please type in password for Café.".to_string()),
            pending_connect: None,
            password_for: Some("wifi:776c616e30:486f6d65".to_string()),
        };
        let decoded = UiState::parse(Some(state.encode()));
        assert!(decoded.initialized);
        assert_eq!(decoded.message, state.message);
        assert_eq!(decoded.password_for, state.password_for);
    }

    #[test]
    fn ui_state_round_trips_pending_connect() {
        let mut state = UiState {
            initialized: true,
            message: Some("Connecting to Café…\nWPA2 · connecting".to_string()),
            pending_connect: Some("wifi:776c616e30:486f6d65".to_string()),
            password_for: None,
        };
        let encoded = state.encode();
        // The message survives across invocations because it is stored in
        // `data` via `msg=`, so the "Connecting…" status persists until the
        // detached connect-bg subprocess writes its result.
        let decoded = UiState::parse(Some(encoded));
        assert_eq!(decoded.pending_connect, state.pending_connect);
        assert_eq!(decoded.message, state.message);
        // Once the result arrives the foreground script clears `pending_connect`;
        // re-encoding then drops the `pending=` part entirely.
        state.pending_connect = None;
        assert!(!state.encode().contains("pending="));
    }

    #[test]
    fn refresh_ticks_only_while_a_connect_is_pending() {
        let mut idle = Vec::new();
        write_refresh_theme(&mut idle, &UiState::default());
        assert!(String::from_utf8_lossy(&idle).contains("delay: 0;"));

        let connecting = UiState {
            pending_connect: Some("wifi:776c616e30:486f6d65".to_string()),
            ..UiState::default()
        };
        let mut armed = Vec::new();
        write_refresh_theme(&mut armed, &connecting);
        let armed = String::from_utf8_lossy(&armed).into_owned();
        assert!(armed.contains("delay: 1;"), "{armed}");
        assert!(armed.contains(REFRESH_ACTION), "{armed}");
    }

    #[test]
    fn script_rows_use_one_metadata_nul() {
        let mut row = b"key".to_vec();
        let mut first = true;
        write_row_option(&mut row, &mut first, "display", "Visible");
        write_row_option(&mut row, &mut first, "info", "key");
        assert_eq!(row.iter().filter(|byte| **byte == 0).count(), 1);
    }

    #[test]
    fn executable_paths_are_shell_quoted() {
        assert_eq!(
            shell_quote("/tmp/Raina's tools/network"),
            "'/tmp/Raina'\\''s tools/network'"
        );
    }
}
