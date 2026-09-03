//! The Do Not Disturb pill's command.
//!
//! SwayNC hands a toggle button's new state to its command in the environment,
//! so this sets Do Not Disturb to exactly that state rather than toggling blind
//! and hoping the button and the daemon still agree.

use std::env;
use std::process::Command;

use crate::executable;

pub(crate) fn apply() -> Result<(), String> {
    let wanted = env::var("SWAYNC_TOGGLE_STATE").unwrap_or_default() == "true";
    let flag = if wanted { "--dnd-on" } else { "--dnd-off" };

    let status = Command::new(executable("SWAYNC_PANEL_SWAYNC_CLIENT", "swaync-client"))
        .arg(flag)
        .arg("--skip-wait")
        .status()
        .map_err(|error| format!("could not run swaync-client: {error}"))?;

    if status.success() {
        Ok(())
    } else {
        Err(format!("swaync-client {flag} failed: {status}"))
    }
}
