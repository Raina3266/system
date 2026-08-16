use std::env;
use std::error::Error;
use std::io;

mod audio;
mod bluetooth;
mod model;
mod rofi;
mod waybar;

pub type AppError = Box<dyn Error + Send + Sync>;
pub type AppResult<T> = Result<T, AppError>;

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("rofi-audio: {error}");
        std::process::exit(2);
    }
}

async fn run() -> AppResult<()> {
    let mut arguments = env::args().skip(1);
    match arguments.next().as_deref() {
        None | Some("launch") => rofi::launch(),
        Some("script") => {
            let mode = arguments
                .next()
                .ok_or_else(|| io::Error::other("script mode is missing"))?
                .parse()?;
            rofi::run_script(mode).await
        }
        Some("connect-bg") => {
            // Detached background pair-and-connect; spawned by the Bluetooth
            // tab so the rofi script can render "Connecting…" immediately and
            // so the BlueZ pairing agent outlives that script invocation.
            // Writes the outcome to $XDG_RUNTIME_DIR/rofi-audio-connect-result.
            let key = arguments
                .next()
                .ok_or_else(|| io::Error::other("connect-bg key is missing"))?;
            rofi::run_connect_bg(&key).await
        }
        Some("status") => {
            waybar::print_status().await;
            Ok(())
        }
        Some("bluetooth-power") => {
            let argument = arguments.next().unwrap_or_else(|| "toggle".to_owned());
            waybar::set_bluetooth_power(&argument).await?;
            Ok(())
        }
        Some("help" | "--help" | "-h") => {
            print!(
                "rofi-audio\n\n\
                 Usage:\n  \
                 rofi-audio\n  \
                 rofi-audio status\n  \
                 rofi-audio bluetooth-power [on|off|toggle]\n"
            );
            Ok(())
        }
        Some(command) => Err(io::Error::other(format!("unknown command {command:?}")).into()),
    }
}
