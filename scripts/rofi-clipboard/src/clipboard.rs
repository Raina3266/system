use std::env;
use std::ffi::OsString;
use std::fs;
use std::io::{self, Read, Write};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, UNIX_EPOCH};

use anyhow::{Context, Result, bail};

use crate::model::{ClipboardItem, ItemKind};
use crate::store::ClipboardStore;

const SCREENSHOT_DIRECTORY_ENV: &str = "ROFI_CLIPBOARD_SCREENSHOT_DIR";
const GNOME_COPIED_FILES_MIME: &str = "x-special/gnome-copied-files";
const TEXT_URI_LIST_MIME: &str = "text/uri-list";
const SCREENSHOT_MATCH_ATTEMPTS: usize = 20;
const SCREENSHOT_MATCH_DELAY: Duration = Duration::from_millis(25);
const SCREENSHOT_CANDIDATE_LIMIT: usize = 16;

pub fn copy_item(store: &ClipboardStore, id: u64) -> Result<()> {
    let history = store.load()?;
    let item = history
        .items
        .iter()
        .find(|item| item.id == id)
        .context("selected clipboard item no longer exists")?;
    let (mime, bytes) = copy_payload(store, item)?;
    let mut child = Command::new(wl_copy_binary())
        .args(["--type", &mime])
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

fn copy_payload(store: &ClipboardStore, item: &ClipboardItem) -> Result<(String, Vec<u8>)> {
    if item.kind == ItemKind::File
        && item.image_file.is_none()
        && let Some(path) = item
            .name
            .as_deref()
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(Path::new)
            .filter(|path| path.is_absolute())
    {
        return Ok((
            TEXT_URI_LIST_MIME.to_owned(),
            local_file_copy_payload(path),
        ));
    }

    Ok((item.mime.clone(), store.item_bytes(item)?))
}

fn local_file_copy_payload(path: &Path) -> Vec<u8> {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";

    // Dolphin and other freedesktop-aware file managers accept local files as
    // text/uri-list. Keep the URI percent-encoded and newline-terminated.
    let mut payload = b"file://".to_vec();
    for byte in path.as_os_str().as_bytes() {
        if byte.is_ascii_alphanumeric() || matches!(*byte, b'-' | b'.' | b'_' | b'~' | b'/') {
            payload.push(*byte);
        } else {
            payload.extend_from_slice(&[
                b'%',
                HEX[(*byte >> 4) as usize],
                HEX[(*byte & 0x0f) as usize],
            ]);
        }
    }
    payload.push(b'\n');
    payload
}

pub fn capture_clipboard() -> Result<()> {
    let mut watched_bytes = Vec::new();
    io::stdin()
        .read_to_end(&mut watched_bytes)
        .context("read watched clipboard data")?;

    if let Ok("sensitive" | "nil") = env::var("CLIPBOARD_STATE").as_deref() {
        return Ok(());
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
        let source =
            clipboard_image_source(&types)?.or_else(|| screenshot_image_source(&types, &bytes));
        ClipboardStore::discover()?.add_image_named(&bytes, mime.to_owned(), source)?;
        return Ok(());
    }

    if store_file_references(&types, &watched_bytes)? {
        return Ok(());
    }

    if watched_bytes.is_empty() {
        return Ok(());
    }
    if let Ok(text) = String::from_utf8(watched_bytes) {
        let mime = preferred_text_mime(types.lines()).unwrap_or("text/plain;charset=utf-8");
        let store = ClipboardStore::discover()?;
        store_text_or_file(&store, text, mime.to_owned())?;
    }
    Ok(())
}

fn store_file_references(types: &str, watched_bytes: &[u8]) -> Result<bool> {
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
    let uri_list = decode_clipboard_text(&uri_bytes);
    let store = ClipboardStore::discover()?;
    let mut stored = false;
    for line in uri_list.lines() {
        let Some(name) = source_from_value(line) else {
            continue;
        };
        if let Some(path) = local_file_path(line)
            && let Ok(bytes) = fs::read(&path)
            && let Some(mime) = detected_image_mime(&bytes)
        {
            stored |= store
                .add_image_named(&bytes, mime.to_owned(), Some(name))?
                .is_some();
            continue;
        }

        let payload = file_reference_payload(uri_mime, &uri_list, line);
        stored |= store
            .add_file(payload, uri_mime.to_owned(), Some(name))?
            .is_some();
    }
    Ok(stored)
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

fn clipboard_image_source(types: &str) -> Result<Option<String>> {
    if let Some(source) = clipboard_source_from_mimes(types.lines().filter(|mime| {
        mime.split(';').next() == Some(TEXT_URI_LIST_MIME) || *mime == GNOME_COPIED_FILES_MIME
    }))? {
        return Ok(Some(source));
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
    if let Some(source) = clipboard_source_from_mimes(
        types
            .lines()
            .filter(|mime| is_browser_image_source_mime(mime)),
    )? {
        return Ok(Some(source));
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

fn clipboard_source_from_mimes<'a>(mimes: impl Iterator<Item = &'a str>) -> Result<Option<String>> {
    for mime in mimes {
        if let Some(text) = read_clipboard_text(mime)? {
            for line in text.lines() {
                if let Some(source) = source_from_value(line) {
                    return Ok(Some(source));
                }
            }
        }
    }
    Ok(None)
}

fn screenshot_image_source(types: &str, bytes: &[u8]) -> Option<String> {
    if !is_pathless_png(types) {
        return None;
    }

    let directory = screenshot_directory()?;
    for attempt in 0..SCREENSHOT_MATCH_ATTEMPTS {
        if let Some(path) = matching_screenshot_path(&directory, bytes) {
            return Some(path.to_string_lossy().into_owned());
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
    env::var_os(SCREENSHOT_DIRECTORY_ENV)
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            env::var_os("HOME")
                .filter(|path| !path.is_empty())
                .map(PathBuf::from)
                .map(|home| home.join("Pictures/Screenshots"))
        })
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

    candidates
        .sort_unstable_by(|left, right| right.0.cmp(&left.0).then_with(|| right.1.cmp(&left.1)));
    candidates
        .into_iter()
        .take(SCREENSHOT_CANDIDATE_LIMIT)
        .find_map(|(_, path)| (fs::read(&path).ok().as_deref() == Some(bytes)).then_some(path))
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
    if (value.starts_with("https://") || value.starts_with("http://"))
        && !value.chars().any(char::is_whitespace)
    {
        return Some(value);
    }
    local_file_path(&value).map(|path| path.to_string_lossy().into_owned())
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
            let before_is_boundary =
                src_start == 0 || tag_lower.as_bytes()[src_start - 1].is_ascii_whitespace();
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
        mime.split(';').next() == Some(TEXT_URI_LIST_MIME) || *mime == GNOME_COPIED_FILES_MIME
    })
}

fn local_file_path(line: &str) -> Option<PathBuf> {
    let value = line.trim();
    if value.is_empty() || value.starts_with('#') || matches!(value, "copy" | "cut") {
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
        store_text_or_file(&store, text, mime.to_owned())?;
    }
    Ok(())
}

fn store_text_or_file(store: &ClipboardStore, text: String, mime: String) -> Result<()> {
    if let Some(name) = standalone_file_source(&text) {
        store.add_file(text, mime, Some(name))?;
    } else {
        store.add_text(text, mime)?;
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
    preferred_mime(types, PREFERENCE, "image/")
}

fn preferred_text_mime<'a>(types: impl Iterator<Item = &'a str> + Clone) -> Option<&'a str> {
    const PREFERENCE: &[&str] = &[
        "text/plain;charset=utf-8",
        "text/plain;charset=UTF-8",
        "UTF8_STRING",
        "text/plain",
    ];
    preferred_mime(types, PREFERENCE, "text/")
}

fn preferred_mime<'a>(
    mut types: impl Iterator<Item = &'a str> + Clone,
    preference: &[&str],
    fallback_prefix: &str,
) -> Option<&'a str> {
    for preferred in preference {
        if let Some(found) = types.clone().find(|mime| *mime == *preferred) {
            return Some(found);
        }
    }
    types.find(|mime| mime.starts_with(fallback_prefix))
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
    use std::time::SystemTime;

    struct TestDirectory(PathBuf);

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn test_directory() -> TestDirectory {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        TestDirectory(env::temp_dir().join(format!(
            "rofi-clipboard-screenshot-test-{}-{unique}",
            std::process::id()
        )))
    }

    #[test]
    fn decodes_utf8_clipboard_url() {
        let value = decode_clipboard_text(b"https://example.com/image.png\n");

        assert_eq!(value, "https://example.com/image.png\n");
    }

    #[test]
    fn recognizes_standalone_urls_and_local_file_paths() {
        assert_eq!(
            standalone_file_source("https://example.com/docs/report.pdf\n").as_deref(),
            Some("https://example.com/docs/report.pdf")
        );
        assert_eq!(
            standalone_file_source("file:///home/raina/My%20Report.pdf").as_deref(),
            Some("/home/raina/My Report.pdf")
        );
        assert_eq!(
            standalone_file_source("/home/raina/My Report.pdf").as_deref(),
            Some("/home/raina/My Report.pdf")
        );
    }

    #[test]
    fn ordinary_text_that_mentions_a_url_stays_text() {
        assert!(standalone_file_source("See https://example.com for details").is_none());
        assert!(standalone_file_source("https://example.com\npage title").is_none());
    }

    #[test]
    fn builds_single_file_payloads_for_wayland_uri_targets() {
        assert_eq!(
            file_reference_payload(
                "text/uri-list",
                "file:///tmp/one.pdf\nfile:///tmp/two.pdf\n",
                "file:///tmp/two.pdf",
            ),
            "file:///tmp/two.pdf\n"
        );
        assert_eq!(
            file_reference_payload(
                "x-special/gnome-copied-files",
                "cut\nfile:///tmp/report.pdf\n",
                "file:///tmp/report.pdf",
            ),
            "cut\nfile:///tmp/report.pdf"
        );
    }

    #[test]
    fn local_paths_copy_back_as_uri_lists_for_dolphin() {
        let payload = local_file_copy_payload(Path::new("/home/raina/My Report #1.pdf"));

        assert_eq!(payload, b"file:///home/raina/My%20Report%20%231.pdf\n");
    }

    #[test]
    fn local_paths_use_uri_list_payloads_while_urls_stay_text() -> Result<()> {
        let root = test_directory();
        let store = ClipboardStore::at(root.0.join("data"));
        let mut item = ClipboardItem {
            id: 1,
            kind: ItemKind::File,
            text: Some("/home/raina/My Report.pdf".to_owned()),
            image_file: None,
            name: Some("/home/raina/My Report.pdf".to_owned()),
            mime: "text/plain;charset=utf-8".to_owned(),
            pinned: false,
            created_at: 0,
            digest: "digest".to_owned(),
        };

        assert_eq!(
            copy_payload(&store, &item)?,
            (
                TEXT_URI_LIST_MIME.to_owned(),
                b"file:///home/raina/My%20Report.pdf\n".to_vec()
            )
        );

        item.text = Some("https://example.com/report.pdf".to_owned());
        item.name = item.text.clone();
        assert_eq!(
            copy_payload(&store, &item)?,
            (
                "text/plain;charset=utf-8".to_owned(),
                b"https://example.com/report.pdf".to_vec()
            )
        );

        item.text = Some("file:///home/raina/My%20Report.pdf\n".to_owned());
        item.name = Some("/home/raina/My Report.pdf".to_owned());
        item.mime = "text/uri-list".to_owned();
        assert_eq!(
            copy_payload(&store, &item)?,
            (
                TEXT_URI_LIST_MIME.to_owned(),
                b"file:///home/raina/My%20Report.pdf\n".to_vec()
            )
        );

        item.text = Some("cut\nfile:///home/raina/My%20Report.pdf\n".to_owned());
        item.mime = GNOME_COPIED_FILES_MIME.to_owned();
        assert_eq!(
            copy_payload(&store, &item)?,
            (
                TEXT_URI_LIST_MIME.to_owned(),
                b"file:///home/raina/My%20Report.pdf\n".to_vec()
            )
        );
        Ok(())
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
        let html =
            r#"<div><img alt="photo" src="https://example.com/image.png?a=1&amp;b=2"></div>"#;

        assert_eq!(
            image_source_from_html(html).as_deref(),
            Some("https://example.com/image.png?a=1&b=2")
        );
    }

    #[test]
    fn recognizes_niri_pathless_png_selection() {
        assert!(is_pathless_png("image/png\n"));
        assert!(!is_pathless_png("image/png\ntext/plain\n"));
        assert!(!is_pathless_png("image/jpeg\n"));
    }

    #[test]
    fn finds_screenshot_with_identical_png_bytes() {
        let directory = test_directory();
        fs::create_dir_all(&directory.0).unwrap();
        let matching = directory.0.join("Screenshot from 2026-08-11 12-00-00.png");
        fs::write(&matching, b"same PNG bytes").unwrap();
        fs::write(
            directory.0.join("Screenshot from 2026-08-11 12-00-01.png"),
            b"different bytes",
        )
        .unwrap();

        assert_eq!(
            matching_screenshot_path(&directory.0, b"same PNG bytes"),
            Some(matching)
        );
    }

    #[test]
    fn does_not_guess_a_screenshot_path_from_length_alone() {
        let directory = test_directory();
        fs::create_dir_all(&directory.0).unwrap();
        fs::write(
            directory.0.join("Screenshot from 2026-08-11 12-00-00.png"),
            b"different bytes",
        )
        .unwrap();

        assert_eq!(
            matching_screenshot_path(&directory.0, b"same byte count"),
            None
        );
    }
}
