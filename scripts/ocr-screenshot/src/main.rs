mod capture;
mod model;
mod ocr;

#[cfg(test)]
mod tests;

use std::env;
use std::fs;
use std::process::ExitCode;

use capture::capture_screenshot;
use model::{Tools, Workspace};
use ocr::{copy_to_clipboard, notify, run_ocr};

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
