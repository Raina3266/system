//! The panel's tests, one module per feature, named after the file it covers.

mod artwork {
    use std::path::PathBuf;

    use crate::artwork::*;

    /// A directory that removes itself, so the tests leave nothing behind.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(label: &str) -> Self {
            let base =
                std::env::temp_dir().join(format!("media-panel-{label}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&base);
            std::fs::create_dir_all(&base).expect("temp dir");
            Self(base)
        }

        fn file(&self, name: &str) -> PathBuf {
            let path = self.0.join(name);
            std::fs::write(&path, b"x").expect("write");
            path
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn local_path_decodes_percent_escapes() {
        // Elisa publishes exactly this shape: spaces and pipes escaped.
        assert_eq!(
            local_path("file:///home/raina/Documents/Destiny%20Rogers%20%7C%7C%20Tomboy/track.mp3"),
            Some(PathBuf::from(
                "/home/raina/Documents/Destiny Rogers || Tomboy/track.mp3"
            ))
        );
    }

    #[test]
    fn local_path_rejects_other_schemes() {
        assert_eq!(local_path("https://music.youtube.com/watch?v=abc"), None);
        assert_eq!(local_path("spotify:track:abc"), None);
    }

    #[test]
    fn local_path_rejects_a_truncated_escape() {
        assert_eq!(local_path("file:///music/track%2.mp3"), None);
    }

    #[test]
    fn percent_decode_leaves_ordinary_text_alone() {
        assert_eq!(
            percent_decode("plain/path.flac"),
            Some(String::from("plain/path.flac"))
        );
    }

    #[test]
    fn a_folder_cover_is_found() {
        let dir = TempDir::new("folder");
        let track = dir.file("01 - song.flac");
        let cover = dir.file("cover.jpg");

        assert_eq!(beside(&track), Some(cover));
    }

    #[test]
    fn the_tracks_own_picture_wins_over_the_folder_cover() {
        let dir = TempDir::new("own");
        let track = dir.file("song.mp3");
        let own = dir.file("song.png");
        dir.file("cover.jpg");

        assert_eq!(beside(&track), Some(own));
    }

    #[test]
    fn the_name_is_matched_whatever_its_case() {
        let dir = TempDir::new("case");
        let track = dir.file("song.flac");
        let cover = dir.file("Folder.JPG");

        assert_eq!(beside(&track), Some(cover));
    }

    #[test]
    fn cover_is_preferred_to_the_other_folder_names() {
        let dir = TempDir::new("order");
        let track = dir.file("song.flac");
        dir.file("thumb.png");
        let cover = dir.file("cover.png");

        assert_eq!(beside(&track), Some(cover));
    }

    #[test]
    fn a_folder_without_a_picture_yields_nothing() {
        let dir = TempDir::new("bare");
        let track = dir.file("song.flac");
        dir.file("notes.txt");

        assert_eq!(beside(&track), None);
    }

    #[test]
    fn a_directory_named_like_a_cover_is_not_one() {
        let dir = TempDir::new("dir");
        let track = dir.file("song.flac");
        std::fs::create_dir(dir.0.join("cover.jpg")).expect("mkdir");

        assert_eq!(beside(&track), None);
    }
}

mod mpris {
    use std::time::Duration;

    use crate::mpris::*;

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
            vec![String::from(
                "org.mpris.MediaPlayer2.mprisence_web.youtube_music.p2911"
            )]
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
            vec![
                "org.mpris.MediaPlayer2.elisa",
                "org.mpris.MediaPlayer2.other"
            ]
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
    fn mpris_volume_is_mapped_to_a_bounded_percentage() {
        assert_eq!(volume_percent(0.0), Some(0));
        assert_eq!(volume_percent(0.425), Some(43));
        assert_eq!(volume_percent(1.0), Some(100));
        assert_eq!(volume_percent(-0.5), Some(0));
        assert_eq!(volume_percent(1.5), Some(100));
        assert_eq!(volume_percent(f64::NAN), None);
        assert_eq!(volume_percent(f64::INFINITY), None);
    }

    #[test]
    fn a_mirror_can_supply_the_cards_missing_volume() {
        let primary = Player {
            volume: None,
            ..elisa()
        };
        let mirror = Player {
            bus: String::from("org.mpris.MediaPlayer2.other"),
            volume: Some(37),
            ..primary.clone()
        };

        let merged = merge_duplicates(vec![primary, mirror]);

        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].volume, Some(37));
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
}

mod ui {
    use crate::ui::*;

    #[test]
    fn a_wide_thumbnail_is_cropped_from_its_middle() {
        // A 16:9 video thumbnail, which is what a browser tab publishes.
        assert_eq!(centre_square(1280, 720), Some((280, 0, 720)));
    }

    #[test]
    fn a_tall_cover_is_cropped_from_its_middle() {
        assert_eq!(centre_square(600, 900), Some((0, 150, 600)));
    }

    #[test]
    fn a_square_cover_is_left_alone() {
        assert_eq!(centre_square(300, 300), Some((0, 0, 300)));
    }

    #[test]
    fn an_empty_image_has_no_square_to_take() {
        assert_eq!(centre_square(0, 500), None);
        assert_eq!(centre_square(-1, -1), None);
    }
}
