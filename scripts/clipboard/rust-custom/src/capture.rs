//! Wayland capture only. Clipboard publication belongs to the resident GTK panel.
use std::env;
use std::ffi::OsString;
use std::fs;
use std::io::{self, Read};
use std::os::unix::ffi::OsStringExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, UNIX_EPOCH};

use anyhow::{Context, Result, ensure};

use super::store::{Captured, Store};

const GNOME_COPIED_FILES_MIME: &str = "x-special/gnome-copied-files";
const TEXT_URI_LIST_MIME: &str = "text/uri-list";
const MAX_CAPTURE_BYTES: u64 = 64 * 1024 * 1024;
const SCREENSHOT_MATCH_ATTEMPTS: usize = 20;
const SCREENSHOT_MATCH_DELAY: Duration = Duration::from_millis(25);
const SCREENSHOT_CANDIDATE_LIMIT: usize = 16;

type TargetReader<'a> = dyn FnMut(&str) -> Result<Option<Vec<u8>>> + 'a;

fn read_limited(reader: impl Read, limit: u64) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader.take(limit + 1).read_to_end(&mut bytes)?;
    ensure!(bytes.len() as u64 <= limit, "Clipboard data is too large");
    Ok(bytes)
}

pub fn read_stdin() -> Result<Vec<u8>> {
    read_limited(io::stdin(), MAX_CAPTURE_BYTES)
}

fn read_target(binary: &std::ffi::OsStr, args: &[&str]) -> Result<Vec<u8>> {
    let mut child = Command::new(binary)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .context("Start clipboard reader")?;
    let result = read_limited(
        child
            .stdout
            .take()
            .context("Open clipboard reader output")?,
        MAX_CAPTURE_BYTES,
    );
    if result.is_err() {
        let _ = child.kill();
    }
    let status = child.wait().context("Wait for clipboard reader")?;
    let bytes = result?;
    ensure!(status.success(), "Could not read clipboard target");
    Ok(bytes)
}

pub fn capture_clipboard() -> Result<()> {
    let watched = read_stdin()?;
    if matches!(
        env::var("CLIPBOARD_STATE").as_deref(),
        Ok("sensitive" | "nil")
    ) {
        return Ok(());
    }
    let binary = env::var_os("CLIPBOARD_WL_PASTE")
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "wl-paste".into());
    let types = String::from_utf8(
        read_target(&binary, &["--list-types"]).context("List clipboard MIME types")?,
    )
    .context("Invalid clipboard MIME types")?;
    let mut read = |mime: &str| {
        let bytes = read_target(&binary, &["--no-newline", "--type", mime])?;
        Ok((!bytes.is_empty()).then_some(bytes))
    };
    let entries = collect(
        &types,
        watched,
        &mut read,
        screenshot_directory().as_deref(),
    )?;
    if !entries.is_empty() {
        Store::discover()?.capture(entries)?;
    }
    Ok(())
}

pub fn store_stdin(mime: &str) -> Result<()> {
    let bytes = read_stdin()?;
    let entries = entries_for_mime(mime, bytes)?;
    if !entries.is_empty() {
        Store::discover()?.capture(entries)?;
    }
    Ok(())
}

fn collect(
    types: &str,
    watched: Vec<u8>,
    read: &mut TargetReader<'_>,
    screenshots: Option<&Path>,
) -> Result<Vec<Captured>> {
    if types.lines().any(is_sensitive_mime) {
        return Ok(Vec::new());
    }
    if let Some(mime) = preferred_image_mime(types.lines()) {
        let bytes = if bytes_match_mime(&watched, mime) {
            watched
        } else {
            read(mime)?.context("Clipboard image is unavailable")?
        };
        let name = clipboard_image_source(types, read)?
            .or_else(|| screenshot_image_source(types, &bytes, screenshots));
        return Ok(vec![Captured::Image {
            bytes,
            mime: mime.into(),
            name,
        }]);
    }
    if let Some(mime) = preferred_uri_mime(types.lines()) {
        let bytes = read(mime)?.unwrap_or(watched);
        let entries = file_references(
            mime,
            &decode_clipboard_text(&bytes),
            preferred_text_mime(types.lines()).unwrap_or("text/plain;charset=utf-8"),
        );
        if !entries.is_empty() {
            return Ok(entries);
        }
        return Ok(Vec::new());
    }
    // Never turn an arbitrary binary target into text merely because its bytes
    // happen to be UTF-8. The watch payload is used verbatim for text targets.
    let Some(mime) = preferred_text_mime(types.lines()) else {
        return Ok(Vec::new());
    };
    match String::from_utf8(watched) {
        Ok(text) => Ok(text_or_file(text, mime)),
        Err(_) => Ok(Vec::new()),
    }
}

fn entries_for_mime(mime: &str, bytes: Vec<u8>) -> Result<Vec<Captured>> {
    if bytes.is_empty() || is_sensitive_mime(mime) {
        return Ok(Vec::new());
    }
    if mime.starts_with("image/") {
        return Ok(vec![Captured::Image {
            bytes,
            mime: mime.into(),
            name: None,
        }]);
    }
    let text = String::from_utf8(bytes).context("Clipboard text is not UTF-8")?;
    if preferred_uri_mime(std::iter::once(mime)).is_some() {
        Ok(file_references(mime, &text, "text/plain;charset=utf-8"))
    } else {
        Ok(text_or_file(text, mime))
    }
}

fn file_references(mime: &str, text: &str, text_mime: &str) -> Vec<Captured> {
    let mut entries = Vec::new();
    for line in text.lines() {
        if let Some(url) = url_value(line) {
            entries.push(Captured::Text {
                text: url,
                mime: text_mime.into(),
            });
            continue;
        }
        let Some(name) = source_from_value(line) else {
            continue;
        };
        if let Some(path) = local_file_path(line)
            && let Some(bytes) = read_regular_file(&path)
            && let Some(mime) = detected_image_mime(&bytes)
        {
            entries.push(Captured::Image {
                bytes,
                mime: mime.into(),
                name: Some(name),
            });
            continue;
        }
        entries.push(Captured::File {
            text: file_reference_payload(mime, text, line),
            mime: mime.into(),
            name: Some(name),
        });
    }
    entries
}

fn file_reference_payload(mime: &str, clipboard_text: &str, reference: &str) -> String {
    let reference = reference.trim();
    if mime == GNOME_COPIED_FILES_MIME {
        let action = clipboard_text
            .lines()
            .map(str::trim)
            .find(|line| matches!(*line, "copy" | "cut"))
            .unwrap_or("copy");
        format!("{action}\n{reference}")
    } else {
        format!("{reference}\n")
    }
}

fn text_or_file(text: String, mime: &str) -> Vec<Captured> {
    if text.is_empty() {
        return Vec::new();
    }
    let entry = if let Some(source) = standalone_file_source(&text) {
        if let Some(url) = url_value(&source) {
            Captured::Text {
                text: url,
                mime: mime.into(),
            }
        } else {
            Captured::File {
                text,
                mime: mime.into(),
                name: Some(source),
            }
        }
    } else {
        Captured::Text {
            text,
            mime: mime.into(),
        }
    };
    vec![entry]
}

fn clipboard_image_source(types: &str, read: &mut TargetReader<'_>) -> Result<Option<String>> {
    if let Some(source) = clipboard_source_from_mimes(
        types.lines().filter(|mime| {
            mime.split(';').next() == Some(TEXT_URI_LIST_MIME) || *mime == GNOME_COPIED_FILES_MIME
        }),
        read,
    )? {
        return Ok(Some(source));
    }
    if let Some(mime) = types
        .lines()
        .find(|mime| mime.split(';').next() == Some("text/html"))
        && let Some(bytes) = read(mime)?
        && let Some(source) = image_source_from_html(&decode_clipboard_text(&bytes))
    {
        return Ok(Some(source));
    }
    // An image's HTML src is more specific than the browser's page URL.
    if let Some(source) = clipboard_source_from_mimes(
        types
            .lines()
            .filter(|mime| is_browser_image_source_mime(mime)),
        read,
    )? {
        return Ok(Some(source));
    }
    if let Some(mime) = types
        .lines()
        .find(|mime| mime.split(';').next() == Some("text/plain"))
        && let Some(bytes) = read(mime)?
        && let Some(source) = source_from_value(decode_clipboard_text(&bytes).trim())
    {
        return Ok(Some(source));
    }
    Ok(None)
}

fn clipboard_source_from_mimes<'a>(
    mimes: impl Iterator<Item = &'a str>,
    read: &mut TargetReader<'_>,
) -> Result<Option<String>> {
    for mime in mimes {
        if let Some(bytes) = read(mime)? {
            for line in decode_clipboard_text(&bytes).lines() {
                if let Some(source) = source_from_value(line) {
                    return Ok(Some(source));
                }
            }
        }
    }
    Ok(None)
}

fn screenshot_image_source(types: &str, bytes: &[u8], directory: Option<&Path>) -> Option<String> {
    if !is_pathless_png(types) {
        return None;
    }
    let directory = directory?;
    for attempt in 0..SCREENSHOT_MATCH_ATTEMPTS {
        if let Some(path) = matching_screenshot_path(directory, bytes) {
            return path.to_str().map(str::to_owned);
        }
        if attempt + 1 < SCREENSHOT_MATCH_ATTEMPTS {
            thread::sleep(SCREENSHOT_MATCH_DELAY);
        }
    }
    None
}

fn is_pathless_png(types: &str) -> bool {
    let mut types = types.lines().filter(|mime| !mime.is_empty());
    types.next() == Some("image/png") && types.next().is_none()
}

fn screenshot_directory() -> Option<PathBuf> {
    env::var_os("CLIPBOARD_SCREENSHOT_DIR")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            env::var_os("HOME")
                .filter(|path| !path.is_empty())
                .map(|home| PathBuf::from(home).join("Pictures/Screenshots"))
        })
}

fn read_regular_file(path: &Path) -> Option<Vec<u8>> {
    let metadata = fs::metadata(path).ok()?;
    // URI lists may include devices, directories or FIFOs. Keep these as file
    // references without opening them or allocating unbounded image buffers.
    if !metadata.is_file() || metadata.len() > MAX_CAPTURE_BYTES {
        return None;
    }
    read_limited(fs::File::open(path).ok()?, MAX_CAPTURE_BYTES).ok()
}

fn matching_screenshot_path(directory: &Path, bytes: &[u8]) -> Option<PathBuf> {
    let mut candidates: Vec<_> = fs::read_dir(directory)
        .ok()?
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let metadata = entry.metadata().ok()?;
            if !metadata.is_file() || metadata.len() != bytes.len() as u64 {
                return None;
            }
            Some((metadata.modified().unwrap_or(UNIX_EPOCH), entry.path()))
        })
        .collect();
    candidates.sort_unstable_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.cmp(&a.1)));
    candidates
        .into_iter()
        .take(SCREENSHOT_CANDIDATE_LIMIT)
        .find_map(|(_, path)| (read_regular_file(&path).as_deref() == Some(bytes)).then_some(path))
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

fn decode_clipboard_text(bytes: &[u8]) -> String {
    if let Some(bytes) = bytes.strip_prefix(&[0xff, 0xfe]) {
        return decode_utf16(bytes, u16::from_le_bytes);
    }
    if let Some(bytes) = bytes.strip_prefix(&[0xfe, 0xff]) {
        return decode_utf16(bytes, u16::from_be_bytes);
    }
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
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| from_bytes(*pair))
        .take_while(|word| *word != 0)
        .collect();
    String::from_utf16_lossy(&words)
}

fn url_value(value: &str) -> Option<String> {
    let value = value.trim().replace("&amp;", "&");
    (value.starts_with("http://") || value.starts_with("https://"))
        .then_some(value)
        .filter(|value| !value.chars().any(char::is_whitespace))
}

fn source_from_value(value: &str) -> Option<String> {
    if let Some(url) = url_value(value) {
        return Some(url);
    }
    local_file_path(value.trim()).and_then(|path| path.into_os_string().into_string().ok())
}

fn standalone_file_source(text: &str) -> Option<String> {
    let value = text.trim();
    if value.is_empty() || value.lines().count() != 1 {
        return None;
    }
    source_from_value(value)
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
            let boundary =
                src_start == 0 || tag_lower.as_bytes()[src_start - 1].is_ascii_whitespace();
            let mut cursor = src_start + 3;
            while tag_lower
                .as_bytes()
                .get(cursor)
                .is_some_and(u8::is_ascii_whitespace)
            {
                cursor += 1;
            }
            if boundary && tag_lower.as_bytes().get(cursor) == Some(&b'=') {
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
                if let Some(source) =
                    source_from_value(&tag_original[value_start..value_end].replace("&amp;", "&"))
                {
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
        mime.split(';').next() == Some(TEXT_URI_LIST_MIME) || *mime == GNOME_COPIED_FILES_MIME
    })
}

fn local_file_path(line: &str) -> Option<PathBuf> {
    let value = line.trim();
    if value.is_empty() || value.starts_with('#') || matches!(value, "copy" | "cut") {
        return None;
    }
    let Some(value) = value.strip_prefix("file://") else {
        // A literal filesystem path is not a URI. Decoding '%' here can target
        // a different source file when the panel later copies or trashes it.
        return (value.starts_with('/') && !value.contains('\0')).then(|| PathBuf::from(value));
    };
    let encoded = value.strip_prefix("localhost").unwrap_or(value);
    if !encoded.starts_with('/') {
        return None;
    }
    let mut decoded = Vec::with_capacity(encoded.len());
    let mut bytes = encoded.bytes();
    while let Some(byte) = bytes.next() {
        let byte = if byte == b'%' {
            let high = char::from(bytes.next()?).to_digit(16)?;
            let low = char::from(bytes.next()?).to_digit(16)?;
            ((high << 4) | low) as u8
        } else {
            byte
        };
        if byte == 0 {
            return None;
        }
        decoded.push(byte);
    }
    Some(PathBuf::from(OsString::from_vec(decoded)))
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

fn preferred_image_mime<'a>(types: impl Iterator<Item = &'a str> + Clone) -> Option<&'a str> {
    preferred_mime(
        types,
        &[
            "image/png",
            "image/jpeg",
            "image/webp",
            "image/gif",
            "image/bmp",
            "image/tiff",
            "image/svg+xml",
        ],
        "image/",
    )
}

fn preferred_text_mime<'a>(types: impl Iterator<Item = &'a str> + Clone) -> Option<&'a str> {
    preferred_mime(
        types,
        &[
            "text/plain;charset=utf-8",
            "text/plain;charset=UTF-8",
            "UTF8_STRING",
            "text/plain",
        ],
        "text/",
    )
}

fn preferred_mime<'a>(
    mut types: impl Iterator<Item = &'a str> + Clone,
    preference: &[&str],
    fallback: &str,
) -> Option<&'a str> {
    for preferred in preference {
        if let Some(found) = types.clone().find(|mime| *mime == *preferred) {
            return Some(found);
        }
    }
    types.find(|mime| mime.starts_with(fallback))
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
        "image/svg+xml" => std::str::from_utf8(bytes).is_ok_and(|text| text.contains("<svg")),
        _ => false,
    }
}

#[cfg(test)]
crate::clipboard_unit_tests!(capture);
