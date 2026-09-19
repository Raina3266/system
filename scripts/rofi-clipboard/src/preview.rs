use std::env;
use std::fs;
use std::io::{self, Read, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result, bail};

use crate::model::{ClipboardItem, ItemKind, abbreviate_home_path};
use crate::store::ClipboardStore;

pub const SOCKET_ENV: &str = "ROFI_CLIPBOARD_PREVIEW_SOCKET";
const UPDATE_TEXT: u8 = 1;
const UPDATE_IMAGE: u8 = 3;
pub(crate) const CLOSE: u8 = 2;
pub(crate) const SAVE_AND_CLOSE: u8 = 4;
pub(crate) const PANEL_STATE: u8 = 5;
const PREPARE_SWITCH: u8 = 6;
const HEADER_SIZE: usize = 17;
const MAX_PAYLOAD_BYTES: usize = 64 * 1024 * 1024;
pub(crate) const SWITCH_REJECTED: u8 = 0;
pub(crate) const SWITCH_SAME_ITEM: u8 = 1;
pub(crate) const SWITCH_READY: u8 = 2;
pub(crate) const CONTENT_NONE: u8 = 0;
pub(crate) const CONTENT_TEXT: u8 = 1;
const CONTENT_IMAGE: u8 = 2;
pub(crate) const TEXT_EDITOR_ARGUMENTS: [&str; 4] =
    ["--stdin", "--title", "Edit clipboard text", "--panel"];
pub(crate) const IMAGE_PREVIEW_ARGUMENTS: [&str; 5] = [
    "--stdin",
    "--title",
    "Preview clipboard image",
    "--read-only",
    "--panel",
];
pub(crate) const FILE_PREVIEW_ARGUMENTS: [&str; 5] = [
    "--stdin",
    "--title",
    "Preview clipboard file",
    "--read-only",
    "--panel",
];

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum PanelContent {
    Text(String),
    ReadOnlyText(String),
    Image(PathBuf),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum PanelSnapshot {
    Text { id: u64, text: String },
    Image { id: u64 },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum SwitchReply {
    Rejected,
    SameItem,
    Ready(Option<PanelSnapshot>),
}

enum SaveOutcome {
    NoPanel,
    NoSnapshot,
    Saved(Option<u64>),
}

pub fn session_socket_path() -> Result<PathBuf> {
    let runtime = env::var_os("XDG_RUNTIME_DIR").context("XDG_RUNTIME_DIR is not set")?;
    Ok(PathBuf::from(runtime).join(format!(
        "rofi-clipboard-preview-{}.sock",
        std::process::id()
    )))
}

pub fn cleanup_socket(path: &Path) -> Result<()> {
    if let Err(error) = fs::remove_file(path)
        && error.kind() != io::ErrorKind::NotFound
    {
        return Err(error).with_context(|| format!("remove preview socket {}", path.display()));
    }
    Ok(())
}

pub fn cleanup_session(path: &Path) -> Result<()> {
    cleanup_socket(path)
}

pub fn close(path: &Path) {
    if let Err(error) = send(path, CLOSE, 0, &[]) {
        eprintln!("rofi-clipboard: close preview panel: {error:#}");
    }
}

pub fn save_and_close(path: &Path) -> Result<()> {
    let store = ClipboardStore::discover()?;
    let _ = save_open_panel(&store, path)?;
    Ok(())
}

pub fn toggle_edit(store: &ClipboardStore, selected_id: Option<u64>) -> Result<Option<u64>> {
    let path = socket_from_environment()?;

    match save_open_panel(store, &path)? {
        SaveOutcome::Saved(saved_id) => return Ok(saved_id),
        SaveOutcome::NoPanel | SaveOutcome::NoSnapshot => {}
    }
    cleanup_socket(&path)?;

    let Some(selected_id) = selected_id else {
        return Ok(None);
    };
    let Some(content) = item_content(store, selected_id)? else {
        return Ok(None);
    };

    launch_content(&path, selected_id, &content)?;

    Ok(Some(selected_id))
}

fn save_open_panel(store: &ClipboardStore, path: &Path) -> Result<SaveOutcome> {
    let Some(reply) = request(path, SAVE_AND_CLOSE, 0, &[])? else {
        return Ok(SaveOutcome::NoPanel);
    };
    wait_for_socket_removal(path)?;

    let SwitchReply::Ready(snapshot) = reply else {
        bail!("preview panel returned an invalid save response");
    };
    match snapshot {
        Some(snapshot) => Ok(SaveOutcome::Saved(save_snapshot(store, snapshot)?)),
        None => Ok(SaveOutcome::NoSnapshot),
    }
}

pub fn selection_changed(id: u64, serial: u64) -> Result<()> {
    let Some(path) = active_socket_from_environment() else {
        return Ok(());
    };

    let store = ClipboardStore::discover()?;
    let Some(content) = item_content(&store, id)? else {
        return Ok(());
    };
    let Some(reply) = request(&path, PREPARE_SWITCH, serial, &id.to_be_bytes())? else {
        return Ok(());
    };
    let SwitchReply::Ready(snapshot) = reply else {
        return Ok(());
    };

    if let Some(snapshot) = snapshot {
        let _ = save_snapshot(&store, snapshot)?;
    }
    let _ = send_content(&path, serial, id, &content)?;
    Ok(())
}

pub fn refresh_after_delete(store: &ClipboardStore, selected_id: Option<u64>) -> Result<()> {
    let Some(path) = active_socket_from_environment() else {
        return Ok(());
    };

    // Rofi keeps the same row index after a deletion and does not emit its
    // selection callback. Recreate only an already-open panel on that row.
    if !send(&path, CLOSE, 0, &[])? {
        cleanup_socket(&path)?;
        return Ok(());
    }
    wait_for_socket_removal(&path)?;
    cleanup_socket(&path)?;

    let Some(selected_id) = selected_id else {
        return Ok(());
    };
    let Some(content) = item_content(store, selected_id)? else {
        return Ok(());
    };

    launch_content(&path, selected_id, &content)
}

fn active_socket_from_environment() -> Option<PathBuf> {
    env::var_os(SOCKET_ENV)
        .map(PathBuf::from)
        .filter(|path| path.exists())
}

fn launch_content(path: &Path, id: u64, content: &PanelContent) -> Result<()> {
    let launch_result = match content {
        PanelContent::Text(text) => launch_text_editor(path, id, text),
        PanelContent::ReadOnlyText(text) => launch_file_preview(path, id, text),
        PanelContent::Image(image_path) => launch_image_preview(path, id, image_path),
    };
    if let Err(error) = launch_result {
        let _ = send(path, CLOSE, 0, &[]);
        let _ = cleanup_session(path);
        return Err(error);
    }
    Ok(())
}

fn launch_text_editor(path: &Path, id: u64, text: &str) -> Result<()> {
    let content = PanelContent::Text(text.to_owned());
    launch_panel(path, &TEXT_EDITOR_ARGUMENTS, text, id, &content)
}

fn launch_file_preview(path: &Path, id: u64, text: &str) -> Result<()> {
    let content = PanelContent::ReadOnlyText(text.to_owned());
    launch_panel(path, &FILE_PREVIEW_ARGUMENTS, text, id, &content)
}

fn launch_image_preview(path: &Path, id: u64, image_path: &Path) -> Result<()> {
    let content = PanelContent::Image(image_path.to_path_buf());
    launch_panel(path, &IMAGE_PREVIEW_ARGUMENTS, "", id, &content)
}

fn launch_panel(
    path: &Path,
    arguments: &[&str],
    initial_text: &str,
    id: u64,
    content: &PanelContent,
) -> Result<()> {
    let mut command = Command::new(preview_panel_binary());
    command.args(arguments);
    command.arg("--layout-file").arg(crate::rofi::theme_path()?);
    append_preview_override(&mut command, "ROFI_CLIPBOARD_PREVIEW_WIDTH", "--width");
    append_preview_override(&mut command, "ROFI_CLIPBOARD_PREVIEW_HEIGHT", "--height");
    append_preview_override(
        &mut command,
        "ROFI_CLIPBOARD_ROFI_WIDTH",
        "--companion-width",
    );
    append_preview_override(&mut command, "ROFI_CLIPBOARD_PREVIEW_SIDE", "--side");
    append_preview_override(&mut command, "ROFI_CLIPBOARD_PREVIEW_GAP", "--gap");
    command
        .arg("--listen")
        .arg(path)
        .stdin(Stdio::piped())
        .stdout(Stdio::null());
    let mut child = command.spawn().context("launch preview-panel")?;

    let mut input = child
        .stdin
        .take()
        .context("open preview-panel standard input")?;
    input
        .write_all(initial_text.as_bytes())
        .context("send initial preview-panel content")?;
    drop(input);

    wait_for_socket(&mut child, path)?;
    if !send_content(path, 0, id, content)? {
        bail!("preview-panel closed before displaying the selected item");
    }
    drop(child);
    Ok(())
}

fn append_preview_override(command: &mut Command, environment: &str, option: &str) {
    if let Some(value) = env::var_os(environment) {
        command.arg(option).arg(value);
    }
}

fn wait_for_socket(child: &mut Child, path: &Path) -> Result<()> {
    for _ in 0..100 {
        if path.exists() {
            return Ok(());
        }
        if let Some(status) = child.try_wait().context("check preview-panel startup")? {
            bail!("preview-panel exited before opening its socket ({status})");
        }
        thread::sleep(Duration::from_millis(10));
    }

    let _ = child.kill();
    let _ = child.wait();
    bail!(
        "preview-panel did not open socket {} within one second",
        path.display()
    )
}

fn wait_for_socket_removal(path: &Path) -> Result<()> {
    for _ in 0..200 {
        if !path.exists() {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(10));
    }
    bail!(
        "preview-panel did not close socket {} within two seconds",
        path.display()
    )
}

fn socket_from_environment() -> Result<PathBuf> {
    env::var_os(SOCKET_ENV)
        .map(PathBuf::from)
        .context("preview panel is only available inside rofi-clipboard")
}

fn item_content(store: &ClipboardStore, id: u64) -> Result<Option<PanelContent>> {
    let history = store.load()?;
    Ok(history
        .items
        .iter()
        .find(|item| item.id == id)
        .and_then(|item| panel_content(item, store.image_path(item))))
}

pub(crate) fn panel_content(
    item: &ClipboardItem,
    image_path: Option<PathBuf>,
) -> Option<PanelContent> {
    match item.kind {
        ItemKind::Memo | ItemKind::Text => {
            Some(PanelContent::Text(item.text.clone().unwrap_or_default()))
        }
        ItemKind::File => image_path.map(PanelContent::Image).or_else(|| {
            item.name
                .as_deref()
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .or(item.text.as_deref())
                .map(|text| PanelContent::ReadOnlyText(abbreviate_home_path(text)))
        }),
    }
}

fn save_snapshot(store: &ClipboardStore, snapshot: PanelSnapshot) -> Result<Option<u64>> {
    let (id, text) = match snapshot {
        PanelSnapshot::Image { id } => return Ok(Some(id)),
        PanelSnapshot::Text { id, text } => (id, text),
    };

    let changed = {
        let history = store.load()?;
        let Some(item) = history.items.iter().find(|item| item.id == id) else {
            return Ok(None);
        };
        if item.kind == ItemKind::File {
            return Ok(Some(id));
        }
        text_is_changed(item, &text)?
    };
    if !changed {
        return Ok(Some(id));
    }

    if store.edit_text(id, text)? {
        Ok(Some(id))
    } else {
        Ok(None)
    }
}

pub(crate) fn text_is_changed(item: &ClipboardItem, text: &str) -> Result<bool> {
    if !item.kind.is_textual() {
        bail!("files cannot be edited as text");
    }
    Ok(item.text.as_deref() != Some(text))
}

fn send_content(path: &Path, serial: u64, id: u64, content: &PanelContent) -> Result<bool> {
    let mut payload = id.to_be_bytes().to_vec();
    let operation = match content {
        PanelContent::Text(text) | PanelContent::ReadOnlyText(text) => {
            payload.extend_from_slice(text.as_bytes());
            UPDATE_TEXT
        }
        PanelContent::Image(image_path) => {
            payload.extend_from_slice(image_path.as_os_str().as_bytes());
            UPDATE_IMAGE
        }
    };
    send(path, operation, serial, &payload)
}

fn request(path: &Path, operation: u8, serial: u64, payload: &[u8]) -> Result<Option<SwitchReply>> {
    let Some(mut stream) = connect(path)? else {
        return Ok(None);
    };
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .with_context(|| format!("set preview socket timeout {}", path.display()))?;
    write_frame(&mut stream, operation, serial, payload)
        .with_context(|| format!("request panel state from {}", path.display()))?;
    read_switch_reply(&mut stream, serial)
        .map(Some)
        .with_context(|| format!("read panel state from {}", path.display()))
}

pub(crate) fn read_switch_reply(
    mut reader: impl Read,
    expected_serial: u64,
) -> io::Result<SwitchReply> {
    let mut header = [0_u8; HEADER_SIZE];
    reader.read_exact(&mut header)?;
    if header[0] != PANEL_STATE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "preview panel returned an unknown response",
        ));
    }
    let serial = u64::from_be_bytes(header[1..9].try_into().expect("fixed serial length"));
    if serial != expected_serial {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "preview panel returned a mismatched selection serial",
        ));
    }
    let length = u64::from_be_bytes(header[9..17].try_into().expect("fixed payload length"));
    let length = usize::try_from(length)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "response is too large"))?;
    if !(10..=MAX_PAYLOAD_BYTES).contains(&length) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "preview panel returned an invalid state response",
        ));
    }

    let mut bytes = vec![0; length];
    reader.read_exact(&mut bytes)?;
    let disposition = bytes[0];
    let kind = bytes[1];
    let id = u64::from_be_bytes(bytes[2..10].try_into().expect("fixed item ID length"));
    let content = &bytes[10..];

    match disposition {
        SWITCH_REJECTED if kind == CONTENT_NONE && id == 0 && content.is_empty() => {
            Ok(SwitchReply::Rejected)
        }
        SWITCH_SAME_ITEM if kind == CONTENT_NONE && id == 0 && content.is_empty() => {
            Ok(SwitchReply::SameItem)
        }
        SWITCH_READY => match kind {
            CONTENT_NONE if id == 0 && content.is_empty() => Ok(SwitchReply::Ready(None)),
            CONTENT_TEXT => String::from_utf8(content.to_vec())
                .map(|text| SwitchReply::Ready(Some(PanelSnapshot::Text { id, text })))
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error)),
            CONTENT_IMAGE if content.is_empty() => {
                Ok(SwitchReply::Ready(Some(PanelSnapshot::Image { id })))
            }
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "preview panel returned an invalid content state",
            )),
        },
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "preview panel returned an invalid switch response",
        )),
    }
}

fn connect(path: &Path) -> Result<Option<UnixStream>> {
    match UnixStream::connect(path) {
        Ok(stream) => Ok(Some(stream)),
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
            ) =>
        {
            Ok(None)
        }
        Err(error) => {
            Err(error).with_context(|| format!("connect to preview socket {}", path.display()))
        }
    }
}

fn send(path: &Path, operation: u8, serial: u64, payload: &[u8]) -> Result<bool> {
    let Some(mut stream) = connect(path)? else {
        return Ok(false);
    };

    if let Err(error) = write_frame(&mut stream, operation, serial, payload) {
        if matches!(
            error.kind(),
            io::ErrorKind::BrokenPipe
                | io::ErrorKind::ConnectionAborted
                | io::ErrorKind::ConnectionReset
        ) {
            return Ok(false);
        }
        return Err(error).with_context(|| format!("write to preview socket {}", path.display()));
    }
    Ok(true)
}

pub(crate) fn write_frame(
    mut writer: impl Write,
    operation: u8,
    serial: u64,
    payload: &[u8],
) -> io::Result<()> {
    writer.write_all(&[operation])?;
    writer.write_all(&serial.to_be_bytes())?;
    writer.write_all(&(payload.len() as u64).to_be_bytes())?;
    writer.write_all(payload)
}

fn preview_panel_binary() -> PathBuf {
    env::var_os("ROFI_CLIPBOARD_PREVIEW_PANEL")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new("preview-panel").to_path_buf())
}
