mod clipboard;
mod editor;
mod model;
mod rofi;
mod store;
mod waybar;

#[cfg(test)]
mod tests;

use std::env;

use anyhow::{Result, bail};

use crate::clipboard::{capture_clipboard, store_stdin};
use crate::rofi::{Mode, launch};

fn main() {
    if let Err(error) = run() {
        eprintln!("rofi-clipboard: {error:#}");
        std::process::exit(1);
    }
}

// Parse commands (such as run, script, capture, and store)
pub fn run() -> Result<()> {
    let mut args = env::args().skip(1);
    match args.next().as_deref() {
        None | Some("run") => launch(Mode::Memo, None),
        Some("capture") => capture_clipboard(),
        Some("status") => waybar::run_status(),
        Some("clear") => waybar::clear_selection(),
        Some("store") => {
            let mime = parse_mime_argument(args)?;
            store_stdin(&mime)
        }
        Some(command) => bail!("unknown command {command:?}; try --help"),
    }
}

fn parse_mime_argument(mut args: impl Iterator<Item = String>) -> Result<String> {
    let flag = args.next();
    match (flag.as_deref(), args.next()) {
        (Some("--mime"), Some(mime)) => Ok(mime),
        _ => bail!("store requires --mime MIME"),
    }
}
