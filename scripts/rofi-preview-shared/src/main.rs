use std::error::Error;

use rofi_preview_shared::panel::{self, Action};

fn main() {
    if let Err(error) = run() {
        eprintln!("rofi-preview-shared: {error}");
        std::process::exit(2);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    match panel::parse_from(std::env::args_os().skip(1))? {
        Action::Help => print!("{}", panel::HELP),
        Action::Version => println!("rofi-preview-shared {}", env!("CARGO_PKG_VERSION")),
        Action::Run(options) => {
            let text = panel::load(&options.source)?;
            let server = options.listen.as_deref().map(panel::bind).transpose()?;
            let (receiver, socket_guard) = match server {
                Some((receiver, guard)) => (Some(receiver), Some(guard)),
                None => (None, None),
            };
            panel::run(options, text, receiver);
            drop(socket_guard);
        }
    }
    Ok(())
}
