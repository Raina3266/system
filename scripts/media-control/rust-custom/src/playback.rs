//! Conservative duplicate matching and source names retained from media-panel.
//!
//! The frontend supplies metadata and effective times in seconds; no bus or GUI access
//! is needed to decide whether two publishers describe the same playback.

use std::borrow::Cow;

use crate::model::Player;

const MPRIS_PREFIX: &str = "org.mpris.MediaPlayer2.";
/// Wide enough that reading players a moment apart is not disagreement.
const POSITION_TOLERANCE: f64 = 5.0;

/// Title alone can collide, so two more fields must agree and a differing
/// position splits them. Publishers that already agree on identity need one.
pub(crate) fn same_playback(left: &Player, right: &Player) -> bool {
    same_text(&left.title, &right.title)
        && agreeing_fields(left, right) >= required_agreement(left, right)
        && !positions_disagree(left, right)
}

fn agreeing_fields(left: &Player, right: &Player) -> usize {
    [
        same_text(&left.artist, &right.artist),
        same_text(&left.album, &right.album),
        same_optional(Some(&left.art_url), Some(&right.art_url)),
        same_optional(Some(&left.url), Some(&right.url)),
        near_length(left.length, right.length),
    ]
    .into_iter()
    .filter(|agrees| *agrees)
    .count()
}

fn required_agreement(left: &Player, right: &Player) -> usize {
    if same_text(&source_name(left), &source_name(right)) {
        1
    } else {
        2
    }
}

fn positions_disagree(left: &Player, right: &Player) -> bool {
    left.position != 0.0
        && right.position != 0.0
        && (left.position - right.position).abs() > POSITION_TOLERANCE
}

fn same_text(left: &str, right: &str) -> bool {
    let left = normalised(left);
    !left.is_empty() && left == normalised(right)
}

fn same_optional(left: Option<&str>, right: Option<&str>) -> bool {
    left.zip(right)
        .is_some_and(|(left, right)| same_text(left, right))
}

fn near_length(left: Option<f64>, right: Option<f64>) -> bool {
    // The old MPRIS reader discarded zero/invalid lengths before matching.
    let known = |length: &f64| length.is_finite() && *length > 0.0;
    left.filter(known)
        .zip(right.filter(known))
        .is_some_and(|(left, right)| (left - right).abs() <= 2.0)
}

fn normalised(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

pub(crate) fn source_name(player: &Player) -> Cow<'_, str> {
    if player.identity.trim().is_empty() || echoes_bus_name(&player.identity) {
        Cow::Owned(friendly_source(&player.bus))
    } else {
        Cow::Borrowed(&player.identity)
    }
}

/// A few players echo their dotted bus name rather than publishing a usable
/// `Identity`; those get the same cleanup as a player with no identity at all.
pub(crate) fn echoes_bus_name(identity: &str) -> bool {
    identity.contains('.') && !identity.contains(' ')
}

/// Chromium numbers its instances (`instance1234`) and mprisence gives every
/// tab a `p` and a hex id (`pa8082085c72e81fe`). Neither names an app.
fn is_instance(part: &str) -> bool {
    let digits = |rest: &str| !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit());
    part.strip_prefix("instance").is_some_and(digits)
        || part
            .strip_prefix('p')
            .is_some_and(|rest| !rest.is_empty() && rest.chars().all(|c| c.is_ascii_hexdigit()))
        || digits(part)
}

/// The player's own name, for a bus that publishes no usable `Identity`.
///
/// mprisence names a bus for publisher, site and tab —
/// `mprisence_web.youtube_music.pa8082085c72e81fe` — so drop the publisher and
/// the per-tab id and keep the middle, which names the app.
pub fn friendly_source(bus: &str) -> String {
    let tail = bus.strip_prefix(MPRIS_PREFIX).unwrap_or(bus);
    let mut parts: Vec<&str> = tail.split('.').filter(|part| !part.is_empty()).collect();

    if parts.len() > 1 && parts[0].split('_').next() == Some("mprisence") {
        parts.remove(0);
    }
    if parts.len() > 1 && is_instance(parts[parts.len() - 1]) {
        parts.pop();
    }

    parts
        .join(" ")
        .split(['_', '-', ' '])
        .filter(|word| !word.is_empty())
        .map(|word| {
            let mut characters = word.chars();
            characters.next().map_or_else(String::new, |first| {
                first.to_uppercase().collect::<String>() + characters.as_str()
            })
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
media_unit_tests!(playback);
