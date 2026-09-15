//! Stopping everything at once, for the Waybar button's right-click.
//!
//! Not `playerctl --all-players pause`: that checks `CanPause` first and skips
//! players answering no, so a browser bridge that cannot reach its tab is never
//! asked. MPRIS says a player that cannot comply should ignore the call, and
//! publishers get the property wrong often, so ask everyone and let them
//! decline.

use std::fmt::Write as _;

use crate::mpris::{Players, Status};

/// Pauses every player, and reports the ones still going afterwards.
pub fn everything() -> Result<String, String> {
    let players = Players::connect()?;
    let snapshot = players.snapshot();

    if snapshot.is_empty() {
        return Ok(String::from("no MPRIS players on the bus\n"));
    }

    for player in &snapshot {
        players.set_playing(player, false);
    }

    let mut report = String::new();
    for player in players.snapshot() {
        let state = if player.status == Status::Playing {
            "still playing"
        } else {
            "paused"
        };
        let _ = writeln!(report, "{}: {state}", player.bus);
    }
    Ok(report)
}
