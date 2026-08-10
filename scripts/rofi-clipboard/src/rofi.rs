use std::env;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::clipboard::copy_item;
use crate::model::{ClipboardItem, ItemKind};
use crate::preview;
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
    initialized: bool,
}

impl UiState {
    fn parse(value: Option<String>) -> Self {
        let Some(value) = value else {
            return Self::default();
        };
        let mut state = Self::default();
        for part in value.split(';') {
            if part == "init=1" {
                state.initialized = true;
            }
        }
        state
    }

    fn encode(self) -> String {
        format!("init={}", u8::from(self.initialized))
    }
}

pub fn launch_rofi(mode: Mode, selected_id: Option<u64>) -> Result<()> {
    let executable = env::current_exe().context("locate rofi-clipboard executable")?;
    let executable = executable.to_string_lossy();
    let preview_socket = preview::session_socket_path()?;
    preview::cleanup_session(&preview_socket)?;
    let modes = format!(
        "pinned:{executable} script pinned,text:{executable} script text,images:{executable} script images"
    );
    let theme = theme_path()?;
    let mut command = Command::new(rofi_binary());
    command
        .env(preview::SOCKET_ENV, &preview_socket)
        .args([
            "-show",
            mode.name(),
            "-show-icons",
            "-modes",
            &modes,
            "-display-pinned",
            "󰐃 Pinned",
            "-display-text",
            "󰦨 Text",
            "-display-images",
            "󰋩 Images",
            "-kb-custom-1",
            "Alt+p",
            "-kb-custom-2",
            "Alt+d",
            "-kb-custom-3",
            "Alt+e",
            "-theme",
        ])
        .arg(theme);

    if let Some(row) = selected_row(mode, selected_id)? {
        command.arg("-selected-row").arg(row.to_string());
    }

    let status = command.status();
    preview::close(&preview_socket);
    if let Err(error) = preview::cleanup_socket(&preview_socket) {
        eprintln!("rofi-clipboard: {error:#}");
    }
    let status = status.context("launch rofi")?;
    if !status.success() && status.code() != Some(1) {
        bail!("rofi exited with {status}");
    }
    Ok(())
}

fn selected_row(mode: Mode, selected_id: Option<u64>) -> Result<Option<usize>> {
    let Some(selected_id) = selected_id else {
        return Ok(None);
    };
    let history = ClipboardStore::discover()?.load()?;
    Ok(history
        .items
        .iter()
        .filter(|item| mode.includes(item))
        .position(|item| item.id == selected_id))
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

pub fn run_script(mode: Mode, _script_argument: Option<String>) -> Result<()> {
    let store = ClipboardStore::discover()?;
    let retv = env::var("ROFI_RETV")
        .ok()
        .and_then(|value| value.parse::<u8>().ok())
        .unwrap_or(0);
    let selected_id = env::var("ROFI_INFO")
        .ok()
        .and_then(|value| value.parse::<u64>().ok());
    let state = UiState::parse(env::var("ROFI_DATA").ok());

    match retv {
        // Pre-arm keep-selection on the initial response so the first button
        // action preserves the highlighted row.
        0 => render_history(&store, mode, state, None),
        // Copies the selected item to the clipboard, then closes Rofi.
        1 => {
            if let Some(id) = selected_id {
                copy_item(&store, id)?;
            }
            Ok(())
        }
        // Deletes the selected item. Rofi's native delete action reports 3;
        // the Delete button uses custom action 2 and reports 11.
        3 | 11 => {
            if let Some(id) = selected_id {
                store.delete(id)?;
            }
            render_history(&store, mode, state, selected_id)
        }
        // Pins or unpins the selected item.
        10 => {
            if let Some(id) = selected_id {
                store.pin(id)?;
            }
            render_history(&store, mode, state, selected_id)
        }
        // First click opens an editor pinned to this text item. The next Edit
        // click saves that panel's complete buffer and closes it.
        12 => {
            let selected_id = preview::toggle_edit(&store, selected_id)?.or(selected_id);
            render_history(&store, mode, state, selected_id)
        }
        _ => render_history(&store, mode, state, selected_id),
    }
}

fn render_history(
    store: &ClipboardStore,
    mode: Mode,
    state: UiState,
    selected_id: Option<u64>,
) -> Result<()> {
    let history = store.load()?;
    let items: Vec<_> = history
        .items
        .iter()
        .filter(|item| mode.includes(item))
        .collect();
    let new_selection =
        selected_id.and_then(|id| items.iter().position(|item| item.id == id));

    let mut output = Vec::new();
    write_common_headers(
        &mut output,
        mode.prompt(),
        state,
        true,
        true,
        new_selection,
    );

    if items.is_empty() {
        write!(&mut output, "Nothing here yet")?;
        let mut first_option = true;
        write_row_option(&mut output, &mut first_option, "nonselectable", "true");
        write_row_option(&mut output, &mut first_option, "permanent", "true");
        output.push(RECORD_SEPARATOR);
    }

    for item in items {
        write!(&mut output, "{}", item.id)?;
        let mut first_option = true;
        write_row_option(
            &mut output,
            &mut first_option,
            "display",
            &row_preview(item),
        );
        write_row_option(
            &mut output,
            &mut first_option,
            "info",
            &item.id.to_string(),
        );
        write_row_option(&mut output, &mut first_option, "meta", &row_value(item));
        if let Some(path) = store.image_path(item) {
            write_row_option(
                &mut output,
                &mut first_option,
                "icon",
                &path.to_string_lossy(),
            );
        }
        if item.pinned {
            write_row_option(&mut output, &mut first_option, "active", "true");
        }
        output.push(RECORD_SEPARATOR);
    }
    io::stdout().write_all(&output).context("write rofi rows")
}

fn write_common_headers(
    output: &mut Vec<u8>,
    prompt: &str,
    mut state: UiState,
    no_custom: bool,
    keep_selection: bool,
    new_selection: Option<usize>,
) {
    if !state.initialized {
        // The first delimiter header must itself end with Rofi's initial '\n'
        // delimiter. Every later record and invocation uses RS.
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

    if keep_selection {
        write_header(output, "keep-selection", "true");
        if let Some(index) = new_selection {
            write_header(output, "new-selection", &index.to_string());
        }
    }
}

fn write_header(output: &mut Vec<u8>, key: &str, value: &str) {
    output.push(0);
    output.extend_from_slice(key.as_bytes());
    output.push(UNIT_SEPARATOR);
    output.extend_from_slice(sanitize_record_value(value).as_bytes());
    output.push(RECORD_SEPARATOR);
}

fn write_row_option(output: &mut Vec<u8>, first: &mut bool, key: &str, value: &str) {
    // A row has one NUL before all metadata. Individual key/value pairs are
    // separated by US. A second NUL would make Rofi ignore every later option.
    output.push(if *first { 0 } else { UNIT_SEPARATOR });
    *first = false;
    output.extend_from_slice(key.as_bytes());
    output.push(UNIT_SEPARATOR);
    output.extend_from_slice(sanitize_option_value(value).as_bytes());
}

fn row_value(item: &ClipboardItem) -> String {
    match item.kind {
        ItemKind::Text => item.text.clone().unwrap_or_default(),
        ItemKind::Image => image_label(item),
    }
}

fn row_preview(item: &ClipboardItem) -> String {
    match item.kind {
        ItemKind::Text => {
            let text = item.text.as_deref().unwrap_or_default();
            let one_line = text.split_whitespace().collect::<Vec<_>>().join(" ");
            truncate_chars(&one_line, 110)
        }
        ItemKind::Image => truncate_chars(&image_label(item), 110),
    }
}

fn image_label(item: &ClipboardItem) -> String {
    item.name
        .as_deref()
        .filter(|source| !source.trim().is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| format!("Image · {}", short_mime(&item.mime)))
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

fn rofi_binary() -> PathBuf {
    env::var_os("ROFI_CLIPBOARD_ROFI")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new("rofi").to_path_buf())
}


#[cfg(test)]
mod tests {
    use super::*;

    fn image_item(name: Option<&str>) -> ClipboardItem {
        ClipboardItem {
            id: 1,
            kind: ItemKind::Image,
            text: None,
            image_file: Some("1.png".to_owned()),
            name: name.map(str::to_owned),
            mime: "image/png".to_owned(),
            pinned: false,
            created_at: 0,
            digest: "digest".to_owned(),
        }
    }

    #[test]
    fn image_row_uses_internet_source_url() {
        let item = image_item(Some("https://example.com/images/photo.png"));

        assert_eq!(row_value(&item), "https://example.com/images/photo.png");
    }

    #[test]
    fn image_row_uses_local_source_path() {
        let item = image_item(Some("/home/raina/Pictures/photo.png"));

        assert_eq!(row_value(&item), "/home/raina/Pictures/photo.png");
    }

    #[test]
    fn image_row_falls_back_to_mime_for_entries_without_a_source() {
        let item = image_item(None);

        assert_eq!(row_value(&item), "Image · png");
    }

    #[test]
    fn text_row_preview_collapses_whitespace_to_one_line() {
        let item = ClipboardItem {
            id: 2,
            kind: ItemKind::Text,
            text: Some("first line\nsecond\tline   third".to_owned()),
            image_file: None,
            name: None,
            mime: "text/plain".to_owned(),
            pinned: false,
            created_at: 0,
            digest: "digest".to_owned(),
        };

        assert_eq!(row_preview(&item), "first line second line third");
        assert_eq!(row_value(&item), "first line\nsecond\tline   third");
    }

    #[test]
    fn text_row_preview_truncates_long_text() {
        let item = ClipboardItem {
            id: 3,
            kind: ItemKind::Text,
            text: Some("x".repeat(111)),
            image_file: None,
            name: None,
            mime: "text/plain".to_owned(),
            pinned: false,
            created_at: 0,
            digest: "digest".to_owned(),
        };

        assert_eq!(row_preview(&item), format!("{}…", "x".repeat(110)));
    }

}
