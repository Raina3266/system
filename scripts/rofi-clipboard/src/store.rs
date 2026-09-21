use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};

use crate::model::{ClipboardItem, HISTORY_VERSION, History, ItemKind};

#[derive(Clone)]
pub struct ClipboardStore {
    root: PathBuf,
    history_path: PathBuf,
    pub(crate) image_dir: PathBuf,
    lock_path: PathBuf,
}

impl ClipboardStore {
    pub fn discover() -> Result<Self> {
        let root = if let Some(path) = std::env::var_os("ROFI_CLIPBOARD_DATA_DIR") {
            PathBuf::from(path)
        } else if let Some(path) = std::env::var_os("XDG_DATA_HOME") {
            PathBuf::from(path).join("rofi-clipboard")
        } else {
            let home = std::env::var_os("HOME").context("HOME is not set")?;
            PathBuf::from(home)
                .join(".local")
                .join("share")
                .join("rofi-clipboard")
        };
        Ok(Self::at(root))
    }

    pub fn at(root: PathBuf) -> Self {
        Self {
            history_path: root.join("history.json"),
            image_dir: root.join("images"),
            lock_path: root.join("history.lock"),
            root,
        }
    }

    pub fn load(&self) -> Result<History> {
        let lock = self.lock(false)?;
        let history = self.load_unlocked();
        unlock(&lock)?;
        history
    }

    /// Path to the JSON history file, exposed so the Waybar status producer
    /// can watch its mtime and refresh the bar only when something changed.
    pub fn history_file(&self) -> &Path {
        &self.history_path
    }

    pub fn add_text(&self, text: String, mime: String) -> Result<Option<u64>> {
        if text.is_empty() {
            return Ok(None);
        }

        self.update(|history| {
            let digest = digest(text.as_bytes());
            let now = unix_timestamp();
            if let Some(position) = history
                .items
                .iter()
                .position(|item| item.kind == ItemKind::Text && item.digest == digest)
            {
                let mut item = history.items.remove(position);
                item.created_at = now;
                item.mime = mime;
                item.text = Some(text);
                let id = item.id;
                history.items.insert(0, item);
                return Ok(Some(id));
            }

            let id = take_id(history);
            history.items.insert(
                0,
                ClipboardItem {
                    id,
                    kind: ItemKind::Text,
                    text: Some(text),
                    image_file: None,
                    name: None,
                    mime,
                    pinned: false,
                    created_at: now,
                    digest,
                },
            );
            Ok(Some(id))
        })
    }

    pub fn add_file(
        &self,
        text: String,
        mime: String,
        name: Option<String>,
    ) -> Result<Option<u64>> {
        if text.is_empty() {
            return Ok(None);
        }

        self.update(|history| {
            let digest = digest(text.as_bytes());
            let now = unix_timestamp();
            if let Some(position) = history.items.iter().position(|item| {
                item.kind == ItemKind::File
                    && item.image_file.is_none()
                    && (item.digest == digest
                        || name
                            .as_deref()
                            .is_some_and(|name| item.name.as_deref() == Some(name)))
            }) {
                let mut item = history.items.remove(position);
                item.created_at = now;
                item.mime = mime;
                item.text = Some(text);
                item.name = name.or(item.name);
                item.digest = digest;
                let id = item.id;
                history.items.insert(0, item);
                return Ok(Some(id));
            }

            let id = take_id(history);
            history.items.insert(
                0,
                ClipboardItem {
                    id,
                    kind: ItemKind::File,
                    text: Some(text),
                    image_file: None,
                    name,
                    mime,
                    pinned: false,
                    created_at: now,
                    digest,
                },
            );
            Ok(Some(id))
        })
    }

    pub fn ensure_memo_draft(&self) -> Result<u64> {
        self.update(|history| Ok(ensure_memo_draft(history)))
    }

    pub fn add_image(&self, bytes: &[u8], mime: String) -> Result<Option<u64>> {
        self.add_image_named(bytes, mime, None)
    }

    pub fn add_image_named(
        &self,
        bytes: &[u8],
        mime: String,
        name: Option<String>,
    ) -> Result<Option<u64>> {
        if bytes.is_empty() {
            return Ok(None);
        }

        let mut created_file = None;
        let result = self.update(|history| {
            let digest = digest(bytes);
            let now = unix_timestamp();
            if let Some(position) = history.items.iter().position(|item| {
                item.kind == ItemKind::File && item.image_file.is_some() && item.digest == digest
            }) {
                let mut item = history.items.remove(position);
                item.created_at = now;
                item.mime = mime;
                item.name = name.or(item.name);
                let id = item.id;
                history.items.insert(0, item);
                return Ok(Some(id));
            }

            fs::create_dir_all(&self.image_dir)
                .with_context(|| format!("create image directory {}", self.image_dir.display()))?;
            let id = take_id(history);
            let filename = format!("{id}.{}", extension_for_mime(&mime));
            let image_path = self.image_dir.join(&filename);
            write_atomic(&image_path, bytes)?;
            created_file = Some(image_path);

            history.items.insert(
                0,
                ClipboardItem {
                    id,
                    kind: ItemKind::File,
                    text: None,
                    image_file: Some(filename),
                    name,
                    mime,
                    pinned: false,
                    created_at: now,
                    digest,
                },
            );
            Ok(Some(id))
        });

        if result.is_err()
            && let Some(path) = created_file
        {
            let _ = fs::remove_file(path);
        }
        result
    }

    pub fn pin(&self, id: u64) -> Result<bool> {
        self.update(|history| {
            let Some(position) = item_position(history, id) else {
                return Ok(false);
            };

            let mut item = history.items.remove(position);
            item.pinned = !item.pinned;
            if item.pinned {
                history.items.insert(0, item);
            } else {
                let first_unpinned = history
                    .items
                    .iter()
                    .position(|item| !item.pinned)
                    .unwrap_or(history.items.len());
                history.items.insert(first_unpinned, item);
            }
            Ok(true)
        })
    }

    pub fn delete(&self, id: u64) -> Result<bool> {
        let mut image_to_delete = None;
        let deleted = self.update(|history| {
            let Some(position) = item_position(history, id) else {
                return Ok(false);
            };
            let item = history.items.remove(position);
            image_to_delete = item
                .image_file
                .as_deref()
                .and_then(safe_filename)
                .map(|filename| self.image_dir.join(filename));
            Ok(true)
        })?;

        if deleted
            && let Some(path) = image_to_delete
            && let Err(error) = fs::remove_file(&path)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            return Err(error).with_context(|| format!("delete image {}", path.display()));
        }
        Ok(deleted)
    }

    pub fn edit_text(&self, id: u64, text: String) -> Result<bool> {
        self.update(|history| {
            let Some(position) = item_position(history, id) else {
                return Ok(false);
            };
            let kind = history.items[position].kind;
            if !kind.is_textual() {
                bail!("files cannot be edited as text");
            }
            if kind == ItemKind::Text && text.is_empty() {
                bail!("clipboard text cannot be empty");
            }

            let new_digest = digest(text.as_bytes());
            let duplicate = (kind == ItemKind::Text)
                .then(|| {
                    history.items.iter().enumerate().find_map(|(index, item)| {
                        (index != position
                            && item.kind == ItemKind::Text
                            && item.digest == new_digest)
                            .then_some(index)
                    })
                })
                .flatten();

            let mut item = history.items.remove(position);
            if let Some(mut duplicate_position) = duplicate {
                if duplicate_position > position {
                    duplicate_position -= 1;
                }
                let duplicate_item = history.items.remove(duplicate_position);
                item.pinned |= duplicate_item.pinned;
            }
            item.text = Some(text);
            item.digest = new_digest;
            item.created_at = unix_timestamp();
            history.items.insert(0, item);
            if kind == ItemKind::Memo {
                ensure_memo_draft(history);
            }
            Ok(true)
        })
    }

    pub fn item_bytes(&self, item: &ClipboardItem) -> Result<Vec<u8>> {
        match item.kind {
            ItemKind::Memo | ItemKind::Text => {
                Ok(item.text.as_deref().unwrap_or_default().as_bytes().to_vec())
            }
            ItemKind::File => {
                if let Some(filename) = item.image_file.as_deref() {
                    let filename = safe_filename(filename)
                        .context("invalid image filename in clipboard history")?;
                    let path = self.image_dir.join(filename);
                    fs::read(&path).with_context(|| format!("read image {}", path.display()))
                } else {
                    item.text
                        .as_deref()
                        .map(|text| text.as_bytes().to_vec())
                        .context("file item has no clipboard payload")
                }
            }
        }
    }

    pub fn image_path(&self, item: &ClipboardItem) -> Option<PathBuf> {
        item.image_file
            .as_deref()
            .and_then(safe_filename)
            .map(|filename| self.image_dir.join(filename))
    }

    pub fn prune_missing_local_files(&self) -> Result<usize> {
        let lock = self.lock(true)?;
        let mut history = self.load_unlocked()?;
        let original_len = history.items.len();
        let mut cached_images = Vec::new();

        history.items.retain(|item| {
            if !linked_local_file_is_missing(item) {
                return true;
            }
            if let Some(filename) = cached_image_filename(item) {
                cached_images.push(filename);
            }
            false
        });

        let removed = original_len - history.items.len();
        if removed == 0 {
            unlock(&lock)?;
            return Ok(0);
        }

        self.save_unlocked(&history)?;
        unlock(&lock)?;

        // The history update is already committed. Cache cleanup is
        // best-effort so a filesystem error cannot restore a stale row.
        self.remove_cached_images(cached_images, "cached");
        Ok(removed)
    }

    fn update<T>(&self, operation: impl FnOnce(&mut History) -> Result<T>) -> Result<T> {
        let lock = self.lock(true)?;
        let mut history = self.load_unlocked()?;
        let result = operation(&mut history)?;
        order_pinned_first(&mut history);
        let removed_images = trim_history(&mut history);
        self.save_unlocked(&history)?;
        unlock(&lock)?;

        // The updated history is already safely stored. Image cleanup is
        // best-effort so a filesystem cleanup error cannot invalidate a new
        // history entry that was successfully committed.
        self.remove_cached_images(removed_images, "pruned");
        Ok(result)
    }

    fn remove_cached_images(&self, filenames: impl IntoIterator<Item = String>, kind: &str) {
        for filename in filenames {
            let path = self.image_dir.join(filename);
            if let Err(error) = fs::remove_file(&path)
                && error.kind() != std::io::ErrorKind::NotFound
            {
                eprintln!(
                    "rofi-clipboard: failed to delete {kind} image {}: {error}",
                    path.display()
                );
            }
        }
    }

    fn lock(&self, exclusive: bool) -> Result<File> {
        fs::create_dir_all(&self.root)
            .with_context(|| format!("create data directory {}", self.root.display()))?;
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .mode(0o600)
            .open(&self.lock_path)
            .with_context(|| format!("open lock file {}", self.lock_path.display()))?;
        if exclusive {
            lock_file(&file, LOCK_EXCLUSIVE)?;
        } else {
            lock_file(&file, LOCK_SHARED)?;
        }
        Ok(file)
    }

    fn load_unlocked(&self) -> Result<History> {
        let mut file = match File::open(&self.history_path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(History::default());
            }
            Err(error) => {
                return Err(error).with_context(|| format!("open {}", self.history_path.display()));
            }
        };
        let mut json = String::new();
        file.read_to_string(&mut json)
            .with_context(|| format!("read {}", self.history_path.display()))?;
        let history = History::from_json(&json)
            .with_context(|| format!("parse {}", self.history_path.display()))?;
        if history.version != HISTORY_VERSION {
            bail!(
                "unsupported clipboard history version {} (expected {HISTORY_VERSION})",
                history.version
            );
        }
        Ok(history)
    }

    pub(crate) fn save_unlocked(&self, history: &History) -> Result<()> {
        fs::create_dir_all(&self.root)
            .with_context(|| format!("create data directory {}", self.root.display()))?;
        let json = history.to_json()?;
        write_atomic(&self.history_path, json.as_bytes())
    }
}

const LOCK_SHARED: i32 = 1;
const LOCK_EXCLUSIVE: i32 = 2;
const LOCK_UN: i32 = 8;
pub(crate) const MAX_HISTORY_ITEMS: usize = 2000;
pub(crate) const MAX_IMAGE_ITEMS: usize = 100;

unsafe extern "C" {
    fn flock(fd: i32, operation: i32) -> i32;
}

pub(crate) fn order_pinned_first(history: &mut History) {
    // Stable sorting keeps the existing newest-first order inside each
    // section while repairing older histories that mixed pinned and normal
    // items together.
    history.items.sort_by_key(|item| !item.pinned);
}

fn item_position(history: &History, id: u64) -> Option<usize> {
    history.items.iter().position(|item| item.id == id)
}

pub(crate) fn ensure_memo_draft(history: &mut History) -> u64 {
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

fn linked_local_file_is_missing(item: &ClipboardItem) -> bool {
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

fn cached_image_filename(item: &ClipboardItem) -> Option<String> {
    item.image_file
        .as_deref()
        .and_then(safe_filename)
        .map(str::to_owned)
}

pub(crate) fn trim_history(history: &mut History) -> Vec<String> {
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

fn take_id(history: &mut History) -> u64 {
    let id = history.next_id;
    history.next_id = history.next_id.saturating_add(1);
    id
}

fn digest(bytes: &[u8]) -> String {
    // Stable FNV-1a is sufficient for history de-duplication; the byte length
    // is included to further separate short clipboard values.
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}-{:x}", bytes.len())
}

fn lock_file(file: &File, operation: i32) -> Result<()> {
    // SAFETY: flock only borrows a valid file descriptor for the duration of
    // the call; `file` remains alive and no pointer is passed across FFI.
    if unsafe { flock(file.as_raw_fd(), operation) } == -1 {
        return Err(std::io::Error::last_os_error()).context("lock clipboard history");
    }
    Ok(())
}

fn unlock(file: &File) -> Result<()> {
    lock_file(file, LOCK_UN).context("unlock clipboard history")
}

fn unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn safe_filename(filename: &str) -> Option<&str> {
    let path = Path::new(filename);
    (path.components().count() == 1
        && path.file_name().and_then(|name| name.to_str()) == Some(filename))
    .then_some(filename)
}

fn extension_for_mime(mime: &str) -> &'static str {
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

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
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
