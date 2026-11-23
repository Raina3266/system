//! A countdown timer for Waybar's centre button, driven over a local socket.

mod ipc;
mod model;

#[cfg(test)]
mod tests;

use std::process;

use clap::{Parser, Subcommand};

use crate::model::Timer;

/// A countdown timer for Waybar's centre button.
#[derive(Debug, Parser)]
#[command(name = "waybar-timer")]
struct Cli {
    #[command(subcommand)]
    action: Option<Action>,
}

#[derive(Debug, Default, Eq, PartialEq, Subcommand)]
enum Action {
    /// Run the countdown service (the default).
    #[default]
    Daemon,
    /// Print Waybar JSON.
    Status,
    /// Add five minutes.
    Add,
    /// Start or pause.
    Toggle,
    /// Stop and clear.
    #[command(visible_alias = "stop")]
    Clear,
}

/// The status line Waybar polls.
pub(crate) const ICON: &str = "<span size='large'>󰔛</span>";

pub(crate) fn status_json(timer: &Timer) -> String {
    let total_seconds = timer.displayed_seconds();

    let text = if total_seconds == 0 {
        ICON.to_string()
    } else {
        format!("{ICON} {:02}:{:02}", total_seconds / 60, total_seconds % 60)
    };

    format!(
        r#"{{"text":"{text}","tooltip":"Timer","class":"{}"}}"#,
        timer.state().output()
    )
}

fn main() {
    let result = match Cli::parse().action.unwrap_or_default() {
        Action::Daemon => ipc::run_server(),
        Action::Status => ipc::print_status(),
        Action::Add => ipc::send_command("add"),
        Action::Toggle => ipc::send_command("toggle"),
        Action::Clear => ipc::send_command("clear"),
    };

    if let Err(error) = result {
        eprintln!("waybar-timer: {error}");
        process::exit(1);
    }
}
