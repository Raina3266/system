//! The active Wayle media title streamed to Waybar.

use std::env;
use std::process::ExitCode;

mod media_badge;

fn main() -> ExitCode {
    let mut arguments = env::args().skip(1);
    match arguments.next().as_deref() {
        Some("media-waybar") => match media_badge::watch() {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("control-centre: {error}");
                ExitCode::FAILURE
            }
        },
        Some("help" | "--help" | "-h") | None => {
            println!(
                "control-centre\n\n\
                 Usage:\n  control-centre media-waybar\n\n\
                 Streams Wayle's active media title as Waybar JSON.\n\
                 Pausing everything and inspecting the bus moved to\n\
                 `media-panel pause-all` and `media-panel players`."
            );
            ExitCode::SUCCESS
        }
        Some(other) => {
            eprintln!("control-centre: unknown command: {other}");
            ExitCode::FAILURE
        }
    }
}
