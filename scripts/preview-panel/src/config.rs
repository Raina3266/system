use std::env;
use std::error::Error;
use std::ffi::OsStr;
use std::fmt;
use std::path::PathBuf;

use toml::Value;

use crate::cli::{Side, WindowOverrides};

const EMBEDDED_CONFIG: &str = include_str!("../config.toml");

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
            x: self.x,
            y: self.y,
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
    parse(EMBEDDED_CONFIG).expect("the embedded preview-panel config must be valid")
}

pub fn parse(source: &str) -> Result<Config, ConfigError> {
    let document = toml::from_str::<Value>(source)
        .map_err(|error| ConfigError::new(format!("invalid TOML: {error}")))?;
    let root = document
        .as_table()
        .ok_or_else(|| ConfigError::new("the TOML root must be a table"))?;
    let window = table(root, "window")?;
    let position = table(root, "position")?;
    let appearance = table(root, "appearance")?;

    Ok(Config {
        window: WindowConfig {
            width: dimension(window, "width")?,
            height: dimension(window, "height")?,
            companion_width: dimension(window, "companion_width")?,
            side: side(window)?,
            gap: gap(window)?,
            x: offset(position, "x")?,
            y: offset(position, "y")?,
        },
        css: string(appearance, "css")?.to_owned(),
    })
}

pub fn configured_path() -> Option<PathBuf> {
    config_path_from(
        env::var_os("PREVIEW_PANEL_CONFIG").as_deref(),
        env::var_os("XDG_CONFIG_HOME").as_deref(),
        env::var_os("HOME").as_deref(),
    )
}

fn config_path_from(
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
    Some(config_home.join("preview-panel/config.toml"))
}

fn table<'a>(root: &'a toml::Table, name: &str) -> Result<&'a toml::Table, ConfigError> {
    root.get(name)
        .and_then(Value::as_table)
        .ok_or_else(|| ConfigError::new(format!("missing [{name}] table")))
}

fn string<'a>(table: &'a toml::Table, key: &str) -> Result<&'a str, ConfigError> {
    table
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| ConfigError::new(format!("{key} must be a string")))
}

fn integer(table: &toml::Table, key: &str) -> Result<i64, ConfigError> {
    table
        .get(key)
        .and_then(Value::as_integer)
        .ok_or_else(|| ConfigError::new(format!("{key} must be a whole number")))
}

fn dimension(table: &toml::Table, key: &str) -> Result<i32, ConfigError> {
    let value = integer(table, key)?;
    if !(200..=8192).contains(&value) {
        return Err(ConfigError::new(format!(
            "{key} must be between 200 and 8192 pixels"
        )));
    }
    Ok(value as i32)
}

fn gap(table: &toml::Table) -> Result<i32, ConfigError> {
    let value = integer(table, "gap")?;
    if !(0..=512).contains(&value) {
        return Err(ConfigError::new("gap must be between 0 and 512 pixels"));
    }
    Ok(value as i32)
}

fn offset(table: &toml::Table, key: &str) -> Result<i32, ConfigError> {
    let value = integer(table, key)?;
    if !(-8192..=8192).contains(&value) {
        return Err(ConfigError::new(format!(
            "{key} must be between -8192 and 8192 pixels"
        )));
    }
    Ok(value as i32)
}

fn side(table: &toml::Table) -> Result<Side, ConfigError> {
    match string(table, "side")? {
        "left" => Ok(Side::Left),
        "right" => Ok(Side::Right),
        _ => Err(ConfigError::new("side must be either left or right")),
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;

    use super::*;

    #[test]
    fn embedded_panel_width_is_400_pixels() {
        assert_eq!(embedded().window.width, 400);
    }

    #[test]
    fn parses_window_values_and_raw_css() {
        let config = parse(
            r##"
                [window]
                width = 480
                height = 615
                companion_width = 400
                side = "right"
                gap = 14

                [position]
                x = 35
                y = -20

                [appearance]
                css = '''window { color: #cbe3e7; }'''
            "##,
        )
        .unwrap();

        assert_eq!(config.window.width, 480);
        assert_eq!(config.window.height, 615);
        assert_eq!(config.window.companion_width, 400);
        assert_eq!(config.window.side, Side::Right);
        assert_eq!(config.window.gap, 14);
        assert_eq!(config.window.x, 35);
        assert_eq!(config.window.y, -20);
        assert_eq!(config.css, "window { color: #cbe3e7; }");
    }

    #[test]
    fn command_line_values_override_toml_window_values() {
        let window = WindowConfig {
            width: 480,
            height: 615,
            companion_width: 400,
            side: Side::Left,
            gap: 10,
            x: 25,
            y: -15,
        };
        let overrides = WindowOverrides {
            width: Some(600),
            side: Some(Side::Right),
            ..WindowOverrides::default()
        };

        assert_eq!(
            window.with_overrides(overrides),
            WindowConfig {
                width: 600,
                height: 615,
                companion_width: 400,
                side: Side::Right,
                gap: 10,
                x: 25,
                y: -15,
            }
        );
    }

    #[test]
    fn rejects_invalid_window_values() {
        let error = parse(
            r#"
                [window]
                width = 100
                height = 615
                companion_width = 400
                side = "middle"
                gap = -1

                [position]
                x = 0
                y = 0

                [appearance]
                css = ""
            "#,
        )
        .unwrap_err();
        assert!(error.to_string().contains("width"));
    }

    #[test]
    fn rejects_position_outside_supported_range() {
        let error = parse(
            r#"
                [window]
                width = 400
                height = 615
                companion_width = 400
                side = "left"
                gap = 10

                [position]
                x = 9000
                y = 0

                [appearance]
                css = ""
            "#,
        )
        .unwrap_err();
        assert!(error.to_string().contains("x"));
    }

    #[test]
    fn explicit_config_path_takes_priority() {
        assert_eq!(
            config_path_from(
                Some(OsStr::new("/tmp/custom.toml")),
                Some(OsStr::new("/tmp/config")),
                Some(OsStr::new("/home/raina")),
            ),
            Some(PathBuf::from("/tmp/custom.toml"))
        );
    }

    #[test]
    fn config_path_uses_xdg_then_home_fallback() {
        assert_eq!(
            config_path_from(None, Some(OsStr::new("/tmp/config")), None),
            Some(PathBuf::from("/tmp/config/preview-panel/config.toml"))
        );
        assert_eq!(
            config_path_from(None, None, Some(OsStr::new("/home/raina"))),
            Some(PathBuf::from(
                "/home/raina/.config/preview-panel/config.toml"
            ))
        );
    }
}
