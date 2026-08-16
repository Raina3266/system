use std::env;
use std::error::Error;
use std::io;

use nmrs::NetworkManager;

mod model;
mod network;
mod preview;
mod rofi;

pub type AppError = Box<dyn Error + Send + Sync>;
pub type AppResult<T> = Result<T, AppError>;

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("rofi-network-manager: {error}");
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
            let manager = NetworkManager::new().await?;
            rofi::run_script(&manager, mode).await
        }
        Some("preview-selection") => {
            let key = arguments
                .next()
                .ok_or_else(|| io::Error::other("preview selection is missing"))?;
            let serial = arguments
                .next()
                .ok_or_else(|| io::Error::other("preview serial is missing"))?
                .parse::<u64>()?;
            if !preview::is_open() {
                return Ok(());
            }
            let manager = NetworkManager::new().await?;
            rofi::update_preview_selection(&manager, &key, serial).await
        }
        Some("connect-bg") => {
            // Detached background connect; spawned by `submit_password` so the
            // rofi script can render "Connecting…" immediately. Writes the
            // outcome to $XDG_RUNTIME_DIR/rofi-network-connect-result.
            let hex_key = arguments
                .next()
                .ok_or_else(|| io::Error::other("connect-bg key is missing"))?;
            let hex_password = arguments
                .next()
                .ok_or_else(|| io::Error::other("connect-bg password is missing"))?;
            rofi::run_connect_bg(&hex_key, &hex_password).await
        }
        Some("status") => {
            match NetworkManager::new().await {
                Ok(manager) => network::print_waybar_status(&manager).await,
                Err(error) => {
                    println!(
                        "{{\"text\":\"󰤭\",\"tooltip\":\"{}\",\"class\":\"offline\"}}",
                        network::json_escape(&format!("NetworkManager unavailable: {error}"))
                    );
                }
            }
            Ok(())
        }
        Some("help" | "--help" | "-h") => {
            print!(
                "rofi-network-manager\n\n\
                 Usage:\n  rofi-network-manager\n  rofi-network-manager status\n"
            );
            Ok(())
        }
        Some(command) => Err(io::Error::other(format!("unknown command {command:?}")).into()),
    }
}
