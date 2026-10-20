//! Conservative duplicate matching and source names retained from media-panel.
//!
//! Quickshell supplies the metadata and times in seconds; no bus or GUI access
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
mod tests {
    use super::*;

    fn player(bus: &str, source: &str, title: &str, artist: &str, album: &str) -> Player {
        Player {
            bus: String::from(bus),
            identity: String::from(source),
            title: String::from(title),
            artist: String::from(artist),
            album: String::from(album),
            length: Some(203.0),
            playing: true,
            ..Player::default()
        }
    }

    fn elisa() -> Player {
        player(
            "org.mpris.MediaPlayer2.elisa",
            "Elisa",
            "Tomboy",
            "Destiny Rogers",
            "Tomboy",
        )
    }

    #[test]
    fn one_app_publishing_itself_twice_needs_one_further_field() {
        let first = player(
            "org.mpris.MediaPlayer2.mprisence_web.youtube_music.pa808",
            "YouTube Music",
            "Back To Sleep",
            "Chris Brown",
            "Royalty (Deluxe Version)",
        );
        let arriving = Player {
            bus: String::from("org.mpris.MediaPlayer2.mprisence_web.youtube_music.p2911"),
            album: String::new(),
            length: None,
            ..first.clone()
        };

        assert!(same_playback(&first, &arriving));
    }

    #[test]
    fn a_mirror_naming_itself_differently_is_asked_for_more() {
        let elisa = elisa();
        let thin_mirror = Player {
            bus: String::from("org.mpris.MediaPlayer2.other"),
            identity: String::from("YouTube Music"),
            album: String::new(),
            length: None,
            ..elisa.clone()
        };
        assert!(!same_playback(&elisa, &thin_mirror));

        let full_mirror = Player {
            album: elisa.album.clone(),
            length: elisa.length,
            ..thin_mirror
        };
        assert!(same_playback(&elisa, &full_mirror));
    }

    #[test]
    fn different_tracks_stay_apart() {
        assert!(!same_playback(
            &elisa(),
            &player(
                "org.mpris.MediaPlayer2.mprisence_web.youtube_music.pa808",
                "YouTube Music",
                "ANGEL (Visualizer)",
                "keshi",
                "",
            ),
        ));
    }

    #[test]
    fn a_shared_title_alone_is_not_enough() {
        let left = player("a", "Elisa", "Everlong", "Foo Fighters", "The Colour");
        let right = Player {
            bus: String::from("b"),
            identity: String::from("Chromium"),
            artist: String::new(),
            album: String::new(),
            length: Some(600.0),
            ..left.clone()
        };

        assert!(!same_playback(&left, &right));
    }

    #[test]
    fn two_players_at_different_points_are_separate() {
        let early = Player {
            position: 6.0,
            ..elisa()
        };
        let late = Player {
            bus: String::from("b"),
            position: 120.0,
            ..elisa()
        };

        assert!(!same_playback(&early, &late));
    }

    #[test]
    fn an_unread_position_of_zero_is_not_disagreement() {
        let watched = Player {
            position: 92.0,
            ..elisa()
        };
        let unread = Player {
            bus: String::from("b"),
            ..elisa()
        };

        assert!(same_playback(&watched, &unread));
        assert!(same_playback(&unread, &watched));
    }

    #[test]
    fn position_tolerance_is_five_seconds_inclusive() {
        let left = Player {
            position: 10.0,
            ..elisa()
        };
        let mut right = left.clone();
        right.position = 15.0;
        assert!(same_playback(&left, &right));
        right.position = 15.001;
        assert!(!same_playback(&left, &right));
    }

    #[test]
    fn an_untitled_player_never_merges() {
        for title in ["", " \t\n "] {
            let blank = Player {
                title: String::from(title),
                ..elisa()
            };
            assert!(!same_playback(&blank, &blank));
        }
    }

    #[test]
    fn metadata_and_source_matching_normalise_whitespace_and_case() {
        let left = elisa();
        let right = Player {
            identity: String::from(" ELISA\t"),
            title: String::from(" \tTOMBOY\n "),
            artist: String::from(" destiny   ROGERS "),
            album: String::new(),
            length: None,
            ..left.clone()
        };
        assert!(same_playback(&left, &right));
    }

    #[test]
    fn each_nonempty_supporting_field_can_supply_the_agreement() {
        let mut left = player("a", "App", "Track", "", "");
        left.length = None;
        let mut right = left.clone();
        right.bus = String::from("b");
        assert!(!same_playback(&left, &right));

        for field in ["artist", "album", "art", "url", "length"] {
            let mut left = left.clone();
            let mut right = right.clone();
            match field {
                "artist" => {
                    left.artist = String::from(" Artist ");
                    right.artist = String::from("ARTIST");
                }
                "album" => {
                    left.album = String::from(" Album ");
                    right.album = String::from("ALBUM");
                }
                "art" => {
                    left.art_url = String::from(" https://example/cover.jpg ");
                    right.art_url = String::from("https://example/cover.jpg");
                }
                "url" => {
                    left.url = String::from(" https://example/track ");
                    right.url = String::from("https://example/track");
                }
                "length" => {
                    left.length = Some(203.0);
                    right.length = Some(205.0);
                }
                _ => unreachable!(),
            }
            assert!(same_playback(&left, &right), "same-source {field}");
            right.identity = String::from("Other app");
            assert!(!same_playback(&left, &right), "cross-source {field}");
        }
    }

    #[test]
    fn matching_support_does_not_require_artist_equality() {
        let left = elisa();
        let right = Player {
            identity: String::from("Other app"),
            artist: String::from("Different artist"),
            ..left.clone()
        };
        // Album and length agree even though artist does not.
        assert!(same_playback(&left, &right));
    }

    #[test]
    fn unknown_text_or_length_never_counts_as_agreement() {
        assert!(!same_optional(None, None));
        assert!(!same_optional(Some("cover"), None));
        assert!(!same_optional(Some(" \t"), Some("\n")));
        for length in [
            None,
            Some(0.0),
            Some(-1.0),
            Some(f64::NAN),
            Some(f64::INFINITY),
        ] {
            assert!(!near_length(length, length));
            assert!(!near_length(Some(203.0), length));
        }
        assert!(near_length(Some(203.0), Some(205.0)));
        assert!(!near_length(Some(203.0), Some(205.001)));
    }

    #[test]
    fn friendly_source_drops_a_chromium_instance_suffix() {
        assert_eq!(
            friendly_source("org.mpris.MediaPlayer2.chromium.instance1234"),
            "Chromium"
        );
        assert_eq!(friendly_source("org.mpris.MediaPlayer2.elisa"), "Elisa");
    }

    #[test]
    fn friendly_source_names_the_app_behind_an_mprisence_tab() {
        assert_eq!(
            friendly_source("org.mpris.MediaPlayer2.mprisence_web.spotify.p2911"),
            "Spotify"
        );
        assert_eq!(
            friendly_source("org.mpris.MediaPlayer2.mprisence_web.youtube_music.pa8082085c72e81fe"),
            "Youtube Music"
        );
        // An app segment that merely starts with `p` is not a tab id.
        assert_eq!(
            friendly_source("org.mpris.MediaPlayer2.mprisence.pandora"),
            "Pandora"
        );
    }

    #[test]
    fn a_published_identity_wins_unless_it_is_the_bus_name() {
        assert!(!echoes_bus_name("YouTube Music"));
        assert!(!echoes_bus_name("Elisa"));
        assert!(!echoes_bus_name("VLC media player"));
        assert!(echoes_bus_name("mprisence_web.spotify.p2911"));

        let mut player = elisa();
        player.identity = String::from("VLC media player");
        assert_eq!(source_name(&player), "VLC media player");
        for identity in ["", " \t ", "org.mpris.MediaPlayer2.elisa"] {
            player.identity = String::from(identity);
            assert_eq!(source_name(&player), "Elisa");
        }
    }

    #[test]
    fn unusable_identities_use_friendly_names_for_agreement_too() {
        let left = Player {
            bus: String::from("org.mpris.MediaPlayer2.mprisence_web.youtube_music.pa808"),
            identity: String::new(),
            ..elisa()
        };
        let right = Player {
            bus: String::from("org.mpris.MediaPlayer2.mprisence_web.youtube_music.p2911"),
            identity: String::from("mprisence_web.youtube_music.p2911"),
            album: String::new(),
            length: None,
            ..left.clone()
        };
        assert!(same_playback(&left, &right));
    }

    #[test]
    fn only_actual_instance_suffixes_are_removed() {
        for suffix in ["instance1234", "pa8082085c72e81fe", "1234"] {
            assert!(is_instance(suffix), "{suffix}");
        }
        for suffix in ["instance", "instanceabc", "p", "pandora", "p12g", ""] {
            assert!(!is_instance(suffix), "{suffix}");
        }
        assert_eq!(
            friendly_source("org.mpris.MediaPlayer2.player.1234"),
            "Player"
        );
        assert_eq!(
            friendly_source("org.mpris.MediaPlayer2.my-player"),
            "My Player"
        );
    }
}
