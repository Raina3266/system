//! `swaync-panel` — everything SwayNC's control center asks a program for.
//!
//! Three of the panel's rows are driven by a command rather than by SwayNC
//! itself (see `niri/swaync/label-exec.patch`), and its Do Not Disturb pill
//! hands its new state to one. Rather than spread those across a shell script,
//! a `jq` program and a second binary, they are subcommands here:
//!
//! | Subcommand | Row |
//! | ---------- | --- |
//! | `sysmon`   | CPU, memory, temperature, disk and network |
//! | `calendar` | Today's Google Calendar events and Tasks |
//! | `dnd`      | The Do Not Disturb toggle's command |

mod calendar;
mod dnd;
mod format;
mod parse;
mod state;
mod sysmon;

use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "\
usage: swaync-panel <sysmon [--plain] | calendar [--plain] | dnd>

Renders one row of the SwayNC control center. `sysmon` and `calendar` print
Pango markup unless --plain is given; `dnd` sets Do Not Disturb to the state
SwayNC puts in SWAYNC_TOGGLE_STATE.

environment:
  SWAYNC_PANEL_PROC            /proc replacement (default: /proc)
  SWAYNC_PANEL_SYS             /sys replacement (default: /sys)
  SWAYNC_PANEL_DF              df executable (default: df)
  SWAYNC_PANEL_THERMAL_ZONE    thermal zone index or sensor path (default: auto)
  SWAYNC_PANEL_DISK            filesystem to report (default: /)
  SWAYNC_PANEL_INTERFACE       network interface (default: the default route's)
  SWAYNC_PANEL_STATE           counter state file
  SWAYNC_PANEL_CALENDAR        waybar-ycal's event cache
  SWAYNC_PANEL_SWAYNC_CLIENT   swaync-client executable (default: swaync-client)
";

fn main() -> ExitCode {
    let arguments: Vec<String> = env::args().skip(1).collect();
    let plain = arguments.iter().any(|argument| argument == "--plain");

    let result = match arguments.first().map(String::as_str) {
        Some("sysmon") => {
            sysmon::render(!plain);
            Ok(())
        }
        Some("calendar") => {
            calendar::render(!plain);
            Ok(())
        }
        Some("dnd") => dnd::apply(),
        Some("-h" | "--help") | None => {
            print!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Some(other) => Err(format!("unknown command: {other}")),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("swaync-panel: {message}\n\n{USAGE}");
            ExitCode::FAILURE
        }
    }
}

/// A path from the environment, or a fallback when it is unset or empty.
pub(crate) fn path_from_environment(variable: &str, fallback: &str) -> PathBuf {
    env::var_os(variable)
        .filter(|value| !value.is_empty())
        .map_or_else(|| PathBuf::from(fallback), PathBuf::from)
}

/// A setting from the environment, treating empty as unset.
pub(crate) fn non_empty(variable: &str) -> Option<String> {
    env::var(variable).ok().filter(|value| !value.is_empty())
}

/// An executable path from the environment, or the bare name to search `PATH`.
pub(crate) fn executable(variable: &str, fallback: &str) -> String {
    non_empty(variable).unwrap_or_else(|| fallback.to_owned())
}
