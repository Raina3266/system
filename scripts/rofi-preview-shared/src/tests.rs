//! Every test in the script, one mod per feature and one test per sub-feature.

mod cli {
    use std::path::PathBuf;

    use crate::cli::*;

    fn run(arguments: &[&str]) -> Options {
        match parse_from(arguments.iter().copied()).expect("arguments should parse") {
            Action::Run(options) => options,
            other => panic!("expected run action, got {other:?}"),
        }
    }

    #[test]
    fn defaults_to_editable_wrapped_standard_input() {
        assert_eq!(run(&[]), Options::default());
    }

    #[test]
    fn accepts_a_file_without_changing_its_path() {
        let options = run(&["folder/a file.txt"]);
        assert_eq!(
            options.source,
            Source::File(PathBuf::from("folder/a file.txt"))
        );
    }

    #[test]
    fn parses_window_and_editor_options() {
        let options = run(&[
            "--title",
            "Clipboard preview",
            "--read-only",
            "--no-wrap",
            "--listen",
            "/run/user/1000/preview.sock",
            "--layout-file",
            "/tmp/rofi-network.rasi",
            "--panel",
            "--width=900",
            "--height",
            "700",
            "--companion-width=420",
            "--side",
            "right",
            "--gap",
            "12",
        ]);
        assert_eq!(options.title, "Clipboard preview");
        assert!(!options.editable);
        assert!(!options.wrap);
        assert_eq!((options.width, options.height), (900, 700));
        assert_eq!(
            options.listen,
            Some(PathBuf::from("/run/user/1000/preview.sock"))
        );
        assert_eq!(
            options.layout_file,
            Some(PathBuf::from("/tmp/rofi-network.rasi"))
        );
        assert!(options.panel);
        assert_eq!(options.companion_width, 420);
        assert_eq!(options.side, Side::Right);
        assert_eq!(options.gap, 12);
        assert_eq!(
            options.window_overrides,
            WindowOverrides {
                width: Some(900),
                height: Some(700),
                companion_width: Some(420),
                side: Some(Side::Right),
                gap: Some(12),
                x: None,
                y: None,
            }
        );
    }

    #[test]
    fn double_dash_allows_a_filename_starting_with_a_dash() {
        let options = run(&["--", "--notes.txt"]);
        assert_eq!(options.source, Source::File(PathBuf::from("--notes.txt")));
    }

    #[test]
    fn rejects_file_with_explicit_stdin() {
        let error = parse_from(["--stdin", "notes.txt"]).unwrap_err();
        assert!(error.to_string().contains("cannot be combined"));
    }

    #[test]
    fn rejects_dimensions_that_would_make_an_unusable_window() {
        let error = parse_from(["--width", "50"]).unwrap_err();
        assert!(error.to_string().contains("between 200 and 8192"));
    }

    #[test]
    fn rejects_negative_panel_gaps() {
        let error = parse_from(["--gap=-1"]).unwrap_err();
        assert!(error.to_string().contains("between 0 and 512"));
    }

    #[test]
    fn rejects_unknown_panel_sides() {
        let error = parse_from(["--side", "middle"]).unwrap_err();
        assert!(error.to_string().contains("left or right"));
    }
}

mod config {
    use std::ffi::OsStr;
    use std::path::PathBuf;

    use crate::cli::{Side, WindowOverrides};
    use crate::config::*;

    const THEME: &str = r#"/* rofi-preview-shared-settings
width: 480px;
height: 615px;
companion_width: 400px;
side: right;
gap: 14px;
x: 35px;
y: -20px;
*/

window.rofi-preview-shared { color: #cbe3e7; }
"#;

    #[test]
    fn embedded_theme_has_expected_defaults() {
        assert_eq!(
            embedded().window,
            WindowConfig {
                width: 400,
                height: 616,
                companion_width: 400,
                side: Side::Left,
                gap: 5,
                x: 770,
                y: -850,
            }
        );
    }

    #[test]
    fn parses_geometry_and_preserves_complete_css() {
        let config = parse(THEME).unwrap();

        assert_eq!(config.window.width, 480);
        assert_eq!(config.window.height, 615);
        assert_eq!(config.window.companion_width, 400);
        assert_eq!(config.window.side, Side::Right);
        assert_eq!(config.window.gap, 14);
        assert_eq!(config.window.x, 35);
        assert_eq!(config.window.y, -20);
        assert_eq!(config.css, THEME);
    }

    #[test]
    fn command_line_values_override_css_window_values() {
        let window = parse(THEME).unwrap().window;
        let overrides = WindowOverrides {
            width: Some(600),
            side: Some(Side::Left),
            ..WindowOverrides::default()
        };

        assert_eq!(
            window.with_overrides(overrides),
            WindowConfig {
                width: 600,
                height: 615,
                companion_width: 400,
                side: Side::Left,
                gap: 14,
                x: 35,
                y: -20,
            }
        );
    }

    #[test]
    fn accepts_companion_width_with_css_style_name() {
        let source = THEME.replace("companion_width", "companion-width");
        assert_eq!(parse(&source).unwrap().window.companion_width, 400);
    }

    #[test]
    fn parses_partial_rasi_layout_and_ignores_missing_block() {
        let rasi = r#"/* rofi-preview-shared-layout
height: 400px;
companion-width: 375px;
x: -25px;
*/

window { width: 375px; }
"#;
        assert_eq!(
            parse_layout(rasi).unwrap(),
            WindowOverrides {
                height: Some(400),
                companion_width: Some(375),
                x: Some(-25),
                ..WindowOverrides::default()
            }
        );
        assert_eq!(
            parse_layout("window { width: 400px; }").unwrap(),
            WindowOverrides::default()
        );
    }

    #[test]
    fn higher_priority_overrides_win_over_rasi_layout() {
        let layout = WindowOverrides {
            width: Some(300),
            height: Some(400),
            ..WindowOverrides::default()
        };
        let command_line = WindowOverrides {
            width: Some(500),
            ..WindowOverrides::default()
        };
        let merged = layout.overlaid_by(command_line);
        assert_eq!(merged.width, Some(500));
        assert_eq!(merged.height, Some(400));
    }

    #[test]
    fn rejects_invalid_rasi_layout_without_affecting_css_parser() {
        let invalid = "/* rofi-preview-shared-layout\nwidth: 100px;\n*/";
        assert!(
            parse_layout(invalid)
                .unwrap_err()
                .to_string()
                .contains("width")
        );
        assert!(parse(THEME).is_ok());
    }

    #[test]
    fn rejects_missing_unknown_and_repeated_settings() {
        let missing = THEME.replace("width: 480px;\n", "");
        assert!(parse(&missing).unwrap_err().to_string().contains("width"));

        let unknown = THEME.replace("*/", "opacity: 1;\n*/");
        assert!(parse(&unknown).unwrap_err().to_string().contains("unknown"));

        let repeated = THEME.replace("*/", "width: 500px;\n*/");
        assert!(
            parse(&repeated)
                .unwrap_err()
                .to_string()
                .contains("repeated")
        );
    }

    #[test]
    fn rejects_values_outside_supported_ranges() {
        let dimension = THEME.replace("width: 480px;", "width: 100px;");
        assert!(parse(&dimension).unwrap_err().to_string().contains("width"));

        let panel_gap = THEME.replace("gap: 14px;", "gap: -1px;");
        assert!(parse(&panel_gap).unwrap_err().to_string().contains("gap"));

        let position = THEME.replace("x: 35px;", "x: 9000px;");
        assert!(parse(&position).unwrap_err().to_string().contains("x"));
    }

    #[test]
    fn requires_a_closed_settings_comment() {
        assert!(
            parse("window { color: red; }")
                .unwrap_err()
                .to_string()
                .contains("missing")
        );

        let unclosed = THEME.replacen("*/", "", 1);
        assert!(
            parse(&unclosed)
                .unwrap_err()
                .to_string()
                .contains("not closed")
        );
    }

    #[test]
    fn explicit_css_path_takes_priority() {
        assert_eq!(
            theme_path_from(
                Some(OsStr::new("/tmp/custom.css")),
                Some(OsStr::new("/tmp/config")),
                Some(OsStr::new("/home/raina")),
            ),
            Some(PathBuf::from("/tmp/custom.css"))
        );
    }

    #[test]
    fn css_path_uses_xdg_then_home_fallback() {
        assert_eq!(
            theme_path_from(None, Some(OsStr::new("/tmp/config")), None),
            Some(PathBuf::from(
                "/tmp/config/rofi-preview-shared/rofi-preview-shared.css"
            ))
        );
        assert_eq!(
            theme_path_from(None, None, Some(OsStr::new("/home/raina"))),
            Some(PathBuf::from(
                "/home/raina/.config/rofi-preview-shared/rofi-preview-shared.css"
            ))
        );
    }
}

mod document {
    use std::io::{self, Cursor};

    use crate::document::*;

    #[test]
    fn preserves_every_whitespace_character_exactly() {
        let original = "heading\r\n\t  first    value\n\n    indented\tcolumn  \n";
        let loaded = read_utf8(Cursor::new(original.as_bytes())).unwrap();
        assert_eq!(loaded, original);
    }

    #[test]
    fn preserves_unicode_without_normalizing_it() {
        let original = "café\n中文\n👩🏽‍💻\n";
        let loaded = read_utf8(Cursor::new(original.as_bytes())).unwrap();
        assert_eq!(loaded.as_bytes(), original.as_bytes());
    }

    #[test]
    fn rejects_non_utf8_input_instead_of_replacing_bytes() {
        let error = read_utf8(Cursor::new([0xff, 0xfe])).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }
}

mod ipc {
    use std::io::{self, Cursor, Read, Write};
    use std::os::unix::net::UnixStream;
    use std::path::PathBuf;
    use std::sync::mpsc;
    use std::time::Duration;

    use crate::ipc::*;

    fn frame(operation: u8, serial: u64, payload: &[u8]) -> Vec<u8> {
        let mut bytes = vec![operation];
        bytes.extend_from_slice(&serial.to_be_bytes());
        bytes.extend_from_slice(&(payload.len() as u64).to_be_bytes());
        bytes.extend_from_slice(payload);
        bytes
    }

    fn item_frame(operation: u8, serial: u64, id: u64, payload: &[u8]) -> Vec<u8> {
        let mut item = id.to_be_bytes().to_vec();
        item.extend_from_slice(payload);
        frame(operation, serial, &item)
    }

    #[test]
    fn update_preserves_whitespace_and_unicode_exactly() {
        let text = "heading\r\n\t  first    value\n\n中文 👩🏽‍💻  \n";
        let message = read_request(Cursor::new(item_frame(
            UPDATE_TEXT,
            17,
            42,
            text.as_bytes(),
        )))
        .unwrap();
        assert_eq!(
            message,
            Request::UpdateText {
                serial: 17,
                id: 42,
                text: text.to_owned()
            }
        );
    }

    #[test]
    fn image_update_preserves_the_cached_file_path() {
        let path = "/home/raina/.local/share/rofi-clipboard/images/7.png";
        assert_eq!(
            read_request(Cursor::new(item_frame(
                UPDATE_IMAGE,
                19,
                7,
                path.as_bytes()
            )))
            .unwrap(),
            Request::UpdateImage {
                serial: 19,
                id: 7,
                path: PathBuf::from(path),
            }
        );
    }

    #[test]
    fn network_update_separates_details_from_png_bytes() {
        let details = "SSID: Café\nIPv4: 192.0.2.10/24";
        let png = b"\x89PNG\r\n\x1a\nmock";
        let mut payload = 88_u64.to_be_bytes().to_vec();
        payload.extend_from_slice(&(details.len() as u64).to_be_bytes());
        payload.extend_from_slice(details.as_bytes());
        payload.extend_from_slice(png);

        assert_eq!(
            read_request(Cursor::new(frame(UPDATE_NETWORK, 29, &payload))).unwrap(),
            Request::UpdateNetwork {
                serial: 29,
                id: 88,
                details: details.to_owned(),
                png: png.to_vec(),
            }
        );
    }

    #[test]
    fn network_update_rejects_an_out_of_bounds_details_length() {
        let mut payload = 88_u64.to_be_bytes().to_vec();
        payload.extend_from_slice(&99_u64.to_be_bytes());
        payload.extend_from_slice(b"short");
        let error = read_request(Cursor::new(frame(UPDATE_NETWORK, 29, &payload))).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn prepare_switch_preserves_target_id_and_serial() {
        assert_eq!(
            read_request(Cursor::new(frame(
                PREPARE_SWITCH,
                23,
                &91_u64.to_be_bytes()
            )))
            .unwrap(),
            Request::PrepareSwitch {
                serial: 23,
                target_id: 91,
            }
        );
    }

    #[test]
    fn close_requires_an_empty_payload() {
        assert_eq!(
            read_request(Cursor::new(frame(CLOSE, 0, &[]))).unwrap(),
            Request::Close
        );
        let error = read_request(Cursor::new(frame(CLOSE, 0, b"unexpected"))).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn save_and_close_requires_an_empty_payload() {
        assert_eq!(
            read_request(Cursor::new(frame(SAVE_AND_CLOSE, 0, &[]))).unwrap(),
            Request::SaveAndClose
        );
        let error = read_request(Cursor::new(frame(SAVE_AND_CLOSE, 0, b"unexpected"))).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn panel_state_response_preserves_item_id_and_complete_buffer() {
        let text = "first line\n\tsecond  line\n中文 👩🏽‍💻\n";
        let mut response = Vec::new();
        write_panel_state(
            &mut response,
            31,
            &SwitchReply::Ready(Some(ContentSnapshot::Text {
                id: 77,
                text: text.to_owned(),
            })),
        )
        .unwrap();

        assert_eq!(response[0], PANEL_STATE);
        assert_eq!(u64::from_be_bytes(response[1..9].try_into().unwrap()), 31);
        assert_eq!(
            u64::from_be_bytes(response[9..17].try_into().unwrap()),
            (10 + text.len()) as u64
        );
        assert_eq!(response[17], SWITCH_READY);
        assert_eq!(response[18], CONTENT_TEXT);
        assert_eq!(u64::from_be_bytes(response[19..27].try_into().unwrap()), 77);
        assert_eq!(&response[27..], text.as_bytes());
    }

    #[test]
    fn save_request_round_trip_returns_the_ui_buffer() {
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let (sender, receiver) = mpsc::channel();
        let server = std::thread::spawn(move || handle_connection(&mut server, &sender).unwrap());
        client.write_all(&frame(SAVE_AND_CLOSE, 0, &[])).unwrap();

        let edited = "first line\n\tsecond  line\n中文 👩🏽‍💻\n";
        match receiver.recv_timeout(Duration::from_secs(1)).unwrap() {
            Message::SaveAndClose { reply } => reply
                .send(Some(ContentSnapshot::Text {
                    id: 55,
                    text: edited.to_owned(),
                }))
                .unwrap(),
            message => panic!("expected save request, got {message:?}"),
        }

        let mut response = Vec::new();
        client.read_to_end(&mut response).unwrap();
        assert!(server.join().unwrap());
        assert_eq!(response[0], PANEL_STATE);
        assert_eq!(response[17], SWITCH_READY);
        assert_eq!(response[18], CONTENT_TEXT);
        assert_eq!(u64::from_be_bytes(response[19..27].try_into().unwrap()), 55);
        assert_eq!(&response[27..], edited.as_bytes());
    }

    #[test]
    fn rejects_unknown_operations() {
        let error = read_request(Cursor::new(frame(99, 0, &[]))).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }
}

mod panel_state {
    use crate::panel_state::*;

    #[test]
    fn rapid_updates_cannot_restore_an_older_selection() {
        let mut state = LiveState::default();
        assert!(state.apply_update(8, 80, ContentKind::Text));
        assert!(!state.apply_update(6, 60, ContentKind::Text));
        assert_eq!(state.current.unwrap().id, 80);
        assert!(state.apply_update(9, 90, ContentKind::Text));
    }

    #[test]
    fn newer_prepare_rejects_an_older_delayed_update() {
        let mut state = LiveState::default();
        assert!(state.apply_update(1, 10, ContentKind::Text));
        assert_eq!(state.prepare_switch(8, 80), SwitchDisposition::Ready);
        assert_eq!(state.prepare_switch(9, 90), SwitchDisposition::Ready);
        assert!(!state.apply_update(8, 80, ContentKind::Text));
        assert!(state.apply_update(9, 90, ContentKind::Image));
        assert_eq!(state.current.unwrap().id, 90);
    }

    #[test]
    fn callback_for_current_item_cannot_replace_unsaved_text() {
        let mut state = LiveState::default();
        assert!(state.apply_update(0, 44, ContentKind::Text));
        assert_eq!(state.prepare_switch(12, 44), SwitchDisposition::SameItem);
        assert!(!state.apply_update(12, 44, ContentKind::Text));
    }

    #[test]
    fn accepted_update_records_the_new_content_kind() {
        let mut state = LiveState::default();
        assert!(state.apply_update(0, 7, ContentKind::Text));
        assert!(state.apply_update(1, 8, ContentKind::Image));
        assert_eq!(
            state.current,
            Some(CurrentItem {
                id: 8,
                kind: ContentKind::Image,
            })
        );

        assert!(state.apply_update(2, 9, ContentKind::Network));
        assert_eq!(
            state.current,
            Some(CurrentItem {
                id: 9,
                kind: ContentKind::Network,
            })
        );
    }

    #[test]
    fn stale_prepare_does_not_change_the_current_item() {
        let mut state = LiveState::default();
        assert!(state.apply_update(10, 10, ContentKind::Text));
        assert_eq!(state.prepare_switch(9, 9), SwitchDisposition::Rejected);
        assert_eq!(state.current.unwrap().id, 10);
    }
}

mod ui {
    use crate::cli::Side;
    use crate::ui::{PanelKeyboardState, companion_margin, horizontal_margin, vertical_margin};
    use gtk4_layer_shell::KeyboardMode;

    #[test]
    fn panel_only_accepts_keyboard_focus_after_the_editor_is_armed() {
        assert!(matches!(
            PanelKeyboardState::Browsing.mode(),
            KeyboardMode::None
        ));
        assert!(matches!(
            PanelKeyboardState::EditorArmed.mode(),
            KeyboardMode::OnDemand
        ));
    }

    #[test]
    fn companion_margin_places_the_panel_beside_a_centered_window() {
        assert_eq!(companion_margin(1920, 400, 480, 10), 270);
        assert_eq!(companion_margin(1280, 400, 480, 10), 0);
    }

    #[test]
    fn x_offset_moves_in_the_same_screen_direction_on_both_sides() {
        assert_eq!(horizontal_margin(1920, 400, 480, 10, Side::Left, 25), 295);
        assert_eq!(horizontal_margin(1920, 400, 480, 10, Side::Right, 25), 245);
    }

    #[test]
    fn y_offset_moves_from_center_and_stays_on_screen() {
        assert_eq!(vertical_margin(1080, 615, 0), 232);
        assert_eq!(vertical_margin(1080, 615, 40), 272);
        assert_eq!(vertical_margin(1080, 615, -500), 0);
        assert_eq!(vertical_margin(1080, 615, 1000), 465);
    }
}
