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

    fn icon(self) -> &'static str {
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
    pub(crate) pinned: bool,
    pub(crate) activity: u128,
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
