use std::time::Duration;

use super::{clock, friendly_source, Player, Status};

#[test]
fn a_clock_grows_an_hours_field_only_when_it_needs_one() {
    assert_eq!(clock(Duration::from_secs(0)), "0:00");
    assert_eq!(clock(Duration::from_secs(9)), "0:09");
    assert_eq!(clock(Duration::from_secs(83)), "1:23");
    assert_eq!(clock(Duration::from_secs(3_599)), "59:59");
    assert_eq!(clock(Duration::from_secs(3_600)), "1:00:00");
}

#[test]
fn a_bus_name_reads_as_the_player_a_person_would_name() {
    assert_eq!(friendly_source("org.mpris.MediaPlayer2.spotify"), "Spotify");
    assert_eq!(
        friendly_source("org.mpris.MediaPlayer2.chromium.instance1234"),
        "Chromium"
    );
    assert_eq!(friendly_source("org.mpris.MediaPlayer2.vlc"), "Vlc");
}

#[test]
fn progress_is_a_fraction_of_the_track_and_never_leaves_the_bar() {
    let player = Player {
        position: Duration::from_secs(30),
        length: Duration::from_secs(120),
        ..Player::default()
    };
    assert!((player.progress() - 0.25).abs() < f64::EPSILON);
}

#[test]
fn a_track_with_no_length_reports_no_progress_rather_than_dividing_by_zero() {
    let player = Player {
        position: Duration::from_secs(30),
        length: Duration::ZERO,
        ..Player::default()
    };
    assert_eq!(player.progress(), 0.0);
    assert!(!player.has_length());
}

#[test]
fn a_position_past_the_end_still_fills_the_bar_exactly_once() {
    let player = Player {
        position: Duration::from_secs(500),
        length: Duration::from_secs(120),
        ..Player::default()
    };
    assert_eq!(player.progress(), 1.0);
}

#[test]
fn only_a_playing_player_shows_the_pause_glyph() {
    assert_eq!(Status::Playing.icon(), "󰏤");
    assert_eq!(Status::Paused.icon(), "󰐊");
    assert_eq!(Status::Stopped.icon(), "󰐊");
    assert!(Status::Playing.is_playing());
    assert!(!Status::Paused.is_playing());
}
