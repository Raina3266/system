use std::env;
use std::ffi::{OsStr, OsString};
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};

use crate::model::{Entry, Mode, path_from_key, path_key};
use crate::{AppResult, preview, search};

const RECORD_SEPARATOR: u8 = 0x1e;
const UNIT_SEPARATOR: u8 = 0x1f;
const RETV_ACTIVATE: u8 = 1;
const RETV_PREVIEW: u8 = 10;
const RETV_REVEAL: u8 = 11;
const WAYLAND_KEYBOARD_MODE_ENV: &str = "ROFI_WAYLAND_KEYBOARD_MODE";
const PRESERVE_SELECTION_ENV: &str = "ROFI_PRESERVE_SELECTION_ON_FILTER";
const REFRESH_ON_MODE_SWITCH_ENV: &str = "ROFI_REFRESH_SCRIPT_ON_MODE_SWITCH";

#[derive(Clone, Debug)]
struct UiState {
    initialized: bool,
    home: PathBuf,
    folder: PathBuf,
}

impl UiState {
    fn from_environment() -> AppResult<Self> {
        let home = search::home_directory()?;
        let data = env::var("ROFI_DATA").unwrap_or_default();
        let initialized = data == "initialized" || data.split(';').any(|part| part == "init=1");
        let folder = data
            .split(';')
            .find_map(|part| part.strip_prefix("folder="))
            .and_then(|key| path_from_key(key, Mode::Folder))
            .filter(|path| path.starts_with(&home) && path.is_dir())
            .unwrap_or_else(|| home.clone());
        Ok(Self {
            initialized,
            home,
            folder,
        })
    }

    fn at_folder(&self, folder: PathBuf) -> Self {
        Self {
            initialized: true,
            home: self.home.clone(),
            folder,
        }
    }

    fn encode(&self, mode: Mode) -> String {
        if mode == Mode::Folder {
            format!("init=1;folder={}", path_key(Mode::Folder, &self.folder))
        } else {
            "init=1".to_owned()
        }
    }
}

pub fn launch() -> AppResult<()> {
    let executable = env::current_exe()?;
    let executable_text = executable.to_string_lossy();
    let modes = format!(
        "app:{executable_text} script app,file:{executable_text} script file,\
         folder:{executable_text} script folder"
    );
    let selection_command = format!(
        "{} preview-selection {{completion}} {{selection-serial}}",
        shell_quote(&executable_text)
    );
    let socket = preview::session_socket_path()?;
    preview::cleanup(&socket)?;
    let status = Command::new(rofi_binary())
        .env(WAYLAND_KEYBOARD_MODE_ENV, "on-demand")
        .env(PRESERVE_SELECTION_ENV, "true")
        .env(REFRESH_ON_MODE_SWITCH_ENV, "true")
        .env(preview::SOCKET_ENV, &socket)
        .args([
            "-show",
            "app",
            "-show-icons",
            "-modes",
            &modes,
            "-display-app",
            "󰀻 App",
            "-display-file",
            "󰈞 File",
            "-display-folder",
            " Folder",
            "-kb-custom-1",
            "Alt+p",
            "-kb-custom-2",
            "Alt+o",
            "-on-selection-changed",
            &selection_command,
            "-theme",
        ])
        .arg(theme_path()?)
        .status();
    preview::close_at(&socket);
    preview::cleanup(&socket)?;
    let status = status?;
    if !status.success() && status.code() != Some(1) {
        return Err(io::Error::other(format!("rofi exited with {status}")).into());
    }
    Ok(())
}

pub fn run_script(mode: Mode) -> AppResult<()> {
    let retv = env::var("ROFI_RETV")
        .ok()
        .and_then(|value| value.parse::<u8>().ok())
        .unwrap_or(0);
    let selected_key = env::var("ROFI_INFO").ok();
    let state = UiState::from_environment()?;
    match retv {
        RETV_ACTIVATE => {
            if let Some(key) = selected_key.as_deref() {
                if mode == Mode::Folder {
                    return activate_folder(key, &state);
                }
                activate(mode, key)?;
            }
            Ok(())
        }
        RETV_PREVIEW if mode == Mode::File => {
            if let Some(key) = selected_key.as_deref() {
                preview::toggle(key)?;
            }
            render(mode, selected_key.as_deref(), &state)
        }
        RETV_REVEAL if mode == Mode::File => {
            if let Some(path) = selected_key
                .as_deref()
                .and_then(|key| path_from_key(key, Mode::File))
            {
                spawn_background(dolphin_binary(), [OsStr::new("--select"), path.as_os_str()])?;
            }
            Ok(())
        }
        _ => render(mode, selected_key.as_deref(), &state),
    }
}

fn activate_folder(key: &str, state: &UiState) -> AppResult<()> {
    let Some(path) = path_from_key(key, Mode::Folder) else {
        return render(Mode::Folder, None, state);
    };
    if path.is_dir() && path.starts_with(&state.home) {
        let selected_key = (state.folder.parent() == Some(path.as_path()))
            .then(|| path_key(Mode::Folder, &state.folder));
        return render(
            Mode::Folder,
            selected_key.as_deref(),
            &state.at_folder(path),
        );
    }
    spawn_background(xdg_open_binary(), [path.as_os_str()])
}

fn activate(mode: Mode, key: &str) -> AppResult<()> {
    let Some(path) = path_from_key(key, mode) else {
        return Ok(());
    };
    match mode {
        Mode::App => spawn_background(gio_binary(), [OsStr::new("launch"), path.as_os_str()]),
        Mode::File => spawn_background(xdg_open_binary(), [path.as_os_str()]),
        Mode::Folder => unreachable!("folder activation is handled by the browser"),
    }
}

fn spawn_background<I, S>(program: OsString, arguments: I) -> AppResult<()>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    Command::new(program)
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    Ok(())
}

fn render(mode: Mode, selected_key: Option<&str>, state: &UiState) -> AppResult<()> {
    let entries = if mode == Mode::Folder {
        search::folder_entries(&state.home, &state.folder)?
    } else {
        search::entries(mode)?
    };
    let selected_row = selected_key
        .and_then(|key| entries.iter().position(|entry| entry.key == key))
        .or_else(|| (!entries.is_empty()).then_some(0));
    let mut output = Vec::new();
    if !state.initialized {
        output.push(0);
        output.extend_from_slice(b"delim");
        output.push(UNIT_SEPARATOR);
        output.push(RECORD_SEPARATOR);
        output.push(b'\n');
    }
    write_header(&mut output, "prompt", &prompt(mode, state));
    write_header(&mut output, "markup-rows", "true");
    write_header(&mut output, "no-custom", "true");
    write_header(&mut output, "use-hot-keys", "true");
    write_header(&mut output, "keep-selection", "true");
    write_header(&mut output, "data", &state.encode(mode));
    write_header(&mut output, "theme", &mode_theme(mode));
    if let Some(selected_row) = selected_row {
        write_header(&mut output, "new-selection", &selected_row.to_string());
    }
    if entries.is_empty() {
        write_empty_row(&mut output, mode)?;
    } else {
        for entry in &entries {
            write_row(&mut output, entry)?;
        }
    }
    io::stdout().write_all(&output)?;
    Ok(())
}

fn prompt(mode: Mode, state: &UiState) -> String {
    if mode == Mode::Folder {
        format!(" {}", search::abbreviate_home(&state.folder, &state.home))
    } else {
        mode.prompt().to_owned()
    }
}

fn mode_theme(mode: Mode) -> String {
    let action_colour = if mode == Mode::File { "@cyan" } else { "@dim" };
    let icon_size = if mode == Mode::File { "3em" } else { "2em" };
    format!(
        "configuration {{ eh: {}; }} \
         element-icon {{ size: {icon_size}; }} \
         button-preview, button-reveal {{ text-color: {action_colour}; \
         border-color: {action_colour}; }}",
        mode.row_height()
    )
}

fn write_row(output: &mut Vec<u8>, entry: &Entry) -> io::Result<()> {
    write!(output, "{}", entry.key)?;
    let mut first = true;
    write_row_option(output, &mut first, "display", &entry.display);
    write_row_option(output, &mut first, "info", &entry.key);
    write_row_option(output, &mut first, "meta", &entry.meta);
    write_row_option(output, &mut first, "icon", &entry.icon);
    output.push(RECORD_SEPARATOR);
    Ok(())
}

fn write_empty_row(output: &mut Vec<u8>, mode: Mode) -> io::Result<()> {
    write!(output, "empty")?;
    let mut first = true;
    write_row_option(
        output,
        &mut first,
        "display",
        &format!("No {}s found", mode.name()),
    );
    write_row_option(output, &mut first, "nonselectable", "true");
    write_row_option(output, &mut first, "permanent", "true");
    output.push(RECORD_SEPARATOR);
    Ok(())
}

fn write_header(output: &mut Vec<u8>, key: &str, value: &str) {
    output.push(0);
    output.extend_from_slice(key.as_bytes());
    output.push(UNIT_SEPARATOR);
    output.extend_from_slice(sanitize_record_value(value).as_bytes());
    output.push(RECORD_SEPARATOR);
}

fn write_row_option(output: &mut Vec<u8>, first: &mut bool, key: &str, value: &str) {
    output.push(if *first { 0 } else { UNIT_SEPARATOR });
    *first = false;
    output.extend_from_slice(key.as_bytes());
    output.push(UNIT_SEPARATOR);
    output.extend_from_slice(sanitize_option_value(value).as_bytes());
}

fn sanitize_record_value(value: &str) -> String {
    value
        .replace('\0', "␀")
        .replace(char::from(RECORD_SEPARATOR), "\n")
}

fn sanitize_option_value(value: &str) -> String {
    sanitize_record_value(value).replace(char::from(UNIT_SEPARATOR), " ")
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn theme_path() -> AppResult<PathBuf> {
    if let Some(path) = env::var_os("ROFI_FILESEARCH_THEME") {
        return Ok(PathBuf::from(path));
    }
    if let Some(config) = env::var_os("XDG_CONFIG_HOME") {
        return Ok(PathBuf::from(config).join("rofi/rofi-finder.rasi"));
    }
    let home = env::var_os("HOME").ok_or_else(|| io::Error::other("HOME is not set"))?;
    Ok(PathBuf::from(home).join(".config/rofi/rofi-finder.rasi"))
}

fn binary(environment: &str, fallback: &str) -> OsString {
    env::var_os(environment).unwrap_or_else(|| OsString::from(fallback))
}

fn rofi_binary() -> OsString {
    binary("ROFI_FILESEARCH_ROFI", "rofi")
}

fn gio_binary() -> OsString {
    binary("ROFI_FILESEARCH_GIO", "gio")
}

fn xdg_open_binary() -> OsString {
    binary("ROFI_FILESEARCH_XDG_OPEN", "xdg-open")
}

fn dolphin_binary() -> OsString {
    binary("ROFI_FILESEARCH_DOLPHIN", "dolphin")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_file_rows_request_two_lines() {
        assert!(mode_theme(Mode::App).contains("eh: 1;"));
        assert!(mode_theme(Mode::Folder).contains("eh: 1;"));
        assert!(mode_theme(Mode::File).contains("eh: 2;"));
    }

    #[test]
    fn paths_with_quotes_are_safe_in_the_selection_callback() {
        assert_eq!(shell_quote("/tmp/Raina's app"), "'/tmp/Raina'\\''s app'");
    }

    #[test]
    fn row_options_share_one_nul_metadata_marker() {
        let entry = Entry {
            key: "file:4141".to_owned(),
            display: "Visible".to_owned(),
            meta: "Searchable".to_owned(),
            icon: "text-x-generic".to_owned(),
        };
        let mut output = Vec::new();
        write_row(&mut output, &entry).unwrap();
        assert_eq!(output.iter().filter(|byte| **byte == 0).count(), 1);
    }
}
