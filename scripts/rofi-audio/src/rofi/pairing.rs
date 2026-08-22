use std::env;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

use crate::bluetooth::Backend;
use crate::model::{CodeKind, hex_decode, hex_encode};
use crate::{AppResult, bluetooth};

const CONNECT_RESULT_FILENAME: &str = "rofi-audio-connect-result";
/// How long a script invocation waits for a detached connect before handing
/// control back to Rofi. Long enough for most pairings, short enough that the
/// menu never feels frozen.
const CONNECT_POLL_ATTEMPTS: u32 = 60;
const CONNECT_POLL_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Clone, Debug, Default)]
pub(super) struct UiState {
    pub(super) initialized: bool,
    pub(super) message: Option<String>,
    /// Row key whose connection is being performed by a detached `connect-bg`
    /// subprocess. While set, the subprocess owns the outcome and writes it to
    /// $XDG_RUNTIME_DIR/rofi-audio-connect-result.
    pub(super) pending_connect: Option<String>,
    /// Row key we are waiting for the user to type a pairing code for. When
    /// set, the filter input becomes the code entry box: the typed text is
    /// submitted with Enter (Rofi custom-input, RETV=2) and handed to the
    /// pairing agent instead of opening a nested Rofi dialog.
    pub(super) code_for: Option<String>,
    pub(super) code_kind: Option<CodeKind>,
}

impl UiState {
    pub(super) fn parse(value: Option<String>) -> Self {
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

    pub(super) fn encode(&self) -> String {
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

    pub(super) fn set_message(&mut self, message: impl Into<String>) {
        self.message = Some(message.into());
    }

    pub(super) fn clear_code_prompt(&mut self) {
        self.code_for = None;
        self.code_kind = None;
    }
}

/// The user moved on without answering an open code prompt. Tell the waiting
/// agent so the abandoned pairing fails now instead of holding BlueZ for the
/// full two-minute timeout.
pub(super) fn abandon_code_prompt(state: &mut UiState) {
    if state.code_for.is_none() {
        return;
    }
    state.clear_code_prompt();
    bluetooth::answer_request(None);
}

/// Hands the typed pairing code to the agent running inside `connect-bg`.
pub(super) fn submit_code(state: &mut UiState) {
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
pub(super) fn spawn_connect_background(key: &str) {
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

pub(super) fn address_from_key(key: &str) -> Option<String> {
    hex_decode(key.strip_prefix("bt:")?)
}

/// Rofi script mode has no auto-refresh directive, so one invocation can only
/// paint one frame. Polling here lets a quick connect show its result
/// immediately; a slow one falls through with `pending_connect` still set and
/// is picked up by the next keypress.
pub(super) fn wait_for_connect(state: &mut UiState) {
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
pub(super) fn consume_pair_request(state: &mut UiState) -> bool {
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

pub(super) fn clear_connect_result() {
    if let Some(path) = result_file_path() {
        let _ = fs::remove_file(path);
    }
}

/// If the `connect-bg` subprocess has finished, pull its outcome into the
/// message widget and clear `pending_connect`. Stale results (a different key,
/// or no pending connection) are discarded so they never clobber the UI.
pub(super) fn consume_connect_result(state: &mut UiState) {
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
