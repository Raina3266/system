use std::env;
use std::fs;
use std::fs::OpenOptions;
use std::io::{self, Read, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result, bail};

use crate::model::{ClipboardItem, ItemKind};
use crate::store::ClipboardStore;

pub const SOCKET_ENV: &str = "ROFI_CLIPBOARD_PREVIEW_SOCKET";
const UPDATE_IMAGE: u8 = 3;
const CLOSE: u8 = 2;
const SAVE_AND_CLOSE: u8 = 4;
const SAVED_TEXT: u8 = 5;
const HEADER_SIZE: usize = 17;
const MAX_PAYLOAD_BYTES: usize = 64 * 1024 * 1024;
const TEXT_EDITOR_ARGUMENTS: [&str; 4] =
    ["--stdin", "--title", "Edit clipboard text", "--panel"];
const IMAGE_PREVIEW_ARGUMENTS: [&str; 5] = [
    "--stdin",
    "--title",
    "Preview clipboard image",
    "--read-only",
    "--panel",
];

#[derive(Clone, Debug, Eq, PartialEq)]
enum PanelContent {
    Text(String),
    Image(PathBuf),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PanelState {
    Text(u64),
    Image(u64),
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
    cleanup_socket(path)?;
    cleanup_panel_state(path)
}

pub fn close(path: &Path) {
    if let Err(error) = send(path, CLOSE, 0, &[]) {
        eprintln!("rofi-clipboard: close preview panel: {error:#}");
    }
    if let Err(error) = cleanup_panel_state(path) {
        eprintln!("rofi-clipboard: clean preview panel state: {error:#}");
    }
}

pub fn toggle_edit(store: &ClipboardStore, selected_id: Option<u64>) -> Result<Option<u64>> {
    let path = socket_from_environment()?;

    if let Some(panel_state) = read_panel_state(&path)? {
        match panel_state {
            PanelState::Text(editing_id) => {
                if let Some(text) = request_text_and_close(&path)? {
                    wait_for_socket_removal(&path)?;
                    cleanup_panel_state(&path)?;
                    if !store.edit_text(editing_id, text)? {
                        bail!("clipboard item no longer exists");
                    }
                    return Ok(Some(editing_id));
                }
            }
            PanelState::Image(previewing_id) => {
                if send(&path, CLOSE, 0, &[])? {
                    wait_for_socket_removal(&path)?;
                    cleanup_panel_state(&path)?;
                    return Ok(Some(previewing_id));
                }
            }
        }
        // The state file outlived its panel. Remove both before opening the
        // currently selected item.
        cleanup_session(&path)?;
    }

    let Some(selected_id) = selected_id else {
        return Ok(None);
    };
    let Some(content) = item_content(store, selected_id)? else {
        return Ok(None);
    };

    if send(&path, CLOSE, 0, &[])? {
        // Close a stale panel before opening the selected item.
        wait_for_socket_removal(&path)?;
    } else {
        cleanup_socket(&path)?;
    }

    let panel_state = match &content {
        PanelContent::Text(_) => PanelState::Text(selected_id),
        PanelContent::Image(_) => PanelState::Image(selected_id),
    };
    write_panel_state(&path, panel_state)?;

    let launch_result = match content {
        PanelContent::Text(text) => launch_text_editor(&path, &text),
        PanelContent::Image(image_path) => launch_image_preview(&path, &image_path),
    };
    if let Err(error) = launch_result {
        let _ = send(&path, CLOSE, 0, &[]);
        let _ = cleanup_session(&path);
        return Err(error);
    }

    Ok(Some(selected_id))
}

fn launch_text_editor(path: &Path, text: &str) -> Result<()> {
    launch_panel(path, &TEXT_EDITOR_ARGUMENTS, text, None)
}

fn launch_image_preview(path: &Path, image_path: &Path) -> Result<()> {
    launch_panel(path, &IMAGE_PREVIEW_ARGUMENTS, "", Some(image_path))
}

fn launch_panel(
    path: &Path,
    arguments: &[&str],
    initial_text: &str,
    image_path: Option<&Path>,
) -> Result<()> {
    let mut command = Command::new(preview_panel_binary());
    command.args(arguments);
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
    if let Some(image_path) = image_path
        && !send(
            path,
            UPDATE_IMAGE,
            0,
            image_path.as_os_str().as_bytes(),
        )?
    {
        bail!("preview-panel closed before displaying the image");
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

fn panel_state_path(socket: &Path) -> PathBuf {
    let mut path = socket.as_os_str().to_os_string();
    path.push(".state");
    PathBuf::from(path)
}

fn write_panel_state(socket: &Path, state: PanelState) -> Result<()> {
    let path = panel_state_path(socket);
    let value = match state {
        PanelState::Text(id) => format!("text:{id}"),
        PanelState::Image(id) => format!("image:{id}"),
    };
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .mode(0o600)
        .open(&path)
        .with_context(|| format!("create preview panel state {}", path.display()))?;
    writeln!(file, "{value}")
        .with_context(|| format!("write preview panel state {}", path.display()))
}

fn read_panel_state(socket: &Path) -> Result<Option<PanelState>> {
    let path = panel_state_path(socket);
    let source = match fs::read_to_string(&path) {
        Ok(source) => source,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("read preview panel state {}", path.display()));
        }
    };
    parse_panel_state(&source)
        .map(Some)
        .with_context(|| format!("parse preview panel state {}", path.display()))
}

fn parse_panel_state(source: &str) -> Result<PanelState> {
    let (kind, id) = source
        .trim()
        .split_once(':')
        .context("panel state must use kind:id")?;
    let id = id.parse::<u64>().context("panel state ID must be numeric")?;
    match kind {
        "text" => Ok(PanelState::Text(id)),
        "image" => Ok(PanelState::Image(id)),
        _ => bail!("unknown panel state kind {kind:?}"),
    }
}

fn cleanup_panel_state(socket: &Path) -> Result<()> {
    let path = panel_state_path(socket);
    if let Err(error) = fs::remove_file(&path)
        && error.kind() != io::ErrorKind::NotFound
    {
        return Err(error)
            .with_context(|| format!("remove preview panel state {}", path.display()));
    }
    Ok(())
}

fn item_content(store: &ClipboardStore, id: u64) -> Result<Option<PanelContent>> {
    let history = store.load()?;
    Ok(history
        .items
        .iter()
        .find(|item| item.id == id)
        .and_then(|item| panel_content(item, store.image_path(item))))
}

fn panel_content(item: &ClipboardItem, image_path: Option<PathBuf>) -> Option<PanelContent> {
    match item.kind {
        ItemKind::Text => Some(PanelContent::Text(
            item.text.clone().unwrap_or_default(),
        )),
        ItemKind::Image => image_path.map(PanelContent::Image),
    }
}

fn request_text_and_close(path: &Path) -> Result<Option<String>> {
    let Some(mut stream) = connect(path)? else {
        return Ok(None);
    };
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .with_context(|| format!("set preview socket timeout {}", path.display()))?;
    write_frame(&mut stream, SAVE_AND_CLOSE, 0, &[])
        .with_context(|| format!("request edited text from {}", path.display()))?;
    read_saved_text(&mut stream)
        .map(Some)
        .with_context(|| format!("read edited text from {}", path.display()))
}

fn read_saved_text(mut reader: impl Read) -> io::Result<String> {
    let mut header = [0_u8; HEADER_SIZE];
    reader.read_exact(&mut header)?;
    if header[0] != SAVED_TEXT {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "preview panel returned an unknown response",
        ));
    }
    let length = u64::from_be_bytes(header[9..17].try_into().expect("fixed payload length"));
    let length = usize::try_from(length)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "response is too large"))?;
    if length > MAX_PAYLOAD_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "response is too large",
        ));
    }

    let mut bytes = vec![0; length];
    reader.read_exact(&mut bytes)?;
    String::from_utf8(bytes).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
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

fn write_frame(
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

#[cfg(test)]
mod tests {
    use super::*;

    fn item(kind: ItemKind, text: Option<&str>, name: Option<&str>) -> ClipboardItem {
        ClipboardItem {
            id: 7,
            kind,
            text: text.map(str::to_owned),
            image_file: (kind == ItemKind::Image).then(|| "7.png".to_owned()),
            name: name.map(str::to_owned),
            mime: match kind {
                ItemKind::Text => "text/plain",
                ItemKind::Image => "image/png",
            }
            .to_owned(),
            pinned: false,
            created_at: 0,
            digest: "digest".to_owned(),
        }
    }

    #[test]
    fn editable_text_is_byte_for_byte_unchanged() {
        let original = "heading\r\n\t  repeated    spaces\n中文 👩🏽‍💻  \n";
        assert_eq!(
            panel_content(
                &item(ItemKind::Text, Some(original), None),
                None,
            ),
            Some(PanelContent::Text(original.to_owned()))
        );
    }

    #[test]
    fn image_items_open_the_cached_image_preview() {
        let path = PathBuf::from("/home/raina/.local/share/rofi-clipboard/images/7.png");
        assert_eq!(
            panel_content(
                &item(
                    ItemKind::Image,
                    None,
                    Some("/home/raina/Pictures/example.png"),
                ),
                Some(path.clone()),
            ),
            Some(PanelContent::Image(path))
        );
    }

    #[test]
    fn close_frame_has_no_payload() {
        let mut frame = Vec::new();
        write_frame(&mut frame, CLOSE, 0, &[]).unwrap();
        assert_eq!(
            frame,
            [CLOSE, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
        );
    }

    #[test]
    fn save_request_has_no_payload() {
        let mut frame = Vec::new();
        write_frame(&mut frame, SAVE_AND_CLOSE, 0, &[]).unwrap();
        assert_eq!(frame[0], SAVE_AND_CLOSE);
        assert_eq!(u64::from_be_bytes(frame[9..17].try_into().unwrap()), 0);
    }

    #[test]
    fn saved_text_response_preserves_the_complete_buffer() {
        let text = "first line\n\tsecond  line\n中文 👩🏽‍💻\n";
        let mut frame = Vec::new();
        write_frame(&mut frame, SAVED_TEXT, 0, text.as_bytes()).unwrap();

        assert_eq!(read_saved_text(frame.as_slice()).unwrap(), text);
    }

    #[test]
    fn panel_state_uses_a_session_scoped_sibling_path() {
        assert_eq!(
            panel_state_path(Path::new("/run/user/1000/preview.sock")),
            PathBuf::from("/run/user/1000/preview.sock.state")
        );
    }

    #[test]
    fn panel_state_distinguishes_editable_text_from_image_preview() {
        assert_eq!(parse_panel_state("text:17\n").unwrap(), PanelState::Text(17));
        assert_eq!(
            parse_panel_state("image:23\n").unwrap(),
            PanelState::Image(23)
        );
    }

    #[test]
    fn text_editor_is_editable_and_soft_wraps() {
        assert!(!TEXT_EDITOR_ARGUMENTS.contains(&"--read-only"));
        assert!(!TEXT_EDITOR_ARGUMENTS.contains(&"--no-wrap"));
    }

    #[test]
    fn image_preview_is_read_only() {
        assert!(IMAGE_PREVIEW_ARGUMENTS.contains(&"--read-only"));
    }
}
