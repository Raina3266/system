use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;

use crate::model::{ClipboardItem, History, ItemKind};

use super::ClipboardStore;
use super::helpers::{
    MAX_HISTORY_ITEMS, MAX_IMAGE_ITEMS, ensure_memo_draft, order_pinned_first, trim_history,
};

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

mod files;
mod history;
mod limits;
mod pruning;
