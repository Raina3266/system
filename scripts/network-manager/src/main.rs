use std::env;
use std::error::Error;
use std::io;

use nmrs::NetworkManager;

mod model;
mod network;
mod wayle;

#[cfg(test)]
mod tests;

pub type AppError = Box<dyn Error + Send + Sync>;
pub type AppResult<T> = Result<T, AppError>;

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("network-manager: {error}");
        std::process::exit(2);
    }
}

async fn run() -> AppResult<()> {
    let mut arguments = env::args().skip(1);
    match arguments.next().as_deref() {
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
        Some("wayle-info") => {
            let ssid = arguments
                .next()
                .ok_or_else(|| io::Error::other("wayle-info SSID is missing"))?;
            let manager = NetworkManager::new().await?;
            wayle::print_wifi_info(&manager, &ssid).await
        }
        Some("wayle-qr") => {
            let ssid = arguments
                .next()
                .ok_or_else(|| io::Error::other("wayle-qr SSID is missing"))?;
            let manager = NetworkManager::new().await?;
            wayle::write_wifi_qr(&manager, &ssid).await
        }
        None | Some("help" | "--help" | "-h") => {
            print!(
                "network-manager\n\n\
                 Usage:\n  network-manager status\n\
                 \nWayle bridge:\n  network-manager wayle-info <SSID>\n  network-manager wayle-qr <SSID>\n"
            );
            Ok(())
        }
        Some(command) => Err(io::Error::other(format!("unknown command {command:?}")).into()),
    }
}
