use std::ffi::{OsStr, OsString};
use std::fmt;
use std::io;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::str::FromStr;

use crate::{AppError, AppResult};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Mode {
    App,
    File,
    Folder,
}

impl Mode {
    pub fn name(self) -> &'static str {
        match self {
            Self::App => "app",
            Self::File => "file",
            Self::Folder => "folder",
        }
    }

    pub fn prompt(self) -> &'static str {
        match self {
            Self::App => "󰀻 App",
            Self::File => "󰈞 File",
            Self::Folder => " Folder",
        }
    }

    pub fn row_height(self) -> u8 {
        match self {
            Self::App | Self::Folder => 1,
            Self::File => 2,
        }
    }
}

impl FromStr for Mode {
    type Err = AppError;

    fn from_str(value: &str) -> AppResult<Self> {
        match value.to_ascii_lowercase().as_str() {
            "app" | "apps" | "application" | "applications" => Ok(Self::App),
            "file" | "files" => Ok(Self::File),
            "folder" | "folders" | "directory" | "directories" => Ok(Self::Folder),
            _ => Err(io::Error::other(format!("unknown mode {value:?}")).into()),
        }
    }
}

impl fmt::Display for Mode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.name())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Entry {
    pub key: String,
    pub display: String,
    pub meta: String,
    pub icon: String,
}

impl Entry {
    pub fn for_path(mode: Mode, path: &Path, display: String, meta: String, icon: String) -> Self {
        Self {
            key: path_key(mode, path),
            display,
            meta,
            icon,
        }
    }
}

pub fn path_key(mode: Mode, path: &Path) -> String {
    format!(
        "{}:{}",
        mode.name(),
        hex_encode(path.as_os_str().as_bytes())
    )
}

pub fn path_from_key(key: &str, expected: Mode) -> Option<PathBuf> {
    let encoded = key.strip_prefix(expected.name())?.strip_prefix(':')?;
    hex_decode(encoded).map(|bytes| PathBuf::from(OsString::from_vec(bytes)))
}

pub fn mode_from_key(key: &str) -> Option<Mode> {
    let (mode, _) = key.split_once(':')?;
    mode.parse().ok()
}

pub fn hex_encode(value: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(value.len() * 2);
    for byte in value {
        let byte = *byte;
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}

fn hex_decode(value: &str) -> Option<Vec<u8>> {
    if value.len() % 2 != 0 {
        return None;
    }
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| Some((hex_digit(pair[0])? << 4) | hex_digit(pair[1])?))
        .collect()
}

fn hex_digit(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

pub fn escape_markup(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

pub fn single_line(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn lossy(value: &OsStr) -> String {
    value.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arbitrary_unix_paths_round_trip_through_rofi_keys() {
        let path = PathBuf::from(OsString::from_vec(b"/tmp/Raina's \xff.pdf".to_vec()));
        let key = path_key(Mode::File, &path);
        assert_eq!(path_from_key(&key, Mode::File).as_ref(), Some(&path));
        assert_eq!(path_from_key(&key, Mode::Folder), None);
    }

    #[test]
    fn markup_from_file_and_application_names_is_escaped() {
        assert_eq!(
            escape_markup("A&B <Preview> \"Raina's\""),
            "A&amp;B &lt;Preview&gt; &quot;Raina&apos;s&quot;"
        );
    }

    #[test]
    fn hostile_row_text_is_flattened_to_one_visual_line() {
        assert_eq!(single_line("one\n two\tthree"), "one two three");
    }

    #[test]
    fn modes_parse_their_singular_and_plural_names() {
        assert_eq!("applications".parse::<Mode>().unwrap(), Mode::App);
        assert_eq!("files".parse::<Mode>().unwrap(), Mode::File);
        assert_eq!("folders".parse::<Mode>().unwrap(), Mode::Folder);
    }
}
