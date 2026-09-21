//! Clipboard-specific persistence around the shared companion panel.

use std::env;
use std::path::PathBuf;

use anyhow::{Result, bail};
use rofi_preview_shared::panel_client::{
    PanelClient, PanelContent, PanelSnapshot, SaveResult, SwitchReply,
};

use crate::model::{ClipboardItem, ItemKind, abbreviate_home_path};
use crate::store::ClipboardStore;

#[derive(Clone)]
pub struct ClipboardEditor {
    panel: PanelClient,
}

impl ClipboardEditor {
    pub fn new() -> Result<Self> {
        let executable = env::var_os("ROFI_CLIPBOARD_ROFI_PREVIEW_SHARED")
            .or_else(|| env::var_os("ROFI_CLIPBOARD_PREVIEW_PANEL"))
            .unwrap_or_else(|| "rofi-preview-shared".into());
        let panel = PanelClient::new("rofi-clipboard", executable, "ROFI_CLIPBOARD")?;
        panel.cleanup()?;
        Ok(Self { panel })
    }

    pub fn toggle(&self, store: &ClipboardStore, selected_id: Option<u64>) -> Result<Option<u64>> {
        match self.panel.save_and_close()? {
            SaveResult::Closed(Some(snapshot)) => return save_snapshot(store, snapshot),
            SaveResult::NoPanel | SaveResult::Closed(None) => {}
        }

        let Some(selected_id) = selected_id else {
            return Ok(None);
        };
        let Some(content) = item_content(store, selected_id)? else {
            return Ok(None);
        };
        self.panel
            .open(selected_id, content_title(&content), &content)?;
        Ok(Some(selected_id))
    }

    pub fn selection_changed(&self, store: &ClipboardStore, id: u64, serial: u64) -> Result<()> {
        if !self.panel.is_open() {
            return Ok(());
        }
        let Some(content) = item_content(store, id)? else {
            return Ok(());
        };
        let Some(reply) = self.panel.prepare_switch(id, serial)? else {
            return Ok(());
        };
        let SwitchReply::Ready(snapshot) = reply else {
            return Ok(());
        };
        if let Some(snapshot) = snapshot {
            let _ = save_snapshot(store, snapshot)?;
        }
        let _ = self.panel.update(id, serial, &content)?;
        Ok(())
    }

    pub fn refresh_after_delete(
        &self,
        store: &ClipboardStore,
        selected_id: Option<u64>,
    ) -> Result<()> {
        if !self.panel.is_open() {
            return Ok(());
        }
        self.panel.close()?;
        let Some(selected_id) = selected_id else {
            return Ok(());
        };
        let Some(content) = item_content(store, selected_id)? else {
            return Ok(());
        };
        self.panel
            .open(selected_id, content_title(&content), &content)?;
        Ok(())
    }

    pub fn save_and_close(&self, store: &ClipboardStore) -> Result<()> {
        match self.panel.save_and_close()? {
            SaveResult::NoPanel | SaveResult::Closed(None) => Ok(()),
            SaveResult::Closed(Some(snapshot)) => {
                let _ = save_snapshot(store, snapshot)?;
                Ok(())
            }
        }
    }

    pub fn close_silently(&self) {
        self.panel.close_silently();
    }
}

fn item_content(store: &ClipboardStore, id: u64) -> Result<Option<PanelContent>> {
    let history = store.load()?;
    Ok(history
        .items
        .iter()
        .find(|item| item.id == id)
        .and_then(|item| panel_content(item, store.image_path(item))))
}

pub(crate) fn panel_content(
    item: &ClipboardItem,
    image_path: Option<PathBuf>,
) -> Option<PanelContent> {
    match item.kind {
        ItemKind::Memo | ItemKind::Text => Some(PanelContent::EditableText(
            item.text.clone().unwrap_or_default(),
        )),
        ItemKind::File => image_path.map(PanelContent::Image).or_else(|| {
            item.name
                .as_deref()
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .or(item.text.as_deref())
                .map(|text| PanelContent::ReadOnlyText(abbreviate_home_path(text)))
        }),
    }
}

fn content_title(content: &PanelContent) -> &'static str {
    match content {
        PanelContent::EditableText(_) => "Edit clipboard text",
        PanelContent::ReadOnlyText(_) => "Preview clipboard file",
        PanelContent::Image(_) => "Preview clipboard image",
    }
}

fn save_snapshot(store: &ClipboardStore, snapshot: PanelSnapshot) -> Result<Option<u64>> {
    let (id, text) = match snapshot {
        PanelSnapshot::Image { id } => return Ok(Some(id)),
        PanelSnapshot::Text { id, text } => (id, text),
    };
    let changed = {
        let history = store.load()?;
        let Some(item) = history.items.iter().find(|item| item.id == id) else {
            return Ok(None);
        };
        if item.kind == ItemKind::File {
            return Ok(Some(id));
        }
        text_is_changed(item, &text)?
    };
    if !changed {
        return Ok(Some(id));
    }
    if store.edit_text(id, text)? {
        Ok(Some(id))
    } else {
        Ok(None)
    }
}

pub(crate) fn text_is_changed(item: &ClipboardItem, text: &str) -> Result<bool> {
    if !item.kind.is_textual() {
        bail!("files cannot be edited as text");
    }
    Ok(item.text.as_deref() != Some(text))
}
