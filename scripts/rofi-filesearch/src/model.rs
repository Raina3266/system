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
    pub title: String,
    pub subtitle: Option<String>,
    pub meta: String,
    pub icon: String,
}

impl Entry {
    pub fn for_path(
        mode: Mode,
        path: &Path,
        title: String,
        subtitle: Option<String>,
        meta: String,
        icon: String,
    ) -> Self {
        Self {
            key: path_key(mode, path),
            title,
            subtitle,
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
    if !value.len().is_multiple_of(2) {
        return None;
    }
    value
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
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

pub fn single_line(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn lossy(value: &OsStr) -> String {
    value.to_string_lossy().into_owned()
}
