use std::error::Error;
use std::ffi::OsString;
use std::fmt;
use std::path::PathBuf;

pub const HELP: &str = r#"rofi-preview-shared - reusable GTK4 text preview window

USAGE:
    rofi-preview-shared [OPTIONS] [FILE]

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
    --layout-file FILE  Read partial panel geometry from a Rasi theme
    --width PIXELS      Initial width (default: 720)
    --height PIXELS     Initial height (default: 520)
    --companion-width N Width of the centered companion window (default: 400)
    --side SIDE         Place the panel on the left or right (default: left)
    --gap PIXELS        Gap beside the companion window (default: 10)
    -h, --help          Show this help
    -V, --version       Show the version

CONFIGURATION:
    CSS is loaded from $ROFI_PREVIEW_SHARED_CSS when set, otherwise from
    $XDG_CONFIG_HOME/rofi-preview-shared/rofi-preview-shared.css (or the matching path under
    ~/.config). Its /* rofi-preview-shared-settings ... */ comment controls width,
    height, companion_width, side, gap, x, and y. The rest is normal GTK4 CSS.
    Saving valid CSS reloads appearance and geometry in every open panel.
    A Rasi /* rofi-preview-shared-layout ... */ block may override any geometry field.
    Explicit window options above override both Rasi and CSS settings.

BUILT-IN GTK CONTROLS:
    Mouse drag          Select text
    Ctrl+C              Copy the selection
    Mouse wheel         Scroll vertically
    Page Up/Page Down   Move by one visible page
    Arrow keys          Move the text cursor and scroll as needed

EXAMPLES:
    printf 'first line\n\tindented line\n' | rofi-preview-shared --title Clipboard
    rofi-preview-shared --read-only --no-wrap ./source.rs
    printf 'initial text' | rofi-preview-shared --listen /run/user/1000/preview.sock \
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
    pub x: Option<i32>,
    pub y: Option<i32>,
}

impl WindowOverrides {
    pub fn overlaid_by(self, higher_priority: Self) -> Self {
        Self {
            width: higher_priority.width.or(self.width),
            height: higher_priority.height.or(self.height),
            companion_width: higher_priority.companion_width.or(self.companion_width),
            side: higher_priority.side.or(self.side),
            gap: higher_priority.gap.or(self.gap),
            x: higher_priority.x.or(self.x),
            y: higher_priority.y.or(self.y),
        }
    }
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
    pub layout_file: Option<PathBuf>,
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
            layout_file: None,
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
                Some("--layout-file") => {
                    options.layout_file =
                        Some(PathBuf::from(next_os(&mut arguments, "--layout-file")?));
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
                Some(value) if value.starts_with("--layout-file=") => {
                    options.layout_file = Some(PathBuf::from(&value["--layout-file=".len()..]));
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
