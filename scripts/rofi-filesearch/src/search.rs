use std::env;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::AppResult;
use crate::desktop;
use crate::model::{Entry, Mode, lossy, single_line};

pub fn entries(mode: Mode) -> AppResult<Vec<Entry>> {
    match mode {
        Mode::App => desktop::entries(),
        Mode::File => file_entries(),
        Mode::Folder => unreachable!("folder entries require the current directory"),
    }
}

fn file_entries() -> AppResult<Vec<Entry>> {
    let home = home_directory()?;
    let output = Command::new(fd_binary())
        .args([
            OsStr::new("--one-file-system"),
            OsStr::new("--type"),
            OsStr::new("f"),
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
        .map(|bytes| file_entry(&home, PathBuf::from(OsString::from_vec(bytes.to_vec()))))
        .collect()
}

pub(crate) fn file_entry(home: &Path, relative: PathBuf) -> AppResult<Entry> {
    let path = home.join(&relative);
    let name = path
        .file_name()
        .map(lossy)
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| lossy(path.as_os_str()));
    let abbreviated = abbreviate_home(path.parent().unwrap_or(home), home);
    let visible_name = single_line(&name);
    let visible_path = single_line(&abbreviated);
    Ok(Entry::for_path(
        Mode::File,
        &path,
        visible_name,
        Some(format!("{visible_path}/")),
        format!("{abbreviated}/{name}"),
        "text-x-generic".to_owned(),
    ))
}

pub fn folder_entries(home: &Path, current: &Path) -> AppResult<Vec<Entry>> {
    let mut entries = Vec::new();
    if current != home {
        let parent = current
            .parent()
            .filter(|path| path.starts_with(home))
            .unwrap_or(home);
        entries.push(Entry::for_path(
            Mode::Folder,
            parent,
            "󰁞  ..".to_owned(),
            None,
            abbreviate_home(parent, home),
            "folder,inode-directory".to_owned(),
        ));
    }

    let mut children = Vec::new();
    for item in fs::read_dir(current)? {
        let item = item?;
        let name = item.file_name();
        if name.as_bytes().starts_with(b".") {
            continue;
        }
        let path = item.path();
        let is_directory = path.is_dir();
        if current == home && !is_directory {
            continue;
        }
        children.push((
            !is_directory,
            single_line(&lossy(&name)).to_lowercase(),
            path,
            is_directory,
        ));
    }
    children.sort_by(|left, right| (left.0, &left.1).cmp(&(right.0, &right.1)));

    for (_, _, path, is_directory) in children {
        let name = path
            .file_name()
            .map(lossy)
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| lossy(path.as_os_str()));
        let icon = if is_directory {
            "folder,inode-directory".to_owned()
        } else {
            format!("thumbnail://{},text-x-generic", path.display())
        };
        entries.push(Entry::for_path(
            Mode::Folder,
            &path,
            single_line(&name),
            None,
            abbreviate_home(&path, home),
            icon,
        ));
    }
    Ok(entries)
}

pub fn home_directory() -> AppResult<PathBuf> {
    env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::other("HOME is not set").into())
}

pub fn abbreviate_home(path: &Path, home: &Path) -> String {
    path.strip_prefix(home)
        .ok()
        .filter(|relative| !relative.as_os_str().is_empty())
        .map(|relative| format!("~/{}", relative.display()))
        .unwrap_or_else(|| "~".to_owned())
}

fn fd_binary() -> OsString {
    env::var_os("ROFI_FILESEARCH_FD").unwrap_or_else(|| OsString::from("fd"))
}
