use std::env;
use std::fmt::Write as _;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};

const RECORD_SEPARATOR: u8 = 0x1e;
const UNIT_SEPARATOR: u8 = 0x1f;

fn main() {
    if let Err(error) = run() {
        eprintln!("rofi-clipboard: {error:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let mut args = env::args().skip(1);
    match args.next().as_deref() {
        None | Some("run") => launch_rofi(Mode::Pinned, false, None),
        Some("relaunch") => {
            let mode = Mode::parse(args.next().as_deref().unwrap_or("pinned"))?;
            let preview = args.next().as_deref() == Some("preview");
            let selected_id = args.next().and_then(|value| value.parse().ok());
            std::thread::sleep(Duration::from_millis(120));
            launch_rofi(mode, preview, selected_id)
        }
        Some("script") => {
            let mode = Mode::parse(args.next().as_deref().unwrap_or("pinned"))?;
            run_script(mode)
        }
        Some("capture") => capture_clipboard(),
        Some("store") => {
            let mime = parse_mime_argument(args)?;
            store_stdin(&mime)
        }
        Some("print-path") => {
            println!("{}", ClipboardStore::discover()?.history_path().display());
            Ok(())
        }
        Some("help" | "--help" | "-h") => {
            print_help();
            Ok(())
        }
        Some(command) => bail!("unknown command {command:?}; try --help"),
    }
}

fn print_help() {
    println!(
        "rofi-clipboard — JSON-backed Wayland clipboard menu\n\n\
         Usage:\n  rofi-clipboard [run]\n  rofi-clipboard capture\n  \
         rofi-clipboard store --mime MIME\n  rofi-clipboard print-path"
    );
}

fn parse_mime_argument(mut args: impl Iterator<Item = String>) -> Result<String> {
    let flag = args.next();
    match (flag.as_deref(), args.next()) {
        (Some("--mime"), Some(mime)) => Ok(mime),
        _ => bail!("store requires --mime MIME"),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Mode {
    Pinned,
    Text,
    Images,
}

impl Mode {
    fn parse(value: &str) -> Result<Self> {
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
                preview: env::var_os("ROFI_CLIPBOARD_PREVIEW").is_some(),
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

fn launch_rofi(mode: Mode, preview: bool, selected_id: Option<u64>) -> Result<()> {
    let executable = env::current_exe().context("locate rofi-clipboard executable")?;
    let executable = executable.to_string_lossy();
    let modes = format!(
        "pinned:{executable} script pinned,text:{executable} script text,images:{executable} script images"
    );
    let theme = theme_path()?;
    let mut command = Command::new(rofi_binary());
    command
        .env_remove("ROFI_CLIPBOARD_PREVIEW")
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
            "Alt+p",
            "-kb-custom-2",
            "Alt+d",
            "-kb-custom-3",
            "Alt+e",
            "-kb-custom-4",
            "Alt+v",
            "-theme",
        ])
        .arg(theme);
    if preview {
        command.env("ROFI_CLIPBOARD_PREVIEW", "true");
    }
    if let Some(row) = selected_row(mode, selected_id)? {
        command.arg("-selected-row").arg(row.to_string());
    }
    let status = command.status().context("launch rofi")?;
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

fn relaunch_rofi(mode: Mode, preview: bool, selected_id: Option<u64>) -> Result<()> {
    let executable = env::current_exe().context("locate rofi-clipboard executable")?;
    let mut command = Command::new(executable);
    command
        .args([
            "relaunch",
            mode.name(),
            if preview { "preview" } else { "compact" },
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some(id) = selected_id {
        command.arg(id.to_string());
    }
    command.spawn().context("schedule rofi preview relaunch")?;
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

fn run_script(mode: Mode) -> Result<()> {
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
        0 => render_history(&store, mode, state, None, false),
        1 => {
            if let Some(id) = selected_id {
                copy_item(&store, id)?;
            }
            Ok(())
        }
        2 if state.editing.is_some() => {
            let id = state.editing.take().expect("editing ID checked above");
            let replacement = decode_edit_input(&env::var("ROFI_INPUT").unwrap_or_default());
            match store.edit_text(id, replacement) {
                Ok(true) => render_history(&store, mode, state, Some("Text updated"), true),
                Ok(false) => {
                    render_history(&store, mode, state, Some("Item no longer exists"), true)
                }
                Err(error) => render_history(&store, mode, state, Some(&error.to_string()), true),
            }
        }
        3 | 11 => match selected_id {
            Some(id) if store.delete(id)? => {
                render_history(&store, mode, state, Some("Item deleted"), true)
            }
            _ => render_history(&store, mode, state, Some("Select an item to delete"), true),
        },
        10 => match selected_id {
            Some(id) if store.pin(id)? => {
                render_history(&store, mode, state, Some("Item pinned"), true)
            }
            Some(_) => render_history(&store, mode, state, Some("Item is already pinned"), true),
            None => render_history(&store, mode, state, Some("Select an item to pin"), true),
        },
        12 => match selected_id {
            Some(id) => render_editor(&store, mode, state, id),
            None => render_history(&store, mode, state, Some("Select text to edit"), true),
        },
        13 => relaunch_rofi(mode, !state.preview, selected_id),
        _ if state.editing.is_some() => render_editor(
            &store,
            mode,
            state,
            state.editing.expect("editing ID checked above"),
        ),
        _ => render_history(&store, mode, state, None, true),
    }
}

fn render_history(
    store: &ClipboardStore,
    mode: Mode,
    state: UiState,
    message: Option<&str>,
    keep_selection: bool,
) -> Result<()> {
    let history = store.load()?;
    let items: Vec<_> = history
        .items
        .iter()
        .filter(|item| mode.includes(item))
        .collect();
    let mut output = Vec::new();
    write_common_headers(
        &mut output,
        mode.prompt(),
        state,
        message.or_else(|| items.is_empty().then_some(empty_message(mode))),
        true,
        keep_selection,
    );

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

fn render_editor(store: &ClipboardStore, mode: Mode, mut state: UiState, id: u64) -> Result<()> {
    let history = store.load()?;
    let Some(item) = history.items.iter().find(|item| item.id == id) else {
        state.editing = None;
        return render_history(store, mode, state, Some("Item no longer exists"), true);
    };
    if item.kind != ItemKind::Text {
        state.editing = None;
        return render_history(
            store,
            mode,
            state,
            Some("Images cannot be edited as text"),
            true,
        );
    }
    state.editing = Some(id);
    let current = item.text.as_deref().unwrap_or_default();
    let current = truncate_chars(current, 1600);
    let message = format!(
        "Current text:\n{}\n\nType the replacement above. Use \\n and \\t for line breaks and tabs.",
        current
    );

    let mut output = Vec::new();
    write_common_headers(&mut output, "Edit", state, Some(&message), false, false);
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
    message: Option<&str>,
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
    if keep_selection {
        write_header(output, "keep-selection", "true");
        write_header(output, "keep-filter", "true");
    }
    if let Some(message) = message {
        write_header(output, "message", message);
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

fn empty_message(mode: Mode) -> &'static str {
    match mode {
        Mode::Pinned => "No pinned items yet. Open Text or Images, select one, then press Pin.",
        Mode::Text => "No text has been copied yet.",
        Mode::Images => "No images have been copied yet.",
    }
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

fn copy_item(store: &ClipboardStore, id: u64) -> Result<()> {
    let history = store.load()?;
    let item = history
        .items
        .iter()
        .find(|item| item.id == id)
        .context("selected clipboard item no longer exists")?;
    let bytes = store.item_bytes(item)?;
    let mut child = Command::new(wl_copy_binary())
        .args(["--type", &item.mime])
        .stdin(Stdio::piped())
        .spawn()
        .context("launch wl-copy")?;
    child
        .stdin
        .as_mut()
        .context("open wl-copy stdin")?
        .write_all(&bytes)
        .context("write clipboard data")?;
    let status = child.wait().context("wait for wl-copy")?;
    if !status.success() {
        bail!("wl-copy exited with {status}");
    }
    Ok(())
}

fn capture_clipboard() -> Result<()> {
    let mut watched_bytes = Vec::new();
    io::stdin()
        .read_to_end(&mut watched_bytes)
        .context("read watched clipboard data")?;

    match env::var("CLIPBOARD_STATE").as_deref() {
        Ok("sensitive" | "nil") => return Ok(()),
        _ => {}
    }

    let types_output = Command::new(wl_paste_binary())
        .arg("--list-types")
        .output()
        .context("list clipboard MIME types")?;
    let types = String::from_utf8_lossy(&types_output.stdout);
    if types.lines().any(is_sensitive_mime) {
        return Ok(());
    }

    if let Some(mime) = preferred_image_mime(types.lines()) {
        let bytes = if bytes_match_mime(&watched_bytes, mime) {
            watched_bytes
        } else {
            let output = Command::new(wl_paste_binary())
                .args(["--type", mime])
                .output()
                .with_context(|| format!("read clipboard as {mime}"))?;
            if !output.status.success() {
                return Ok(());
            }
            output.stdout
        };
        ClipboardStore::discover()?.add_image(&bytes, mime.to_owned())?;
        return Ok(());
    }

    if watched_bytes.is_empty() {
        return Ok(());
    }
    if let Ok(text) = String::from_utf8(watched_bytes) {
        let mime = preferred_text_mime(types.lines()).unwrap_or("text/plain;charset=utf-8");
        ClipboardStore::discover()?.add_text(text, mime.to_owned())?;
    }
    Ok(())
}

fn store_stdin(mime: &str) -> Result<()> {
    let mut bytes = Vec::new();
    io::stdin().read_to_end(&mut bytes).context("read stdin")?;
    let store = ClipboardStore::discover()?;
    if mime.starts_with("image/") {
        store.add_image(&bytes, mime.to_owned())?;
    } else {
        let text = String::from_utf8(bytes).context("clipboard text is not UTF-8")?;
        store.add_text(text, mime.to_owned())?;
    }
    Ok(())
}

fn preferred_image_mime<'a>(types: impl Iterator<Item = &'a str> + Clone) -> Option<&'a str> {
    const PREFERENCE: &[&str] = &[
        "image/png",
        "image/jpeg",
        "image/webp",
        "image/gif",
        "image/bmp",
        "image/tiff",
        "image/svg+xml",
    ];
    for preferred in PREFERENCE {
        if let Some(found) = types.clone().find(|mime| *mime == *preferred) {
            return Some(found);
        }
    }
    types.filter(|mime| mime.starts_with("image/")).next()
}

fn preferred_text_mime<'a>(types: impl Iterator<Item = &'a str> + Clone) -> Option<&'a str> {
    const PREFERENCE: &[&str] = &[
        "text/plain;charset=utf-8",
        "text/plain;charset=UTF-8",
        "UTF8_STRING",
        "text/plain",
    ];
    for preferred in PREFERENCE {
        if let Some(found) = types.clone().find(|mime| *mime == *preferred) {
            return Some(found);
        }
    }
    types.filter(|mime| mime.starts_with("text/")).next()
}

fn is_sensitive_mime(mime: &str) -> bool {
    matches!(
        mime,
        "x-kde-passwordManagerHint" | "application/x-kde-passwordManagerHint"
    )
}

fn bytes_match_mime(bytes: &[u8], mime: &str) -> bool {
    match mime {
        "image/png" => bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        "image/jpeg" => bytes.starts_with(b"\xff\xd8\xff"),
        "image/gif" => bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a"),
        "image/webp" => bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP"),
        "image/bmp" => bytes.starts_with(b"BM"),
        "image/tiff" => bytes.starts_with(b"II*\0") || bytes.starts_with(b"MM\0*"),
        "image/svg+xml" => std::str::from_utf8(bytes)
            .map(|text| text.contains("<svg"))
            .unwrap_or(false),
        _ => false,
    }
}

fn rofi_binary() -> PathBuf {
    env_binary("ROFI_CLIPBOARD_ROFI", "rofi")
}

fn wl_copy_binary() -> PathBuf {
    env_binary("ROFI_CLIPBOARD_WL_COPY", "wl-copy")
}

fn wl_paste_binary() -> PathBuf {
    env_binary("ROFI_CLIPBOARD_WL_PASTE", "wl-paste")
}

fn env_binary(variable: &str, fallback: &str) -> PathBuf {
    env::var_os(variable)
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(fallback).to_path_buf())
}

pub const HISTORY_VERSION: u32 = 1;

#[derive(Clone, Debug)]
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
    pub fn to_json(&self) -> String {
        let mut output = String::new();
        writeln!(&mut output, "{{").expect("write to String");
        writeln!(&mut output, "  \"version\": {},", self.version).expect("write to String");
        writeln!(&mut output, "  \"next_id\": {},", self.next_id).expect("write to String");
        writeln!(&mut output, "  \"items\": [").expect("write to String");
        for (index, item) in self.items.iter().enumerate() {
            write!(&mut output, "    {{").expect("write to String");
            write!(&mut output, "\"id\":{},", item.id).expect("write to String");
            write!(
                &mut output,
                "\"kind\":{},",
                json_string(match item.kind {
                    ItemKind::Text => "text",
                    ItemKind::Image => "image",
                })
            )
            .expect("write to String");
            write!(
                &mut output,
                "\"text\":{},",
                optional_json_string(item.text.as_deref())
            )
            .expect("write to String");
            write!(
                &mut output,
                "\"image_file\":{},",
                optional_json_string(item.image_file.as_deref())
            )
            .expect("write to String");
            write!(&mut output, "\"mime\":{},", json_string(&item.mime)).expect("write to String");
            write!(&mut output, "\"pinned\":{},", item.pinned).expect("write to String");
            write!(&mut output, "\"created_at\":{},", item.created_at).expect("write to String");
            write!(&mut output, "\"digest\":{}", json_string(&item.digest))
                .expect("write to String");
            if index + 1 == self.items.len() {
                writeln!(&mut output, "}}").expect("write to String");
            } else {
                writeln!(&mut output, "}},").expect("write to String");
            }
        }
        output.push_str("  ]\n}\n");
        output
    }

    pub fn from_json(json: &str) -> Result<Self> {
        let version = parse_u64(field(json, "version")?)? as u32;
        let next_id = parse_u64(field(json, "next_id")?)?;
        let items_json = field(json, "items")?;
        let mut items = Vec::new();
        for object in array_values(items_json)? {
            let kind = match parse_string(field(object, "kind")?)?.as_str() {
                "text" => ItemKind::Text,
                "image" => ItemKind::Image,
                value => bail!("unknown clipboard item kind {value:?}"),
            };
            items.push(ClipboardItem {
                id: parse_u64(field(object, "id")?)?,
                kind,
                text: parse_optional_string(field(object, "text")?)?,
                image_file: parse_optional_string(field(object, "image_file")?)?,
                mime: parse_string(field(object, "mime")?)?,
                pinned: parse_bool(field(object, "pinned")?)?,
                created_at: parse_u64(field(object, "created_at")?)?,
                digest: parse_string(field(object, "digest")?)?,
            });
        }
        Ok(Self {
            version,
            next_id,
            items,
        })
    }
}

#[derive(Clone, Debug)]
pub struct ClipboardItem {
    pub id: u64,
    pub kind: ItemKind,
    pub text: Option<String>,
    pub image_file: Option<String>,
    pub mime: String,
    pub pinned: bool,
    pub created_at: u64,
    pub digest: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ItemKind {
    Text,
    Image,
}

fn optional_json_string(value: Option<&str>) -> String {
    value.map(json_string).unwrap_or_else(|| "null".to_owned())
}

fn json_string(value: &str) -> String {
    let mut output = String::with_capacity(value.len() + 2);
    output.push('"');
    for character in value.chars() {
        match character {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            '\u{08}' => output.push_str("\\b"),
            '\u{0c}' => output.push_str("\\f"),
            value if value <= '\u{1f}' => {
                write!(&mut output, "\\u{:04x}", value as u32).expect("write to String");
            }
            value => output.push(value),
        }
    }
    output.push('"');
    output
}

fn field<'a>(object: &'a str, wanted: &str) -> Result<&'a str> {
    let bytes = object.as_bytes();
    let mut cursor = skip_whitespace(bytes, 0);
    if bytes.get(cursor) == Some(&b'{') {
        cursor += 1;
    }
    loop {
        cursor = skip_delimiters(bytes, cursor);
        if cursor >= bytes.len() || bytes[cursor] == b'}' {
            bail!("JSON field {wanted:?} is missing");
        }
        let (key, after_key) = parse_string_at(object, cursor)?;
        cursor = skip_whitespace(bytes, after_key);
        if bytes.get(cursor) != Some(&b':') {
            bail!("expected ':' after JSON field {key:?}");
        }
        cursor = skip_whitespace(bytes, cursor + 1);
        let end = value_end(object, cursor)?;
        if key == wanted {
            return Ok(object[cursor..end].trim());
        }
        cursor = end;
    }
}

fn array_values(array: &str) -> Result<Vec<&str>> {
    let bytes = array.as_bytes();
    let mut cursor = skip_whitespace(bytes, 0);
    if bytes.get(cursor) != Some(&b'[') {
        bail!("expected JSON array");
    }
    cursor += 1;
    let mut values = Vec::new();
    loop {
        cursor = skip_delimiters(bytes, cursor);
        if bytes.get(cursor) == Some(&b']') {
            return Ok(values);
        }
        if cursor >= bytes.len() {
            bail!("unterminated JSON array");
        }
        let end = value_end(array, cursor)?;
        values.push(array[cursor..end].trim());
        cursor = end;
    }
}

fn value_end(json: &str, start: usize) -> Result<usize> {
    let bytes = json.as_bytes();
    match bytes.get(start) {
        Some(b'"') => Ok(parse_string_at(json, start)?.1),
        Some(b'{' | b'[') => {
            let opening = bytes[start];
            let closing = if opening == b'{' { b'}' } else { b']' };
            let mut depth = 0_u32;
            let mut cursor = start;
            while cursor < bytes.len() {
                match bytes[cursor] {
                    b'"' => cursor = parse_string_at(json, cursor)?.1,
                    value if value == opening => {
                        depth += 1;
                        cursor += 1;
                    }
                    value if value == closing => {
                        depth -= 1;
                        cursor += 1;
                        if depth == 0 {
                            return Ok(cursor);
                        }
                    }
                    _ => cursor += 1,
                }
            }
            bail!("unterminated JSON container")
        }
        Some(_) => {
            let mut cursor = start;
            while cursor < bytes.len() && !matches!(bytes[cursor], b',' | b'}' | b']') {
                cursor += 1;
            }
            Ok(cursor)
        }
        None => bail!("missing JSON value"),
    }
}

fn parse_optional_string(value: &str) -> Result<Option<String>> {
    if value.trim() == "null" {
        Ok(None)
    } else {
        parse_string(value).map(Some)
    }
}

fn parse_string(value: &str) -> Result<String> {
    let start = skip_whitespace(value.as_bytes(), 0);
    let (parsed, end) = parse_string_at(value, start)?;
    if !value[end..].trim().is_empty() {
        bail!("unexpected content after JSON string");
    }
    Ok(parsed)
}

fn parse_string_at(value: &str, start: usize) -> Result<(String, usize)> {
    let bytes = value.as_bytes();
    if bytes.get(start) != Some(&b'"') {
        bail!("expected JSON string");
    }
    let mut cursor = start + 1;
    let mut chunk_start = cursor;
    let mut output = String::new();
    while cursor < bytes.len() {
        match bytes[cursor] {
            b'"' => {
                output.push_str(&value[chunk_start..cursor]);
                return Ok((output, cursor + 1));
            }
            b'\\' => {
                output.push_str(&value[chunk_start..cursor]);
                cursor += 1;
                let escape = *bytes.get(cursor).context("unterminated JSON escape")?;
                match escape {
                    b'"' => output.push('"'),
                    b'\\' => output.push('\\'),
                    b'/' => output.push('/'),
                    b'b' => output.push('\u{08}'),
                    b'f' => output.push('\u{0c}'),
                    b'n' => output.push('\n'),
                    b'r' => output.push('\r'),
                    b't' => output.push('\t'),
                    b'u' => {
                        let end = cursor + 5;
                        let hex = value.get(cursor + 1..end).context("short Unicode escape")?;
                        let code =
                            u32::from_str_radix(hex, 16).context("invalid Unicode escape")?;
                        output.push(char::from_u32(code).context("invalid Unicode code point")?);
                        cursor = end - 1;
                    }
                    _ => bail!("invalid JSON escape"),
                }
                cursor += 1;
                chunk_start = cursor;
            }
            value if value < 0x20 => bail!("unescaped control character in JSON string"),
            _ => cursor += 1,
        }
    }
    bail!("unterminated JSON string")
}

fn parse_u64(value: &str) -> Result<u64> {
    value.trim().parse().context("invalid JSON integer")
}

fn parse_bool(value: &str) -> Result<bool> {
    match value.trim() {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => bail!("invalid JSON boolean"),
    }
}

fn skip_whitespace(bytes: &[u8], mut cursor: usize) -> usize {
    while bytes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
        cursor += 1;
    }
    cursor
}

fn skip_delimiters(bytes: &[u8], mut cursor: usize) -> usize {
    while bytes
        .get(cursor)
        .is_some_and(|value| value.is_ascii_whitespace() || *value == b',')
    {
        cursor += 1;
    }
    cursor
}

#[cfg(test)]
mod model_tests {
    use super::*;

    #[test]
    fn history_json_round_trip_preserves_formatted_text() {
        let history = History {
            version: 1,
            next_id: 2,
            items: vec![ClipboardItem {
                id: 1,
                kind: ItemKind::Text,
                text: Some("line one\n\t\"quoted\" and emoji 🦀".into()),
                image_file: None,
                mime: "text/plain;charset=utf-8".into(),
                pinned: true,
                created_at: 123,
                digest: "abc".into(),
            }],
        };
        let parsed = History::from_json(&history.to_json()).unwrap();
        assert_eq!(parsed.items[0].text, history.items[0].text);
        assert_eq!(parsed.items[0].pinned, history.items[0].pinned);
        assert_eq!(parsed.next_id, 2);
    }
}

const LOCK_SHARED: i32 = 1;
const LOCK_EXCLUSIVE: i32 = 2;
const LOCK_UN: i32 = 8;

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

    pub fn history_path(&self) -> &Path {
        &self.history_path
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
                    mime,
                    pinned: false,
                    created_at: now,
                    digest,
                },
            );
            Ok(Some(id))
        })
    }

    pub fn add_image(&self, bytes: &[u8], mime: String) -> Result<Option<u64>> {
        if bytes.is_empty() {
            return Ok(None);
        }

        let mut created_file = None;
        let result = self.update(|history| {
            let digest = digest(bytes);
            let now = unix_timestamp();
            if let Some(position) = history
                .items
                .iter()
                .position(|item| item.kind == ItemKind::Image && item.digest == digest)
            {
                let mut item = history.items.remove(position);
                item.created_at = now;
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
                    kind: ItemKind::Image,
                    text: None,
                    image_file: Some(filename),
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
            let Some(item) = history.items.iter_mut().find(|item| item.id == id) else {
                return Ok(false);
            };
            let changed = !item.pinned;
            item.pinned = true;
            Ok(changed)
        })
    }

    pub fn delete(&self, id: u64) -> Result<bool> {
        let mut image_to_delete = None;
        let deleted = self.update(|history| {
            let Some(position) = history.items.iter().position(|item| item.id == id) else {
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
        if text.is_empty() {
            bail!("clipboard text cannot be empty");
        }

        self.update(|history| {
            let Some(position) = history.items.iter().position(|item| item.id == id) else {
                return Ok(false);
            };
            if history.items[position].kind != ItemKind::Text {
                bail!("images cannot be edited as text");
            }

            let new_digest = digest(text.as_bytes());
            let duplicate = history.items.iter().enumerate().find_map(|(index, item)| {
                (index != position && item.kind == ItemKind::Text && item.digest == new_digest)
                    .then_some(index)
            });

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
            Ok(true)
        })
    }

    pub fn item_bytes(&self, item: &ClipboardItem) -> Result<Vec<u8>> {
        match item.kind {
            ItemKind::Text => Ok(item.text.as_deref().unwrap_or_default().as_bytes().to_vec()),
            ItemKind::Image => {
                let filename = item
                    .image_file
                    .as_deref()
                    .and_then(safe_filename)
                    .context("invalid image filename in clipboard history")?;
                let path = self.image_dir.join(filename);
                fs::read(&path).with_context(|| format!("read image {}", path.display()))
            }
        }
    }

    pub fn image_path(&self, item: &ClipboardItem) -> Option<PathBuf> {
        item.image_file
            .as_deref()
            .and_then(safe_filename)
            .map(|filename| self.image_dir.join(filename))
    }

    fn update<T>(&self, operation: impl FnOnce(&mut History) -> Result<T>) -> Result<T> {
        let lock = self.lock(true)?;
        let mut history = self.load_unlocked()?;
        let result = operation(&mut history)?;
        self.save_unlocked(&history)?;
        unlock(&lock)?;
        Ok(result)
    }

    fn lock(&self, exclusive: bool) -> Result<File> {
        fs::create_dir_all(&self.root)
            .with_context(|| format!("create data directory {}", self.root.display()))?;
        let file = OpenOptions::new()
            .create(true)
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
        write_atomic(&self.history_path, history.to_json().as_bytes())
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
