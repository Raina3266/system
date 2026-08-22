use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};

use crate::model::{ClipboardItem, History, ItemKind};

pub(super) const LOCK_SHARED: i32 = 1;
pub(super) const LOCK_EXCLUSIVE: i32 = 2;
const LOCK_UN: i32 = 8;
pub(super) const MAX_HISTORY_ITEMS: usize = 2000;
pub(super) const MAX_IMAGE_ITEMS: usize = 100;

unsafe extern "C" {
    fn flock(fd: i32, operation: i32) -> i32;
}

pub(super) fn order_pinned_first(history: &mut History) {
    // Stable sorting keeps the existing newest-first order inside each
    // section while repairing older histories that mixed pinned and normal
    // items together.
    history.items.sort_by_key(|item| !item.pinned);
}

pub(super) fn item_position(history: &History, id: u64) -> Option<usize> {
    history.items.iter().position(|item| item.id == id)
}

pub(super) fn ensure_memo_draft(history: &mut History) -> u64 {
    let draft_id = history
        .items
        .iter()
        .find(|item| item.is_empty_memo())
        .map(|item| item.id);

    if let Some(draft_id) = draft_id {
        // Empty memos are interchangeable drafts. Keep a single stable row so
        // histories created by the old button behavior do not accumulate
        // several "New memo" entries.
        history
            .items
            .retain(|item| !item.is_empty_memo() || item.id == draft_id);
        let position = history
            .items
            .iter()
            .position(|item| item.id == draft_id)
            .expect("retained memo draft");
        let mut draft = history.items.remove(position);
        draft.pinned = false;
        history.items.insert(0, draft);
        return draft_id;
    }

    let id = take_id(history);
    history.items.insert(
        0,
        ClipboardItem {
            id,
            kind: ItemKind::Memo,
            text: Some(String::new()),
            image_file: None,
            name: None,
            mime: "text/plain;charset=utf-8".to_owned(),
            pinned: false,
            created_at: unix_timestamp(),
            digest: digest(b""),
        },
    );
    id
}

pub(super) fn linked_local_file_is_missing(item: &ClipboardItem) -> bool {
    if item.kind != ItemKind::File {
        return false;
    }
    let Some(source) = item
        .name
        .as_deref()
        .map(str::trim)
        .filter(|source| !source.is_empty())
    else {
        return false;
    };
    let path = Path::new(source);
    if !path.is_absolute() {
        return false;
    }

    match fs::metadata(path) {
        Ok(_) => false,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => true,
        Err(error) => {
            eprintln!(
                "rofi-clipboard: failed to check linked file {}: {error}",
                path.display()
            );
            false
        }
    }
}

pub(super) fn cached_image_filename(item: &ClipboardItem) -> Option<String> {
    item.image_file
        .as_deref()
        .and_then(safe_filename)
        .map(str::to_owned)
}

pub(super) fn trim_history(history: &mut History) -> Vec<String> {
    let mut removed_images = Vec::new();
    let mut excess_images = history
        .items
        .iter()
        .filter(|item| item.image_file.is_some())
        .count()
        .saturating_sub(MAX_IMAGE_ITEMS);

    // Items are kept newest-first. Remove images from the end so text entries
    // do not count toward the independent local-image limit.
    for index in (0..history.items.len()).rev() {
        if excess_images == 0 {
            break;
        }
        if history.items[index].image_file.is_none() {
            continue;
        }

        let item = history.items.remove(index);
        if let Some(filename) = cached_image_filename(&item) {
            removed_images.push(filename);
        }
        excess_images -= 1;
    }

    while history.items.len() > MAX_HISTORY_ITEMS {
        // The empty Memo row is UI state as well as history data, so retain it
        // and remove the oldest ordinary item when the overall limit is hit.
        let index = history
            .items
            .iter()
            .rposition(|item| !item.is_empty_memo())
            .unwrap_or(history.items.len() - 1);
        let item = history.items.remove(index);
        if let Some(filename) = cached_image_filename(&item) {
            removed_images.push(filename);
        }
    }

    removed_images
}

pub(super) fn take_id(history: &mut History) -> u64 {
    let id = history.next_id;
    history.next_id = history.next_id.saturating_add(1);
    id
}

pub(super) fn digest(bytes: &[u8]) -> String {
    // Stable FNV-1a is sufficient for history de-duplication; the byte length
    // is included to further separate short clipboard values.
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}-{:x}", bytes.len())
}

pub(super) fn lock_file(file: &File, operation: i32) -> Result<()> {
    // SAFETY: flock only borrows a valid file descriptor for the duration of
    // the call; `file` remains alive and no pointer is passed across FFI.
    if unsafe { flock(file.as_raw_fd(), operation) } == -1 {
        return Err(std::io::Error::last_os_error()).context("lock clipboard history");
    }
    Ok(())
}

pub(super) fn unlock(file: &File) -> Result<()> {
    lock_file(file, LOCK_UN).context("unlock clipboard history")
}

pub(super) fn unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub(super) fn safe_filename(filename: &str) -> Option<&str> {
    let path = Path::new(filename);
    (path.components().count() == 1
        && path.file_name().and_then(|name| name.to_str()) == Some(filename))
    .then_some(filename)
}

pub(super) fn extension_for_mime(mime: &str) -> &'static str {
    match mime.split(';').next().unwrap_or(mime) {
        "image/jpeg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/bmp" => "bmp",
        "image/tiff" => "tiff",
        "image/svg+xml" => "svg",
        _ => "png",
    }
}

pub(super) fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().context("path has no parent directory")?;
    fs::create_dir_all(parent).with_context(|| format!("create directory {}", parent.display()))?;
    let temporary = parent.join(format!(
        ".{}.{}.tmp",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("data"),
        std::process::id()
    ));
    let write_result = (|| -> Result<()> {
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .mode(0o600)
            .open(&temporary)
            .with_context(|| format!("create temporary file {}", temporary.display()))?;
        file.write_all(bytes)
            .with_context(|| format!("write temporary file {}", temporary.display()))?;
        file.sync_all()
            .with_context(|| format!("sync temporary file {}", temporary.display()))?;
        fs::rename(&temporary, path)
            .with_context(|| format!("replace {} with {}", path.display(), temporary.display()))?;
        Ok(())
    })();
    if write_result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    write_result
}
