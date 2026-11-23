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
mod tests {
    use super::*;
    use clap::error::ErrorKind;

    #[test]
    fn cli_accepts_no_arguments_and_handles_help_without_starting_backend() {
        assert!(Cli::try_parse_from(["network-control"]).is_ok());
        for argument in ["-h", "--help"] {
            assert_eq!(
                Cli::try_parse_from(["network-control", argument])
                    .unwrap_err()
                    .kind(),
                ErrorKind::DisplayHelp
            );
        }
        assert_eq!(
            Cli::try_parse_from(["network-control", "--version"])
                .unwrap_err()
                .kind(),
            ErrorKind::DisplayVersion
        );
        assert!(Cli::try_parse_from(["network-control", "unexpected"]).is_err());
    }
}
