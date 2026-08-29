use std::env;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::AppResult;
use crate::desktop;
use crate::model::{Entry, Mode, escape_markup, lossy, single_line};

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

fn file_entry(home: &Path, relative: PathBuf) -> AppResult<Entry> {
    let path = home.join(&relative);
    let name = path
        .file_name()
        .map(lossy)
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| lossy(path.as_os_str()));
    let abbreviated = abbreviate_home(path.parent().unwrap_or(home), home);
    let visible_name = single_line(&name);
    let visible_path = single_line(&abbreviated);
    let display = format!(
        "{}\u{2029}<span size=\"80%\" alpha=\"50%\">{}/</span>",
        escape_markup(&visible_name),
        escape_markup(&visible_path)
    );
    Ok(Entry::for_path(
        Mode::File,
        &path,
        display,
        format!("{abbreviated}/{name}"),
        format!("thumbnail://{},text-x-generic", path.display()),
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
            escape_markup(&single_line(&name)),
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
        let file = file_entry(home, relative).unwrap();
        assert!(file.display.contains('\u{2029}'));
    }

    #[test]
    fn folder_root_lists_only_visible_home_directories() {
        let home = test_home("root");
        fs::create_dir_all(home.join("Documents")).unwrap();
        fs::create_dir_all(home.join(".hidden")).unwrap();
        fs::write(home.join("notes.txt"), "notes").unwrap();

        let entries = folder_entries(&home, &home).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].display, "Documents");
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn nested_folder_lists_parent_folders_then_files() {
        let home = test_home("nested");
        let current = home.join("Documents");
        fs::create_dir_all(current.join("Projects")).unwrap();
        fs::write(current.join("notes.txt"), "notes").unwrap();

        let entries = folder_entries(&home, &current).unwrap();
        let displays = entries
            .iter()
            .map(|entry| entry.display.as_str())
            .collect::<Vec<_>>();
        assert_eq!(displays, ["󰁞  ..", "Projects", "notes.txt"]);
        assert!(
            entries
                .iter()
                .all(|entry| !entry.display.contains('\u{2029}'))
        );
        fs::remove_dir_all(home).unwrap();
    }

    fn test_home(name: &str) -> PathBuf {
        let path = env::temp_dir().join(format!("rofi-filesearch-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }
}
