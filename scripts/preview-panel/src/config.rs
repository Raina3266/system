use std::env;
use std::error::Error;
use std::ffi::OsStr;
use std::fmt;
use std::path::PathBuf;

use crate::cli::{Side, WindowOverrides};

const SETTINGS_START: &str = "/* preview-panel-settings";

const EMBEDDED_THEME: &str = r#"/* preview-panel-settings
width: 400px;
height: 616px;
companion_width: 400px;
side: left;
gap: 5px;
x: 770px;
y: -850px;
*/

window.preview-panel {
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
    parse(EMBEDDED_THEME).expect("the embedded preview-panel theme must be valid")
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
                    "unknown preview-panel setting {key:?}"
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

pub fn configured_path() -> Option<PathBuf> {
    theme_path_from(
        env::var_os("PREVIEW_PANEL_CSS").as_deref(),
        env::var_os("XDG_CONFIG_HOME").as_deref(),
        env::var_os("HOME").as_deref(),
    )
}

fn theme_path_from(
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
    Some(config_home.join("preview-panel/preview-panel.css"))
}

fn settings_block(source: &str) -> Result<&str, ConfigError> {
    let marker = source
        .find(SETTINGS_START)
        .ok_or_else(|| ConfigError::new("missing /* preview-panel-settings configuration block"))?;
    let settings = &source[marker + SETTINGS_START.len()..];
    let end = settings.find("*/").ok_or_else(|| {
        ConfigError::new("preview-panel-settings configuration block is not closed")
    })?;
    Ok(&settings[..end])
}

fn set_once<T>(slot: &mut Option<T>, value: T, key: &str) -> Result<(), ConfigError> {
    if slot.replace(value).is_some() {
        return Err(ConfigError::new(format!(
            "preview-panel setting {key:?} is repeated"
        )));
    }
    Ok(())
}

fn required<T>(value: Option<T>, key: &str) -> Result<T, ConfigError> {
    value.ok_or_else(|| ConfigError::new(format!("missing preview-panel setting {key:?}")))
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

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;

    use super::*;

    const THEME: &str = r#"/* preview-panel-settings
width: 480px;
height: 615px;
companion_width: 400px;
side: right;
gap: 14px;
x: 35px;
y: -20px;
*/

window.preview-panel { color: #cbe3e7; }
"#;

    #[test]
    fn embedded_theme_has_expected_defaults() {
        assert_eq!(
            embedded().window,
            WindowConfig {
                width: 400,
                height: 616,
                companion_width: 400,
                side: Side::Left,
                gap: 5,
                x: 770,
                y: -850,
            }
        );
    }

    #[test]
    fn parses_geometry_and_preserves_complete_css() {
        let config = parse(THEME).unwrap();

        assert_eq!(config.window.width, 480);
        assert_eq!(config.window.height, 615);
        assert_eq!(config.window.companion_width, 400);
        assert_eq!(config.window.side, Side::Right);
        assert_eq!(config.window.gap, 14);
        assert_eq!(config.window.x, 35);
        assert_eq!(config.window.y, -20);
        assert_eq!(config.css, THEME);
    }

    #[test]
    fn command_line_values_override_css_window_values() {
        let window = parse(THEME).unwrap().window;
        let overrides = WindowOverrides {
            width: Some(600),
            side: Some(Side::Left),
            ..WindowOverrides::default()
        };

        assert_eq!(
            window.with_overrides(overrides),
            WindowConfig {
                width: 600,
                height: 615,
                companion_width: 400,
                side: Side::Left,
                gap: 14,
                x: 35,
                y: -20,
            }
        );
    }

    #[test]
    fn accepts_companion_width_with_css_style_name() {
        let source = THEME.replace("companion_width", "companion-width");
        assert_eq!(parse(&source).unwrap().window.companion_width, 400);
    }

    #[test]
    fn rejects_missing_unknown_and_repeated_settings() {
        let missing = THEME.replace("width: 480px;\n", "");
        assert!(parse(&missing).unwrap_err().to_string().contains("width"));

        let unknown = THEME.replace("*/", "opacity: 1;\n*/");
        assert!(parse(&unknown).unwrap_err().to_string().contains("unknown"));

        let repeated = THEME.replace("*/", "width: 500px;\n*/");
        assert!(
            parse(&repeated)
                .unwrap_err()
                .to_string()
                .contains("repeated")
        );
    }

    #[test]
    fn rejects_values_outside_supported_ranges() {
        let dimension = THEME.replace("width: 480px;", "width: 100px;");
        assert!(parse(&dimension).unwrap_err().to_string().contains("width"));

        let panel_gap = THEME.replace("gap: 14px;", "gap: -1px;");
        assert!(parse(&panel_gap).unwrap_err().to_string().contains("gap"));

        let position = THEME.replace("x: 35px;", "x: 9000px;");
        assert!(parse(&position).unwrap_err().to_string().contains("x"));
    }

    #[test]
    fn requires_a_closed_settings_comment() {
        assert!(
            parse("window { color: red; }")
                .unwrap_err()
                .to_string()
                .contains("missing")
        );

        let unclosed = THEME.replacen("*/", "", 1);
        assert!(
            parse(&unclosed)
                .unwrap_err()
                .to_string()
                .contains("not closed")
        );
    }

    #[test]
    fn explicit_css_path_takes_priority() {
        assert_eq!(
            theme_path_from(
                Some(OsStr::new("/tmp/custom.css")),
                Some(OsStr::new("/tmp/config")),
                Some(OsStr::new("/home/raina")),
            ),
            Some(PathBuf::from("/tmp/custom.css"))
        );
    }

    #[test]
    fn css_path_uses_xdg_then_home_fallback() {
        assert_eq!(
            theme_path_from(None, Some(OsStr::new("/tmp/config")), None),
            Some(PathBuf::from("/tmp/config/preview-panel/preview-panel.css"))
        );
        assert_eq!(
            theme_path_from(None, None, Some(OsStr::new("/home/raina"))),
            Some(PathBuf::from(
                "/home/raina/.config/preview-panel/preview-panel.css"
            ))
        );
    }
}
