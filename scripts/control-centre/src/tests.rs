mod waybar {
    use crate::waybar::*;

    fn track(id: &str, title: &str, artist: &str) -> Track {
        Track {
            id: id.to_owned(),
            identity: String::from("Browser"),
            state: String::from("Playing"),
            title: title.to_owned(),
            artist: artist.to_owned(),
            album: String::new(),
            art_url: String::new(),
            length_us: None,
        }
    }

    #[test]
    fn exactly_fifty_characters_are_untouched() {
        let title = "a".repeat(50);
        assert_eq!(truncate_title(&title), title);
    }

    #[test]
    fn the_fifty_first_character_is_truncated() {
        assert_eq!(
            truncate_title(&"a".repeat(51)),
            format!("{}…", "a".repeat(50))
        );
    }

    #[test]
    fn the_thirty_sixth_ideograph_is_truncated() {
        assert_eq!(
            truncate_title(&"真".repeat(36)),
            format!("{}…", "真".repeat(35))
        );
    }

    #[test]
    fn a_mixed_title_stops_at_whichever_limit_arrives_first() {
        let title = format!("{}abcdefghijklmnop", "真".repeat(35));
        assert_eq!(
            truncate_title(&title),
            format!("{}abcdefghijklmno…", "真".repeat(35))
        );
    }

    #[test]
    fn a_richer_bridge_replaces_playerctld_for_the_same_track() {
        let selected = track(
            "org.mpris.MediaPlayer2.playerctld",
            "Being a Good Girl Hurts",
            "YENA",
        );
        let mut bridge = track(
            "org.mpris.MediaPlayer2.mprisence.tab-1",
            "Being a Good Girl Hurts",
            "YENA",
        );
        bridge.album = String::from("Good Morning");
        assert!(duplicate_metadata(&selected, &bridge));
        assert!(is_bridge(&preferred_duplicate(selected, bridge).id));
    }

    #[test]
    fn a_real_elisa_player_is_not_treated_as_the_bridge() {
        assert!(!is_bridge("org.mpris.MediaPlayer2.elisa"));
    }

    #[test]
    fn title_only_matches_do_not_merge_unrelated_players() {
        let left = track("org.mpris.MediaPlayer2.chromium", "Intro", "");
        let right = track("org.mpris.MediaPlayer2.mprisence.tab-1", "Intro", "");
        assert!(!duplicate_metadata(&left, &right));
    }
}
