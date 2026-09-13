use std::time::Duration;

use super::{
    Player, Status, clock, echoes_the_bus_name, friendly_source, merge_duplicates, same_playback,
};

fn player(bus: &str, source: &str, title: &str, artist: &str, album: &str) -> Player {
    Player {
        bus: String::from(bus),
        source: String::from(source),
        title: String::from(title),
        artist: String::from(artist),
        album: String::from(album),
        length: Some(Duration::from_secs(203)),
        status: Status::Playing,
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
fn one_app_publishing_itself_twice_becomes_one_card() {
    // Two "YouTube Music" cards for one song. They agree on who they are, so
    // one further field is enough; holding out for more leaves the pair side
    // by side while the second one's metadata is still arriving.
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

    let merged = merge_duplicates(vec![first, arriving]);

    assert_eq!(merged.len(), 1);
    assert_eq!(
        merged[0].also,
        vec![String::from("org.mpris.MediaPlayer2.mprisence_web.youtube_music.p2911")]
    );
}

#[test]
fn a_mirror_naming_itself_differently_is_asked_for_more() {
    let elisa = elisa();
    let thin_mirror = Player {
        bus: String::from("org.mpris.MediaPlayer2.other"),
        source: String::from("YouTube Music"),
        album: String::new(),
        length: None,
        ..elisa.clone()
    };
    assert!(!same_playback(&elisa, &thin_mirror));

    let full_mirror = Player {
        source: String::from("YouTube Music"),
        ..thin_mirror.clone()
    };
    let full_mirror = Player {
        album: elisa.album.clone(),
        length: elisa.length,
        ..full_mirror
    };
    assert!(same_playback(&elisa, &full_mirror));
}

#[test]
fn different_tracks_stay_apart() {
    let merged = merge_duplicates(vec![
        elisa(),
        player(
            "org.mpris.MediaPlayer2.mprisence_web.youtube_music.pa808",
            "YouTube Music",
            "ANGEL (Visualizer)",
            "keshi",
            "",
        ),
    ]);

    assert_eq!(merged.len(), 2);
}

#[test]
fn a_shared_title_alone_is_not_enough() {
    let left = player("a", "Elisa", "Everlong", "Foo Fighters", "The Colour");
    let right = Player {
        bus: String::from("b"),
        source: String::from("Chromium"),
        artist: String::new(),
        album: String::new(),
        length: Some(Duration::from_secs(600)),
        ..left.clone()
    };

    assert!(!same_playback(&left, &right));
}

#[test]
fn two_players_at_different_points_are_separate() {
    let early = Player {
        position: Duration::from_secs(6),
        ..elisa()
    };
    let late = Player {
        bus: String::from("b"),
        position: Duration::from_secs(120),
        ..elisa()
    };

    assert!(!same_playback(&early, &late));
}

#[test]
fn an_unread_position_of_zero_is_not_disagreement() {
    let watched = Player {
        position: Duration::from_secs(92),
        ..elisa()
    };
    let unread = Player {
        bus: String::from("b"),
        ..elisa()
    };

    assert!(same_playback(&watched, &unread));
}

#[test]
fn an_untitled_player_never_merges() {
    let blank = Player {
        title: String::new(),
        ..elisa()
    };

    assert!(!same_playback(&blank, &blank));
}

#[test]
fn the_card_commands_every_player_it_stands_for() {
    let mut card = elisa();
    card.also.push(String::from("org.mpris.MediaPlayer2.other"));

    let targets: Vec<&str> = card.targets().collect();

    assert_eq!(
        targets,
        vec!["org.mpris.MediaPlayer2.elisa", "org.mpris.MediaPlayer2.other"]
    );
}

#[test]
fn progress_needs_a_length() {
    let no_length = Player {
        position: Duration::from_secs(30),
        length: None,
        ..elisa()
    };
    assert_eq!(no_length.progress(), 0.0);

    let halfway = Player {
        position: Duration::from_secs(100),
        length: Some(Duration::from_secs(200)),
        ..elisa()
    };
    assert!((halfway.progress() - 0.5).abs() < f64::EPSILON);
}

#[test]
fn progress_is_clamped_past_the_end() {
    let overrun = Player {
        position: Duration::from_secs(400),
        length: Some(Duration::from_secs(200)),
        ..elisa()
    };

    assert_eq!(overrun.progress(), 1.0);
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
    // One bus per browser tab: publisher, site, then the tab's own id.
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
    assert!(!echoes_the_bus_name("YouTube Music"));
    assert!(!echoes_the_bus_name("Elisa"));
    assert!(!echoes_the_bus_name("VLC media player"));
    assert!(echoes_the_bus_name("mprisence_web.spotify.p2911"));
}

#[test]
fn clock_grows_an_hours_field_only_when_needed() {
    assert_eq!(clock(Duration::from_secs(65)), "1:05");
    assert_eq!(clock(Duration::ZERO), "0:00");
    assert_eq!(clock(Duration::from_secs(3 * 3600 + 4 * 60 + 5)), "3:04:05");
}
