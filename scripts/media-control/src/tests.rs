use crate::model::{PlaybackStatus, Player, compare_players, media_label, row_text, volume_label};
use crate::mpris::{
    display_source, is_excluded_player, is_publishable_web_media_url, should_include_player,
};
use crate::text::{hex_decode, hex_encode, json_escape, truncate_display};
use crate::ui::{rofi_row_state, waybar_json, waybar_toggle_action};

#[test]
fn truncates_ascii_at_about_forty_columns() {
    let input = "123456789012345678901234567890123456789012345";
    let result = truncate_display(input, 40);
    assert!(result.ends_with('…'));
    assert_eq!(result.chars().count(), 40);
}

#[test]
fn counts_cjk_as_two_columns() {
    let input = "这是一个用于测试中文标题截断长度的字符串这是额外文字";
    let result = truncate_display(input, 40);
    assert!(result.ends_with('…'));
    assert!(result.chars().count() <= 20);
}

#[test]
fn state_key_round_trip() {
    let player = "chromium.instance1234";
    assert_eq!(hex_decode(&hex_encode(player)).as_deref(), Some(player));
    assert_eq!(hex_decode(&hex_encode(player)).as_deref(), Some(player));
}

#[test]
fn escapes_waybar_json() {
    assert_eq!(json_escape("a\"b\\c\n"), "a\\\"b\\\\c\\n");
}

#[test]
fn visible_media_labels_include_the_artist_when_available() {
    let player = Player {
        id: "youtube-music".to_owned(),
        source: "Chrome".to_owned(),
        title: "Delulu".to_owned(),
        artist: "SZA".to_owned(),
        status: PlaybackStatus::Playing,
        volume: None,
        pinned: false,
        activity: 0,
    };

    assert_eq!(media_label(&player), "Delulu — SZA");
    assert!(row_text(&player).ends_with("{Chrome} Delulu — SZA"));
    assert!(waybar_json(Some(&player)).contains("\"text\":\"Delulu — SZA\""));
}

#[test]
fn visible_media_labels_fall_back_to_the_title_without_an_artist() {
    let player = Player {
        id: "browser-video".to_owned(),
        source: "Chrome".to_owned(),
        title: "Video title".to_owned(),
        artist: String::new(),
        status: PlaybackStatus::Paused,
        volume: None,
        pinned: false,
        activity: 0,
    };

    assert_eq!(media_label(&player), "Video title");
    assert!(waybar_json(Some(&player)).contains("\"text\":\"Video title\""));
}

#[test]
fn pinned_players_sort_first() {
    let make = |id: &str, pinned: bool, status: PlaybackStatus, activity| Player {
        id: id.to_owned(),
        source: String::new(),
        title: String::new(),
        artist: String::new(),
        status,
        volume: None,
        pinned,
        activity,
    };
    let mut values = vec![
        make("new", false, PlaybackStatus::Playing, 3),
        make("pin", true, PlaybackStatus::Paused, 1),
        make("old", false, PlaybackStatus::Paused, 2),
    ];
    values.sort_by(compare_players);
    assert_eq!(values[0].id, "pin");
    assert_eq!(values[1].id, "new");
}

#[test]
fn pinned_rofi_style_takes_priority_over_playback_status() {
    let make = |pinned, status| Player {
        id: String::new(),
        source: String::new(),
        title: String::new(),
        artist: String::new(),
        status,
        volume: None,
        pinned,
        activity: 0,
    };

    assert_eq!(
        rofi_row_state(&make(true, PlaybackStatus::Playing)),
        Some("urgent")
    );
    assert_eq!(
        rofi_row_state(&make(false, PlaybackStatus::Playing)),
        Some("active")
    );
    assert_eq!(rofi_row_state(&make(false, PlaybackStatus::Paused)), None);
}

#[test]
fn waybar_click_toggles_the_displayed_player_even_when_paused() {
    let paused = Player {
        id: "paused-player".to_owned(),
        source: String::new(),
        title: String::new(),
        artist: String::new(),
        status: PlaybackStatus::Paused,
        volume: None,
        pinned: false,
        activity: 1,
    };

    assert_eq!(
        waybar_toggle_action(&[paused]),
        Some(("paused-player", "play-pause"))
    );
    assert_eq!(waybar_toggle_action(&[]), None);
}

#[test]
fn unavailable_volume_has_an_explicit_placeholder() {
    assert_eq!(volume_label(None), "  --");
    assert_eq!(volume_label(Some(7)), "  7%");
    assert_eq!(volume_label(Some(100)), "100%");
}

#[test]
fn mprisence_web_uses_chrome_as_its_display_source() {
    assert_eq!(
        display_source("mprisence_web.web.youtube.p123", "mprisence_web"),
        "Chrome"
    );
    assert_eq!(display_source("spotify", "Spotify"), "Spotify");
}

#[test]
fn tauon_and_kid3_are_excluded_from_media_control() {
    assert!(is_excluded_player("tauon", "Tauon Music Box"));
    assert!(is_excluded_player("org.mpris.MediaPlayer2.kid3", "Kid3"));
    assert!(!is_excluded_player(
        "mprisence_web.web.youtube.p123",
        "mprisence_web"
    ));
}

#[test]
fn keeps_canonical_youtube_and_bilibili_media_urls() {
    for url in [
        "https://www.youtube.com/watch?v=H9vpsqr0U8A",
        "https://music.youtube.com/watch?v=BTlj6Sls7KE",
        "https://youtu.be/H9vpsqr0U8A",
        "https://www.youtube.com/shorts/H9vpsqr0U8A",
        "https://www.bilibili.com/video/BV1F54y127A8/?spm_id_from=333",
        "https://www.bilibili.com/bangumi/play/ep123456",
        "https://live.bilibili.com/123456",
    ] {
        assert!(is_publishable_web_media_url(url), "should keep {url}");
    }
}

#[test]
fn hides_home_search_and_listing_pages_from_mprisence() {
    for url in [
        "https://www.bilibili.com/",
        "https://search.bilibili.com/all?keyword=test",
        "https://music.youtube.com/",
        "https://www.youtube.com/results?search_query=test",
        "https://www.google.com/search?q=test",
        "https://example.com/",
    ] {
        assert!(!is_publishable_web_media_url(url), "should hide {url}");
    }
}

#[test]
fn leaves_non_browser_players_and_real_generic_media_unchanged() {
    assert!(should_include_player("vlc", "VLC", ""));
    assert!(should_include_player(
        "mprisence_web.generic.p123",
        "mprisence_web",
        "https://example.com/videos/episode-1"
    ));
}
