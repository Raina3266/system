//! Adapted from rofi-clipboard's model/store and file-copy policy. Keep the
//! version, filenames, flock, digest and limits compatible with that writer.
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

#[derive(Clone, Debug, Deserialize, Serialize)]
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

unsafe extern "C" {
    fn flock(fd: i32, operation: i32) -> i32;
}

impl Store {
    pub fn discover() -> Result<Self> {
        let root = if let Some(path) = std::env::var_os("ROFI_CLIPBOARD_DATA_DIR") {
            PathBuf::from(path)
        } else if let Some(path) = std::env::var_os("XDG_DATA_HOME") {
            PathBuf::from(path).join("rofi-clipboard")
        } else {
            PathBuf::from(std::env::var_os("HOME").context("HOME is not set")?)
                .join(".local/share/rofi-clipboard")
        };
        Ok(Self { root })
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
        let mut history: History = match fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes).context("Invalid clipboard history")?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => History::default(),
            Err(error) => return Err(error).context("Read clipboard history"),
        };
        ensure!(
            history.version == 1,
            "Unsupported clipboard history version"
        );
        let reply = operation(&mut history)?;
        if write {
            history.items.sort_by_key(|item| !item.pinned);
            let removed = trim(&mut history);
            let temporary = self
                .root
                .join(format!(".clipboard-panel.{}.tmp", std::process::id()));
            let result = (|| -> Result<()> {
                let mut file = OpenOptions::new()
                    .create_new(true)
                    .write(true)
                    .mode(0o600)
                    .open(&temporary)?;
                serde_json::to_writer_pretty(&mut file, &history)?;
                file.write_all(b"\n")?;
                file.sync_all()?;
                fs::rename(&temporary, &path)?;
                Ok(())
            })();
            if temporary.exists() {
                let _ = fs::remove_file(&temporary);
            }
            result?;
            for item in removed {
                if let Ok(Some(path)) = self.image_path(&item) {
                    let _ = fs::remove_file(path);
                }
            }
        }
        drop(lock);
        Ok(reply)
    }

    pub fn snapshot(&self) -> Result<Value> {
        self.transaction(false, |history| {
            let items: Result<Vec<_>> = history
                .items
                .iter()
                .map(|item| {
                    let mut value = serde_json::to_value(item)?;
                    value["image_path"] = json!(self.image_path(item)?);
                    Ok(value)
                })
                .collect();
            Ok(json!({ "items": items? }))
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
        let mut removed = None;
        let reply = self.transaction(true, |history| {
            removed = Some(history.items.remove(position(history, id)?));
            Ok(json!({ "id": id }))
        })?;
        if let Some(item) = removed
            && let Some(path) = self.image_path(&item)?
        {
            let _ = fs::remove_file(path);
        }
        Ok(reply)
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
                    .map(str::trim)
                    .map(Path::new)
                    .filter(|p| p.is_absolute())
            {
                ensure!(path.exists(), "The linked file no longer exists");
                return Ok(json!({ "mime": "text/uri-list", "text": file_uri(path) }));
            }
            Ok(json!({ "mime": item.mime, "text": item.text.as_deref().unwrap_or_default() }))
        })
    }

    pub fn prepare(&self) -> Result<()> {
        let mut removed = Vec::new();
        self.transaction(true, |history| {
            history.items.retain(|item| {
                let missing = matches!(item.kind, Kind::File)
                    && item.name.as_deref().map(Path::new).filter(|p| p.is_absolute())
                        .is_some_and(|p| matches!(fs::metadata(p), Err(e) if e.kind() == std::io::ErrorKind::NotFound));
                if missing { removed.push(item.clone()); }
                !missing
            });
            Ok(Value::Null)
        })?;
        for item in removed {
            if let Ok(Some(path)) = self.image_path(&item) {
                let _ = fs::remove_file(path);
            }
        }
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

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
