//! The media row inside SwayNC's control center.
//!
//! SwayNC's patched `label` widget runs a command and shows its output as Pango
//! markup (see `niri/swaync/label-exec.patch`), so the panel's media overview is
//! this program rendering the same snapshot the bar button uses — one source of
//! truth for what "the current player" means.

use std::io;

use crate::model::{Player, media_label};
use crate::text::{pango_escape, truncate_display};

/// The palette shared with `themes/swaync.css` and `themes/waybar.css`.
const PINK: &str = "#ff7edb";
const FOREGROUND: &str = "#cbe3e7";
const DIM: &str = "#5c6776";

/// The panel is 380px wide; this is what fits on one line beside the icon.
const PANEL_WIDTH: usize = 34;

pub(crate) fn render() -> Result<(), String> {
    let players = crate::mpris::snapshot();
    println!("{}", markup(players.first()));
    io::Write::flush(&mut io::stdout()).map_err(|error| error.to_string())
}

/// Two lines at most: the track, then who is playing it and how loudly.
pub(crate) fn markup(player: Option<&Player>) -> String {
    let Some(player) = player else {
        return span(DIM, "󰝛  No active media");
    };

    let title = truncate_display(&media_label(player), PANEL_WIDTH);
    let mut lines = format!(
        "{}  {}",
        span(PINK, player.status.icon()),
        span(FOREGROUND, &title),
    );

    let mut detail = player.source.clone();
    if let Some(volume) = player.volume {
        detail.push_str(&format!("  ·  {volume}%"));
    }
    lines.push('\n');
    lines.push_str("   ");
    lines.push_str(&span(DIM, &detail));
    lines
}

fn span(colour: &str, text: &str) -> String {
    format!(
        "<span foreground=\"{colour}\">{}</span>",
        pango_escape(text)
    )
}
