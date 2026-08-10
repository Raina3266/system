use std::error::Error;

mod cli;
mod config;
mod document;
mod ipc;
mod ui;

use crate::cli::Action;

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
            let server = options
                .listen
                .as_deref()
                .map(ipc::bind)
                .transpose()?;
            let (receiver, socket_guard) = match server {
                Some((receiver, guard)) => (Some(receiver), Some(guard)),
                None => (None, None),
            };
            ui::run(options, text, receiver);
            drop(socket_guard);
        }
    }
    Ok(())
}
