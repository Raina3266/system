use std::collections::hash_map::DefaultHasher;
use std::env;
use std::ffi::{OsStr, OsString};
use std::fs::{self, File};
use std::hash::{Hash, Hasher};
use std::io::{self, Read, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, UNIX_EPOCH};

use crate::AppResult;
use crate::model::{Mode, mode_from_key, path_from_key};

pub const SOCKET_ENV: &str = "ROFI_FILESEARCH_PREVIEW_SOCKET";
const UPDATE_TEXT: u8 = 1;
const CLOSE: u8 = 2;
const UPDATE_IMAGE: u8 = 3;
const MAX_TEXT_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
enum PanelContent {
    Text(String),
    Image(PathBuf),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PreviewKind {
    Text,
    Image,
    Pdf,
    Video,
    Unsupported,
}

pub fn session_socket_path() -> AppResult<PathBuf> {
    let runtime = env::var_os("XDG_RUNTIME_DIR")
        .ok_or_else(|| io::Error::other("XDG_RUNTIME_DIR is not set"))?;
    Ok(PathBuf::from(runtime).join(format!(
        "rofi-filesearch-preview-{}.sock",
        std::process::id()
    )))
}

pub fn cleanup(path: &Path) -> AppResult<()> {
    if let Err(error) = fs::remove_file(path)
        && error.kind() != io::ErrorKind::NotFound
    {
        return Err(error.into());
    }
    Ok(())
}

pub fn close_at(path: &Path) {
    let _ = send(path, CLOSE, 0, 0, &[]);
    for _ in 0..100 {
        if !path.exists() {
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
    let _ = cleanup(path);
}

pub fn toggle(key: &str) -> AppResult<()> {
    let path = required_socket_from_environment()?;
    if path.exists() {
        close_at(&path);
        return Ok(());
    }
    let file = path_from_key(key, Mode::File)
        .ok_or_else(|| io::Error::other("preview requires a selected file"))?;
    let content = preview_content(&file)?;
    cleanup(&path)?;
    launch_panel(&path)?;
    update_at(&path, &file, &content, 0)
}

pub fn selection_changed(key: &str, serial: u64) -> AppResult<()> {
    let Some(socket) = socket_from_environment() else {
        return Ok(());
    };
    if mode_from_key(key) != Some(Mode::File) {
        if socket.exists() {
            close_at(&socket);
        }
        return Ok(());
    }
    if !socket.exists() {
        return Ok(());
    }
    let Some(file) = path_from_key(key, Mode::File) else {
        return Ok(());
    };
    let content = preview_content(&file)?;
    update_at(&socket, &file, &content, serial)
}

fn preview_content(path: &Path) -> AppResult<PanelContent> {
    let mime = mime_type(path)?;
    match preview_kind(&mime) {
        PreviewKind::Text => Ok(PanelContent::Text(read_text(path)?)),
        PreviewKind::Image => Ok(PanelContent::Image(path.to_path_buf())),
        PreviewKind::Pdf => Ok(PanelContent::Image(cached_pdf(path)?)),
        PreviewKind::Video => Ok(PanelContent::Image(cached_video(path)?)),
        PreviewKind::Unsupported => Ok(PanelContent::Text(format!(
            "Preview is not available for {mime}.\n\n{}",
            path.display()
        ))),
    }
}

fn mime_type(path: &Path) -> AppResult<String> {
    let output = Command::new(file_binary())
        .args([
            OsStr::new("--brief"),
            OsStr::new("--mime-type"),
            OsStr::new("--"),
        ])
        .arg(path)
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!("file exited with {}", output.status)).into());
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}

fn preview_kind(mime: &str) -> PreviewKind {
    if mime == "application/pdf" {
        PreviewKind::Pdf
    } else if mime.starts_with("image/") {
        PreviewKind::Image
    } else if mime.starts_with("video/") {
        PreviewKind::Video
    } else if mime.starts_with("text/")
        || matches!(
            mime,
            "application/json"
                | "application/ld+json"
                | "application/javascript"
                | "application/xml"
                | "application/x-httpd-php"
                | "application/x-shellscript"
                | "application/x-yaml"
        )
    {
        PreviewKind::Text
    } else {
        PreviewKind::Unsupported
    }
}

fn read_text(path: &Path) -> AppResult<String> {
    let mut bytes = Vec::new();
    File::open(path)?
        .take(MAX_TEXT_BYTES + 1)
        .read_to_end(&mut bytes)?;
    let clipped = bytes.len() as u64 > MAX_TEXT_BYTES;
    if clipped {
        bytes.truncate(MAX_TEXT_BYTES as usize);
        while std::str::from_utf8(&bytes).is_err() && !bytes.is_empty() {
            bytes.pop();
        }
    }
    let mut text = String::from_utf8(bytes)?;
    if clipped {
        text.push_str("\n\n… preview truncated at 2 MiB …\n");
    }
    Ok(text)
}

fn cached_pdf(path: &Path) -> AppResult<PathBuf> {
    let target = cache_path(path, "pdf", "png")?;
    if !target.exists() {
        render_pdf_to(path, &target, 1200)?;
    }
    Ok(target)
}

fn cached_video(path: &Path) -> AppResult<PathBuf> {
    let target = cache_path(path, "video", "png")?;
    if target.exists() {
        return Ok(target);
    }
    let temporary = temporary_path(&target);
    let status = Command::new(ffmpegthumbnailer_binary())
        .arg("-i")
        .arg(path)
        .arg("-o")
        .arg(&temporary)
        .args(["-s", "1200", "-t", "10%", "-q", "8"])
        .status()?;
    if !status.success() || !temporary.is_file() {
        let _ = fs::remove_file(&temporary);
        return Err(io::Error::other(format!("ffmpegthumbnailer exited with {status}")).into());
    }
    fs::rename(&temporary, &target)?;
    Ok(target)
}

fn cache_path(path: &Path, category: &str, extension: &str) -> AppResult<PathBuf> {
    let root = env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
        .ok_or_else(|| io::Error::other("neither XDG_CACHE_HOME nor HOME is set"))?
        .join("rofi-filesearch")
        .join(category);
    fs::create_dir_all(&root)?;
    let metadata = fs::metadata(path)?;
    let modified = metadata
        .modified()?
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let mut hasher = DefaultHasher::new();
    path.as_os_str().as_bytes().hash(&mut hasher);
    metadata.len().hash(&mut hasher);
    modified.as_secs().hash(&mut hasher);
    modified.subsec_nanos().hash(&mut hasher);
    Ok(root.join(format!("{:016x}.{extension}", hasher.finish())))
}

pub fn thumbnail_pdf(input: &Path, output: &Path, size: u32) -> AppResult<()> {
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }
    render_pdf_to(input, output, size)
}

fn render_pdf_to(input: &Path, output: &Path, size: u32) -> AppResult<()> {
    let temporary = temporary_path(output);
    let prefix = temporary.with_extension("");
    let status = Command::new(pdftoppm_binary())
        .args(["-png", "-f", "1", "-singlefile", "-scale-to"])
        .arg(size.to_string())
        .arg("--")
        .arg(input)
        .arg(&prefix)
        .status()?;
    let rendered = prefix.with_extension("png");
    if !status.success() || !rendered.is_file() {
        let _ = fs::remove_file(&rendered);
        return Err(io::Error::other(format!("pdftoppm exited with {status}")).into());
    }
    fs::rename(rendered, output)?;
    Ok(())
}

fn temporary_path(target: &Path) -> PathBuf {
    let name = target
        .file_name()
        .map(|name| name.to_string_lossy())
        .unwrap_or_default();
    target.with_file_name(format!(".{name}.{}.tmp.png", std::process::id()))
}

fn launch_panel(path: &Path) -> AppResult<()> {
    let mut command = Command::new(preview_panel_binary());
    command
        .args([
            "--stdin",
            "--title",
            "File preview",
            "--read-only",
            "--panel",
            "--listen",
        ])
        .arg(path)
        .arg("--companion-width")
        .arg("500")
        .stdin(Stdio::piped())
        .stdout(Stdio::null());
    append_override(&mut command, "ROFI_FILESEARCH_PREVIEW_WIDTH", "--width");
    append_override(&mut command, "ROFI_FILESEARCH_PREVIEW_HEIGHT", "--height");
    append_override(&mut command, "ROFI_FILESEARCH_PREVIEW_SIDE", "--side");
    append_override(&mut command, "ROFI_FILESEARCH_PREVIEW_GAP", "--gap");
    let mut child = command.spawn()?;
    drop(child.stdin.take());
    wait_for_socket(&mut child, path)
}

fn append_override(command: &mut Command, environment: &str, option: &str) {
    if let Some(value) = env::var_os(environment) {
        command.arg(option).arg(value);
    }
}

fn wait_for_socket(child: &mut Child, path: &Path) -> AppResult<()> {
    for _ in 0..100 {
        if path.exists() {
            return Ok(());
        }
        if let Some(status) = child.try_wait()? {
            return Err(io::Error::other(format!(
                "preview-panel exited before opening its socket ({status})"
            ))
            .into());
        }
        thread::sleep(Duration::from_millis(10));
    }
    let _ = child.kill();
    let _ = child.wait();
    Err(io::Error::other("preview-panel did not open its socket within one second").into())
}

fn update_at(path: &Path, file: &Path, content: &PanelContent, serial: u64) -> AppResult<()> {
    let id = item_id(file);
    let sent = match content {
        PanelContent::Text(text) => send(path, UPDATE_TEXT, serial, id, text.as_bytes())?,
        PanelContent::Image(image) => {
            send(path, UPDATE_IMAGE, serial, id, image.as_os_str().as_bytes())?
        }
    };
    if sent {
        Ok(())
    } else {
        Err(io::Error::other("preview-panel closed before the update arrived").into())
    }
}

fn item_id(path: &Path) -> u64 {
    let mut hasher = DefaultHasher::new();
    path.as_os_str().as_bytes().hash(&mut hasher);
    hasher.finish()
}

fn send(path: &Path, operation: u8, serial: u64, id: u64, content: &[u8]) -> AppResult<bool> {
    let mut payload = Vec::with_capacity(8 + content.len());
    if operation != CLOSE {
        payload.extend_from_slice(&id.to_be_bytes());
        payload.extend_from_slice(content);
    }
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
        Err(error) => return Err(error.into()),
    };
    write_frame(&mut stream, operation, serial, &payload)?;
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

fn socket_from_environment() -> Option<PathBuf> {
    env::var_os(SOCKET_ENV).map(PathBuf::from)
}

fn required_socket_from_environment() -> AppResult<PathBuf> {
    match socket_from_environment() {
        Some(path) => Ok(path),
        None => Err(io::Error::other("preview is only available inside File Search").into()),
    }
}

fn binary(environment: &str, fallback: &str) -> OsString {
    env::var_os(environment).unwrap_or_else(|| OsString::from(fallback))
}

fn preview_panel_binary() -> OsString {
    binary("ROFI_FILESEARCH_PREVIEW_PANEL", "preview-panel")
}

fn file_binary() -> OsString {
    binary("ROFI_FILESEARCH_FILE", "file")
}

fn pdftoppm_binary() -> OsString {
    binary("ROFI_FILESEARCH_PDFTOPPM", "pdftoppm")
}

fn ffmpegthumbnailer_binary() -> OsString {
    binary("ROFI_FILESEARCH_FFMPEGTHUMBNAILER", "ffmpegthumbnailer")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_requested_preview_families_are_supported() {
        assert_eq!(preview_kind("text/plain"), PreviewKind::Text);
        assert_eq!(preview_kind("application/json"), PreviewKind::Text);
        assert_eq!(preview_kind("image/webp"), PreviewKind::Image);
        assert_eq!(preview_kind("application/pdf"), PreviewKind::Pdf);
        assert_eq!(preview_kind("video/mp4"), PreviewKind::Video);
        assert_eq!(preview_kind("audio/mpeg"), PreviewKind::Unsupported);
    }

    #[test]
    fn close_frame_has_no_item_payload() {
        let mut frame = Vec::new();
        write_frame(&mut frame, CLOSE, 0, &[]).unwrap();
        assert_eq!(frame.len(), 17);
        assert_eq!(frame[0], CLOSE);
    }
}
