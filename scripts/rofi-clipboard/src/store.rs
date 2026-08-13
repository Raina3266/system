use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};

use crate::model::{ClipboardItem, HISTORY_VERSION, History, ItemKind};

const LOCK_SHARED: i32 = 1;
const LOCK_EXCLUSIVE: i32 = 2;
const LOCK_UN: i32 = 8;
const MAX_HISTORY_ITEMS: usize = 2000;
const MAX_IMAGE_ITEMS: usize = 100;

unsafe extern "C" {
    fn flock(fd: i32, operation: i32) -> i32;
}

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

fn order_pinned_first(history: &mut History) {
    // Stable sorting keeps the existing newest-first order inside each
    // section while repairing older histories that mixed pinned and normal
    // items together.
    history.items.sort_by_key(|item| !item.pinned);
}

fn item_position(history: &History, id: u64) -> Option<usize> {
    history.items.iter().position(|item| item.id == id)
}

fn ensure_memo_draft(history: &mut History) -> u64 {
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

fn trim_history(history: &mut History) -> Vec<String> {
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

#[cfg(test)]
mod tests {
    use super::*;

    struct TestRoot(PathBuf);

    impl Drop for TestRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn test_root() -> TestRoot {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        TestRoot(std::env::temp_dir().join(format!(
            "rofi-clipboard-test-{}-{unique}",
            std::process::id()
        )))
    }

    #[test]
    fn editing_by_id_preserves_complete_text_and_does_not_modify_another_item() {
        let root = test_root();
        let store = ClipboardStore::at(root.0.clone());
        let editing_id = store
            .add_text("original".to_owned(), "text/plain".to_owned())
            .unwrap()
            .unwrap();
        let other_id = store
            .add_text("other selection".to_owned(), "text/plain".to_owned())
            .unwrap()
            .unwrap();
        let edited = "first line\n\tsecond  line\n中文 👩🏽‍💻  \n";

        assert!(store.edit_text(editing_id, edited.to_owned()).unwrap());

        let history = store.load().unwrap();
        assert_eq!(history.items[0].id, editing_id);
        assert_eq!(history.items[0].text.as_deref(), Some(edited));
        assert_eq!(
            history
                .items
                .iter()
                .find(|item| item.id == other_id)
                .and_then(|item| item.text.as_deref()),
            Some("other selection")
        );
    }

    #[test]
    fn file_reference_preserves_its_clipboard_payload_and_display_name() -> Result<()> {
        let root = test_root();
        let store = ClipboardStore::at(root.0.clone());
        let payload = "file:///home/raina/Documents/report.pdf\n";
        let id = store
            .add_file(
                payload.to_owned(),
                "text/uri-list".to_owned(),
                Some("/home/raina/Documents/report.pdf".to_owned()),
            )?
            .unwrap();

        let history = store.load()?;
        let item = history.items.iter().find(|item| item.id == id).unwrap();
        assert_eq!(item.kind, ItemKind::File);
        assert_eq!(item.text.as_deref(), Some(payload));
        assert_eq!(
            item.name.as_deref(),
            Some("/home/raina/Documents/report.pdf")
        );
        assert_eq!(store.item_bytes(item)?.as_slice(), payload.as_bytes());
        Ok(())
    }

    #[test]
    fn file_references_are_deduplicated_by_local_source_after_payload_conversion() -> Result<()> {
        let root = test_root();
        let store = ClipboardStore::at(root.0.clone());
        let source = "/home/raina/Documents/My Report.pdf";
        let id = store
            .add_file(
                source.to_owned(),
                "text/plain;charset=utf-8".to_owned(),
                Some(source.to_owned()),
            )?
            .unwrap();

        let second_id = store
            .add_file(
                "file:///home/raina/Documents/My%20Report.pdf\n".to_owned(),
                "text/uri-list".to_owned(),
                Some(source.to_owned()),
            )?
            .unwrap();

        assert_eq!(second_id, id);
        let history = store.load()?;
        assert_eq!(history.items.len(), 1);
        assert_eq!(history.items[0].mime, "text/uri-list");
        assert_eq!(
            history.items[0].text.as_deref(),
            Some("file:///home/raina/Documents/My%20Report.pdf\n")
        );
        Ok(())
    }

    fn item(id: u64, kind: ItemKind) -> ClipboardItem {
        ClipboardItem {
            id,
            kind,
            text: kind.is_textual().then(|| format!("text {id}")),
            image_file: (kind == ItemKind::File).then(|| format!("{id}.png")),
            name: None,
            mime: match kind {
                ItemKind::Memo | ItemKind::Text => "text/plain",
                ItemKind::File => "image/png",
            }
            .to_owned(),
            pinned: false,
            created_at: 0,
            digest: format!("digest-{id}"),
        }
    }

    #[test]
    fn removes_oldest_image_above_image_limit() {
        let mut history = History::default();
        history.items = (1..=101).rev().map(|id| item(id, ItemKind::File)).collect();

        let removed = trim_history(&mut history);

        assert_eq!(history.items.len(), MAX_IMAGE_ITEMS);
        assert_eq!(history.items.last().map(|item| item.id), Some(2));
        assert_eq!(removed, vec!["1.png"]);
    }

    #[test]
    fn file_references_do_not_count_toward_cached_image_limit() {
        let mut history = History::default();
        history.items = (1..=101)
            .rev()
            .map(|id| ClipboardItem {
                id,
                kind: ItemKind::File,
                text: Some(format!("file:///tmp/{id}.pdf\n")),
                image_file: None,
                name: Some(format!("/tmp/{id}.pdf")),
                mime: "text/uri-list".to_owned(),
                pinned: false,
                created_at: 0,
                digest: format!("file-digest-{id}"),
            })
            .collect();

        let removed = trim_history(&mut history);

        assert_eq!(history.items.len(), 101);
        assert!(removed.is_empty());
    }

    #[test]
    fn image_limit_does_not_remove_text_items() {
        let mut history = History::default();
        history.items = (1..=251)
            .rev()
            .map(|id| {
                let kind = if id >= 151 {
                    ItemKind::File
                } else {
                    ItemKind::Text
                };
                item(id, kind)
            })
            .collect();

        let removed = trim_history(&mut history);

        assert_eq!(
            history
                .items
                .iter()
                .filter(|item| item.kind == ItemKind::Text)
                .count(),
            150
        );
        assert_eq!(
            history
                .items
                .iter()
                .filter(|item| item.image_file.is_some())
                .count(),
            MAX_IMAGE_ITEMS
        );
        assert_eq!(removed, vec!["151.png"]);
    }

    #[test]
    fn overall_history_limit_preserves_the_empty_memo_draft() {
        let mut history = History::default();
        history.next_id = MAX_HISTORY_ITEMS as u64 + 1;
        history.items = (1..=MAX_HISTORY_ITEMS as u64)
            .rev()
            .map(|id| ClipboardItem {
                pinned: true,
                ..item(id, ItemKind::Text)
            })
            .collect();

        let draft_id = ensure_memo_draft(&mut history);
        order_pinned_first(&mut history);
        let removed = trim_history(&mut history);

        assert_eq!(history.items.len(), MAX_HISTORY_ITEMS);
        assert!(history.items.iter().any(|item| item.id == draft_id));
        assert!(removed.is_empty());
    }

    #[test]
    fn adding_image_above_limit_deletes_oldest_cached_file() -> Result<()> {
        let root = test_root();
        let store = ClipboardStore::at(root.0.clone());
        fs::create_dir_all(&store.image_dir)?;

        let mut history = History::default();
        history.next_id = 101;
        history.items = (1..=100).rev().map(|id| item(id, ItemKind::File)).collect();
        store.save_unlocked(&history)?;

        let oldest_path = store.image_dir.join("1.png");
        fs::write(&oldest_path, b"oldest image")?;

        assert_eq!(
            store.add_image(b"new image", "image/png".to_owned())?,
            Some(101)
        );
        assert!(!oldest_path.exists());

        let history = store.load()?;
        assert_eq!(
            history
                .items
                .iter()
                .filter(|item| item.image_file.is_some())
                .count(),
            MAX_IMAGE_ITEMS
        );
        assert_eq!(history.items.first().map(|item| item.id), Some(101));
        assert!(store.image_dir.join("101.png").exists());
        Ok(())
    }

    #[test]
    fn pinning_moves_items_to_top_and_unpinning_moves_them_below_pins() -> Result<()> {
        let root = test_root();
        let store = ClipboardStore::at(root.0.clone());
        let older_text = store
            .add_text("older text".to_owned(), "text/plain".to_owned())?
            .unwrap();
        let image = store
            .add_image(b"image bytes", "image/png".to_owned())?
            .unwrap();
        let newer_text = store
            .add_text("newer text".to_owned(), "text/plain".to_owned())?
            .unwrap();

        assert!(store.pin(image)?);
        assert!(store.pin(older_text)?);
        assert_eq!(
            store
                .load()?
                .items
                .iter()
                .map(|item| item.id)
                .collect::<Vec<_>>(),
            vec![older_text, image, newer_text]
        );

        assert!(store.pin(image)?);
        let history = store.load()?;
        assert_eq!(
            history.items.iter().map(|item| item.id).collect::<Vec<_>>(),
            vec![older_text, image, newer_text]
        );
        assert!(history.items[0].pinned);
        assert!(!history.items[1].pinned);
        assert!(!history.items[2].pinned);
        Ok(())
    }

    #[test]
    fn memo_draft_is_unique_and_replaced_after_it_is_filled() -> Result<()> {
        let root = test_root();
        let store = ClipboardStore::at(root.0.clone());
        let text_id = store
            .add_text("same content".to_owned(), "text/plain".to_owned())?
            .unwrap();
        let memo_id = store.ensure_memo_draft()?;
        assert_eq!(store.ensure_memo_draft()?, memo_id);

        let draft = store
            .load()?
            .items
            .into_iter()
            .find(|item| item.id == memo_id)
            .unwrap();
        assert_eq!(draft.kind, ItemKind::Memo);
        assert_eq!(draft.text.as_deref(), Some(""));

        assert!(store.edit_text(memo_id, "same content".to_owned())?);
        let second_memo_id = store.ensure_memo_draft()?;
        assert_ne!(second_memo_id, memo_id);
        assert!(store.edit_text(second_memo_id, "same content".to_owned())?);
        let final_draft_id = store.ensure_memo_draft()?;
        assert_ne!(final_draft_id, second_memo_id);

        let history = store.load()?;
        assert_eq!(history.items.len(), 4);
        assert_eq!(
            history
                .items
                .iter()
                .filter(|item| item.is_empty_memo())
                .count(),
            1
        );
        assert!(
            history
                .items
                .iter()
                .any(|item| item.id == text_id && item.kind == ItemKind::Text)
        );
        assert!(history.items.iter().any(|item| {
            item.id == memo_id
                && item.kind == ItemKind::Memo
                && item.text.as_deref() == Some("same content")
        }));
        assert!(history.items.iter().any(|item| {
            item.id == second_memo_id
                && item.kind == ItemKind::Memo
                && item.text.as_deref() == Some("same content")
        }));
        assert!(history.items.iter().any(|item| {
            item.id == final_draft_id
                && item.kind == ItemKind::Memo
                && item.text.as_deref() == Some("")
                && !item.pinned
        }));
        Ok(())
    }

    #[test]
    fn pinning_a_memo_moves_it_above_other_memos() -> Result<()> {
        let root = test_root();
        let store = ClipboardStore::at(root.0.clone());
        let older = store.ensure_memo_draft()?;
        assert!(store.edit_text(older, "older".to_owned())?);
        let newer = store.ensure_memo_draft()?;
        assert!(store.edit_text(newer, "newer".to_owned())?);

        assert!(store.pin(older)?);
        let memo_ids: Vec<_> = store
            .load()?
            .items
            .iter()
            .filter(|item| item.kind == ItemKind::Memo && !item.is_empty_memo())
            .map(|item| item.id)
            .collect();
        assert_eq!(memo_ids, vec![older, newer]);
        Ok(())
    }

    #[test]
    fn clearing_a_memo_reuses_it_as_the_only_draft() -> Result<()> {
        let root = test_root();
        let store = ClipboardStore::at(root.0.clone());
        let memo_id = store.ensure_memo_draft()?;
        assert!(store.edit_text(memo_id, "keep me".to_owned())?);
        let old_draft_id = store.ensure_memo_draft()?;

        assert!(store.edit_text(memo_id, String::new())?);

        let history = store.load()?;
        let drafts: Vec<_> = history
            .items
            .iter()
            .filter(|item| item.is_empty_memo())
            .collect();
        assert_eq!(drafts.len(), 1);
        assert_eq!(drafts[0].id, memo_id);
        assert!(!history.items.iter().any(|item| item.id == old_draft_id));
        Ok(())
    }

    #[test]
    fn new_items_stay_below_pins_and_existing_history_is_repaired() -> Result<()> {
        let root = test_root();
        let store = ClipboardStore::at(root.0.clone());
        let mut history = History::default();
        history.next_id = 5;
        history.items = vec![
            item(1, ItemKind::Text),
            ClipboardItem {
                pinned: true,
                ..item(2, ItemKind::File)
            },
            item(3, ItemKind::File),
            ClipboardItem {
                pinned: true,
                ..item(4, ItemKind::Text)
            },
        ];
        store.save_unlocked(&history)?;

        let new_id = store
            .add_text("new item".to_owned(), "text/plain".to_owned())?
            .unwrap();
        let history = store.load()?;

        assert_eq!(
            history.items.iter().map(|item| item.id).collect::<Vec<_>>(),
            vec![2, 4, new_id, 1, 3]
        );
        assert!(history.items[..2].iter().all(|item| item.pinned));
        assert!(history.items[2..].iter().all(|item| !item.pinned));
        Ok(())
    }

    #[test]
    fn pruning_missing_local_images_removes_rows_and_cached_files() -> Result<()> {
        let root = test_root();
        let store = ClipboardStore::at(root.0.join("data"));
        let existing_source = root.0.join("existing.png");
        let deleted_source = root.0.join("deleted.png");
        let deleted_pinned_source = root.0.join("deleted-pinned.png");
        fs::create_dir_all(&root.0)?;
        fs::write(&existing_source, b"existing source")?;
        fs::write(&deleted_source, b"deleted source")?;
        fs::write(&deleted_pinned_source, b"deleted pinned source")?;

        let existing_id = store
            .add_image_named(
                b"existing cache",
                "image/png".to_owned(),
                Some(existing_source.to_string_lossy().into_owned()),
            )?
            .unwrap();
        let deleted_id = store
            .add_image_named(
                b"deleted cache",
                "image/png".to_owned(),
                Some(deleted_source.to_string_lossy().into_owned()),
            )?
            .unwrap();
        let deleted_pinned_id = store
            .add_image_named(
                b"deleted pinned cache",
                "image/png".to_owned(),
                Some(deleted_pinned_source.to_string_lossy().into_owned()),
            )?
            .unwrap();
        assert!(store.pin(deleted_pinned_id)?);
        let url_id = store
            .add_image_named(
                b"url cache",
                "image/png".to_owned(),
                Some("https://example.com/image.png".to_owned()),
            )?
            .unwrap();
        let clipboard_only_id = store
            .add_image(b"clipboard cache", "image/png".to_owned())?
            .unwrap();
        let text_id = store
            .add_text("keep text".to_owned(), "text/plain".to_owned())?
            .unwrap();

        let history = store.load()?;
        let deleted_cache = store.image_path(
            history
                .items
                .iter()
                .find(|item| item.id == deleted_id)
                .unwrap(),
        );
        let deleted_pinned_cache = store.image_path(
            history
                .items
                .iter()
                .find(|item| item.id == deleted_pinned_id)
                .unwrap(),
        );
        fs::remove_file(&deleted_source)?;
        fs::remove_file(&deleted_pinned_source)?;

        assert_eq!(store.prune_missing_local_files()?, 2);

        let history = store.load()?;
        let retained_ids: Vec<_> = history.items.iter().map(|item| item.id).collect();
        assert!(!retained_ids.contains(&deleted_id));
        assert!(!retained_ids.contains(&deleted_pinned_id));
        assert!(retained_ids.contains(&existing_id));
        assert!(retained_ids.contains(&url_id));
        assert!(retained_ids.contains(&clipboard_only_id));
        assert!(retained_ids.contains(&text_id));
        assert!(!deleted_cache.unwrap().exists());
        assert!(!deleted_pinned_cache.unwrap().exists());
        assert_eq!(store.prune_missing_local_files()?, 0);
        Ok(())
    }

    #[test]
    fn pruning_missing_local_file_references_keeps_existing_paths_and_urls() -> Result<()> {
        let root = test_root();
        let store = ClipboardStore::at(root.0.join("data"));
        let existing_source = root.0.join("existing report.pdf");
        let deleted_source = root.0.join("deleted report.pdf");
        fs::create_dir_all(&root.0)?;
        fs::write(&existing_source, b"existing")?;
        fs::write(&deleted_source, b"deleted")?;

        let existing_id = store
            .add_file(
                format!("file://{}\n", existing_source.to_string_lossy()),
                "text/uri-list".to_owned(),
                Some(existing_source.to_string_lossy().into_owned()),
            )?
            .unwrap();
        let deleted_id = store
            .add_file(
                format!("file://{}\n", deleted_source.to_string_lossy()),
                "text/uri-list".to_owned(),
                Some(deleted_source.to_string_lossy().into_owned()),
            )?
            .unwrap();
        assert!(store.pin(deleted_id)?);
        let url_id = store
            .add_file(
                "https://example.com/report.pdf".to_owned(),
                "text/plain".to_owned(),
                Some("https://example.com/report.pdf".to_owned()),
            )?
            .unwrap();
        fs::remove_file(&deleted_source)?;

        assert_eq!(store.prune_missing_local_files()?, 1);

        let retained_ids: Vec<_> = store.load()?.items.iter().map(|item| item.id).collect();
        assert!(retained_ids.contains(&existing_id));
        assert!(!retained_ids.contains(&deleted_id));
        assert!(retained_ids.contains(&url_id));
        assert_eq!(store.prune_missing_local_files()?, 0);
        Ok(())
    }
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
