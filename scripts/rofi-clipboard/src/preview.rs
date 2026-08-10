use std::env;
use std::ffi::OsString;
use std::fs;
use std::io::{self, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result, bail};

use crate::model::{ClipboardItem, ItemKind};
use crate::store::ClipboardStore;

pub const SOCKET_ENV: &str = "ROFI_CLIPBOARD_PREVIEW_SOCKET";
const UPDATE_TEXT: u8 = 1;
const CLOSE: u8 = 2;
const UPDATE_IMAGE: u8 = 3;
const DEFAULT_PANEL_WIDTH: &str = "480";
const DEFAULT_PANEL_HEIGHT: &str = "615";
const DEFAULT_ROFI_WIDTH: &str = "400";
const DEFAULT_PANEL_SIDE: &str = "left";
const DEFAULT_PANEL_GAP: &str = "10";

#[derive(Clone, Debug, Eq, PartialEq)]
enum PreviewContent {
    Text(String),
    Image(PathBuf),
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

pub fn close(path: &Path) {
    if let Err(error) = send(path, CLOSE, 0, &[]) {
        eprintln!("rofi-clipboard: close preview panel: {error:#}");
    }
}

pub fn toggle(store: &ClipboardStore, id: u64) -> Result<()> {
    let path = socket_from_environment()?;
    if send(&path, CLOSE, 0, &[])? {
        return Ok(());
    }
    cleanup_socket(&path)?;

    let Some(content) = item_content(store, id)? else {
        return Ok(());
    };
    let mut command = Command::new(preview_panel_binary());
    command
        .args([
            "--stdin",
            "--title",
            "Clipboard preview",
            "--panel",
            "--no-wrap",
            "--width",
        ])
        .arg(preview_setting(
            "ROFI_CLIPBOARD_PREVIEW_WIDTH",
            DEFAULT_PANEL_WIDTH,
        ))
        .arg("--height")
        .arg(preview_setting(
            "ROFI_CLIPBOARD_PREVIEW_HEIGHT",
            DEFAULT_PANEL_HEIGHT,
        ))
        .arg("--companion-width")
        .arg(preview_setting(
            "ROFI_CLIPBOARD_ROFI_WIDTH",
            DEFAULT_ROFI_WIDTH,
        ))
        .arg("--side")
        .arg(preview_setting(
            "ROFI_CLIPBOARD_PREVIEW_SIDE",
            DEFAULT_PANEL_SIDE,
        ))
        .arg("--gap")
        .arg(preview_setting(
            "ROFI_CLIPBOARD_PREVIEW_GAP",
            DEFAULT_PANEL_GAP,
        ))
        .arg("--listen")
        .arg(&path)
        .stdin(Stdio::piped())
        .stdout(Stdio::null());
    let mut child = command
        .spawn()
        .context("launch preview-panel")?;

    let mut input = child
        .stdin
        .take()
        .context("open preview-panel standard input")?;
    if let PreviewContent::Text(text) = &content {
        input
            .write_all(text.as_bytes())
            .context("send initial preview text")?;
    }
    drop(input);

    wait_for_socket(&mut child, &path)?;
    if matches!(&content, PreviewContent::Image(_)) && !send_content(&path, 0, &content)? {
        bail!("preview-panel closed before the initial image could be displayed");
    }
    drop(child);
    Ok(())
}

fn preview_setting(name: &str, default: &str) -> OsString {
    env::var_os(name).unwrap_or_else(|| OsString::from(default))
}

pub fn selection_changed(id: u64, serial: u64) -> Result<()> {
    let Some(path) = env::var_os(SOCKET_ENV).map(PathBuf::from) else {
        return Ok(());
    };
    if !path.exists() {
        return Ok(());
    }

    let store = ClipboardStore::discover()?;
    let Some(content) = item_content(&store, id)? else {
        return Ok(());
    };
    let _ = send_content(&path, serial, &content)?;
    Ok(())
}

fn send_content(socket: &Path, serial: u64, content: &PreviewContent) -> Result<bool> {
    match content {
        PreviewContent::Text(text) => send(socket, UPDATE_TEXT, serial, text.as_bytes()),
        PreviewContent::Image(path) => send(
            socket,
            UPDATE_IMAGE,
            serial,
            path.as_os_str().as_bytes(),
        ),
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

fn socket_from_environment() -> Result<PathBuf> {
    env::var_os(SOCKET_ENV)
        .map(PathBuf::from)
        .context("preview panel is only available inside rofi-clipboard")
}

fn item_content(store: &ClipboardStore, id: u64) -> Result<Option<PreviewContent>> {
    let history = store.load()?;
    Ok(history
        .items
        .iter()
        .find(|item| item.id == id)
        .map(|item| match item.kind {
            ItemKind::Image => store
                .image_path(item)
                .filter(|path| path.is_file())
                .map(PreviewContent::Image)
                .unwrap_or_else(|| PreviewContent::Text(preview_text(item))),
            ItemKind::Text => PreviewContent::Text(preview_text(item)),
        }))
}

fn preview_text(item: &ClipboardItem) -> String {
    match item.kind {
        ItemKind::Text => item.text.clone().unwrap_or_default(),
        ItemKind::Image => {
            let source = item
                .name
                .as_deref()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or("Image clipboard item");
            format!("{source}\n\nMIME type: {}", item.mime)
        }
    }
}

fn send(path: &Path, operation: u8, serial: u64, payload: &[u8]) -> Result<bool> {
    let mut stream = match UnixStream::connect(path) {
        Ok(stream) => stream,
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
            ) =>
        {
            return Ok(false);
        }
        Err(error) => {
            return Err(error)
                .with_context(|| format!("connect to preview socket {}", path.display()));
        }
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
    fn text_preview_is_byte_for_byte_unchanged() {
        let original = "heading\r\n\t  repeated    spaces\n中文 👩🏽‍💻  \n";
        assert_eq!(
            preview_text(&item(ItemKind::Text, Some(original), None)).as_bytes(),
            original.as_bytes()
        );
    }

    #[test]
    fn image_preview_identifies_the_source_and_mime_type() {
        let preview = preview_text(&item(
            ItemKind::Image,
            None,
            Some("/home/raina/Pictures/example.png"),
        ));
        assert!(preview.contains("/home/raina/Pictures/example.png"));
        assert!(preview.contains("image/png"));
    }

    #[test]
    fn update_frame_contains_exact_text_bytes() {
        let text = "first\n\tsecond  \n";
        let mut frame = Vec::new();
        write_frame(&mut frame, UPDATE_TEXT, 42, text.as_bytes()).unwrap();
        assert_eq!(frame[0], UPDATE_TEXT);
        assert_eq!(
            u64::from_be_bytes(frame[1..9].try_into().unwrap()),
            42
        );
        assert_eq!(
            u64::from_be_bytes(frame[9..17].try_into().unwrap()),
            text.len() as u64
        );
        assert_eq!(&frame[17..], text.as_bytes());
    }

    #[test]
    fn image_update_frame_contains_the_cached_path() {
        let path = "/home/raina/.local/share/rofi-clipboard/images/7.png";
        let mut frame = Vec::new();
        write_frame(&mut frame, UPDATE_IMAGE, 43, path.as_bytes()).unwrap();
        assert_eq!(frame[0], UPDATE_IMAGE);
        assert_eq!(
            u64::from_be_bytes(frame[1..9].try_into().unwrap()),
            43
        );
        assert_eq!(&frame[17..], path.as_bytes());
    }

    #[test]
    fn close_frame_has_no_payload() {
        let mut frame = Vec::new();
        write_frame(&mut frame, CLOSE, 0, &[]).unwrap();
        assert_eq!(frame, [CLOSE, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    }
}
