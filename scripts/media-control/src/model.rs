use std::cmp::Ordering;

use crate::text::truncate_display;

const DISPLAY_WIDTH: usize = 60;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PlaybackStatus {
    Playing,
    Paused,
}

impl PlaybackStatus {
    pub(crate) fn is_playing(self) -> bool {
        matches!(self, Self::Playing)
    }

    pub(crate) fn class(self) -> &'static str {
        match self {
            Self::Playing => "playing",
            Self::Paused => "paused",
        }
    }

    pub(crate) fn icon(self) -> &'static str {
        match self {
            Self::Playing => "󰐊",
            Self::Paused => "󰏤",
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Player {
    pub(crate) id: String,
    pub(crate) source: String,
    pub(crate) title: String,
    pub(crate) artist: String,
    pub(crate) status: PlaybackStatus,
    pub(crate) volume: Option<u8>,
    /// How far into the track playback is, in seconds. MPRIS makes both this
    /// and the length optional, and browser-tab bridges often omit them.
    pub(crate) position: Option<f64>,
    pub(crate) length: Option<f64>,
    pub(crate) pinned: bool,
    pub(crate) activity: u128,
}

/// `1:23` below an hour, `1:02:03` above it.
pub(crate) fn clock(seconds: f64) -> String {
    let total = seconds.max(0.0).round() as u64;
    let (hours, minutes, seconds) = (total / 3600, (total % 3600) / 60, total % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct PlayerState {
    pub(crate) pinned: bool,
    pub(crate) was_playing: bool,
    pub(crate) activity: u128,
}

pub(crate) fn compare_players(left: &Player, right: &Player) -> Ordering {
    right
        .pinned
        .cmp(&left.pinned)
        .then_with(|| right.status.is_playing().cmp(&left.status.is_playing()))
        .then_with(|| right.activity.cmp(&left.activity))
        .then_with(|| left.id.cmp(&right.id))
}

pub(crate) fn media_label(player: &Player) -> String {
    if player.artist.is_empty() {
        player.title.clone()
    } else {
        format!("{} — {}", player.title, player.artist)
    }
}

pub(crate) fn row_text(player: &Player) -> String {
    let pin = if player.pinned { "󰐃 " } else { "" };
    format!(
        "{}{} {} {{{}}} {}",
        pin,
        player.status.icon(),
        volume_label(player.volume),
        player.source,
        truncate_display(&media_label(player), DISPLAY_WIDTH)
    )
}

pub(crate) fn volume_label(volume: Option<u8>) -> String {
    volume
        .map(|value| format!("{value:>3}%"))
        .unwrap_or_else(|| "  --".to_owned())
}
