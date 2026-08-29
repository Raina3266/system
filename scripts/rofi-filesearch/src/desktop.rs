use std::collections::{HashMap, HashSet};
use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use crate::AppResult;
use crate::model::{Entry, Mode, escape_markup, lossy, single_line};

pub fn entries() -> AppResult<Vec<Entry>> {
    let mut entries = Vec::new();
    let mut seen = HashSet::new();
    for directory in application_directories() {
        let mut files = Vec::new();
        collect_desktop_files(&directory, &directory, &mut files)?;
        files.sort();
        for (id, path) in files {
            if !seen.insert(id) {
                continue;
            }
            if let Some(entry) = parse_entry(&path)? {
                entries.push(entry);
            }
        }
    }
    entries.sort_by_key(|entry| entry.display.to_lowercase());
    Ok(entries)
}

fn application_directories() -> Vec<PathBuf> {
    let data_home = env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")));
    let mut directories = data_home
        .map(|path| vec![path.join("applications")])
        .unwrap_or_default();
    let data_dirs = env::var_os("XDG_DATA_DIRS")
        .unwrap_or_else(|| OsString::from("/usr/local/share:/usr/share"));
    directories.extend(env::split_paths(&data_dirs).map(|path| path.join("applications")));
    directories
}

fn collect_desktop_files(
    root: &Path,
    directory: &Path,
    files: &mut Vec<(String, PathBuf)>,
) -> AppResult<()> {
    let listing = match fs::read_dir(directory) {
        Ok(listing) => listing,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    for item in listing {
        let item = item?;
        let file_type = item.file_type()?;
        let path = item.path();
        if file_type.is_dir() {
            collect_desktop_files(root, &path, files)?;
        } else if path
            .extension()
            .is_some_and(|extension| extension == "desktop")
        {
            let relative = path.strip_prefix(root).unwrap_or(&path);
            let id = relative
                .components()
                .map(|component| lossy(component.as_os_str()))
                .collect::<Vec<_>>()
                .join("-");
            files.push((id, path));
        }
    }
    Ok(())
}

fn parse_entry(path: &Path) -> AppResult<Option<Entry>> {
    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::InvalidData => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let fields = desktop_fields(&content);
    if fields.get("Type").map(String::as_str) != Some("Application")
        || boolean_field(&fields, "Hidden")
        || boolean_field(&fields, "NoDisplay")
    {
        return Ok(None);
    }
    let Some(name) = localized_field(&fields, "Name") else {
        return Ok(None);
    };
    let generic = localized_field(&fields, "GenericName").unwrap_or_default();
    let keywords = localized_field(&fields, "Keywords").unwrap_or_default();
    let icon = fields
        .get("Icon")
        .map(|value| desktop_unescape(value))
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "application-x-executable".to_owned());
    let meta = format!("{name} {generic} {keywords} {}", path.display());
    Ok(Some(Entry::for_path(
        Mode::App,
        path,
        escape_markup(&single_line(&name)),
        meta,
        icon,
    )))
}

fn desktop_fields(content: &str) -> HashMap<String, String> {
    let mut fields = HashMap::new();
    let mut in_desktop_entry = false;
    for raw_line in content.lines() {
        let line = raw_line.trim();
        if line.starts_with('[') && line.ends_with(']') {
            in_desktop_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_desktop_entry || line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            fields
                .entry(key.trim().to_owned())
                .or_insert_with(|| value.to_owned());
        }
    }
    fields
}

fn boolean_field(fields: &HashMap<String, String>, key: &str) -> bool {
    fields
        .get(key)
        .is_some_and(|value| value.eq_ignore_ascii_case("true"))
}

fn localized_field(fields: &HashMap<String, String>, key: &str) -> Option<String> {
    for locale in locale_candidates() {
        if let Some(value) = fields.get(&format!("{key}[{locale}]")) {
            return Some(desktop_unescape(value));
        }
    }
    fields.get(key).map(|value| desktop_unescape(value))
}

fn locale_candidates() -> Vec<String> {
    let locale = env::var("LC_MESSAGES")
        .ok()
        .filter(|value| !value.is_empty())
        .or_else(|| env::var("LANG").ok())
        .unwrap_or_default();
    let locale = locale
        .split(['.', '@'])
        .next()
        .unwrap_or_default()
        .to_owned();
    let mut candidates = Vec::new();
    if !locale.is_empty() && locale != "C" && locale != "POSIX" {
        candidates.push(locale.clone());
        if let Some((language, _)) = locale.split_once('_') {
            candidates.push(language.to_owned());
        }
    }
    candidates
}

fn desktop_unescape(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut characters = value.chars();
    while let Some(character) = characters.next() {
        if character != '\\' {
            output.push(character);
            continue;
        }
        match characters.next() {
            Some('n') => output.push('\n'),
            Some('r') => output.push('\r'),
            Some('t') => output.push('\t'),
            Some('s') => output.push(' '),
            Some('\\') => output.push('\\'),
            Some(';') => output.push(';'),
            Some(other) => output.push(other),
            None => output.push('\\'),
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_parser_only_reads_the_desktop_entry_group() {
        let fields = desktop_fields(
            "[Desktop Entry]\nType=Application\nName=Raina & App\n\
             [Desktop Action New]\nName=Wrong name\n",
        );
        assert_eq!(fields.get("Name").map(String::as_str), Some("Raina & App"));
    }

    #[test]
    fn desktop_escapes_are_decoded() {
        assert_eq!(
            desktop_unescape(r"Line\sOne\nLine\sTwo"),
            "Line One\nLine Two"
        );
    }
}
