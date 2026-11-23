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
mod cli_tests {
    use super::*;
    use clap::error::ErrorKind;

    #[test]
    fn cli_accepts_no_arguments_and_handles_help_without_starting_backend() {
        assert!(Cli::try_parse_from(["dashboard-control"]).is_ok());
        for argument in ["-h", "--help"] {
            assert_eq!(
                Cli::try_parse_from(["dashboard-control", argument])
                    .unwrap_err()
                    .kind(),
                ErrorKind::DisplayHelp
            );
        }
        assert_eq!(
            Cli::try_parse_from(["dashboard-control", "--version"])
                .unwrap_err()
                .kind(),
            ErrorKind::DisplayVersion
        );
        assert!(Cli::try_parse_from(["dashboard-control", "unexpected"]).is_err());
    }
}
