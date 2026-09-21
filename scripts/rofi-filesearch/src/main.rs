use std::env;
use std::error::Error;
use std::path::Path;

mod desktop;
mod model;
mod rofi;
mod search;

#[cfg(test)]
mod tests;

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
        Some("thumbnail") => {
            let input = arguments
                .next()
                .ok_or_else(|| std::io::Error::other("thumbnail input path is missing"))?;
            let output = arguments
                .next()
                .ok_or_else(|| std::io::Error::other("thumbnail output path is missing"))?;
            let size = arguments
                .next()
                .and_then(|value| value.to_str().and_then(|value| value.parse::<u32>().ok()))
                .ok_or_else(|| std::io::Error::other("thumbnail size is missing or invalid"))?;
            rofi_preview_shared::file_preview::render_pdf_thumbnail(
                Path::new(input.as_os_str()),
                Path::new(output.as_os_str()),
                size,
                rofi::pdftoppm_binary().as_os_str(),
            )
            .map_err(Into::into)
        }
        Some("--help" | "-h") => {
            println!(
                "rofi-filesearch\n\n\
                 USAGE:\n  \
                 rofi-filesearch\n  \
                 rofi-filesearch thumbnail <input.pdf> <output.png> <size>"
            );
            Ok(())
        }
        Some(argument) => {
            Err(std::io::Error::other(format!("unknown command {argument:?}")).into())
        }
    }
}
