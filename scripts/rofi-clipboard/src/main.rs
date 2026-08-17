mod clipboard;
mod model;
mod preview;
mod rofi;
mod store;
mod waybar;

use std::env;

use anyhow::{Context, Result, bail};

use crate::clipboard::{capture_clipboard, store_stdin};
use crate::rofi::{Mode, launch_rofi, run_script};

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
        None | Some("run") => launch_rofi(Mode::Memo, None),
        Some("script") => {
            let mode = Mode::parse(args.next().as_deref().unwrap_or("memo"))?;
            run_script(mode, args.next())
        }
        Some("capture") => capture_clipboard(),
        Some("status") => waybar::run_status(),
        Some("clear") => waybar::clear_selection(),
        Some("preview-selection") => {
            let id = args
                .next()
                .context("preview-selection requires a clipboard item ID")?
                .parse::<u64>()
                .context("preview-selection ID must be a positive integer")?;
            let serial = args
                .next()
                .context("preview-selection requires a selection serial")?
                .parse::<u64>()
                .context("preview-selection serial must be a positive integer")?;
            preview::selection_changed(id, serial)
        }
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
