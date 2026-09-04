//! Compact player rows for integrations outside the Waybar button.
//!
//! Integrations can run `media-control players`, turn each line into a row with
//! a play/pause button, a progress bar and a volume slider, and call back into
//! this program when one is used. Everything that knows what a player is stays
//! here.

use std::io::{self, Write};

use crate::model::{Player, clock, media_label};
use crate::mpris::{player_command, set_position, set_volume, snapshot};

/// Field separator. `clean_text` collapses every run of whitespace — tabs
/// included — into single spaces, so no field can contain one.
const SEPARATOR: char = '\t';

/// What a missing optional field looks like. MPRIS makes volume, position and
/// length all optional, and browser-tab bridges routinely omit them.
const ABSENT: &str = "-";

/// Print one line per player, most relevant first.
pub(crate) fn print_players() -> Result<(), String> {
    let mut output = io::stdout().lock();
    for player in snapshot() {
        writeln!(output, "{}", row(&player)).map_err(|error| error.to_string())?;
    }
    output.flush().map_err(|error| error.to_string())
}

/// One player as the widget's seven tab-separated fields.
pub(crate) fn row(player: &Player) -> String {
    let fields = [
        player.id.clone(),
        player.status.class().to_owned(),
        optional(player.volume.map(|volume| volume.to_string())),
        optional(player.position.map(|seconds| seconds.round().to_string())),
        optional(player.length.map(|seconds| seconds.round().to_string())),
        media_label(player),
        subtitle(player),
    ];
    fields.join(&SEPARATOR.to_string())
}

fn optional(value: Option<String>) -> String {
    value.unwrap_or_else(|| ABSENT.to_owned())
}

/// The dimmed second line of a row: where it is playing, and how far in.
fn subtitle(player: &Player) -> String {
    let mut parts = vec![player.source.clone()];
    if let (Some(position), Some(length)) = (player.position, player.length) {
        parts.push(format!("{} / {}", clock(position), clock(length)));
    } else if let Some(position) = player.position {
        parts.push(clock(position));
    }
    parts.join("  ·  ")
}

/// Set one player's volume, as the widget's slider asks.
pub(crate) fn volume(arguments: &[String]) -> Result<(), String> {
    let [id, percent] = arguments else {
        return Err("usage: media-control volume <player> <percent>".to_owned());
    };
    let percent: f64 = percent
        .parse()
        .map_err(|_| format!("not a percentage: {percent}"))?;
    set_volume(id, percent)
}

/// Seek one player, as the widget's progress bar asks.
pub(crate) fn seek(arguments: &[String]) -> Result<(), String> {
    let [id, seconds] = arguments else {
        return Err("usage: media-control seek <player> <seconds>".to_owned());
    };
    let seconds: f64 = seconds
        .parse()
        .map_err(|_| format!("not a number of seconds: {seconds}"))?;
    set_position(id, seconds)
}

/// Play or pause one player, as the widget's button asks.
pub(crate) fn play_pause(arguments: &[String]) -> Result<(), String> {
    let [id] = arguments else {
        return Err("usage: media-control play-pause <player>".to_owned());
    };
    player_command(id, "play-pause")
}
