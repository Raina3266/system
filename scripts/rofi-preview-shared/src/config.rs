use std::env;
use std::error::Error;
use std::ffi::OsStr;
use std::fmt;
use std::path::PathBuf;

use crate::cli::{Side, WindowOverrides};

const SETTINGS_START: &str = "/* rofi-preview-shared-settings";
const LAYOUT_START: &str = "/* rofi-preview-shared-layout";
const LEGACY_SETTINGS_START: &str = "/* preview-panel-settings";
const LEGACY_LAYOUT_START: &str = "/* preview-panel-layout";

const EMBEDDED_THEME: &str = r#"/* rofi-preview-shared-settings
width: 400px;
height: 616px;
companion_width: 400px;
side: left;
gap: 5px;
x: 770px;
y: -850px;
*/

window.rofi-preview-shared {
    background: rgba(24, 10, 16, 0.95);
    border: 1px solid rgba(214, 86, 199, 0.55);
    border-radius: 15px;
    padding: 10px;
}

textview.preview-text,
textview.preview-text text {
    background: transparent;
    color: #5DF4FE;
    caret-color: #D656C7;
    font-family: "JetBrains Mono Nerd Font", monospace;
    font-size: 12pt;
}

picture.preview-image {
    background: transparent;
}

scrollbar slider {
    min-width: 4px;
    min-height: 4px;
    background: #D52C35;
    border-radius: 4px;
}
"#;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Config {
    pub window: WindowConfig,
    pub css: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WindowConfig {
    pub width: i32,
    pub height: i32,
    pub companion_width: i32,
    pub side: Side,
    pub gap: i32,
    pub x: i32,
    pub y: i32,
}

impl WindowConfig {
    pub fn with_overrides(&self, overrides: WindowOverrides) -> Self {
        Self {
            width: overrides.width.unwrap_or(self.width),
            height: overrides.height.unwrap_or(self.height),
            companion_width: overrides.companion_width.unwrap_or(self.companion_width),
            side: overrides.side.unwrap_or(self.side),
            gap: overrides.gap.unwrap_or(self.gap),
            x: overrides.x.unwrap_or(self.x),
            y: overrides.y.unwrap_or(self.y),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConfigError(String);

impl ConfigError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for ConfigError {}

pub fn embedded() -> Config {
    parse(EMBEDDED_THEME).expect("the embedded rofi-preview-shared theme must be valid")
}

pub fn parse(source: &str) -> Result<Config, ConfigError> {
    let settings = settings_block(source)?;
    let mut width = None;
    let mut height = None;
    let mut companion_width = None;
    let mut panel_side = None;
    let mut panel_gap = None;
    let mut x = None;
    let mut y = None;

    for line in settings.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let (key, value) = line.split_once(':').ok_or_else(|| {
            ConfigError::new(format!("invalid setting {line:?}; expected name: value;"))
        })?;
        let key = key.trim();
        let value = value
            .trim()
            .strip_suffix(';')
            .ok_or_else(|| ConfigError::new(format!("{key} must end with a semicolon")))?
            .trim();

        match key {
            "width" => set_once(&mut width, dimension(value, "width")?, "width")?,
            "height" => set_once(&mut height, dimension(value, "height")?, "height")?,
            "companion_width" | "companion-width" => set_once(
                &mut companion_width,
                dimension(value, "companion_width")?,
                "companion_width",
            )?,
            "side" => set_once(&mut panel_side, side(value)?, "side")?,
            "gap" => set_once(&mut panel_gap, gap(value)?, "gap")?,
            "x" => set_once(&mut x, offset(value, "x")?, "x")?,
            "y" => set_once(&mut y, offset(value, "y")?, "y")?,
            _ => {
                return Err(ConfigError::new(format!(
                    "unknown rofi-preview-shared setting {key:?}"
                )));
            }
        }
    }

    Ok(Config {
        window: WindowConfig {
            width: required(width, "width")?,
            height: required(height, "height")?,
            companion_width: required(companion_width, "companion_width")?,
            side: required(panel_side, "side")?,
            gap: required(panel_gap, "gap")?,
            x: required(x, "x")?,
            y: required(y, "y")?,
        },
        css: source.to_owned(),
    })
}

pub fn parse_layout(source: &str) -> Result<WindowOverrides, ConfigError> {
    let settings = optional_settings_block(source, LAYOUT_START)?
        .or(optional_settings_block(source, LEGACY_LAYOUT_START)?);
    let Some(settings) = settings else {
        return Ok(WindowOverrides::default());
    };
    let mut overrides = WindowOverrides::default();

    for line in settings.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let (key, value) = line.split_once(':').ok_or_else(|| {
            ConfigError::new(format!("invalid setting {line:?}; expected name: value;"))
        })?;
        let key = key.trim();
        let value = value
            .trim()
            .strip_suffix(';')
            .ok_or_else(|| ConfigError::new(format!("{key} must end with a semicolon")))?
            .trim();

        match key {
            "width" => set_once(&mut overrides.width, dimension(value, "width")?, "width")?,
            "height" => set_once(&mut overrides.height, dimension(value, "height")?, "height")?,
            "companion_width" | "companion-width" => set_once(
                &mut overrides.companion_width,
                dimension(value, "companion_width")?,
                "companion_width",
            )?,
            "side" => set_once(&mut overrides.side, side(value)?, "side")?,
            "gap" => set_once(&mut overrides.gap, gap(value)?, "gap")?,
            "x" => set_once(&mut overrides.x, offset(value, "x")?, "x")?,
            "y" => set_once(&mut overrides.y, offset(value, "y")?, "y")?,
            _ => {
                return Err(ConfigError::new(format!(
                    "unknown rofi-preview-shared layout setting {key:?}"
                )));
            }
        }
    }

    Ok(overrides)
}

pub fn configured_path() -> Option<PathBuf> {
    theme_path_from(
        env::var_os("ROFI_PREVIEW_SHARED_CSS")
            .or_else(|| env::var_os("PREVIEW_PANEL_CSS"))
            .as_deref(),
        env::var_os("XDG_CONFIG_HOME").as_deref(),
        env::var_os("HOME").as_deref(),
    )
}

pub(crate) fn theme_path_from(
    override_path: Option<&OsStr>,
    xdg_config_home: Option<&OsStr>,
    home: Option<&OsStr>,
) -> Option<PathBuf> {
    if let Some(path) = override_path.filter(|path| !path.is_empty()) {
        return Some(PathBuf::from(path));
    }

    let config_home = xdg_config_home
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            home.filter(|path| !path.is_empty())
                .map(|path| PathBuf::from(path).join(".config"))
        })?;
    Some(config_home.join("rofi-preview-shared/rofi-preview-shared.css"))
}

fn settings_block(source: &str) -> Result<&str, ConfigError> {
    let (marker, start) = source
        .find(SETTINGS_START)
        .map(|start| (SETTINGS_START, start))
        .or_else(|| {
            source
                .find(LEGACY_SETTINGS_START)
                .map(|start| (LEGACY_SETTINGS_START, start))
        })
        .ok_or_else(|| {
            ConfigError::new("missing /* rofi-preview-shared-settings configuration block")
        })?;
    let settings = &source[start + marker.len()..];
    let end = settings.find("*/").ok_or_else(|| {
        ConfigError::new("rofi-preview-shared-settings configuration block is not closed")
    })?;
    Ok(&settings[..end])
}

fn optional_settings_block<'a>(
    source: &'a str,
    marker: &str,
) -> Result<Option<&'a str>, ConfigError> {
    let Some(start) = source.find(marker) else {
        return Ok(None);
    };
    let settings = &source[start + marker.len()..];
    let end = settings.find("*/").ok_or_else(|| {
        ConfigError::new("rofi-preview-shared-layout configuration block is not closed")
    })?;
    Ok(Some(&settings[..end]))
}

fn set_once<T>(slot: &mut Option<T>, value: T, key: &str) -> Result<(), ConfigError> {
    if slot.replace(value).is_some() {
        return Err(ConfigError::new(format!(
            "rofi-preview-shared setting {key:?} is repeated"
        )));
    }
    Ok(())
}

fn required<T>(value: Option<T>, key: &str) -> Result<T, ConfigError> {
    value.ok_or_else(|| ConfigError::new(format!("missing rofi-preview-shared setting {key:?}")))
}

fn pixels(value: &str, key: &str) -> Result<i64, ConfigError> {
    let number = value.strip_suffix("px").unwrap_or(value).trim();
    number.parse::<i64>().map_err(|_| {
        ConfigError::new(format!(
            "{key} must be a whole number, optionally followed by px"
        ))
    })
}

fn dimension(value: &str, key: &str) -> Result<i32, ConfigError> {
    let value = pixels(value, key)?;
    if !(200..=8192).contains(&value) {
        return Err(ConfigError::new(format!(
            "{key} must be between 200 and 8192 pixels"
        )));
    }
    Ok(value as i32)
}

fn gap(value: &str) -> Result<i32, ConfigError> {
    let value = pixels(value, "gap")?;
    if !(0..=512).contains(&value) {
        return Err(ConfigError::new("gap must be between 0 and 512 pixels"));
    }
    Ok(value as i32)
}

fn offset(value: &str, key: &str) -> Result<i32, ConfigError> {
    let value = pixels(value, key)?;
    if !(-8192..=8192).contains(&value) {
        return Err(ConfigError::new(format!(
            "{key} must be between -8192 and 8192 pixels"
        )));
    }
    Ok(value as i32)
}

fn side(value: &str) -> Result<Side, ConfigError> {
    match value {
        "left" => Ok(Side::Left),
        "right" => Ok(Side::Right),
        _ => Err(ConfigError::new("side must be either left or right")),
    }
}
