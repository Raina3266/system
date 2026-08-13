use std::path::Path;

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

pub fn abbreviate_home_path(value: &str) -> String {
    let Some(home) = std::env::var_os("HOME").filter(|home| !home.is_empty()) else {
        return value.to_owned();
    };
    abbreviate_home_path_with(value, Path::new(&home))
}

fn abbreviate_home_path_with(value: &str, home: &Path) -> String {
    let Ok(relative) = Path::new(value).strip_prefix(home) else {
        return value.to_owned();
    };
    if relative.as_os_str().is_empty() {
        "~".to_owned()
    } else {
        format!("~/{}", relative.to_string_lossy())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ItemKind {
    Memo,
    Text,
    #[serde(alias = "image")]
    File,
}

impl ItemKind {
    pub fn is_textual(self) -> bool {
        matches!(self, Self::Memo | Self::Text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_image_kind_loads_as_file() {
        assert_eq!(
            serde_json::from_str::<ItemKind>("\"image\"").unwrap(),
            ItemKind::File
        );
        assert_eq!(serde_json::to_string(&ItemKind::File).unwrap(), "\"file\"");
    }

    #[test]
    fn abbreviates_only_paths_inside_home() {
        let home = Path::new("/home/raina");

        assert_eq!(
            abbreviate_home_path_with("/home/raina/Documents/report.pdf", home),
            "~/Documents/report.pdf"
        );
        assert_eq!(abbreviate_home_path_with("/home/raina", home), "~");
        assert_eq!(
            abbreviate_home_path_with("/home/rainart/report.pdf", home),
            "/home/rainart/report.pdf"
        );
        assert_eq!(
            abbreviate_home_path_with("https://example.com/report.pdf", home),
            "https://example.com/report.pdf"
        );
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
