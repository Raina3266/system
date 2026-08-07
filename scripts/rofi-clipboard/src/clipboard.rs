use std::env;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};

use crate::store::ClipboardStore;

pub fn copy_item(store: &ClipboardStore, id: u64) -> Result<()> {
    let history = store.load()?;
    let item = history
        .items
        .iter()
        .find(|item| item.id == id)
        .context("selected clipboard item no longer exists")?;
    let bytes = store.item_bytes(item)?;
    let mut child = Command::new(wl_copy_binary())
        .args(["--type", &item.mime])
        .stdin(Stdio::piped())
        .spawn()
        .context("launch wl-copy")?;
    child
        .stdin
        .as_mut()
        .context("open wl-copy stdin")?
        .write_all(&bytes)
        .context("write clipboard data")?;
    let status = child.wait().context("wait for wl-copy")?;
    if !status.success() {
        bail!("wl-copy exited with {status}");
    }
    Ok(())
}

pub fn capture_clipboard() -> Result<()> {
    let mut watched_bytes = Vec::new();
    io::stdin()
        .read_to_end(&mut watched_bytes)
        .context("read watched clipboard data")?;

    match env::var("CLIPBOARD_STATE").as_deref() {
        Ok("sensitive" | "nil") => return Ok(()),
        _ => {}
    }

    let types_output = Command::new(wl_paste_binary())
        .arg("--list-types")
        .output()
        .context("list clipboard MIME types")?;
    let types = String::from_utf8_lossy(&types_output.stdout);
    if types.lines().any(is_sensitive_mime) {
        return Ok(());
    }

    if let Some(mime) = preferred_image_mime(types.lines()) {
        let bytes = if bytes_match_mime(&watched_bytes, mime) {
            watched_bytes
        } else {
            let output = Command::new(wl_paste_binary())
                .args(["--type", mime])
                .output()
                .with_context(|| format!("read clipboard as {mime}"))?;
            if !output.status.success() {
                return Ok(());
            }
            output.stdout
        };
        ClipboardStore::discover()?.add_image(&bytes, mime.to_owned())?;
        return Ok(());
    }

    if watched_bytes.is_empty() {
        return Ok(());
    }
    if let Ok(text) = String::from_utf8(watched_bytes) {
        let mime = preferred_text_mime(types.lines()).unwrap_or("text/plain;charset=utf-8");
        ClipboardStore::discover()?.add_text(text, mime.to_owned())?;
    }
    Ok(())
}

pub fn store_stdin(mime: &str) -> Result<()> {
    let mut bytes = Vec::new();
    io::stdin().read_to_end(&mut bytes).context("read stdin")?;
    let store = ClipboardStore::discover()?;
    if mime.starts_with("image/") {
        store.add_image(&bytes, mime.to_owned())?;
    } else {
        let text = String::from_utf8(bytes).context("clipboard text is not UTF-8")?;
        store.add_text(text, mime.to_owned())?;
    }
    Ok(())
}

fn preferred_image_mime<'a>(types: impl Iterator<Item = &'a str> + Clone) -> Option<&'a str> {
    const PREFERENCE: &[&str] = &[
        "image/png",
        "image/jpeg",
        "image/webp",
        "image/gif",
        "image/bmp",
        "image/tiff",
        "image/svg+xml",
    ];
    for preferred in PREFERENCE {
        if let Some(found) = types.clone().find(|mime| *mime == *preferred) {
            return Some(found);
        }
    }
    types.filter(|mime| mime.starts_with("image/")).next()
}

fn preferred_text_mime<'a>(types: impl Iterator<Item = &'a str> + Clone) -> Option<&'a str> {
    const PREFERENCE: &[&str] = &[
        "text/plain;charset=utf-8",
        "text/plain;charset=UTF-8",
        "UTF8_STRING",
        "text/plain",
    ];
    for preferred in PREFERENCE {
        if let Some(found) = types.clone().find(|mime| *mime == *preferred) {
            return Some(found);
        }
    }
    types.filter(|mime| mime.starts_with("text/")).next()
}

fn is_sensitive_mime(mime: &str) -> bool {
    matches!(
        mime,
        "x-kde-passwordManagerHint" | "application/x-kde-passwordManagerHint"
    )
}

fn bytes_match_mime(bytes: &[u8], mime: &str) -> bool {
    match mime {
        "image/png" => bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        "image/jpeg" => bytes.starts_with(b"\xff\xd8\xff"),
        "image/gif" => bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a"),
        "image/webp" => bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP"),
        "image/bmp" => bytes.starts_with(b"BM"),
        "image/tiff" => bytes.starts_with(b"II*\0") || bytes.starts_with(b"MM\0*"),
        "image/svg+xml" => std::str::from_utf8(bytes)
            .map(|text| text.contains("<svg"))
            .unwrap_or(false),
        _ => false,
    }
}

fn wl_copy_binary() -> PathBuf {
    env_binary("ROFI_CLIPBOARD_WL_COPY", "wl-copy")
}

fn wl_paste_binary() -> PathBuf {
    env_binary("ROFI_CLIPBOARD_WL_PASTE", "wl-paste")
}

fn env_binary(variable: &str, fallback: &str) -> PathBuf {
    env::var_os(variable)
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(fallback).to_path_buf())
}
