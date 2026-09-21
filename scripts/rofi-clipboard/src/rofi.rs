use std::path::Path;

use anyhow::{Result, bail};
use rofi_preview_shared::launcher::{
    Action, Controller, Icon, Mode as SharedMode, Outcome, Row, UiResult, View,
};

use crate::clipboard::copy_item;
use crate::editor::ClipboardEditor;
use crate::model::{ClipboardItem, ItemKind, abbreviate_home_path};
use crate::store::ClipboardStore;

const ACTION_PIN: &str = "pin";
const ACTION_DELETE: &str = "delete";
const ACTION_EDIT: &str = "edit";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Mode {
    Memo,
    Text,
    Files,
}

impl Mode {
    pub fn parse(value: &str) -> Result<Self> {
        match value.to_ascii_lowercase().as_str() {
            "memo" => Ok(Self::Memo),
            "text" => Ok(Self::Text),
            "files" | "images" => Ok(Self::Files),
            _ => bail!("unknown clipboard mode {value:?}"),
        }
    }

    pub(crate) fn prompt(self) -> &'static str {
        match self {
            Self::Memo => "󰍩 Memo",
            Self::Text => "󰦨 Text",
            Self::Files => "󰈔 Files",
        }
    }

    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Memo => "memo",
            Self::Text => "text",
            Self::Files => "files",
        }
    }

    pub(crate) fn includes(self, item: &ClipboardItem) -> bool {
        match self {
            Self::Memo => item.kind == ItemKind::Memo,
            Self::Text => item.kind == ItemKind::Text,
            Self::Files => item.kind == ItemKind::File,
        }
    }
}

pub fn launch(mode: Mode, selected_id: Option<u64>) -> Result<()> {
    let controller = ClipboardUi::new(mode, selected_id)?;
    rofi_preview_shared::launcher::run(controller);
    Ok(())
}

struct ClipboardUi {
    store: ClipboardStore,
    editor: ClipboardEditor,
    mode: Mode,
    selected_id: Option<u64>,
}

impl ClipboardUi {
    fn new(mode: Mode, selected_id: Option<u64>) -> Result<Self> {
        let store = ClipboardStore::discover()?;
        Ok(Self {
            store,
            editor: ClipboardEditor::new()?,
            mode,
            selected_id,
        })
    }

    fn build_view(&mut self) -> Result<View> {
        prepare_mode(&self.store, self.mode)?;
        let history = self.store.load()?;
        let items = mode_items(&history.items, self.mode);
        let selected = preferred_selection(&items, self.selected_id)
            .and_then(|index| items.get(index))
            .map(|item| item.id.to_string());
        let rows = items
            .into_iter()
            .map(|item| Row {
                id: item.id.to_string(),
                title: row_preview(item),
                subtitle: None,
                search_text: row_value(item),
                icon: item_icon(&self.store, item),
                active: item.pinned,
                permanent: item.is_empty_memo(),
            })
            .collect();
        Ok(View {
            prompt: self.mode.prompt().to_owned(),
            rows,
            actions: vec![
                Action::new(ACTION_PIN, "󰐃 Pin", Some('p')),
                Action::new(ACTION_EDIT, "󰏫 Edit", Some('e')),
                Action::new(ACTION_DELETE, "󰆴 Delete", Some('d')),
            ],
            selected,
            empty_message: Some("Nothing here yet".to_owned()),
        })
    }

    fn selected_id(value: Option<&str>) -> Option<u64> {
        value.and_then(|value| value.parse().ok())
    }
}

impl Controller for ClipboardUi {
    fn application_id(&self) -> &'static str {
        "io.github.raina.RofiClipboard"
    }

    fn namespace(&self) -> &'static str {
        "rofi-clipboard"
    }

    fn modes(&self) -> Vec<SharedMode> {
        [Mode::Memo, Mode::Text, Mode::Files]
            .into_iter()
            .map(|mode| SharedMode::new(mode.name(), mode.prompt()))
            .collect()
    }

    fn active_mode(&self) -> &str {
        self.mode.name()
    }

    fn switch_mode(&mut self, mode: &str) -> UiResult<View> {
        self.mode = Mode::parse(mode).map_err(display_error)?;
        self.selected_id = None;
        self.build_view().map_err(display_error)
    }

    fn view(&mut self) -> UiResult<View> {
        self.build_view().map_err(display_error)
    }

    fn activate(&mut self, row: &str) -> UiResult<Outcome> {
        let Some(id) = Self::selected_id(Some(row)) else {
            return Ok(Outcome::None);
        };
        copy_item(&self.store, id).map_err(display_error)?;
        Ok(Outcome::Close)
    }

    fn action(&mut self, action: &str, selected: Option<&str>) -> UiResult<Outcome> {
        let Some(id) = Self::selected_id(selected) else {
            return Ok(Outcome::None);
        };
        match action {
            ACTION_DELETE => {
                let replacement =
                    selection_after_delete(&self.store, self.mode, id).map_err(display_error)?;
                if self.store.delete(id).map_err(display_error)? {
                    self.editor
                        .refresh_after_delete(&self.store, replacement)
                        .map_err(display_error)?;
                    self.selected_id = replacement;
                }
            }
            ACTION_PIN => {
                self.store.pin(id).map_err(display_error)?;
                self.selected_id = Some(id);
            }
            ACTION_EDIT => {
                self.selected_id = self
                    .editor
                    .toggle(&self.store, Some(id))
                    .map_err(display_error)?
                    .or(Some(id));
            }
            _ => return Ok(Outcome::None),
        }
        Ok(Outcome::Refresh {
            selected: self.selected_id.map(|id| id.to_string()),
        })
    }

    fn selection_changed(&mut self, selected: &str, serial: u64) -> UiResult<()> {
        let Some(id) = Self::selected_id(Some(selected)) else {
            return Ok(());
        };
        self.selected_id = Some(id);
        self.editor
            .selection_changed(&self.store, id, serial)
            .map_err(display_error)
    }

    fn close(&mut self) -> UiResult<()> {
        if let Err(error) = self.editor.save_and_close(&self.store) {
            self.editor.close_silently();
            return Err(display_error(error));
        }
        Ok(())
    }
}

fn prepare_mode(store: &ClipboardStore, mode: Mode) -> Result<()> {
    store.prune_missing_local_files()?;
    if mode == Mode::Memo {
        store.ensure_memo_draft()?;
    }
    Ok(())
}

pub(crate) fn mode_items(items: &[ClipboardItem], mode: Mode) -> Vec<&ClipboardItem> {
    let mut items: Vec<_> = items.iter().filter(|item| mode.includes(item)).collect();
    if mode == Mode::Memo {
        items.sort_by_key(|item| item.is_empty_memo());
    }
    items
}

pub(crate) fn preferred_selection(
    items: &[&ClipboardItem],
    selected_id: Option<u64>,
) -> Option<usize> {
    selected_id
        .and_then(|id| items.iter().position(|item| item.id == id))
        .or_else(|| items.iter().position(|item| !item.pinned))
        .or_else(|| (!items.is_empty()).then_some(0))
}

fn selection_after_delete(
    store: &ClipboardStore,
    mode: Mode,
    selected_id: u64,
) -> Result<Option<u64>> {
    let history = store.load()?;
    let items = mode_items(&history.items, mode);
    Ok(replacement_selection(&items, selected_id))
}

pub(crate) fn replacement_selection(items: &[&ClipboardItem], selected_id: u64) -> Option<u64> {
    let index = items.iter().position(|item| item.id == selected_id)?;
    items
        .get(index + 1)
        .or_else(|| index.checked_sub(1).and_then(|index| items.get(index)))
        .map(|item| item.id)
}

fn item_icon(store: &ClipboardStore, item: &ClipboardItem) -> Option<Icon> {
    store.image_path(item).map(Icon::Image).or_else(|| {
        item.name
            .as_deref()
            .map(Path::new)
            .filter(|path| path.exists())
            .map(|path| Icon::File(path.to_path_buf()))
    })
}

pub(crate) fn row_value(item: &ClipboardItem) -> String {
    match item.kind {
        ItemKind::Memo | ItemKind::Text => item.text.clone().unwrap_or_default(),
        ItemKind::File => file_label(item),
    }
}

pub(crate) fn row_preview(item: &ClipboardItem) -> String {
    match item.kind {
        ItemKind::Memo => {
            let preview = text_row_preview(item);
            if preview.is_empty() {
                "New memo".to_owned()
            } else {
                preview
            }
        }
        ItemKind::Text => text_row_preview(item),
        ItemKind::File => truncate_chars(&file_label(item), 110),
    }
}

fn text_row_preview(item: &ClipboardItem) -> String {
    let text = item.text.as_deref().unwrap_or_default();
    let one_line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    truncate_chars(&one_line, 110)
}

fn file_label(item: &ClipboardItem) -> String {
    if let Some(name) = item
        .name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
    {
        return abbreviate_home_path(name);
    }
    if let Some(text) = item
        .text
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
    {
        return abbreviate_home_path(text);
    }
    if item.image_file.is_some() {
        format!("Image · {}", short_mime(&item.mime))
    } else {
        format!("File · {}", short_mime(&item.mime))
    }
}

fn short_mime(mime: &str) -> &str {
    mime.split('/')
        .nth(1)
        .unwrap_or(mime)
        .split(';')
        .next()
        .unwrap_or(mime)
}

fn truncate_chars(value: &str, maximum: usize) -> String {
    let mut chars = value.chars();
    let mut result: String = chars.by_ref().take(maximum).collect();
    if chars.next().is_some() {
        result.push('…');
    }
    result
}

fn display_error(error: impl std::fmt::Display) -> String {
    error.to_string()
}
