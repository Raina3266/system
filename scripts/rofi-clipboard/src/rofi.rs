use std::env;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::clipboard::copy_item;
use crate::model::{ClipboardItem, ItemKind, abbreviate_home_path};
use crate::preview;
use crate::store::ClipboardStore;

const WAYLAND_KEYBOARD_MODE_ENV: &str = "ROFI_WAYLAND_KEYBOARD_MODE";
const WAYLAND_KEYBOARD_MODE_ON_DEMAND: &str = "on-demand";
const PRESERVE_FILTER_SELECTION_ENV: &str = "ROFI_PRESERVE_SELECTION_ON_FILTER";
const PRESERVE_FILTER_SELECTION_ENABLED: &str = "true";

const RECORD_SEPARATOR: u8 = 0x1e;
const UNIT_SEPARATOR: u8 = 0x1f;

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

    fn prompt(self) -> &'static str {
        match self {
            Self::Memo => "󰍩 Memo",
            Self::Text => "󰦨 Text",
            Self::Files => "󰈔 Files",
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Memo => "memo",
            Self::Text => "text",
            Self::Files => "files",
        }
    }

    fn includes(self, item: &ClipboardItem) -> bool {
        match self {
            Self::Memo => item.kind == ItemKind::Memo,
            Self::Text => item.kind == ItemKind::Text,
            Self::Files => item.kind == ItemKind::File,
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

fn configure_rofi_environment(command: &mut Command) {
    // The companion editor is another overlay layer surface. On-demand focus
    // lets Niri transfer keyboard input between Rofi and that panel when
    // either one is clicked.
    command.env(WAYLAND_KEYBOARD_MODE_ENV, WAYLAND_KEYBOARD_MODE_ON_DEMAND);
    // Rofi normally keeps the same visible row number while refiltering. The
    // patched opt-in behavior instead follows the selected clipboard item.
    command.env(
        PRESERVE_FILTER_SELECTION_ENV,
        PRESERVE_FILTER_SELECTION_ENABLED,
    );
}

pub fn launch_rofi(mode: Mode, selected_id: Option<u64>) -> Result<()> {
    let executable = env::current_exe().context("locate rofi-clipboard executable")?;
    let executable = executable.to_string_lossy();
    let preview_socket = preview::session_socket_path()?;
    preview::cleanup_session(&preview_socket)?;
    let modes = format!(
        "memo:{executable} script memo,text:{executable} script text,files:{executable} script files"
    );
    let selection_command = format!(
        "{} preview-selection {{completion}} {{selection-serial}}",
        shell_quote(&executable)
    );
    let theme = theme_path()?;
    let mut command = Command::new(rofi_binary());
    configure_rofi_environment(&mut command);
    command
        .env(preview::SOCKET_ENV, &preview_socket)
        .args([
            "-show",
            mode.name(),
            "-show-icons",
            "-modes",
            &modes,
            "-display-memo",
            "󰍩 Memo",
            "-display-text",
            "󰦨 Text",
            "-display-files",
            "󰈔 Files",
            "-kb-custom-1",
            "Alt+p",
            "-kb-custom-2",
            "Alt+d",
            "-kb-custom-3",
            "Alt+e",
            "-on-selection-changed",
            &selection_command,
            "-theme",
        ])
        .arg(theme);

    if let Some(row) = selected_row(mode, selected_id)? {
        command.arg("-selected-row").arg(row.to_string());
    }

    let status = command.status();
    if let Err(error) = preview::save_and_close(&preview_socket) {
        eprintln!("rofi-clipboard: save preview before closing: {error:#}");
        preview::close(&preview_socket);
    }
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
    let store = ClipboardStore::discover()?;
    prepare_mode(&store, mode)?;
    let history = store.load()?;
    let items = mode_items(&history.items, mode);
    Ok(preferred_selection(&items, selected_id))
}

fn prepare_mode(store: &ClipboardStore, mode: Mode) -> Result<()> {
    store.prune_missing_local_files()?;
    if mode == Mode::Memo {
        store.ensure_memo_draft()?;
    }
    Ok(())
}

fn mode_items(items: &[ClipboardItem], mode: Mode) -> Vec<&ClipboardItem> {
    let mut items: Vec<_> = items.iter().filter(|item| mode.includes(item)).collect();
    if mode == Mode::Memo {
        // The draft is stored near the newest entries so history trimming can
        // never discard it, but it is presented as the final Memo row.
        items.sort_by_key(|item| item.is_empty_memo());
    }
    items
}

fn preferred_selection(items: &[&ClipboardItem], selected_id: Option<u64>) -> Option<usize> {
    selected_id
        .and_then(|id| items.iter().position(|item| item.id == id))
        .or_else(|| items.iter().position(|item| !item.pinned))
        .or_else(|| (!items.is_empty()).then_some(0))
}

pub(crate) fn theme_path() -> Result<PathBuf> {
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
            let selected_id = if let Some(id) = selected_id {
                let replacement = selection_after_delete(&store, mode, id)?;
                if store.delete(id)? {
                    if let Err(error) = preview::refresh_after_delete(&store, replacement) {
                        eprintln!("rofi-clipboard: refresh preview after delete: {error:#}");
                    }
                    replacement
                } else {
                    Some(id)
                }
            } else {
                None
            };
            render_history(&store, mode, state, selected_id)
        }
        // Pins or unpins the selected item.
        10 => {
            if let Some(id) = selected_id {
                store.pin(id)?;
            }
            render_history(&store, mode, state, selected_id)
        }
        // The first click opens the selected text editor or image preview. The
        // next Edit click saves text or closes the image panel.
        12 => {
            let selected_id = preview::toggle_edit(&store, selected_id)?.or(selected_id);
            render_history(&store, mode, state, selected_id)
        }
        _ => render_history(&store, mode, state, selected_id),
    }
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

fn replacement_selection(items: &[&ClipboardItem], selected_id: u64) -> Option<u64> {
    let index = items.iter().position(|item| item.id == selected_id)?;
    items
        .get(index + 1)
        .or_else(|| index.checked_sub(1).and_then(|index| items.get(index)))
        .map(|item| item.id)
}

fn render_history(
    store: &ClipboardStore,
    mode: Mode,
    state: UiState,
    selected_id: Option<u64>,
) -> Result<()> {
    prepare_mode(store, mode)?;
    let history = store.load()?;
    let items = mode_items(&history.items, mode);
    let new_selection = preferred_selection(&items, selected_id);

    let mut output = Vec::new();
    write_common_headers(&mut output, mode.prompt(), state, true, true, new_selection);

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
        write_row_option(&mut output, &mut first_option, "info", &item.id.to_string());
        write_row_option(&mut output, &mut first_option, "meta", &row_value(item));
        if item.is_empty_memo() {
            // Keep the creation row available even while Rofi is filtering.
            write_row_option(&mut output, &mut first_option, "permanent", "true");
        }
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
        ItemKind::Memo | ItemKind::Text => item.text.clone().unwrap_or_default(),
        ItemKind::File => file_label(item),
    }
}

fn row_preview(item: &ClipboardItem) -> String {
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

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn textual_item(id: u64, kind: ItemKind, text: &str, pinned: bool) -> ClipboardItem {
        ClipboardItem {
            id,
            kind,
            text: Some(text.to_owned()),
            image_file: None,
            name: None,
            mime: "text/plain".to_owned(),
            pinned,
            created_at: 0,
            digest: format!("digest-{id}"),
        }
    }

    fn image_item(name: Option<&str>) -> ClipboardItem {
        ClipboardItem {
            id: 1,
            kind: ItemKind::File,
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
    fn memo_mode_excludes_pinned_clipboard_items() {
        let memo = textual_item(1, ItemKind::Memo, "memo", false);
        let pinned_text = textual_item(2, ItemKind::Text, "clipboard", true);
        let mut pinned_image = image_item(None);
        pinned_image.id = 3;
        pinned_image.pinned = true;

        assert!(Mode::Memo.includes(&memo));
        assert!(!Mode::Memo.includes(&pinned_text));
        assert!(!Mode::Memo.includes(&pinned_image));
        assert!(Mode::Text.includes(&pinned_text));
        assert!(Mode::Files.includes(&pinned_image));
    }

    #[test]
    fn text_mode_contains_urls_and_excludes_them_from_file_mode() {
        let url = textual_item(
            5,
            ItemKind::Text,
            "https://example.com/download.tar.zst",
            false,
        );

        assert!(Mode::Text.includes(&url));
        assert!(!Mode::Files.includes(&url));
        assert_eq!(row_value(&url), "https://example.com/download.tar.zst");
    }

    #[test]
    fn file_is_the_named_replacement_for_image_mode() {
        assert_eq!(Mode::parse("files").unwrap(), Mode::Files);
        assert_eq!(Mode::parse("images").unwrap(), Mode::Files);
        assert_eq!(Mode::Files.name(), "files");
        assert_eq!(Mode::Files.prompt(), "󰈔 Files");
    }

    #[test]
    fn memo_is_the_named_replacement_for_pinned_mode() {
        assert_eq!(Mode::parse("memo").unwrap(), Mode::Memo);
        assert_eq!(Mode::Memo.name(), "memo");
        assert_eq!(Mode::Memo.prompt(), "󰍩 Memo");
        assert!(Mode::parse("pinned").is_err());
    }

    #[test]
    fn empty_memo_has_a_visible_draft_label() {
        let memo = textual_item(4, ItemKind::Memo, "", false);

        assert_eq!(row_preview(&memo), "New memo");
        assert_eq!(row_value(&memo), "");
    }

    #[test]
    fn empty_memo_is_the_last_item_in_memo_mode() {
        let draft = textual_item(4, ItemKind::Memo, "", false);
        let newer = textual_item(3, ItemKind::Memo, "newer", false);
        let clipboard_text = textual_item(2, ItemKind::Text, "clipboard", false);
        let older = textual_item(1, ItemKind::Memo, "older", true);
        let history = vec![draft, newer, clipboard_text, older];

        let items = mode_items(&history, Mode::Memo);

        assert_eq!(
            items.iter().map(|item| item.id).collect::<Vec<_>>(),
            vec![3, 1, 4]
        );
    }

    #[test]
    fn clipboard_rofi_enables_companion_focus_and_stable_filter_selection() {
        let mut command = Command::new("rofi");
        configure_rofi_environment(&mut command);

        let keyboard_mode = command
            .get_envs()
            .find(|(name, _)| *name == std::ffi::OsStr::new(WAYLAND_KEYBOARD_MODE_ENV))
            .and_then(|(_, value)| value)
            .and_then(std::ffi::OsStr::to_str);
        assert_eq!(keyboard_mode, Some(WAYLAND_KEYBOARD_MODE_ON_DEMAND));

        let preserve_selection = command
            .get_envs()
            .find(|(name, _)| *name == std::ffi::OsStr::new(PRESERVE_FILTER_SELECTION_ENV))
            .and_then(|(_, value)| value)
            .and_then(std::ffi::OsStr::to_str);
        assert_eq!(preserve_selection, Some(PRESERVE_FILTER_SELECTION_ENABLED));
    }

    #[test]
    fn text_row_preview_collapses_whitespace_to_one_line() {
        let item = textual_item(2, ItemKind::Text, "first line\nsecond\tline   third", false);

        assert_eq!(row_preview(&item), "first line second line third");
        assert_eq!(row_value(&item), "first line\nsecond\tline   third");
    }

    #[test]
    fn text_row_preview_truncates_long_text() {
        let text = "x".repeat(111);
        let item = textual_item(3, ItemKind::Text, &text, false);

        assert_eq!(row_preview(&item), format!("{}…", "x".repeat(110)));
    }

    #[test]
    fn selection_callback_executable_is_shell_quoted() {
        assert_eq!(
            shell_quote("/nix/store/example/bin/tool"),
            "'/nix/store/example/bin/tool'"
        );
        assert_eq!(shell_quote("/tmp/raina's tool"), "'/tmp/raina'\\''s tool'");
    }

    #[test]
    fn deletion_selects_the_following_row_or_the_previous_row_at_the_end() {
        let first = textual_item(1, ItemKind::Text, "first", false);
        let second = textual_item(2, ItemKind::Text, "second", false);
        let third = textual_item(3, ItemKind::Text, "third", false);
        let items = vec![&first, &second, &third];

        assert_eq!(replacement_selection(&items, first.id), Some(second.id));
        assert_eq!(replacement_selection(&items, second.id), Some(third.id));
        assert_eq!(replacement_selection(&items, third.id), Some(second.id));
        assert_eq!(replacement_selection(&[&first], first.id), None);
    }

    #[test]
    fn initial_selection_prefers_the_first_unpinned_item_in_every_mode() {
        let first_pin = textual_item(1, ItemKind::Text, "first pin", true);
        let second_pin = textual_item(2, ItemKind::Text, "second pin", true);
        let first_unpinned = textual_item(3, ItemKind::Text, "first normal", false);
        let second_unpinned = textual_item(4, ItemKind::Text, "second normal", false);
        let items = vec![&first_pin, &second_pin, &first_unpinned, &second_unpinned];

        assert_eq!(preferred_selection(&items, None), Some(2));
        assert_eq!(preferred_selection(&items, Some(first_pin.id)), Some(0));
        assert_eq!(
            preferred_selection(&[&first_pin, &second_pin], None),
            Some(0)
        );
        assert_eq!(preferred_selection(&[], None), None);
    }
}
