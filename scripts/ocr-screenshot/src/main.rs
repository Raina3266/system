use std::env;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::Write;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug)]
struct Tools {
    gnome_screenshot: OsString,
    grim: OsString,
    slurp: OsString,
    tesseract: OsString,
    wl_copy: OsString,
    notify_send: OsString,
}

impl Tools {
    fn from_environment() -> Self {
        Self {
            gnome_screenshot: executable("OCR_SCREENSHOT_GNOME_SCREENSHOT", "gnome-screenshot"),
            grim: executable("OCR_SCREENSHOT_GRIM", "grim"),
            slurp: executable("OCR_SCREENSHOT_SLURP", "slurp"),
            tesseract: executable("OCR_SCREENSHOT_TESSERACT", "tesseract"),
            wl_copy: executable("OCR_SCREENSHOT_WL_COPY", "wl-copy"),
            notify_send: executable("OCR_SCREENSHOT_NOTIFY_SEND", "notify-send"),
        }
    }
}

fn executable(variable: &str, fallback: &str) -> OsString {
    env::var_os(variable).unwrap_or_else(|| OsString::from(fallback))
}

struct Workspace {
    directory: PathBuf,
}

impl Workspace {
    fn create() -> Result<Self, String> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| format!("could not read the system clock: {error}"))?
            .as_nanos();
        let base = format!("ocr-screenshot-{}-{timestamp}", std::process::id());

        for attempt in 0..100 {
            let suffix = if attempt == 0 {
                String::new()
            } else {
                format!("-{attempt}")
            };
            let directory = env::temp_dir().join(format!("{base}{suffix}"));

            match fs::DirBuilder::new().mode(0o700).create(&directory) {
                Ok(()) => return Ok(Self { directory }),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => {
                    return Err(format!(
                        "could not create temporary directory {}: {error}",
                        directory.display()
                    ));
                }
            }
        }

        Err("could not create a unique temporary directory".to_owned())
    }

    fn image_path(&self) -> PathBuf {
        self.directory.join("capture.png")
    }

    fn output_prefix(&self) -> PathBuf {
        self.directory.join("output")
    }

    fn output_path(&self) -> PathBuf {
        self.directory.join("output.txt")
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}

fn desktop_is_gnome(desktop: &str) -> bool {
    desktop.to_ascii_lowercase().contains("gnome")
}

fn capture_screenshot(tools: &Tools, image_path: &Path) -> Result<bool, String> {
    let desktop = env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();

    if desktop_is_gnome(&desktop) {
        let status = Command::new(&tools.gnome_screenshot)
            .arg("--area")
            .arg(format!("--file={}", image_path.display()))
            .status()
            .map_err(|error| format!("could not start gnome-screenshot: {error}"))?;

        if !status.success() {
            return if image_path.is_file() {
                Err(format!("gnome-screenshot failed with {status}"))
            } else {
                Ok(false)
            };
        }
    } else {
        let selection = Command::new(&tools.slurp)
            .output()
            .map_err(|error| format!("could not start slurp: {error}"))?;
        if !selection.status.success() {
            return Ok(false);
        }

        let geometry = String::from_utf8(selection.stdout)
            .map_err(|error| format!("slurp returned invalid UTF-8: {error}"))?;
        let geometry = geometry.trim();
        if geometry.is_empty() {
            return Ok(false);
        }

        let status = Command::new(&tools.grim)
            .arg("-g")
            .arg(geometry)
            .arg(image_path)
            .status()
            .map_err(|error| format!("could not start grim: {error}"))?;
        ensure_success("grim", status)?;
    }

    Ok(image_path.is_file())
}

fn run_ocr(tools: &Tools, image_path: &Path, output_prefix: &Path) -> Result<(), String> {
    let status = Command::new(&tools.tesseract)
        .arg(image_path)
        .arg(output_prefix)
        .status()
        .map_err(|error| format!("could not start tesseract: {error}"))?;
    ensure_success("tesseract", status)
}

fn copy_to_clipboard(program: &OsStr, text: &str) -> Result<(), String> {
    let mut child = Command::new(program)
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|error| format!("could not start wl-copy: {error}"))?;

    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| "could not open wl-copy standard input".to_owned())?;
    stdin
        .write_all(clipboard_payload(text).as_bytes())
        .map_err(|error| format!("could not write to wl-copy: {error}"))?;
    drop(stdin);

    let status = child
        .wait()
        .map_err(|error| format!("could not wait for wl-copy: {error}"))?;
    ensure_success("wl-copy", status)
}

fn clipboard_payload(text: &str) -> String {
    format!("{}\n", trimmed_ocr_text(text))
}

fn trimmed_ocr_text(text: &str) -> &str {
    text.trim_end_matches(|character| matches!(character, '\r' | '\n'))
}

fn notify(program: &OsStr, text: &str) -> Result<(), String> {
    let status = Command::new(program)
        .arg("OCR")
        .arg(format!("Copied: {}", trimmed_ocr_text(text)))
        .status()
        .map_err(|error| format!("could not start notify-send: {error}"))?;
    ensure_success("notify-send", status)
}

fn ensure_success(name: &str, status: std::process::ExitStatus) -> Result<(), String> {
    if status.success() {
        Ok(())
    } else {
        Err(format!("{name} failed with {status}"))
    }
}

fn run() -> Result<(), String> {
    let workspace = Workspace::create()?;
    let tools = Tools::from_environment();
    let image_path = workspace.image_path();

    if !capture_screenshot(&tools, &image_path)? {
        return Ok(());
    }

    run_ocr(&tools, &image_path, &workspace.output_prefix())?;
    let text = fs::read_to_string(workspace.output_path())
        .map_err(|error| format!("could not read tesseract output: {error}"))?;
    copy_to_clipboard(&tools.wl_copy, &text)?;
    notify(&tools.notify_send, &text)
}

fn print_help() {
    println!(
        "ocr-screenshot\n\n\
         Select a screen region, recognize its text, and copy it to the clipboard.\n\n\
         Usage: ocr-screenshot"
    );
}

fn main() -> ExitCode {
    match env::args().nth(1).as_deref() {
        None => match run() {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("ocr-screenshot: {error}");
                ExitCode::FAILURE
            }
        },
        Some("help" | "--help" | "-h") => {
            print_help();
            ExitCode::SUCCESS
        }
        Some(argument) => {
            eprintln!("ocr-screenshot: unexpected argument: {argument}\n");
            print_help();
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_gnome_desktop_variants() {
        assert!(desktop_is_gnome("GNOME"));
        assert!(desktop_is_gnome("ubuntu:GNOME"));
        assert!(desktop_is_gnome("GNOME-Classic"));
        assert!(!desktop_is_gnome("niri"));
    }

    #[test]
    fn clipboard_matches_the_shell_scripts_newline_behavior() {
        assert_eq!(clipboard_payload("hello\n\n"), "hello\n");
        assert_eq!(clipboard_payload(""), "\n");
    }
}
