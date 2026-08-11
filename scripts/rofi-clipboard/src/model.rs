use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

pub const HISTORY_VERSION: u32 = 1;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ClipboardItem {
    pub id: u64,
    pub kind: ItemKind,
    pub text: Option<String>,
    pub image_file: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    pub mime: String,
    pub pinned: bool,
    pub created_at: u64,
    pub digest: String,
}

impl ClipboardItem {
    pub fn is_empty_memo(&self) -> bool {
        self.kind == ItemKind::Memo && self.text.as_deref().unwrap_or_default().is_empty()
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ItemKind {
    Memo,
    Text,
    Image,
}

impl ItemKind {
    pub fn is_textual(self) -> bool {
        matches!(self, Self::Memo | Self::Text)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct History {
    pub version: u32,
    pub next_id: u64,
    pub items: Vec<ClipboardItem>,
}

impl Default for History {
    fn default() -> Self {
        Self {
            version: HISTORY_VERSION,
            next_id: 1,
            items: Vec::new(),
        }
    }
}

impl History {
    pub fn to_json(&self) -> Result<String> {
        let mut json =
            serde_json::to_string_pretty(self).context("serialize clipboard history")?;
        json.push('\n');
        Ok(json)
    }

    pub fn from_json(json: &str) -> Result<Self> {
        serde_json::from_str(json).context("parse clipboard history JSON")
    }
}
