use std::env;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::bluetooth::Backend;
use crate::model::{Devices, Mode};
use crate::{AppResult, bluetooth};

mod mixer;
mod pairing;
mod render;

pub use pairing::run_connect_bg;
use pairing::{
    UiState, abandon_code_prompt, clear_connect_result, consume_connect_result,
    consume_pair_request, spawn_connect_background, submit_code, wait_for_connect,
};
const PRESERVE_SELECTION_ENV: &str = "ROFI_PRESERVE_SELECTION_ON_FILTER";

/// Rofi maps `-kb-custom-N` to `ROFI_RETV` 9 + N. The action-bar buttons
/// are wired to these in rofi-audio.rasi.
///
/// Connect/disconnect and confirm have no button and no hotkey of their own:
/// both are `RETV_ACTIVATE`, which Rofi raises on Enter and on a double-click
/// (its default `me-accept-entry` binding is `MouseDPrimary`).
const RETV_ACTIVATE: u8 = 1;
const RETV_CUSTOM_INPUT: u8 = 2;
const RETV_SCAN: u8 = 10;
const RETV_FORGET: u8 = 11;
const RETV_VOLUME_UP: u8 = 12;
const RETV_VOLUME_DOWN: u8 = 13;
/// Refresh tick: Rofi's idle `timeout` action re-runs the script so every tab
/// picks up devices that appeared after it was first shown (a Bluetooth
/// headset's PulseAudio sink lands a second or two after BlueZ finishes
/// connecting, discovery keeps turning up devices after the Bluetooth tab has
/// rendered, and Rofi never re-runs a script mode it has already shown), and so
/// a Bluetooth connect that outlasts `wait_for_connect`'s 6-second poll still
/// surfaces its result. `kb-custom-5` is RETV 14.
const RETV_REFRESH: u8 = 14;
const RETV_MUTE: u8 = 15;
const RETV_ROUTE: u8 = 16;
const RETV_PORT: u8 = 17;
const RETV_BACK: u8 = 18;

pub fn launch() -> AppResult<()> {
    // A pairing abandoned by closing the menu can leave a prompt behind; drop
    // it so this launch never opens straight into a stale code box.
    bluetooth::cleanup();
    clear_connect_result();
    // Opening the menu starts discovery. Detached, so the window appears at
    // once with the devices BlueZ already knows about while anything newly
    // switched on has a chance to turn up.
    spawn_scan_background();
    let executable = env::current_exe()?;
    let executable = executable.to_string_lossy();
    let modes = format!(
        "bluetooth:{executable} script bluetooth,\
         output:{executable} script output,\
         input:{executable} script input,\
         playback:{executable} script playback,\
         recording:{executable} script recording"
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
            "-display-playback",
            "󰐊 Playback",
            "-display-recording",
            "󰑋 Recording",
            // Alt+1..Alt+4 are Rofi's defaults for the action-bar buttons;
            // volume also answers to Alt+Up/Alt+Down, which nothing else uses.
            "-kb-custom-3",
            "Alt+3,Alt+Up",
            "-kb-custom-4",
            "Alt+4,Alt+Down",
            // Use unused default numbered bindings; do not collide with
            // Rofi's built-in Alt+letter/arrow actions.
            "-kb-custom-6",
            "Alt+6",
            "-kb-custom-7",
            "Alt+7",
            "-kb-custom-8",
            "Alt+8",
            "-kb-custom-9",
            "Alt+9",
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
    // from outliving the keypress that caused it. The refresh tick is not a
    // user action, though, so it must leave the message alone — otherwise a
    // pending "Connecting…" would be wiped on every idle tick and fall back to
    // the selected row's status.
    if retv != 0 && retv != RETV_REFRESH {
        state.message = None;
    }
    // Pull anything the detached connect left behind before acting, so a
    // finished pairing or a pending code prompt is visible right away.
    if mode == Mode::Bluetooth {
        consume_connect_result(&mut state);
        consume_pair_request(&mut state);
    }

    let devices = match mode {
        Mode::Bluetooth => run_bluetooth(retv, selected_key.as_deref(), &mut state).await,
        _ => mixer::run(mode, retv, selected_key.as_deref(), &mut state),
    };
    render::render(mode, state, selected_key.as_deref(), &devices)
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
    let acts_on_row = matches!(retv, RETV_ACTIVATE | RETV_FORGET);
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
        RETV_ACTIVATE => {
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
        RETV_VOLUME_UP | RETV_VOLUME_DOWN | RETV_MUTE | RETV_ROUTE | RETV_PORT => {
            state.set_message("Audio controls apply to the other tabs.");
        }
        _ => {}
    }

    let devices = match backend.snapshot().await {
        Ok(entries) => Devices::Bluetooth(entries),
        Err(error) => {
            state.set_message(format!("Cannot list Bluetooth devices: {error}"));
            Devices::Bluetooth(Vec::new())
        }
    };
    // Say so while discovery is running, unless the action had something more
    // specific to report. The refresh tick keeps redrawing the list underneath
    // it, so this reads as "more may still appear" rather than as a prompt to
    // press Scan again.
    if state.message.is_none() && bluetooth::is_scanning() {
        state.set_message("Scanning for devices…");
    }
    devices
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

/// The Scan button. Discovery already runs detached, so this returns at once:
/// the render that follows picks up everything found so far, and a new window
/// is opened only when the previous one has closed. Pressing it repeatedly is
/// therefore a refresh, not a series of stalls.
async fn scan(backend: &Backend, state: &mut UiState) {
    // Powering on here is explicit user intent, unlike the automatic scan at
    // launch, which leaves a deliberately switched-off adapter alone.
    if let Err(error) = backend.power_on().await {
        state.set_message(format!("Cannot power on the Bluetooth adapter: {error}"));
        return;
    }
    if !bluetooth::is_scanning() {
        spawn_scan_background();
    }
    state.set_message("Scanning for devices…");
}

/// Fire-and-forget `scan-bg` subprocess, dropped so this process can exit.
fn spawn_scan_background() {
    if bluetooth::is_scanning() {
        return;
    }
    let Ok(executable) = env::current_exe() else {
        return;
    };
    let _ = Command::new(executable)
        .arg("scan-bg")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}

/// Detached discovery handler. Leaves a powered-off adapter alone: opening the
/// menu should not undo a deliberate right-click switch-off.
pub async fn run_scan_bg() -> AppResult<()> {
    let backend = Backend::new().await?;
    if !backend.is_powered().await? {
        return Ok(());
    }
    let outcome = backend.scan().await;
    // Never leave a lapsed marker behind on a failure; the menu would keep
    // reporting a scan that is not running.
    bluetooth::expire_scanning();
    outcome
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
mod tests;
