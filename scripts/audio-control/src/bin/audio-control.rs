//! The command line: the Waybar status line and the small machine interface
//! Wayle's device picker calls. The panel is the other binary in this crate.

use std::env;
use std::io;

use audio_control::{AppResult, waybar, wayle};

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("audio-control: {error}");
        std::process::exit(2);
    }
}

async fn run() -> AppResult<()> {
    let mut arguments = env::args().skip(1);
    match arguments.next().as_deref() {
        Some("status") => {
            waybar::print_status().await;
            Ok(())
        }
        Some("bluetooth-power") => {
            let argument = arguments.next().unwrap_or_else(|| "toggle".to_owned());
            waybar::set_bluetooth_power(&argument).await?;
            Ok(())
        }
        Some("wayle-list") => {
            let kind = arguments
                .next()
                .ok_or_else(|| io::Error::other("wayle-list kind is missing"))?;
            wayle::list(&kind)
        }
        Some("wayle-set-default") => {
            let kind = arguments
                .next()
                .ok_or_else(|| io::Error::other("wayle-set-default kind is missing"))?;
            let key = arguments
                .next()
                .ok_or_else(|| io::Error::other("wayle-set-default key is missing"))?;
            wayle::set_default(&kind, &key)
        }
        None | Some("help" | "--help" | "-h") => {
            print!(
                "audio-control\n\n\
                 Usage:\n  \
                 audio-control status\n  \
                 audio-control bluetooth-power [on|off|toggle]\n  \
                 audio-control wayle-list <output|input>\n  \
                 audio-control wayle-set-default <output|input> <key>\n"
            );
            Ok(())
        }
        Some(command) => Err(io::Error::other(format!("unknown command {command:?}")).into()),
    }
}
