use std::error::Error;
use std::ffi::OsString;
use std::fmt;
use std::path::PathBuf;

pub const HELP: &str = r#"preview-panel - reusable GTK4 text preview window

USAGE:
    preview-panel [OPTIONS] [FILE]

INPUT:
    With FILE, the file is read as UTF-8.
    Without FILE, text is read from standard input.
    If standard input is a terminal, the window starts with an empty buffer.

OPTIONS:
    --stdin             Read standard input explicitly
    -t, --title TITLE   Window title (default: Preview)
    --read-only         Allow selection and copying, but disable editing
    --no-wrap           Keep long lines intact and enable horizontal scrolling
    --listen SOCKET     Accept live text updates and a close command on SOCKET
    --panel             Use a borderless Wayland layer-shell companion panel
    --width PIXELS      Initial width (default: 720)
    --height PIXELS     Initial height (default: 520)
    --companion-width N Width of the centered companion window (default: 400)
    --side SIDE         Place the panel on the left or right (default: left)
    --gap PIXELS        Gap beside the companion window (default: 10)
    -h, --help          Show this help
    -V, --version       Show the version

CONFIGURATION:
    TOML is loaded from $PREVIEW_PANEL_CONFIG when set, otherwise from
    $XDG_CONFIG_HOME/preview-panel/config.toml (or ~/.config/preview-panel/config.toml).
    [window] controls panel size and automatic companion placement.
    [position] provides x/y screen offsets. [appearance].css contains raw GTK4
    CSS. Saving valid TOML reloads an open panel automatically.
    Explicit window options above override their matching TOML values.

BUILT-IN GTK CONTROLS:
    Mouse drag          Select text
    Ctrl+C              Copy the selection
    Mouse wheel         Scroll vertically
    Page Up/Page Down   Move by one visible page
    Arrow keys          Move the text cursor and scroll as needed

EXAMPLES:
    printf 'first line\n\tindented line\n' | preview-panel --title Clipboard
    preview-panel --read-only --no-wrap ./source.rs
    printf 'initial text' | preview-panel --listen /run/user/1000/preview.sock \
        --panel --width 300 --height 615
"#;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Side {
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct WindowOverrides {
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub companion_width: Option<i32>,
    pub side: Option<Side>,
    pub gap: Option<i32>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Source {
    Stdin,
    File(PathBuf),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Options {
    pub source: Source,
    pub title: String,
    pub editable: bool,
    pub wrap: bool,
    pub width: i32,
    pub height: i32,
    pub listen: Option<PathBuf>,
    pub panel: bool,
    pub companion_width: i32,
    pub side: Side,
    pub gap: i32,
    pub window_overrides: WindowOverrides,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            source: Source::Stdin,
            title: "Preview".to_owned(),
            editable: true,
            wrap: true,
            width: 720,
            height: 520,
            listen: None,
            panel: false,
            companion_width: 400,
            side: Side::Left,
            gap: 10,
            window_overrides: WindowOverrides::default(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Action {
    Run(Options),
    Help,
    Version,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CliError(String);

impl CliError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}; try --help", self.0)
    }
}

impl Error for CliError {}

pub fn parse_from<I, S>(arguments: I) -> Result<Action, CliError>
where
    I: IntoIterator<Item = S>,
    S: Into<OsString>,
{
    let mut arguments = arguments.into_iter().map(Into::into);
    let mut options = Options::default();
    let mut window_overrides = WindowOverrides::default();
    let mut file = None;
    let mut explicit_stdin = false;
    let mut options_finished = false;

    while let Some(argument) = arguments.next() {
        if !options_finished && argument.as_os_str() == "--" {
            options_finished = true;
            continue;
        }

        if !options_finished {
            match argument.to_str() {
                Some("-h" | "--help") => return Ok(Action::Help),
                Some("-V" | "--version") => return Ok(Action::Version),
                Some("--stdin") => {
                    explicit_stdin = true;
                    continue;
                }
                Some("--read-only") => {
                    options.editable = false;
                    continue;
                }
                Some("--no-wrap") => {
                    options.wrap = false;
                    continue;
                }
                Some("--panel") => {
                    options.panel = true;
                    continue;
                }
                Some("--listen") => {
                    options.listen = Some(PathBuf::from(next_os(&mut arguments, "--listen")?));
                    continue;
                }
                Some("-t" | "--title") => {
                    options.title = next_text(&mut arguments, "--title")?;
                    continue;
                }
                Some("--width") => {
                    options.width = next_dimension(&mut arguments, "--width")?;
                    window_overrides.width = Some(options.width);
                    continue;
                }
                Some("--height") => {
                    options.height = next_dimension(&mut arguments, "--height")?;
                    window_overrides.height = Some(options.height);
                    continue;
                }
                Some("--companion-width") => {
                    options.companion_width = next_dimension(&mut arguments, "--companion-width")?;
                    window_overrides.companion_width = Some(options.companion_width);
                    continue;
                }
                Some("--side") => {
                    options.side = parse_side(&next_text(&mut arguments, "--side")?)?;
                    window_overrides.side = Some(options.side);
                    continue;
                }
                Some("--gap") => {
                    options.gap = next_gap(&mut arguments)?;
                    window_overrides.gap = Some(options.gap);
                    continue;
                }
                Some(value) if value.starts_with("--title=") => {
                    options.title = value["--title=".len()..].to_owned();
                    continue;
                }
                Some(value) if value.starts_with("--width=") => {
                    options.width = parse_dimension(&value["--width=".len()..], "--width")?;
                    window_overrides.width = Some(options.width);
                    continue;
                }
                Some(value) if value.starts_with("--height=") => {
                    options.height = parse_dimension(&value["--height=".len()..], "--height")?;
                    window_overrides.height = Some(options.height);
                    continue;
                }
                Some(value) if value.starts_with("--companion-width=") => {
                    options.companion_width =
                        parse_dimension(&value["--companion-width=".len()..], "--companion-width")?;
                    window_overrides.companion_width = Some(options.companion_width);
                    continue;
                }
                Some(value) if value.starts_with("--side=") => {
                    options.side = parse_side(&value["--side=".len()..])?;
                    window_overrides.side = Some(options.side);
                    continue;
                }
                Some(value) if value.starts_with("--gap=") => {
                    options.gap = parse_gap(&value["--gap=".len()..])?;
                    window_overrides.gap = Some(options.gap);
                    continue;
                }
                Some(value) if value.starts_with('-') => {
                    return Err(CliError::new(format!("unknown option {value:?}")));
                }
                _ => {}
            }
        }

        if file.replace(PathBuf::from(argument)).is_some() {
            return Err(CliError::new("only one input file may be supplied"));
        }
    }

    if explicit_stdin && file.is_some() {
        return Err(CliError::new(
            "--stdin cannot be combined with an input file",
        ));
    }
    if let Some(path) = file {
        options.source = Source::File(path);
    }
    options.window_overrides = window_overrides;

    Ok(Action::Run(options))
}

fn next_text(
    arguments: &mut impl Iterator<Item = OsString>,
    option: &str,
) -> Result<String, CliError> {
    let value = arguments
        .next()
        .ok_or_else(|| CliError::new(format!("{option} requires a value")))?;
    value
        .into_string()
        .map_err(|_| CliError::new(format!("{option} must be valid UTF-8")))
}

fn next_os(
    arguments: &mut impl Iterator<Item = OsString>,
    option: &str,
) -> Result<OsString, CliError> {
    arguments
        .next()
        .ok_or_else(|| CliError::new(format!("{option} requires a value")))
}

fn next_dimension(
    arguments: &mut impl Iterator<Item = OsString>,
    option: &str,
) -> Result<i32, CliError> {
    let value = next_text(arguments, option)?;
    parse_dimension(&value, option)
}

fn parse_dimension(value: &str, option: &str) -> Result<i32, CliError> {
    let pixels = value
        .parse::<i32>()
        .map_err(|_| CliError::new(format!("{option} must be a whole number")))?;
    if !(200..=8192).contains(&pixels) {
        return Err(CliError::new(format!(
            "{option} must be between 200 and 8192 pixels"
        )));
    }
    Ok(pixels)
}

fn next_gap(arguments: &mut impl Iterator<Item = OsString>) -> Result<i32, CliError> {
    let value = next_text(arguments, "--gap")?;
    parse_gap(&value)
}

fn parse_gap(value: &str) -> Result<i32, CliError> {
    let pixels = value
        .parse::<i32>()
        .map_err(|_| CliError::new("--gap must be a whole number"))?;
    if !(0..=512).contains(&pixels) {
        return Err(CliError::new("--gap must be between 0 and 512 pixels"));
    }
    Ok(pixels)
}

fn parse_side(value: &str) -> Result<Side, CliError> {
    match value {
        "left" => Ok(Side::Left),
        "right" => Ok(Side::Right),
        _ => Err(CliError::new("--side must be either left or right")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(arguments: &[&str]) -> Options {
        match parse_from(arguments.iter().copied()).expect("arguments should parse") {
            Action::Run(options) => options,
            other => panic!("expected run action, got {other:?}"),
        }
    }

    #[test]
    fn defaults_to_editable_wrapped_standard_input() {
        assert_eq!(run(&[]), Options::default());
    }

    #[test]
    fn accepts_a_file_without_changing_its_path() {
        let options = run(&["folder/a file.txt"]);
        assert_eq!(
            options.source,
            Source::File(PathBuf::from("folder/a file.txt"))
        );
    }

    #[test]
    fn parses_window_and_editor_options() {
        let options = run(&[
            "--title",
            "Clipboard preview",
            "--read-only",
            "--no-wrap",
            "--listen",
            "/run/user/1000/preview.sock",
            "--panel",
            "--width=900",
            "--height",
            "700",
            "--companion-width=420",
            "--side",
            "right",
            "--gap",
            "12",
        ]);
        assert_eq!(options.title, "Clipboard preview");
        assert!(!options.editable);
        assert!(!options.wrap);
        assert_eq!((options.width, options.height), (900, 700));
        assert_eq!(
            options.listen,
            Some(PathBuf::from("/run/user/1000/preview.sock"))
        );
        assert!(options.panel);
        assert_eq!(options.companion_width, 420);
        assert_eq!(options.side, Side::Right);
        assert_eq!(options.gap, 12);
        assert_eq!(
            options.window_overrides,
            WindowOverrides {
                width: Some(900),
                height: Some(700),
                companion_width: Some(420),
                side: Some(Side::Right),
                gap: Some(12),
            }
        );
    }

    #[test]
    fn double_dash_allows_a_filename_starting_with_a_dash() {
        let options = run(&["--", "--notes.txt"]);
        assert_eq!(options.source, Source::File(PathBuf::from("--notes.txt")));
    }

    #[test]
    fn rejects_file_with_explicit_stdin() {
        let error = parse_from(["--stdin", "notes.txt"]).unwrap_err();
        assert!(error.to_string().contains("cannot be combined"));
    }

    #[test]
    fn rejects_dimensions_that_would_make_an_unusable_window() {
        let error = parse_from(["--width", "50"]).unwrap_err();
        assert!(error.to_string().contains("between 200 and 8192"));
    }

    #[test]
    fn rejects_negative_panel_gaps() {
        let error = parse_from(["--gap=-1"]).unwrap_err();
        assert!(error.to_string().contains("between 0 and 512"));
    }

    #[test]
    fn rejects_unknown_panel_sides() {
        let error = parse_from(["--side", "middle"]).unwrap_err();
        assert!(error.to_string().contains("left or right"));
    }
}
