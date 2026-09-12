//! The command line: the Rofi menu, the Waybar status line, and the small
//! machine interface Wayle's device picker calls.

use std::env;
use std::io;

use audio_control::{AppResult, rofi, waybar, wayle};

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
            // Writes the outcome to $XDG_RUNTIME_DIR/audio-control-connect-result.
            let key = arguments
                .next()
                .ok_or_else(|| io::Error::other("connect-bg key is missing"))?;
            rofi::run_connect_bg(&key).await
        }
        Some("scan-bg") => {
            // Detached Bluetooth discovery; spawned when the menu opens and by
            // the Scan button, so neither ever waits on the discovery window.
            rofi::run_scan_bg().await
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
        Some("help" | "--help" | "-h") => {
            print!(
                "audio-control\n\n\
                 Usage:\n  \
                 audio-control\n  \
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
