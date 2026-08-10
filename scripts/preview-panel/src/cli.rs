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
    --width PIXELS      Initial width (default: 720)
    --height PIXELS     Initial height (default: 520)
    -h, --help          Show this help
    -V, --version       Show the version

BUILT-IN GTK CONTROLS:
    Mouse drag          Select text
    Ctrl+C              Copy the selection
    Mouse wheel         Scroll vertically
    Page Up/Page Down   Move by one visible page
    Arrow keys          Move the text cursor and scroll as needed

EXAMPLES:
    printf 'first line\n\tindented line\n' | preview-panel --title Clipboard
    preview-panel --read-only --no-wrap ./source.rs
"#;

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
                Some("-t" | "--title") => {
                    options.title = next_text(&mut arguments, "--title")?;
                    continue;
                }
                Some("--width") => {
                    options.width = next_dimension(&mut arguments, "--width")?;
                    continue;
                }
                Some("--height") => {
                    options.height = next_dimension(&mut arguments, "--height")?;
                    continue;
                }
                Some(value) if value.starts_with("--title=") => {
                    options.title = value["--title=".len()..].to_owned();
                    continue;
                }
                Some(value) if value.starts_with("--width=") => {
                    options.width = parse_dimension(&value["--width=".len()..], "--width")?;
                    continue;
                }
                Some(value) if value.starts_with("--height=") => {
                    options.height = parse_dimension(&value["--height=".len()..], "--height")?;
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
            "--width=900",
            "--height",
            "700",
        ]);
        assert_eq!(options.title, "Clipboard preview");
        assert!(!options.editable);
        assert!(!options.wrap);
        assert_eq!((options.width, options.height), (900, 700));
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
}
