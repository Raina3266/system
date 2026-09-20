//! A countdown timer for Waybar's centre button, driven over a local socket.

mod ipc;
mod model;

#[cfg(test)]
mod tests;

use std::env;
use std::process;

use crate::model::Timer;

/// The status line Waybar polls.
pub(crate) const ICON: &str = "<span size='large'>󰔛</span>";

pub(crate) fn status_json(timer: &Timer) -> String {
    let total_seconds = timer.displayed_seconds();

    let minutes = total_seconds / 60;
    let seconds = total_seconds % 60;
    let time = format!("{minutes:02}:{seconds:02}");

    let text = if total_seconds == 0 {
        ICON.to_string()
    } else {
        format!("{ICON} {time}")
    };

    format!(
        r#"{{"text":"{text}","tooltip":"Timer","class":"{}"}}"#,
        timer.state().output()
    )
}

fn print_help(program: &str) {
    eprintln!(
        "Usage:\n  {program} daemon   Run the countdown service\n  {program} status   Print Waybar JSON\n  {program} add      Add five minutes\n  {program} toggle   Start or pause\n  {program} clear    Stop and clear"
    );
}

fn main() {
    let mut args = env::args();
    let program = args.next().unwrap_or_else(|| "waybar-timer".to_string());

    let result = match args.next().as_deref() {
        None | Some("daemon") => ipc::run_server(),
        Some("status") => ipc::print_status(),
        Some("add") => ipc::send_command("add"),
        Some("toggle") => ipc::send_command("toggle"),
        Some("clear" | "stop") => ipc::send_command("clear"),
        Some("-h" | "--help" | "help") => {
            print_help(&program);
            return;
        }
        Some(other) => {
            eprintln!("waybar-timer: unknown argument: {other}");
            print_help(&program);
            process::exit(2);
        }
    };

    if let Err(error) = result {
        eprintln!("waybar-timer: {error}");
        process::exit(1);
    }
}
