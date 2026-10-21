//! Supplemental policy/agent helper. Astal owns all native controls and status.

use std::{env, io};

use audio_control::{AppResult, bluetooth, policy};

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("audio-control: {error}");
        std::process::exit(2);
    }
}

async fn run() -> AppResult<()> {
    let arguments: Vec<_> = env::args().skip(1).collect();
    match arguments.first().map(String::as_str) {
        Some("policy") if arguments.len() == 1 => policy::run(),
        Some("bluetooth-pair") if arguments.len() == 3 => {
            bluetooth::pair(&arguments[1], &arguments[2]).await
        }
        Some("battery-provider") if arguments.len() == 1 => {
            bluetooth::battery_provider::run().await
        }
        None | Some("help" | "--help" | "-h") if arguments.len() <= 1 => {
            print!(
                "audio-control\n\nUsage:\n  \
                audio-control policy (one JSON request on stdin; JSON result/RPC lines on stdout)\n  \
                audio-control bluetooth-pair <adapter> <address> (hciN and AA:BB:CC:DD:EE:FF)\n  \
                audio-control battery-provider (bt-battery-provider.service)\n"
            );
            Ok(())
        }
        Some("policy" | "bluetooth-pair" | "battery-provider" | "help" | "--help" | "-h") => {
            Err(io::Error::other("invalid arguments; use audio-control --help").into())
        }
        Some(command) => Err(io::Error::other(format!("unknown command {command:?}")).into()),
        None => unreachable!(),
    }
}
