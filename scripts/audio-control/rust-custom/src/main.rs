//! Supplemental policy/agent helper. Astal owns all native controls and status.

use clap::{CommandFactory, Parser, Subcommand};

use audio_control::{AppResult, bluetooth, policy};

#[derive(Parser)]
#[command(
    name = "audio-control",
    about = "Audio/Bluetooth policy and agents for the Astal panel"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<AudioCommand>,
}

#[derive(Subcommand)]
enum AudioCommand {
    /// Resident protocol v1: JSONL snapshots/intents, guarded native RPCs and models
    Panel,
    /// One JSON request on stdin; JSON result/RPC lines on stdout
    Policy,
    /// Pair an explicit hciN adapter and AA:BB:CC:DD:EE:FF address
    BluetoothPair {
        #[arg(value_name = "adapter")]
        adapter: String,
        #[arg(value_name = "address")]
        address: String,
    },
    /// Export HID++ battery levels for bt-battery-provider.service
    BatteryProvider,
}

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("audio-control: {error}");
        std::process::exit(2);
    }
}

async fn run() -> AppResult<()> {
    match Cli::parse().command {
        Some(AudioCommand::Policy) => policy::run(),
        Some(AudioCommand::Panel) => policy::panel::run(),
        Some(AudioCommand::BluetoothPair { adapter, address }) => {
            bluetooth::pair(&adapter, &address).await
        }
        Some(AudioCommand::BatteryProvider) => bluetooth::battery_provider::run().await,
        None => {
            Cli::command().print_help()?;
            Ok(())
        }
    }
}
