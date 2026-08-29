use std::env;
use std::error::Error;
use std::io;
use std::path::Path;

mod desktop;
mod model;
mod preview;
mod rofi;
mod search;

pub type AppError = Box<dyn Error + Send + Sync>;
pub type AppResult<T> = Result<T, AppError>;

fn main() {
    if let Err(error) = run() {
        eprintln!("rofi-filesearch: {error}");
        std::process::exit(1);
    }
}

fn run() -> AppResult<()> {
    let mut arguments = env::args_os();
    let _program = arguments.next();
    let command = arguments.next();
    match command.as_deref().and_then(|value| value.to_str()) {
        None => rofi::launch(),
        Some("script") => {
            let mode = arguments
                .next()
                .and_then(|value| value.to_str().map(str::to_owned))
                .ok_or_else(|| io::Error::other("script mode is missing"))?
                .parse()?;
            rofi::run_script(mode)
        }
        Some("preview-selection") => {
            let key = arguments
                .next()
                .and_then(|value| value.to_str().map(str::to_owned))
                .ok_or_else(|| io::Error::other("preview selection is missing"))?;
            let serial = arguments
                .next()
                .and_then(|value| value.to_str().and_then(|value| value.parse::<u64>().ok()))
                .ok_or_else(|| io::Error::other("preview serial is missing or invalid"))?;
            preview::selection_changed(&key, serial)
        }
        Some("thumbnail") => {
            let input = arguments
                .next()
                .ok_or_else(|| io::Error::other("thumbnail input path is missing"))?;
            let output = arguments
                .next()
                .ok_or_else(|| io::Error::other("thumbnail output path is missing"))?;
            let size = arguments
                .next()
                .and_then(|value| value.to_str().and_then(|value| value.parse::<u32>().ok()))
                .ok_or_else(|| io::Error::other("thumbnail size is missing or invalid"))?;
            preview::thumbnail_pdf(
                Path::new(input.as_os_str()),
                Path::new(output.as_os_str()),
                size,
            )
        }
        Some("--help" | "-h") => {
            println!(
                "rofi-filesearch\n\n\
                 USAGE:\n  \
                 rofi-filesearch\n  \
                 rofi-filesearch script <app|file|folder>\n  \
                 rofi-filesearch preview-selection <key> <serial>\n  \
                 rofi-filesearch thumbnail <input.pdf> <output.png> <size>"
            );
            Ok(())
        }
        Some(argument) => Err(io::Error::other(format!("unknown command {argument:?}")).into()),
    }
}
