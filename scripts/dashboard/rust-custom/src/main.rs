//! Native telemetry, idle policy, and notification backend for the dashboard.

mod idle;
mod metrics;
mod notifications;
mod protocol;
mod service;
mod storage;

#[cfg(test)]
mod tests;

use clap::Parser;

/// Run the dashboard JSONL backend on stdin and stdout.
#[derive(Debug, Parser)]
#[command(version)]
struct Cli {}

fn main() {
    Cli::parse();
    std::process::exit(service::run());
}

#[cfg(test)]
crate::tests::cli_tests!();
