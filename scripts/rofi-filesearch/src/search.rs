use std::env;
use std::ffi::{OsStr, OsString};
use std::io;
use std::os::unix::ffi::OsStringExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::AppResult;
use crate::desktop;
use crate::model::{Entry, Mode, escape_markup, lossy, single_line};

pub fn entries(mode: Mode) -> AppResult<Vec<Entry>> {
    match mode {
        Mode::App => desktop::entries(),
        Mode::File => path_entries(mode, "f"),
        Mode::Folder => path_entries(mode, "d"),
    }
}

fn path_entries(mode: Mode, file_type: &str) -> AppResult<Vec<Entry>> {
    let home = env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::other("HOME is not set"))?;
    let output = Command::new(fd_binary())
        .args([
            OsStr::new("--one-file-system"),
            OsStr::new("--type"),
            OsStr::new(file_type),
            OsStr::new("--base-directory"),
            home.as_os_str(),
            OsStr::new("--print0"),
            OsStr::new("."),
        ])
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!("fd exited with {}", output.status)).into());
    }
    output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|bytes| !bytes.is_empty())
        .map(|bytes| {
            path_entry(
                mode,
                &home,
                PathBuf::from(OsString::from_vec(bytes.to_vec())),
            )
        })
        .collect()
}

fn path_entry(mode: Mode, home: &Path, relative: PathBuf) -> AppResult<Entry> {
    let path = home.join(&relative);
    let name = path
        .file_name()
        .map(lossy)
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| lossy(path.as_os_str()));
    let abbreviated = abbreviate_home(path.parent().unwrap_or(home), home);
    let visible_name = single_line(&name);
    let visible_path = single_line(&abbreviated);
    let (display, icon) = match mode {
        Mode::File => (
            format!(
                "{}\u{2029}<span size=\"80%\" alpha=\"50%\">{}/</span>",
                escape_markup(&visible_name),
                escape_markup(&visible_path)
            ),
            format!("thumbnail://{},text-x-generic", path.display()),
        ),
        Mode::Folder => (
            escape_markup(&visible_name),
            "folder,inode-directory".to_owned(),
        ),
        Mode::App => unreachable!("application entries do not come from fd"),
    };
    Ok(Entry::for_path(
        mode,
        &path,
        display,
        format!("{abbreviated}/{name}"),
        icon,
    ))
}

fn abbreviate_home(path: &Path, home: &Path) -> String {
    path.strip_prefix(home)
        .ok()
        .filter(|relative| !relative.as_os_str().is_empty())
        .map(|relative| format!("~/{}", relative.display()))
        .unwrap_or_else(|| "~".to_owned())
}

fn fd_binary() -> OsString {
    env::var_os("ROFI_FILESEARCH_FD").unwrap_or_else(|| OsString::from("fd"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn home_paths_are_abbreviated_for_the_second_file_line() {
        let home = Path::new("/home/raina");
        assert_eq!(abbreviate_home(Path::new("/home/raina"), home), "~");
        assert_eq!(
            abbreviate_home(Path::new("/home/raina/Documents/PDF"), home),
            "~/Documents/PDF"
        );
    }

    #[test]
    fn only_file_rows_contain_a_second_display_line() {
        let home = Path::new("/home/raina");
        let relative = PathBuf::from("Documents/notes.txt");
        let file = path_entry(Mode::File, home, relative.clone()).unwrap();
        let folder = path_entry(Mode::Folder, home, relative).unwrap();
        assert!(file.display.contains('\u{2029}'));
        assert!(!folder.display.contains('\u{2029}'));
    }
}
