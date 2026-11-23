//! Native NetworkManager backend for the network frontend.

mod bus;
mod connection;
mod engine;
mod model;
mod protocol;
mod ssid;
mod transport;

use clap::Parser;

/// Run the NetworkManager JSONL backend on stdin and stdout.
#[derive(Debug, Parser)]
#[command(version)]
struct Cli {}

fn main() {
    Cli::parse();
    std::process::exit(transport::run());
}

#[cfg(test)]
mod tests;
