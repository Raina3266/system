use crate::model::{
    PlaybackStatus, Player, clock, compare_players, media_label, row_text, volume_label,
};
use crate::mpris::{
    display_source, is_excluded_player, is_publishable_web_media_url, should_include_player,
};
use crate::notifications::{Notifications, parse};
use crate::players;
use crate::text::{hex_decode, hex_encode, json_escape, truncate_display};
use crate::ui::{bar_text, bar_tooltip, rofi_row_state, waybar_json, waybar_toggle_action};

/// Nothing waiting and Do Not Disturb off: the state most tests want.
const QUIET: Notifications = Notifications {
    count: 0,
    dnd: false,
};

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
        position: None,
        length: None,
        pinned: false,
        activity: 0,
    };

    assert_eq!(media_label(&player), "Delulu — SZA");
    assert!(row_text(&player).ends_with("{Chrome} Delulu — SZA"));
    assert!(bar_text(Some(&player), QUIET).ends_with("Delulu — SZA"));
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
        position: None,
        length: None,
        pinned: false,
        activity: 0,
    };

    assert_eq!(media_label(&player), "Video title");
    assert!(bar_text(Some(&player), QUIET).ends_with("Video title"));
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
        position: None,
        length: None,
        pinned,
        activity,
    };
    let mut values = [
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
        position: None,
        length: None,
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
        position: None,
        length: None,
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

#[test]
fn the_bar_button_shows_a_quiet_bell_with_nothing_to_report() {
    assert_eq!(bar_text(None, QUIET), "󰂜");
    assert_eq!(
        bar_tooltip(None, QUIET),
        "No notifications\nNo active media"
    );
}

#[test]
fn the_bar_button_counts_waiting_notifications() {
    let waiting = Notifications {
        count: 3,
        dnd: false,
    };
    assert_eq!(bar_text(None, waiting), "󰂚 3");
    assert!(bar_tooltip(None, waiting).starts_with("3 notifications"));
    assert!(waybar_json(None, waiting).contains("\"class\":[\"notification\",\"empty\"]"));
}

#[test]
fn do_not_disturb_silences_the_bell_and_keeps_the_count_out_of_the_label() {
    let silenced = Notifications {
        count: 3,
        dnd: true,
    };
    assert_eq!(bar_text(None, silenced), "󰂛");
    assert!(bar_tooltip(None, silenced).contains("Do Not Disturb is on"));
    assert!(waybar_json(None, silenced).contains("\"class\":[\"dnd\",\"empty\"]"));
}

#[test]
fn the_bar_button_carries_the_badge_and_the_track_together() {
    let player = playing_player();
    let text = bar_text(
        Some(&player),
        Notifications {
            count: 2,
            dnd: false,
        },
    );
    assert!(text.starts_with("󰂚 2"), "{text}");
    assert!(text.ends_with("Delulu \u{2014} SZA"), "{text}");
    assert!(
        text.contains('\u{b7}'),
        "the two halves are separated: {text}"
    );

    let json = waybar_json(Some(&player), QUIET);
    assert!(json.contains("\"class\":[\"quiet\",\"playing\"]"), "{json}");
}

#[test]
fn a_long_track_is_truncated_rather_than_pushing_the_bar_wide() {
    let mut player = playing_player();
    player.title = "A".repeat(120);
    let text = bar_text(Some(&player), QUIET);
    assert!(text.contains('\u{2026}'), "{text}");
    assert!(text.chars().count() < 60, "{text}");
}

#[test]
fn a_widget_row_carries_every_field_the_widget_draws() {
    let mut player = playing_player();
    player.position = Some(83.0);
    player.length = Some(296.0);

    let row = players::row(&player);
    let fields: Vec<&str> = row.split('\t').collect();
    assert_eq!(fields.len(), 7, "{fields:?}");
    assert_eq!(fields[0], "youtube-music");
    assert_eq!(fields[1], "playing");
    assert_eq!(fields[2], "45");
    assert_eq!(fields[3], "83");
    assert_eq!(fields[4], "296");
    assert_eq!(fields[5], "Delulu \u{2014} SZA");
    assert_eq!(fields[6], "Chrome  \u{b7}  1:23 / 4:56");
}

#[test]
fn a_row_marks_the_fields_mpris_leaves_out() {
    let mut player = playing_player();
    player.volume = None;
    player.position = None;
    player.length = None;

    let row = players::row(&player);
    let fields: Vec<&str> = row.split('\t').collect();
    assert_eq!([fields[2], fields[3], fields[4]], ["-", "-", "-"]);
    assert_eq!(
        fields[6], "Chrome",
        "no timing means no timing in the subtitle"
    );
}

#[test]
fn a_row_without_a_length_still_reports_how_far_in_it_is() {
    let mut player = playing_player();
    player.position = Some(3_671.0);
    player.length = None;

    let row = players::row(&player);
    let fields: Vec<&str> = row.split('\t').collect();
    assert_eq!(fields[3], "3671");
    assert_eq!(fields[4], "-");
    assert_eq!(fields[6], "Chrome  \u{b7}  1:01:11");
}

#[test]
fn a_clock_grows_an_hours_field_only_when_it_needs_one() {
    assert_eq!(clock(0.0), "0:00");
    assert_eq!(clock(9.4), "0:09");
    assert_eq!(clock(83.0), "1:23");
    assert_eq!(clock(3_599.0), "59:59");
    assert_eq!(clock(3_600.0), "1:00:00");
}

#[test]
fn a_wayle_status_line_yields_the_count_and_the_dnd_flag() {
    assert_eq!(
        parse("{\"count\":3,\"dnd\":false}"),
        Some(Notifications {
            count: 3,
            dnd: false
        })
    );
    assert_eq!(
        parse("{\"count\":0,\"dnd\":true}"),
        Some(Notifications {
            count: 0,
            dnd: true
        })
    );
}

#[test]
fn a_line_without_a_count_is_not_a_wayle_status_line() {
    assert_eq!(parse("Wayle is starting..."), None);
    assert_eq!(parse(""), None);
}

/// A player the bar and row assertions above can share.
fn playing_player() -> Player {
    Player {
        id: "youtube-music".to_owned(),
        source: "Chrome".to_owned(),
        title: "Delulu".to_owned(),
        artist: "SZA".to_owned(),
        status: PlaybackStatus::Playing,
        volume: Some(45),
        position: None,
        length: None,
        pinned: false,
        activity: 0,
    }
}
