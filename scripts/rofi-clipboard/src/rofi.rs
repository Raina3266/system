use std::env;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::clipboard::copy_item;
use crate::model::{ClipboardItem, ItemKind};
use crate::store::ClipboardStore;

const RECORD_SEPARATOR: u8 = 0x1e;
const UNIT_SEPARATOR: u8 = 0x1f;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Mode {
    Pinned,
    Text,
    Images,
}

impl Mode {
    pub fn parse(value: &str) -> Result<Self> {
        match value.to_ascii_lowercase().as_str() {
            "pinned" => Ok(Self::Pinned),
            "text" => Ok(Self::Text),
            "images" => Ok(Self::Images),
            _ => bail!("unknown clipboard mode {value:?}"),
        }
    }

    fn prompt(self) -> &'static str {
        match self {
            Self::Pinned => "󰐃 Pinned",
            Self::Text => "󰦨 Text",
            Self::Images => "󰋩 Images",
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Pinned => "pinned",
            Self::Text => "text",
            Self::Images => "images",
        }
    }

    fn includes(self, item: &ClipboardItem) -> bool {
        match self {
            Self::Pinned => item.pinned,
            Self::Text => item.kind == ItemKind::Text,
            Self::Images => item.kind == ItemKind::Image,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct UiState {
    preview: bool,
    editing: Option<u64>,
    initialized: bool,
}

impl UiState {
    fn parse(value: Option<String>) -> Self {
        let Some(value) = value else {
            return Self {
                preview: false,
                editing: None,
                initialized: false,
            };
        };
        let mut state = Self::default();
        for part in value.split(';') {
            if part == "preview=1" {
                state.preview = true;
            } else if let Some(id) = part.strip_prefix("edit=") {
                state.editing = id.parse().ok();
            } else if part == "init=1" {
                state.initialized = true;
            }
        }
        state
    }

    fn encode(self) -> String {
        format!(
            "preview={};edit={};init={}",
            u8::from(self.preview),
            self.editing.map(|id| id.to_string()).unwrap_or_default(),
            u8::from(self.initialized)
        )
    }
}

pub fn launch_rofi(mode: Mode) -> Result<()> {
    let executable = env::current_exe().context("locate rofi-clipboard executable")?;
    let executable = executable.to_string_lossy();
    let modes = format!(
        "pinned:{executable} script pinned,text:{executable} script text,images:{executable} script images"
    );
    let theme = theme_path()?;
    let mut command = Command::new(rofi_binary());
    command
        .args([
            "-show",
            mode.name(),
            "-modes",
            &modes,
            "-display-pinned",
            "󰐃 Pinned",
            "-display-text",
            "󰦨 Text",
            "-display-images",
            "󰋩 Images",
            "-kb-custom-1",
            "",
            "-kb-custom-2",
            "",
            "-kb-custom-3",
            "",
            "-kb-custom-4",
            "",
            "-theme",
        ])
        .arg(theme);
    let status = command.status().context("launch rofi")?;
    if !status.success() && status.code() != Some(1) {
        bail!("rofi exited with {status}");
    }
    Ok(())
}

fn theme_path() -> Result<PathBuf> {
    if let Some(path) = env::var_os("ROFI_CLIPBOARD_THEME") {
        return Ok(PathBuf::from(path));
    }
    if let Some(config) = env::var_os("XDG_CONFIG_HOME") {
        return Ok(PathBuf::from(config)
            .join("rofi")
            .join("rofi-clipboard.rasi"));
    }
    let home = env::var_os("HOME").context("HOME is not set")?;
    Ok(PathBuf::from(home)
        .join(".config")
        .join("rofi")
        .join("rofi-clipboard.rasi"))
}

pub fn run_script(mode: Mode) -> Result<()> {
    let store = ClipboardStore::discover()?;
    let retv = env::var("ROFI_RETV")
        .ok()
        .and_then(|value| value.parse::<u8>().ok())
        .unwrap_or(0);
    let selected_id = env::var("ROFI_INFO")
        .ok()
        .and_then(|value| value.parse::<u64>().ok());
    let mut state = UiState::parse(env::var("ROFI_DATA").ok());

    match retv {
        // Initially opens and renders the clipboard history
        0 => render_history(&store, mode, state, false),
        // Copies the selected item to the clipboard, then closes Rofi
        1 => {
            if let Some(id) = selected_id {
                copy_item(&store, id)?;
            }
            Ok(())
        }
        // Saves the edited text from ROFI_INPUT
        2 if state.editing.is_some() => {
            let id = state.editing.take().expect("editing ID checked above");
            let replacement = decode_edit_input(&env::var("ROFI_INPUT").unwrap_or_default());
            if !store.edit_text(id, replacement)? {
                bail!("clipboard item no longer exists");
            }
            render_history(&store, mode, state, true)
        }
        // Deletes the selected item
        3 | 11 => {
            if let Some(id) = selected_id {
                store.delete(id)?;
            }
            render_history(&store, mode, state, true)
        }
        // Pins or unpins the selected item
        10 => {
            if let Some(id) = selected_id {
                store.pin(id)?;
            }
            render_history(&store, mode, state, true)
        }
        // Opens the editor for the selected item
        12 => match selected_id {
            Some(id) => render_editor(&store, state, id),
            None => render_history(&store, mode, state, true),
        },
        // Opens or closes the preview pane
        13 => {
            state.preview = !state.preview;
            render_history(&store, mode, state, true)
        }
        _ if state.editing.is_some() => render_editor(
            &store,
            state,
            state.editing.expect("editing ID checked above"),
        ),
        _ => render_history(&store, mode, state, true),
    }
}

fn render_history(
    store: &ClipboardStore,
    mode: Mode,
    state: UiState,
    keep_selection: bool,
) -> Result<()> {
    let history = store.load()?;
    let items: Vec<_> = history
        .items
        .iter()
        .filter(|item| mode.includes(item))
        .collect();
    let mut output = Vec::new();
    write_common_headers(&mut output, mode.prompt(), state, true, keep_selection);

    if items.is_empty() {
        write!(&mut output, "Nothing here yet")?;
        write_row_option(&mut output, "nonselectable", "true");
        write_row_option(&mut output, "permanent", "true");
        output.push(RECORD_SEPARATOR);
    }

    for item in items {
        let raw = row_value(item);
        write!(&mut output, "{}", sanitize_record_value(&raw))?;
        // Rofi's textbox-current-entry uses the row's display value. While the
        // preview is open we therefore expose the complete row value; the
        // fixed one-line list height still keeps the menu rows compact.
        if !state.preview {
            write_row_option(&mut output, "display", &row_preview(item));
        }
        write_row_option(&mut output, "info", &item.id.to_string());
        if let Some(path) = store.image_path(item) {
            write_row_option(&mut output, "icon", &path.to_string_lossy());
        }
        if item.pinned {
            write_row_option(&mut output, "active", "true");
        }
        output.push(RECORD_SEPARATOR);
    }
    io::stdout().write_all(&output).context("write rofi rows")
}

fn render_editor(store: &ClipboardStore, mut state: UiState, id: u64) -> Result<()> {
    let history = store.load()?;
    let Some(item) = history.items.iter().find(|item| item.id == id) else {
        bail!("selected clipboard item no longer exists");
    };
    if item.kind != ItemKind::Text {
        bail!("images cannot be edited as text");
    }
    state.editing = Some(id);

    let mut output = Vec::new();
    write_common_headers(&mut output, "Edit", state, false, false);
    write!(&mut output, "Type replacement text and press Enter")?;
    write_row_option(&mut output, "nonselectable", "true");
    write_row_option(&mut output, "permanent", "true");
    output.push(RECORD_SEPARATOR);
    io::stdout().write_all(&output).context("write rofi editor")
}

fn write_common_headers(
    output: &mut Vec<u8>,
    prompt: &str,
    mut state: UiState,
    no_custom: bool,
    keep_selection: bool,
) {
    if !state.initialized {
        // The first delimiter header must itself end with rofi's initial '\n'
        // delimiter. Every later record (and later script invocation) uses RS.
        output.push(0);
        output.extend_from_slice(b"delim");
        output.push(UNIT_SEPARATOR);
        output.push(RECORD_SEPARATOR);
        output.push(b'\n');
        state.initialized = true;
    }
    write_header(output, "prompt", prompt);
    write_header(
        output,
        "no-custom",
        if no_custom { "true" } else { "false" },
    );
    write_header(output, "use-hot-keys", "true");
    write_header(output, "data", &state.encode());
    write_header(output, "theme", preview_theme(state.preview));
    if keep_selection {
        write_header(output, "keep-selection", "true");
        write_header(output, "keep-filter", "true");
    }
}

fn preview_theme(preview: bool) -> &'static str {
    if preview {
        "preview-pane { enabled: true; }"
    } else {
        "preview-pane { enabled: false; }"
    }
}

fn write_header(output: &mut Vec<u8>, key: &str, value: &str) {
    output.push(0);
    output.extend_from_slice(key.as_bytes());
    output.push(UNIT_SEPARATOR);
    output.extend_from_slice(sanitize_record_value(value).as_bytes());
    output.push(RECORD_SEPARATOR);
}

fn write_row_option(output: &mut Vec<u8>, key: &str, value: &str) {
    output.push(0);
    output.extend_from_slice(key.as_bytes());
    output.push(UNIT_SEPARATOR);
    output.extend_from_slice(sanitize_option_value(value).as_bytes());
}

fn row_value(item: &ClipboardItem) -> String {
    match item.kind {
        ItemKind::Text => item.text.clone().unwrap_or_default(),
        ItemKind::Image => format!("Image · {}", short_mime(&item.mime)),
    }
}

fn row_preview(item: &ClipboardItem) -> String {
    match item.kind {
        ItemKind::Text => {
            let text = item.text.as_deref().unwrap_or_default();
            let one_line = text.split_whitespace().collect::<Vec<_>>().join(" ");
            truncate_chars(&one_line, 110)
        }
        ItemKind::Image => format!("Image · {}", short_mime(&item.mime)),
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

fn sanitize_record_value(value: &str) -> String {
    value
        .replace('\0', "␀")
        .replace(char::from(RECORD_SEPARATOR), "\n")
}

fn sanitize_option_value(value: &str) -> String {
    sanitize_record_value(value).replace(char::from(UNIT_SEPARATOR), " ")
}

fn truncate_chars(value: &str, maximum: usize) -> String {
    let mut chars = value.chars();
    let mut result: String = chars.by_ref().take(maximum).collect();
    if chars.next().is_some() {
        result.push('…');
    }
    result
}

fn decode_edit_input(value: &str) -> String {
    let mut result = String::with_capacity(value.len());
    let mut chars = value.chars().peekable();
    while let Some(character) = chars.next() {
        if character != '\\' {
            result.push(character);
            continue;
        }
        match chars.peek().copied() {
            Some('n') => {
                chars.next();
                result.push('\n');
            }
            Some('t') => {
                chars.next();
                result.push('\t');
            }
            Some('\\') => {
                chars.next();
                result.push('\\');
            }
            _ => result.push('\\'),
        }
    }
    result
}

fn rofi_binary() -> PathBuf {
    env::var_os("ROFI_CLIPBOARD_ROFI")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new("rofi").to_path_buf())
}
