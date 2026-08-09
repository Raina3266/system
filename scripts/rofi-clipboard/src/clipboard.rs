use std::env;
use std::ffi::OsString;
use std::fs;
use std::io::{self, Read, Write};
use std::os::unix::ffi::OsStringExt;
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
        let source = clipboard_image_source(&types)?;
        ClipboardStore::discover()?.add_image_named(&bytes, mime.to_owned(), source)?;
        return Ok(());
    }

    if store_local_image_files(&types, &watched_bytes)? {
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

fn store_local_image_files(types: &str, watched_bytes: &[u8]) -> Result<bool> {
    let Some(uri_mime) = preferred_uri_mime(types.lines()) else {
        return Ok(false);
    };
    let output = Command::new(wl_paste_binary())
        .args(["--type", uri_mime])
        .output()
        .with_context(|| format!("read clipboard as {uri_mime}"))?;
    let uri_bytes = if output.status.success() && !output.stdout.is_empty() {
        output.stdout
    } else {
        watched_bytes.to_vec()
    };
    let uri_list = String::from_utf8_lossy(&uri_bytes);
    let paths: Vec<_> = uri_list.lines().filter_map(local_file_path).collect();
    if paths.is_empty() {
        return Ok(false);
    }

    let store = ClipboardStore::discover()?;
    let mut stored = false;
    for path in paths {
        let Ok(bytes) = fs::read(&path) else {
            continue;
        };
        let Some(mime) = detected_image_mime(&bytes) else {
            continue;
        };
        let name = Some(path.to_string_lossy().into_owned());
        stored |= store
            .add_image_named(&bytes, mime.to_owned(), name)?
            .is_some();
    }
    Ok(stored)
}

fn clipboard_image_source(types: &str) -> Result<Option<String>> {
    for mime in types.lines().filter(|mime| {
        mime.split(';').next() == Some("text/uri-list")
            || *mime == "x-special/gnome-copied-files"
    }) {
        if let Some(text) = read_clipboard_text(mime)? {
            for line in text.lines() {
                if let Some(source) = source_from_value(line) {
                    return Ok(Some(source));
                }
            }
        }
    }

    if let Some(mime) = types
        .lines()
        .find(|mime| mime.split(';').next() == Some("text/html"))
        && let Some(html) = read_clipboard_text(mime)?
        && let Some(source) = image_source_from_html(&html)
    {
        return Ok(Some(source));
    }

    // Browsers can put image bytes and their origin on the clipboard as
    // separate targets. Chromium uses chromium/x-source-url on Linux, while
    // Firefox and compatible applications use one of the Mozilla URL flavors.
    // Prefer an <img src> above because Chromium's source URL can be the page
    // containing the image rather than the image itself.
    for mime in types.lines().filter(|mime| is_browser_image_source_mime(mime)) {
        if let Some(text) = read_clipboard_text(mime)? {
            for line in text.lines() {
                if let Some(source) = source_from_value(line) {
                    return Ok(Some(source));
                }
            }
        }
    }

    if let Some(mime) = types
        .lines()
        .find(|mime| mime.split(';').next() == Some("text/plain"))
        && let Some(text) = read_clipboard_text(mime)?
        && let Some(source) = source_from_value(text.trim())
    {
        return Ok(Some(source));
    }

    Ok(None)
}

fn is_browser_image_source_mime(mime: &str) -> bool {
    matches!(
        mime.split(';').next().unwrap_or(mime),
        "chromium/x-source-url"
            | "text/x-moz-url"
            | "text/x-moz-url-data"
            | "application/x-moz-file-promise-url"
    )
}

fn read_clipboard_text(mime: &str) -> Result<Option<String>> {
    let output = Command::new(wl_paste_binary())
        .args(["--type", mime])
        .output()
        .with_context(|| format!("read clipboard source as {mime}"))?;
    if !output.status.success() || output.stdout.is_empty() {
        return Ok(None);
    }
    Ok(Some(decode_clipboard_text(&output.stdout)))
}

fn decode_clipboard_text(bytes: &[u8]) -> String {
    if let Some(bytes) = bytes.strip_prefix(&[0xff, 0xfe]) {
        return decode_utf16(bytes, u16::from_le_bytes);
    }
    if let Some(bytes) = bytes.strip_prefix(&[0xfe, 0xff]) {
        return decode_utf16(bytes, u16::from_be_bytes);
    }

    // text/x-moz-url is commonly UTF-16 without a BOM. Detect the byte order
    // from the NUL bytes used by ASCII URL characters before falling back to
    // the UTF-8 used by Wayland-native clipboard targets.
    let pairs = bytes.len() / 2;
    if pairs > 0 {
        let even_nuls = bytes.iter().step_by(2).filter(|byte| **byte == 0).count();
        let odd_nuls = bytes
            .iter()
            .skip(1)
            .step_by(2)
            .filter(|byte| **byte == 0)
            .count();
        if odd_nuls > pairs / 2 && even_nuls < pairs / 4 {
            return decode_utf16(bytes, u16::from_le_bytes);
        }
        if even_nuls > pairs / 2 && odd_nuls < pairs / 4 {
            return decode_utf16(bytes, u16::from_be_bytes);
        }
    }

    String::from_utf8_lossy(bytes)
        .trim_end_matches('\0')
        .to_owned()
}

fn decode_utf16(bytes: &[u8], from_bytes: fn([u8; 2]) -> u16) -> String {
    let words: Vec<_> = bytes
        .chunks_exact(2)
        .map(|pair| from_bytes([pair[0], pair[1]]))
        .take_while(|word| *word != 0)
        .collect();
    String::from_utf16_lossy(&words)
}

fn source_from_value(value: &str) -> Option<String> {
    let value = value.trim().replace("&amp;", "&");
    if value.starts_with("https://") || value.starts_with("http://") {
        return Some(value);
    }
    local_file_path(&value).map(|path| path.to_string_lossy().into_owned())
}

fn image_source_from_html(html: &str) -> Option<String> {
    let lowercase = html.to_ascii_lowercase();
    let mut offset = 0;
    while let Some(relative_start) = lowercase[offset..].find("<img") {
        let tag_start = offset + relative_start;
        let tag_end = tag_start + lowercase[tag_start..].find('>')?;
        let tag_lower = &lowercase[tag_start..tag_end];
        let tag_original = &html[tag_start..tag_end];
        let mut search_from = 0;

        while let Some(relative_src) = tag_lower[search_from..].find("src") {
            let src_start = search_from + relative_src;
            let before_is_boundary = src_start == 0
                || tag_lower.as_bytes()[src_start - 1].is_ascii_whitespace();
            let mut cursor = src_start + 3;
            while tag_lower
                .as_bytes()
                .get(cursor)
                .is_some_and(u8::is_ascii_whitespace)
            {
                cursor += 1;
            }
            if before_is_boundary && tag_lower.as_bytes().get(cursor) == Some(&b'=') {
                cursor += 1;
                while tag_lower
                    .as_bytes()
                    .get(cursor)
                    .is_some_and(u8::is_ascii_whitespace)
                {
                    cursor += 1;
                }
                let quote = *tag_original.as_bytes().get(cursor)?;
                let value_start = if matches!(quote, b'\'' | b'"') {
                    cursor + 1
                } else {
                    cursor
                };
                let value_end = if matches!(quote, b'\'' | b'"') {
                    tag_original[value_start..].find(char::from(quote))? + value_start
                } else {
                    tag_original[value_start..]
                        .find(char::is_whitespace)
                        .map(|end| value_start + end)
                        .unwrap_or(tag_original.len())
                };
                if let Some(source) = source_from_value(&tag_original[value_start..value_end]) {
                    return Some(source);
                }
            }
            search_from = src_start + 3;
        }
        offset = tag_end + 1;
    }
    None
}

fn preferred_uri_mime<'a>(types: impl Iterator<Item = &'a str>) -> Option<&'a str> {
    types.into_iter().find(|mime| {
        mime.split(';').next() == Some("text/uri-list")
            || *mime == "x-special/gnome-copied-files"
    })
}

fn local_file_path(line: &str) -> Option<PathBuf> {
    let value = line.trim();
    if value.is_empty()
        || value.starts_with('#')
        || matches!(value, "copy" | "cut")
    {
        return None;
    }

    let encoded_path = if let Some(value) = value.strip_prefix("file://") {
        if let Some(value) = value.strip_prefix("localhost") {
            value.starts_with('/').then_some(value)?
        } else {
            value.starts_with('/').then_some(value)?
        }
    } else {
        value.starts_with('/').then_some(value)?
    };
    let decoded = percent_decode(encoded_path)?;
    let path = PathBuf::from(OsString::from_vec(decoded));
    path.is_absolute().then_some(path)
}

fn percent_decode(value: &str) -> Option<Vec<u8>> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let high = hex_value(*bytes.get(index + 1)?)?;
            let low = hex_value(*bytes.get(index + 2)?)?;
            let byte = (high << 4) | low;
            if byte == 0 {
                return None;
            }
            decoded.push(byte);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    Some(decoded)
}

fn hex_value(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn detected_image_mime(bytes: &[u8]) -> Option<&'static str> {
    [
        "image/png",
        "image/jpeg",
        "image/gif",
        "image/webp",
        "image/bmp",
        "image/tiff",
        "image/svg+xml",
    ]
    .into_iter()
    .find(|mime| bytes_match_mime(bytes, mime))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_utf8_clipboard_url() {
        let value = decode_clipboard_text(b"https://example.com/image.png\n");

        assert_eq!(value, "https://example.com/image.png\n");
    }

    #[test]
    fn decodes_bomless_utf16_little_endian_mozilla_url() {
        let mut bytes = Vec::new();
        for word in "https://example.com/image.png\nImage title".encode_utf16() {
            bytes.extend_from_slice(&word.to_le_bytes());
        }

        assert_eq!(
            decode_clipboard_text(&bytes),
            "https://example.com/image.png\nImage title"
        );
    }

    #[test]
    fn recognizes_chromium_and_mozilla_image_source_targets() {
        assert!(is_browser_image_source_mime("chromium/x-source-url"));
        assert!(is_browser_image_source_mime("text/x-moz-url"));
        assert!(is_browser_image_source_mime(
            "text/x-moz-url-data;charset=utf-8"
        ));
        assert!(!is_browser_image_source_mime("image/png"));
    }

    #[test]
    fn extracts_image_source_from_html() {
        let html = r#"<div><img alt="photo" src="https://example.com/image.png?a=1&amp;b=2"></div>"#;

        assert_eq!(
            image_source_from_html(html).as_deref(),
            Some("https://example.com/image.png?a=1&b=2")
        );
    }
}