use std::fs::{self, File, OpenOptions};
use std::io::Read;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::model::{ClipboardItem, HISTORY_VERSION, History, ItemKind};

mod helpers;

use helpers::{
    LOCK_EXCLUSIVE, LOCK_SHARED, cached_image_filename, digest, ensure_memo_draft,
    extension_for_mime, item_position, linked_local_file_is_missing, lock_file, order_pinned_first,
    safe_filename, take_id, trim_history, unix_timestamp, unlock, write_atomic,
};

pub struct ClipboardStore {
    root: PathBuf,
    history_path: PathBuf,
    image_dir: PathBuf,
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

    fn save_unlocked(&self, history: &History) -> Result<()> {
        fs::create_dir_all(&self.root)
            .with_context(|| format!("create data directory {}", self.root.display()))?;
        let json = history.to_json()?;
        write_atomic(&self.history_path, json.as_bytes())
    }
}

#[cfg(test)]
mod tests;
