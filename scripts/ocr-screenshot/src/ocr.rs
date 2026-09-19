use std::ffi::OsStr;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use crate::model::{Tools, ensure_success};

pub(crate) fn run_ocr(
    tools: &Tools,
    image_path: &Path,
    output_prefix: &Path,
) -> Result<(), String> {
    let status = Command::new(&tools.tesseract)
        .arg(image_path)
        .arg(output_prefix)
        .status()
        .map_err(|error| format!("could not start tesseract: {error}"))?;
    ensure_success("tesseract", status)
}

pub(crate) fn copy_to_clipboard(program: &OsStr, text: &str) -> Result<(), String> {
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

pub(crate) fn clipboard_payload(text: &str) -> String {
    format!("{}\n", trimmed_ocr_text(text))
}

fn trimmed_ocr_text(text: &str) -> &str {
    text.trim_end_matches(['\r', '\n'])
}

pub(crate) fn notify(program: &OsStr, text: &str) -> Result<(), String> {
    let status = Command::new(program)
        .arg("OCR")
        .arg(format!("Copied: {}", trimmed_ocr_text(text)))
        .status()
        .map_err(|error| format!("could not start notify-send: {error}"))?;
    ensure_success("notify-send", status)
}
