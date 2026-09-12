//! The active Wayle media title streamed to Waybar, and the panic button that
//! stops every player.

use std::env;
use std::process::ExitCode;

mod media_badge;
mod mpris;

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
        Some("media-pause-all") => match mpris::pause_all() {
            Ok(outcomes) => {
                for outcome in &outcomes {
                    let state = if outcome.paused { "paused" } else { "still playing" };
                    println!("{}: {state}", outcome.bus_name);
                }
                if outcomes.is_empty() {
                    println!("no MPRIS players on the bus");
                }
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("control-centre: {error}");
                ExitCode::FAILURE
            }
        },
        Some("media-players") => match mpris::describe() {
            Ok(players) => {
                if players.is_empty() {
                    println!("no MPRIS players on the bus");
                }
                for player in &players {
                    println!("{}", player.summary());
                }
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("control-centre: {error}");
                ExitCode::FAILURE
            }
        },
        Some("help" | "--help" | "-h") | None => {
            println!(
                "control-centre\n\n\
                 Usage:\n  \
                 control-centre media-waybar\n  \
                 control-centre media-pause-all\n  \
                 control-centre media-players\n\n\
                 media-waybar streams Wayle's active media title as Waybar JSON.\n\
                 media-pause-all asks every MPRIS player to pause, including the\n\
                 ones that claim they cannot.\n\
                 media-players prints what every player on the bus advertises,\n\
                 including whether it says it can be controlled at all."
            );
            ExitCode::SUCCESS
        }
        Some(other) => {
            eprintln!("control-centre: unknown command: {other}");
            ExitCode::FAILURE
        }
    }
}
