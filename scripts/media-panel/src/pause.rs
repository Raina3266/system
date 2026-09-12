//! Stopping everything at once, for the Waybar button's right-click.
//!
//! Not `playerctl --all-players pause`: that reads each player's `CanPause`
//! first and skips the ones that answer no, so a browser bridge that cannot
//! reach its tab is never even asked and the music it publishes keeps playing.
//! MPRIS tells a player that cannot honour a command to ignore it rather than
//! fail, and publishers get the property wrong often enough, so asking every
//! player and letting those that mean it decline stops more music than
//! trusting what they advertise.

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
