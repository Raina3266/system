mod clipboard;
mod model;
mod rofi;
mod store;

use std::env;
use std::thread;
use std::time::Duration;

use anyhow::{Result, bail};

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
        None | Some("run") => launch_rofi(Mode::Pinned, false, None),
        Some("relaunch") => {
            let mode = Mode::parse(args.next().as_deref().unwrap_or("pinned"))?;
            let preview = args.next().as_deref() == Some("preview");
            let selected_id = args.next().and_then(|value| value.parse().ok());
            thread::sleep(Duration::from_millis(120));
            launch_rofi(mode, preview, selected_id)
        }
        Some("script") => {
            let mode = Mode::parse(args.next().as_deref().unwrap_or("pinned"))?;
            run_script(mode, args.next())
        }
        Some("capture") => capture_clipboard(),
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

