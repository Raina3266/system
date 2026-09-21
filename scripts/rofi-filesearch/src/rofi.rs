use std::env;
use std::ffi::{OsStr, OsString};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::os::unix::ffi::OsStrExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use rofi_preview_shared::file_preview::FilePreviewer;
use rofi_preview_shared::launcher::{
    Action, Controller, Icon, Mode as SharedMode, Outcome, Row, UiResult, View,
};
use rofi_preview_shared::panel_client::PanelClient;

use crate::model::{Entry, Mode, path_from_key, path_key};
use crate::{AppResult, search};

const ACTION_PREVIEW: &str = "preview";
const ACTION_REVEAL: &str = "reveal";

pub fn launch() -> AppResult<()> {
    let controller = FileSearch::new()?;
    rofi_preview_shared::launcher::run(controller);
    Ok(())
}

struct FileSearch {
    mode: Mode,
    home: PathBuf,
    folder: PathBuf,
    panel: PanelClient,
    previewer: FilePreviewer,
}

impl FileSearch {
    fn new() -> AppResult<Self> {
        let home = search::home_directory()?;
        let panel = PanelClient::new(
            "rofi-filesearch",
            binary("ROFI_FILESEARCH_ROFI_PREVIEW_SHARED", "rofi-preview-shared"),
            "ROFI_FILESEARCH",
        )?;
        panel.cleanup()?;
        Ok(Self {
            mode: Mode::App,
            folder: home.clone(),
            home,
            panel,
            previewer: FilePreviewer::new(
                "rofi-filesearch",
                file_binary(),
                pdftoppm_binary(),
                ffmpegthumbnailer_binary(),
            ),
        })
    }

    fn entries(&self) -> AppResult<Vec<Entry>> {
        if self.mode == Mode::Folder {
            search::folder_entries(&self.home, &self.folder)
        } else {
            search::entries(self.mode)
        }
    }

    fn build_view(&self) -> AppResult<View> {
        let rows = self
            .entries()?
            .into_iter()
            .map(|entry| {
                let icon = if self.mode == Mode::App {
                    Some(Icon::Name(entry.icon.clone()))
                } else {
                    path_from_key(&entry.key, self.mode).map(Icon::File)
                };
                Row {
                    id: entry.key,
                    title: entry.title,
                    subtitle: entry.subtitle,
                    search_text: entry.meta,
                    icon,
                    active: false,
                    permanent: false,
                }
            })
            .collect();
        let mut preview = Action::new(ACTION_PREVIEW, "󰈈 Preview", Some('p'));
        preview.enabled = matches!(self.mode, Mode::File | Mode::Folder);
        let mut reveal = Action::new(ACTION_REVEAL, " Reveal", Some('o'));
        reveal.enabled = self.mode == Mode::File;
        Ok(View {
            prompt: if self.mode == Mode::Folder {
                format!(" {}", search::abbreviate_home(&self.folder, &self.home))
            } else {
                self.mode.prompt().to_owned()
            },
            rows,
            actions: vec![preview, reveal],
            selected: None,
            empty_message: Some(format!("No {}s found", self.mode.name())),
        })
    }

    fn activate_folder(&mut self, key: &str) -> AppResult<Outcome> {
        let Some(path) = path_from_key(key, Mode::Folder) else {
            return Ok(Outcome::None);
        };
        if path.is_dir() && path.starts_with(&self.home) {
            let selected = (self.folder.parent() == Some(path.as_path()))
                .then(|| path_key(Mode::Folder, &self.folder));
            self.folder = path;
            return Ok(Outcome::Refresh { selected });
        }
        spawn_background(xdg_open_binary(), [path.as_os_str()])?;
        Ok(Outcome::Close)
    }

    fn toggle_preview(&self, key: &str) -> AppResult<()> {
        if self.panel.is_open() {
            self.panel.close()?;
            return Ok(());
        }
        let Some(file) = preview_file_from_key(key) else {
            return Ok(());
        };
        let content = self.previewer.preview(&file)?;
        self.panel
            .open(preview_id(&file), "File preview", &content)?;
        Ok(())
    }

    fn update_preview(&self, key: &str, serial: u64) -> AppResult<()> {
        let Some(file) = preview_file_from_key(key) else {
            if self.panel.is_open() {
                self.panel.close()?;
            }
            return Ok(());
        };
        if !self.panel.is_open() {
            return Ok(());
        }
        let content = self.previewer.preview(&file)?;
        if self.panel.update(preview_id(&file), serial, &content)? {
            Ok(())
        } else {
            Err(
                std::io::Error::other("rofi-preview-shared closed before the update arrived")
                    .into(),
            )
        }
    }
}

impl Controller for FileSearch {
    fn application_id(&self) -> &'static str {
        "io.github.raina.RofiFileSearch"
    }

    fn namespace(&self) -> &'static str {
        "rofi-filesearch"
    }

    fn modes(&self) -> Vec<SharedMode> {
        [Mode::App, Mode::File, Mode::Folder]
            .into_iter()
            .map(|mode| SharedMode::new(mode.name(), mode.prompt()))
            .collect()
    }

    fn active_mode(&self) -> &str {
        self.mode.name()
    }

    fn switch_mode(&mut self, mode: &str) -> UiResult<View> {
        self.mode = mode.parse().map_err(display_error)?;
        self.build_view().map_err(display_error)
    }

    fn view(&mut self) -> UiResult<View> {
        self.build_view().map_err(display_error)
    }

    fn activate(&mut self, row: &str) -> UiResult<Outcome> {
        if self.mode == Mode::Folder {
            return self.activate_folder(row).map_err(display_error);
        }
        let Some(path) = path_from_key(row, self.mode) else {
            return Ok(Outcome::None);
        };
        match self.mode {
            Mode::App => spawn_background(gio_binary(), [OsStr::new("launch"), path.as_os_str()]),
            Mode::File => spawn_background(xdg_open_binary(), [path.as_os_str()]),
            Mode::Folder => unreachable!(),
        }
        .map_err(display_error)?;
        Ok(Outcome::Close)
    }

    fn action(&mut self, action: &str, selected: Option<&str>) -> UiResult<Outcome> {
        let Some(selected) = selected else {
            return Ok(Outcome::None);
        };
        match action {
            ACTION_PREVIEW if matches!(self.mode, Mode::File | Mode::Folder) => {
                self.toggle_preview(selected).map_err(display_error)?;
            }
            ACTION_REVEAL if self.mode == Mode::File => {
                if let Some(path) = path_from_key(selected, Mode::File) {
                    spawn_background(dolphin_binary(), [OsStr::new("--select"), path.as_os_str()])
                        .map_err(display_error)?;
                }
            }
            _ => {}
        }
        Ok(Outcome::None)
    }

    fn selection_changed(&mut self, selected: &str, serial: u64) -> UiResult<()> {
        self.update_preview(selected, serial).map_err(display_error)
    }

    fn close(&mut self) -> UiResult<()> {
        self.panel.close().map_err(display_error)
    }
}

pub(crate) fn preview_file_from_key(key: &str) -> Option<PathBuf> {
    let mode = crate::model::mode_from_key(key)?;
    if !matches!(mode, Mode::File | Mode::Folder) {
        return None;
    }
    path_from_key(key, mode).filter(|path| path.is_file())
}

fn preview_id(path: &std::path::Path) -> u64 {
    let mut hasher = DefaultHasher::new();
    path.as_os_str().as_bytes().hash(&mut hasher);
    hasher.finish()
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

fn display_error(error: impl std::fmt::Display) -> String {
    error.to_string()
}

fn binary(environment: &str, fallback: &str) -> OsString {
    env::var_os(environment).unwrap_or_else(|| OsString::from(fallback))
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

fn file_binary() -> OsString {
    binary("ROFI_FILESEARCH_FILE", "file")
}

pub(crate) fn pdftoppm_binary() -> OsString {
    binary("ROFI_FILESEARCH_PDFTOPPM", "pdftoppm")
}

fn ffmpegthumbnailer_binary() -> OsString {
    binary("ROFI_FILESEARCH_FFMPEGTHUMBNAILER", "ffmpegthumbnailer")
}
