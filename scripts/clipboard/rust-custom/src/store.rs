//! Transactional clipboard history shared by background capture and the GTK panel.
//! The on-disk version, digests and retention limits remain backward compatible.
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::fd::AsRawFd;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
enum Kind {
    Memo,
    Text,
    #[serde(alias = "image")]
    File,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct Item {
    id: u64,
    kind: Kind,
    text: Option<String>,
    image_file: Option<String>,
    #[serde(default)]
    name: Option<String>,
    mime: String,
    pinned: bool,
    created_at: u64,
    digest: String,
}

impl Item {
    fn draft(&self) -> bool {
        matches!(self.kind, Kind::Memo) && self.text.as_deref().unwrap_or_default().is_empty()
    }
}

#[derive(Deserialize, Serialize)]
struct History {
    version: u32,
    next_id: u64,
    items: Vec<Item>,
}

impl Default for History {
    fn default() -> Self {
        Self {
            version: 1,
            next_id: 1,
            items: Vec::new(),
        }
    }
}

pub struct Store {
    root: PathBuf,
}

pub enum Captured {
    Text {
        text: String,
        mime: String,
    },
    File {
        text: String,
        mime: String,
        name: Option<String>,
    },
    Image {
        bytes: Vec<u8>,
        mime: String,
        name: Option<String>,
    },
}

#[derive(Default)]
struct PendingImages(Vec<PathBuf>);

impl Drop for PendingImages {
    fn drop(&mut self) {
        for path in &self.0 {
            let _ = fs::remove_file(path);
        }
    }
}

unsafe extern "C" {
    fn flock(fd: i32, operation: i32) -> i32;
}

impl Store {
    pub fn discover() -> Result<Self> {
        if let Some(path) = std::env::var_os("CLIPBOARD_DATA_DIR").filter(|p| !p.is_empty()) {
            // Explicit stores are isolated: never import the user's default history.
            return Ok(Self { root: path.into() });
        }
        let base = if let Some(path) = std::env::var_os("XDG_DATA_HOME").filter(|p| !p.is_empty()) {
            PathBuf::from(path)
        } else {
            PathBuf::from(std::env::var_os("HOME").context("HOME is not set")?).join(".local/share")
        };
        Self::at_data_home(&base)
    }

    fn image_path(&self, item: &Item) -> Result<Option<PathBuf>> {
        item.image_file
            .as_ref()
            .map(|name| {
                let path = Path::new(name);
                ensure!(
                    path.components().count() == 1 && path.file_name() == Some(name.as_ref()),
                    "Invalid cached image filename"
                );
                Ok(self.root.join("images").join(name))
            })
            .transpose()
    }

    fn transaction(
        &self,
        write: bool,
        operation: impl FnOnce(&mut History) -> Result<Value>,
    ) -> Result<Value> {
        self.transaction_with_images(write, |history, _| operation(history))
    }

    fn transaction_with_images(
        &self,
        write: bool,
        operation: impl FnOnce(&mut History, &mut PendingImages) -> Result<Value>,
    ) -> Result<Value> {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&self.root)?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .mode(0o600)
            .open(self.root.join("history.lock"))?;
        // SAFETY: the File owns this descriptor for the whole transaction.
        if unsafe { flock(lock.as_raw_fd(), if write { 2 } else { 1 }) } == -1 {
            return Err(std::io::Error::last_os_error()).context("Lock clipboard history");
        }
        // Closing `lock` releases flock, including on every error path.
        let path = self.root.join("history.json");
        let previous = match fs::read(&path) {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error).context("Read clipboard history"),
        };
        let mut history: History = match &previous {
            Some(bytes) => serde_json::from_slice(bytes).context("Invalid clipboard history")?,
            None => History::default(),
        };
        ensure!(
            history.version == 1,
            "Unsupported clipboard history version"
        );
        // Validate before doing any I/O on behalf of a mutation. In particular,
        // an invalid cached filename must not cause an error after committing.
        self.items(&history)?;
        let previous_images: Vec<_> = history
            .items
            .iter()
            .filter_map(|item| item.image_file.clone())
            .collect();
        let mut pending = PendingImages::default();
        let mut reply = operation(&mut history, &mut pending)?;
        if write {
            history.items.sort_by_key(|item| !item.pinned);
            trim(&mut history);
            reply["items"] = self.items(&history)?;
            let mut bytes = serde_json::to_vec_pretty(&history)?;
            bytes.push(b'\n');
            // Serialize once, then perform one buffered write. In particular,
            // opening an unchanged history must not fsync it or wake watchers.
            if previous.as_deref() != Some(bytes.as_slice()) {
                let temporary = self
                    .root
                    .join(format!(".clipboard-panel.{}.tmp", std::process::id()));
                let result = (|| -> Result<()> {
                    let mut file = OpenOptions::new()
                        .create_new(true)
                        .write(true)
                        .mode(0o600)
                        .open(&temporary)?;
                    file.write_all(&bytes)?;
                    file.sync_all()?;
                    fs::rename(&temporary, &path)?;
                    Ok(())
                })();
                if temporary.exists() {
                    let _ = fs::remove_file(&temporary);
                }
                result?;
            }
            // Only delete unreferenced cache files after the history commits,
            // while still holding the lock. A failed commit rolls new files back.
            for filename in previous_images {
                if !history
                    .items
                    .iter()
                    .any(|item| item.image_file.as_ref() == Some(&filename))
                {
                    let _ = fs::remove_file(self.root.join("images").join(filename));
                }
            }
            for path in &pending.0 {
                if !history
                    .items
                    .iter()
                    .any(|item| self.image_path(item).ok().flatten().as_ref() == Some(path))
                {
                    let _ = fs::remove_file(path);
                }
            }
            pending.0.clear();
        }
        drop(lock);
        Ok(reply)
    }

    fn items(&self, history: &History) -> Result<Value> {
        let items: Result<Vec<_>> = history
            .items
            .iter()
            .map(|item| {
                let mut value = serde_json::to_value(item)?;
                value["image_path"] = json!(self.image_path(item)?);
                Ok(value)
            })
            .collect();
        Ok(json!(items?))
    }

    pub fn snapshot(&self) -> Result<Value> {
        self.transaction(false, |history| {
            Ok(json!({ "items": self.items(history)? }))
        })
    }

    pub fn memo(&self) -> Result<Value> {
        self.transaction(true, |history| {
            let id = ensure_draft(history);
            Ok(json!({ "id": id }))
        })
    }

    pub fn pin(&self, id: u64) -> Result<Value> {
        self.transaction(true, |history| {
            let index = position(history, id)?;
            let mut item = history.items.remove(index);
            item.pinned = !item.pinned;
            let index = if item.pinned {
                0
            } else {
                history
                    .items
                    .iter()
                    .position(|item| !item.pinned)
                    .unwrap_or(history.items.len())
            };
            history.items.insert(index, item);
            Ok(json!({ "id": id }))
        })
    }

    pub fn delete(&self, id: u64) -> Result<Value> {
        self.transaction(true, |history| {
            let item = history.items.remove(position(history, id)?);
            if matches!(item.kind, Kind::Memo) {
                ensure_draft(history);
            }
            Ok(json!({ "id": id }))
        })
    }

    pub fn save(&self, id: u64, base: &str, text: String) -> Result<Value> {
        self.transaction(true, |history| {
            let index = position(history, id)?;
            let current = &history.items[index];
            ensure!(!matches!(current.kind, Kind::File), "Files cannot be edited as text");
            ensure!(current.text.as_deref().unwrap_or_default() == base,
                "This item changed in another window. Your draft has been kept; reload before saving.");
            ensure!(!matches!(current.kind, Kind::Text) || !text.is_empty(), "Clipboard text cannot be empty");
            let mut item = history.items.remove(index);
            item.digest = digest(text.as_bytes());
            if matches!(item.kind, Kind::Text) {
                history.items.retain(|other| {
                    let duplicate = matches!(other.kind, Kind::Text) && other.digest == item.digest;
                    if duplicate { item.pinned |= other.pinned; }
                    !duplicate
                });
            }
            item.text = Some(text);
            item.created_at = now();
            let memo = matches!(item.kind, Kind::Memo);
            history.items.insert(0, item);
            if memo { ensure_draft(history); }
            Ok(json!({ "id": id }))
        })
    }

    pub fn payload(&self, id: u64) -> Result<Value> {
        self.transaction(false, |history| {
            let item = &history.items[position(history, id)?];
            if let Some(path) = self.image_path(item)? {
                ensure!(path.is_file(), "Cached image is missing");
                return Ok(json!({ "mime": item.mime, "file": path }));
            }
            if matches!(item.kind, Kind::File)
                && let Some(path) = item
                    .name
                    .as_deref()
                    .map(Path::new)
                    .filter(|p| p.is_absolute())
            {
                ensure!(path.exists(), "The linked file no longer exists");
                return Ok(json!({ "mime": "text/uri-list", "text": file_uri(path) }));
            }
            Ok(json!({ "mime": item.mime, "text": item.text.as_deref().unwrap_or_default() }))
        })
    }

    pub fn prepare(&self) -> Result<Value> {
        self.transaction(true, |history| {
            history.items.retain(|item| {
                let missing = matches!(item.kind, Kind::File)
                    && item.name.as_deref().map(Path::new).filter(|p| p.is_absolute())
                        .is_some_and(|p| matches!(fs::metadata(p), Err(e) if e.kind() == std::io::ErrorKind::NotFound));
                !missing
            });
            let id = ensure_draft(history);
            Ok(json!({ "id": id }))
        })
    }

    pub fn capture(&self, entries: Vec<Captured>) -> Result<()> {
        if entries.is_empty() {
            return Ok(());
        }
        self.transaction_with_images(true, |history, pending| {
            for entry in entries {
                let (kind, text, bytes, mime, name) = match entry {
                    Captured::Text { text, mime } => (Kind::Text, Some(text), None, mime, None),
                    Captured::File { text, mime, name } => {
                        (Kind::File, Some(text), None, mime, name)
                    }
                    Captured::Image { bytes, mime, name } => {
                        (Kind::File, None, Some(bytes), mime, name)
                    }
                };
                let content = bytes
                    .as_deref()
                    .unwrap_or_else(|| text.as_deref().unwrap_or_default().as_bytes());
                if content.is_empty() {
                    continue;
                }
                let hash = digest(content);
                let existing = history.items.iter().position(|item| {
                    item.kind == kind
                        && item.image_file.is_some() == bytes.is_some()
                        && (item.digest == hash
                            || (kind == Kind::File
                                && bytes.is_none()
                                && name
                                    .as_ref()
                                    .is_some_and(|name| item.name.as_ref() == Some(name))))
                });
                let mut item = if let Some(index) = existing {
                    history.items.remove(index)
                } else {
                    let id = take_id(history)?;
                    Item {
                        id,
                        kind,
                        text: None,
                        image_file: None,
                        name: None,
                        mime: String::new(),
                        pinned: false,
                        created_at: 0,
                        digest: String::new(),
                    }
                };
                if let Some(bytes) = bytes {
                    let filename = item
                        .image_file
                        .clone()
                        .unwrap_or_else(|| format!("{}.{}", item.id, extension_for_mime(&mime)));
                    let path = self.root.join("images").join(&filename);
                    if item.image_file.is_none() || !path.try_exists()? {
                        fs::DirBuilder::new()
                            .recursive(true)
                            .mode(0o700)
                            .create(self.root.join("images"))?;
                        // Never replace an existing cache file, even if next_id
                        // in a damaged history happens to point at it.
                        let mut file = OpenOptions::new()
                            .create_new(true)
                            .write(true)
                            .mode(0o600)
                            .open(&path)?;
                        pending.0.push(path);
                        file.write_all(&bytes)?;
                        file.sync_all()?;
                    }
                    item.image_file = Some(filename);
                }
                item.text = text;
                item.name = name.or(item.name);
                item.mime = mime;
                item.digest = hash;
                item.created_at = now();
                history.items.insert(0, item);
            }
            Ok(json!({}))
        })?;
        Ok(())
    }
}

fn position(history: &History, id: u64) -> Result<usize> {
    history
        .items
        .iter()
        .position(|item| item.id == id)
        .context("This clipboard item no longer exists")
}

fn ensure_draft(history: &mut History) -> u64 {
    if let Some(id) = history
        .items
        .iter()
        .find(|item| item.draft())
        .map(|item| item.id)
    {
        history.items.retain(|item| !item.draft() || item.id == id);
        let index = history.items.iter().position(|item| item.id == id).unwrap();
        let mut item = history.items.remove(index);
        item.pinned = false;
        history.items.insert(0, item);
        return id;
    }
    let id = history.next_id;
    history.next_id = history.next_id.saturating_add(1);
    history.items.insert(
        0,
        Item {
            id,
            kind: Kind::Memo,
            text: Some(String::new()),
            image_file: None,
            name: None,
            mime: "text/plain;charset=utf-8".into(),
            pinned: false,
            created_at: now(),
            digest: digest(b""),
        },
    );
    id
}

fn trim(history: &mut History) -> Vec<Item> {
    let mut removed = Vec::new();
    let mut excess = history
        .items
        .iter()
        .filter(|item| item.image_file.is_some())
        .count()
        .saturating_sub(100);
    for index in (0..history.items.len()).rev() {
        if excess > 0 && history.items[index].image_file.is_some() {
            removed.push(history.items.remove(index));
            excess -= 1;
        }
    }
    while history.items.len() > 2000 {
        let index = history
            .items
            .iter()
            .rposition(|item| !item.draft())
            .unwrap_or(history.items.len() - 1);
        removed.push(history.items.remove(index));
    }
    removed
}

fn take_id(history: &mut History) -> Result<u64> {
    let id = history.next_id;
    ensure!(
        id > 0 && !history.items.iter().any(|item| item.id == id),
        "Invalid next clipboard item ID"
    );
    history.next_id = id.checked_add(1).context("Clipboard item IDs exhausted")?;
    Ok(id)
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

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn digest(bytes: &[u8]) -> String {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in bytes {
        hash = (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}-{:x}", bytes.len())
}

fn file_uri(path: &Path) -> String {
    let mut uri = String::from("file://");
    for &byte in path.as_os_str().as_bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~' | b'/') {
            uri.push(char::from(byte));
        } else {
            use std::fmt::Write;
            let _ = write!(uri, "%{byte:02X}");
        }
    }
    uri.push('\n');
    uri
}

#[path = "migration.rs"]
mod migration;

#[cfg(test)]
crate::clipboard_unit_tests!(store);
