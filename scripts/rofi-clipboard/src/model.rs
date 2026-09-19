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

pub(crate) fn abbreviate_home_path_with(value: &str, home: &Path) -> String {
    let Ok(relative) = Path::new(value).strip_prefix(home) else {
        return value.to_owned();
    };
    if relative.as_os_str().is_empty() {
        "~".to_owned()
    } else {
        format!("~/{}", relative.to_string_lossy())
    }
}

/// A bare `http(s)` reference with no embedded whitespace, after trimming and
/// unescaping `&amp;`. Copied URLs are routed into the Text clipboard mode
/// using this rule rather than the file mode.
pub fn url_value(value: &str) -> Option<String> {
    let value = value.trim().replace("&amp;", "&");
    (value.starts_with("http://") || value.starts_with("https://"))
        .then_some(value)
        .filter(|value| !value.chars().any(char::is_whitespace))
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

/// Escape a string for embedding inside a Waybar JSON string field. Waybar
/// parses each `exec` line as JSON, so the text/tooltip we emit must not
/// contain unescaped quotes, backslashes, or control characters.
pub fn json_escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            character if character.is_control() => {
                use std::fmt::Write as _;
                let _ = write!(escaped, "\\u{:04x}", u32::from(character));
            }
            character => escaped.push(character),
        }
    }
    escaped
}

impl History {
    pub fn to_json(&self) -> Result<String> {
        let mut json = serde_json::to_string_pretty(self).context("serialize clipboard history")?;
        json.push('\n');
        Ok(json)
    }

    pub fn from_json(json: &str) -> Result<Self> {
        serde_json::from_str(json).context("parse clipboard history JSON")
    }
}
