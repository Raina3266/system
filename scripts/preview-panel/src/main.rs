use std::error::Error;

use preview_panel::cli::{self, Action};
use preview_panel::{document, ui};

mod cli;
mod document;
mod ui;

fn main() {
    if let Err(error) = run() {
        eprintln!("preview-panel: {error}");
        std::process::exit(2);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    match cli::parse_from(std::env::args_os().skip(1))? {
        Action::Help => print!("{}", cli::HELP),
        Action::Version => println!("preview-panel {}", env!("CARGO_PKG_VERSION")),
        Action::Run(options) => {
            let text = document::load(&options.source)?;
            ui::run(options, text);
        }
    }
    Ok(())
}
