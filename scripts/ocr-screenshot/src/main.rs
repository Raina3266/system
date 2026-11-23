mod capture;
mod model;
mod ocr;

#[cfg(test)]
mod tests;

use std::fs;
use std::process::ExitCode;

use clap::{CommandFactory, Parser, Subcommand};

use capture::capture_screenshot;
use model::{Tools, Workspace};
use ocr::{copy_to_clipboard, notify, run_ocr};

/// Select a screen region, recognize its text, and copy it to the clipboard.
#[derive(Debug, Parser)]
#[command(name = "ocr-screenshot", disable_help_subcommand = true)]
struct Cli {
    #[command(subcommand)]
    action: Option<Action>,
}

#[derive(Debug, Eq, PartialEq, Subcommand)]
enum Action {
    /// Show this help.
    Help,
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

fn main() -> ExitCode {
    let result = match Cli::parse().action {
        None => run(),
        Some(Action::Help) => Cli::command()
            .print_help()
            .map_err(|error| format!("could not print help: {error}")),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("ocr-screenshot: {error}");
            ExitCode::FAILURE
        }
    }
}
