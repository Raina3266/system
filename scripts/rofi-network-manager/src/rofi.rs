use std::env;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use nmrs::NetworkManager;

use crate::model::{EthernetEntry, Mode, SecurityKind, Snapshot, WifiEntry};
use crate::{AppResult, network, preview};

const RECORD_SEPARATOR: u8 = 0x1e;
const UNIT_SEPARATOR: u8 = 0x1f;
const WAYLAND_KEYBOARD_MODE_ENV: &str = "ROFI_WAYLAND_KEYBOARD_MODE";
const PRESERVE_SELECTION_ENV: &str = "ROFI_PRESERVE_SELECTION_ON_FILTER";

#[derive(Clone, Debug, Default)]
struct UiState {
    initialized: bool,
    message: Option<String>,
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
            } else if let Some(awaiting) = part.strip_prefix("await=") {
                state.password_for = hex_decode(awaiting);
            }
        }
        state
    }

    fn encode(&self) -> String {
        let mut parts = Vec::with_capacity(3);
        parts.push(format!("init={}", u8::from(self.initialized)));
        if let Some(message) = self.message.as_deref() {
            parts.push(format!("msg={}", hex_encode(message)));
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
            "󰤨 Wi-Fi",
            "-display-ethernet",
            "󰈀 Ethernet",
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
    let before = network::snapshot(manager).await?;

    match retv {
        0 => {}
        // Rofi custom-input submit: the user typed text into the filter box
        // (used as the password while a Wi-Fi is awaiting credentials) and
        // pressed Enter without the text matching any row.
        2 => {
            if state.password_for.is_some() {
                submit_password(manager, &before, &mut state).await
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
                    submit_password(manager, &before, &mut state).await
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
                Err(error) => state.set_message(format!("Cannot scan for networks: {error}")),
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
                state.set_message(format!("Please type in password for {}.", entry.ssid()));
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
                state.set_message("Select an Ethernet interface first.");
                return;
            };
            let was_connected = entry.connected() || entry.connecting();
            match network::connect_ethernet(entry) {
                Ok(()) if was_connected => state.set_message(format!(
                    "Disconnected Ethernet interface {}.",
                    entry.device.interface
                )),
                Ok(()) => state.set_message(format!(
                    "Connected Ethernet interface {}.",
                    entry.device.interface
                )),
                Err(error) => state.set_message(format!(
                    "Cannot change Ethernet interface {}: {error}",
                    entry.device.interface
                )),
            }
        }
    }
}

async fn submit_password(manager: &NetworkManager, snapshot: &Snapshot, state: &mut UiState) {
    let Some(key) = state.password_for.clone() else {
        return;
    };
    let Some(entry) = find_wifi(snapshot, &key) else {
        state.password_for = None;
        state.set_message("Selected network is no longer available.");
        return;
    };
    // Rofi passes the typed filter text via ROFI_INPUT on custom-input submit
    // (RETV=2) and on row/connection-button actions while in awaiting mode.
    let password = env::var("ROFI_INPUT").unwrap_or_default();
    if password.is_empty() {
        // User pressed Enter without typing; stay in awaiting mode.
        state.set_message(format!("Please type in password for {}.", entry.ssid()));
        return;
    }
    state.password_for = None;
    match network::connect_wifi(manager, entry, Some(password)).await {
        Ok(()) => state.set_message(format!("Connected to {}.", entry.ssid())),
        Err(error) => state.set_message(network::connection_error_message(
            entry.ssid(),
            error.as_ref(),
        )),
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
        state.set_message("Forget applies to saved Wi-Fi networks.");
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
        Ok(()) => state.set_message(format!(
            "Forgot {}. It is now listed as an unsaved network.",
            entry.ssid()
        )),
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
        Ok(()) => state.set_message("Network information is open in the preview panel."),
        Err(error) => state.set_message(format!("Cannot show network information: {error}")),
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
    let message = state
        .message
        .clone()
        .or(selected_message)
        .unwrap_or_else(|| "No network interfaces are available.".to_owned());

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
    if let Some(selected_row) = selected_row {
        write_header(output, "new-selection", &selected_row.to_string());
    }
    write_header(output, "data", &state.encode());
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
            password_for: Some("wifi:776c616e30:486f6d65".to_string()),
        };
        let decoded = UiState::parse(Some(state.encode()));
        assert!(decoded.initialized);
        assert_eq!(decoded.message, state.message);
        assert_eq!(decoded.password_for, state.password_for);
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
