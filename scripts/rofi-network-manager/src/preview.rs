use std::env;
use std::fs;
use std::io::{self, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::Duration;

use crate::AppResult;
use crate::model::InfoContent;

pub const SOCKET_ENV: &str = "ROFI_NETWORK_PREVIEW_SOCKET";
const CLOSE: u8 = 2;
const UPDATE_NETWORK: u8 = 7;

pub fn session_socket_path() -> AppResult<PathBuf> {
    let runtime = env::var_os("XDG_RUNTIME_DIR")
        .ok_or_else(|| io::Error::other("XDG_RUNTIME_DIR is not set"))?;
    Ok(PathBuf::from(runtime).join(format!(
        "rofi-network-preview-{}.sock",
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

pub fn is_open() -> bool {
    socket_from_environment().is_some_and(|path| path.exists())
}

pub fn show(content: &InfoContent, serial: u64) -> AppResult<()> {
    let path = required_socket_from_environment()?;
    if !path.exists() {
        cleanup(&path)?;
        launch_panel(&path)?;
    }
    update_at(&path, content, serial)
}

pub fn update_if_open(content: &InfoContent, serial: u64) -> AppResult<()> {
    let Some(path) = socket_from_environment().filter(|path| path.exists()) else {
        return Ok(());
    };
    update_at(&path, content, serial)
}

pub fn close() {
    let Some(path) = socket_from_environment() else {
        return;
    };
    close_at(&path);
}

pub fn close_at(path: &Path) {
    let _ = send(path, CLOSE, 0, &[]);
    for _ in 0..100 {
        if !path.exists() {
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
    let _ = cleanup(path);
}

fn launch_panel(path: &Path) -> AppResult<()> {
    let mut command = Command::new(preview_panel_binary());
    command
        .args([
            "--stdin",
            "--title",
            "Network information",
            "--read-only",
            "--panel",
            "--listen",
        ])
        .arg(path);
    command
        .arg("--layout-file")
        .arg(crate::rofi::theme_path()?);
    append_override(&mut command, "ROFI_NETWORK_PREVIEW_WIDTH", "--width");
    append_override(&mut command, "ROFI_NETWORK_PREVIEW_HEIGHT", "--height");
    append_override(
        &mut command,
        "ROFI_NETWORK_ROFI_WIDTH",
        "--companion-width",
    );
    command.stdin(Stdio::piped()).stdout(Stdio::null());
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

fn update_at(path: &Path, content: &InfoContent, serial: u64) -> AppResult<()> {
    let png = match content.qr_payload.as_deref() {
        Some(payload) => match qr_png(payload) {
            Ok(png) => png,
            Err(error) => {
                eprintln!("rofi-network-manager: generate QR code: {error}");
                Vec::new()
            }
        },
        None => Vec::new(),
    };
    let details = content.details.as_bytes();
    let mut payload = Vec::with_capacity(16 + details.len() + png.len());
    payload.extend_from_slice(&content.id.to_be_bytes());
    payload.extend_from_slice(&(details.len() as u64).to_be_bytes());
    payload.extend_from_slice(details);
    payload.extend_from_slice(&png);
    if !send(path, UPDATE_NETWORK, serial, &payload)? {
        return Err(io::Error::other("preview-panel closed before the update arrived").into());
    }
    Ok(())
}

fn qr_png(payload: &str) -> AppResult<Vec<u8>> {
    // Feed the Wi-Fi payload through stdin so the password never appears in
    // qrencode's process arguments or in a temporary file.
    let mut child = Command::new(qrencode_binary())
        .args(["-t", "PNG", "-s", "7", "-m", "3", "-o", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;
    let mut input = child
        .stdin
        .take()
        .ok_or_else(|| io::Error::other("qrencode standard input is unavailable"))?;
    input.write_all(payload.as_bytes())?;
    drop(input);
    let output = child.wait_with_output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "qrencode exited with {}",
            output.status
        ))
        .into());
    }
    Ok(output.stdout)
}

fn send(path: &Path, operation: u8, serial: u64, payload: &[u8]) -> AppResult<bool> {
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
    write_frame(&mut stream, operation, serial, payload)?;
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
    socket_from_environment()
        .ok_or_else(|| io::Error::other("preview is only available inside the Rofi session").into())
}

fn preview_panel_binary() -> PathBuf {
    env::var_os("ROFI_NETWORK_PREVIEW_PANEL")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new("preview-panel").to_path_buf())
}

fn qrencode_binary() -> PathBuf {
    env::var_os("ROFI_NETWORK_QRENCODE")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new("qrencode").to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn network_frame_contains_serial_and_exact_payload_length() {
        let mut frame = Vec::new();
        write_frame(&mut frame, UPDATE_NETWORK, 23, b"details").unwrap();
        assert_eq!(frame[0], UPDATE_NETWORK);
        assert_eq!(u64::from_be_bytes(frame[1..9].try_into().unwrap()), 23);
        assert_eq!(u64::from_be_bytes(frame[9..17].try_into().unwrap()), 7);
        assert_eq!(&frame[17..], b"details");
    }
}
