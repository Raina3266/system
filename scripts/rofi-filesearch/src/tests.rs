//! Every test in the script, one mod per feature and one test per sub-feature.

mod desktop {
    use crate::desktop::*;

    #[test]
    fn desktop_parser_only_reads_the_desktop_entry_group() {
        let fields = desktop_fields(
            "[Desktop Entry]\nType=Application\nName=Raina & App\n\
             [Desktop Action New]\nName=Wrong name\n",
        );
        assert_eq!(fields.get("Name").map(String::as_str), Some("Raina & App"));
    }

    #[test]
    fn desktop_escapes_are_decoded() {
        assert_eq!(
            desktop_unescape(r"Line\sOne\nLine\sTwo"),
            "Line One\nLine Two"
        );
    }
}

mod model {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;
    use std::path::PathBuf;

    use crate::model::*;

    #[test]
    fn arbitrary_unix_paths_round_trip_through_rofi_keys() {
        let path = PathBuf::from(OsString::from_vec(b"/tmp/Raina's \xff.pdf".to_vec()));
        let key = path_key(Mode::File, &path);
        assert_eq!(path_from_key(&key, Mode::File).as_ref(), Some(&path));
        assert_eq!(path_from_key(&key, Mode::Folder), None);
    }

    #[test]
    fn markup_from_file_and_application_names_is_escaped() {
        assert_eq!(
            escape_markup("A&B <Preview> \"Raina's\""),
            "A&amp;B &lt;Preview&gt; &quot;Raina&apos;s&quot;"
        );
    }

    #[test]
    fn hostile_row_text_is_flattened_to_one_visual_line() {
        assert_eq!(single_line("one\n two\tthree"), "one two three");
    }

    #[test]
    fn modes_parse_their_singular_and_plural_names() {
        assert_eq!("applications".parse::<Mode>().unwrap(), Mode::App);
        assert_eq!("files".parse::<Mode>().unwrap(), Mode::File);
        assert_eq!("folders".parse::<Mode>().unwrap(), Mode::Folder);
    }
}

mod preview {
    use std::env;
    use std::fs;

    use crate::model::{Mode, path_key};
    use crate::preview::*;

    #[test]
    fn only_requested_preview_families_are_supported() {
        assert_eq!(preview_kind("text/plain"), PreviewKind::Text);
        assert_eq!(preview_kind("application/json"), PreviewKind::Text);
        assert_eq!(preview_kind("image/webp"), PreviewKind::Image);
        assert_eq!(preview_kind("application/pdf"), PreviewKind::Pdf);
        assert_eq!(preview_kind("video/mp4"), PreviewKind::Video);
        assert_eq!(preview_kind("audio/mpeg"), PreviewKind::Unsupported);
    }

    #[test]
    fn close_frame_has_no_item_payload() {
        let mut frame = Vec::new();
        write_frame(&mut frame, CLOSE, 0, &[]).unwrap();
        assert_eq!(frame.len(), 17);
        assert_eq!(frame[0], CLOSE);
    }

    #[test]
    fn file_and_folder_mode_keys_can_preview_files_but_not_directories() {
        let root = env::temp_dir().join(format!(
            "rofi-filesearch-preview-key-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let file = root.join("notes.txt");
        fs::write(&file, "notes").unwrap();

        assert_eq!(
            preview_file_from_key(&path_key(Mode::File, &file)),
            Some(file.clone())
        );
        assert_eq!(
            preview_file_from_key(&path_key(Mode::Folder, &file)),
            Some(file)
        );
        assert_eq!(preview_file_from_key(&path_key(Mode::Folder, &root)), None);
        fs::remove_dir_all(root).unwrap();
    }
}

mod rofi {
    use crate::model::{Entry, Mode};
    use crate::rofi::*;

    #[test]
    fn only_file_rows_request_two_lines() {
        assert!(mode_theme(Mode::App).contains("eh: 1;"));
        assert!(mode_theme(Mode::Folder).contains("eh: 1;"));
        assert!(mode_theme(Mode::File).contains("eh: 2;"));
    }

    #[test]
    fn folder_mode_enables_preview_but_not_reveal() {
        let theme = mode_theme(Mode::Folder);
        assert!(theme.contains("button-preview { text-color: @cyan;"));
        assert!(theme.contains("button-reveal { text-color: @dim;"));
    }

    #[test]
    fn paths_with_quotes_are_safe_in_the_selection_callback() {
        assert_eq!(shell_quote("/tmp/Raina's app"), "'/tmp/Raina'\\''s app'");
    }

    #[test]
    fn row_options_share_one_nul_metadata_marker() {
        let entry = Entry {
            key: "file:4141".to_owned(),
            display: "Visible".to_owned(),
            meta: "Searchable".to_owned(),
            icon: "text-x-generic".to_owned(),
        };
        let mut output = Vec::new();
        write_row(&mut output, &entry).unwrap();
        assert_eq!(output.iter().filter(|byte| **byte == 0).count(), 1);
    }
}

mod search {
    use std::env;
    use std::fs;
    use std::path::{Path, PathBuf};

    use crate::search::*;

    #[test]
    fn home_paths_are_abbreviated_for_the_second_file_line() {
        let home = Path::new("/home/raina");
        assert_eq!(abbreviate_home(Path::new("/home/raina"), home), "~");
        assert_eq!(
            abbreviate_home(Path::new("/home/raina/Documents/PDF"), home),
            "~/Documents/PDF"
        );
    }

    #[test]
    fn only_file_rows_contain_a_second_display_line() {
        let home = Path::new("/home/raina");
        let relative = PathBuf::from("Documents/notes.txt");
        let file = file_entry(home, relative).unwrap();
        assert!(file.display.contains('\u{2029}'));
    }

    #[test]
    fn folder_root_lists_only_visible_home_directories() {
        let home = test_home("root");
        fs::create_dir_all(home.join("Documents")).unwrap();
        fs::create_dir_all(home.join(".hidden")).unwrap();
        fs::write(home.join("notes.txt"), "notes").unwrap();

        let entries = folder_entries(&home, &home).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].display, "Documents");
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn nested_folder_lists_parent_folders_then_files() {
        let home = test_home("nested");
        let current = home.join("Documents");
        fs::create_dir_all(current.join("Projects")).unwrap();
        fs::write(current.join("notes.txt"), "notes").unwrap();

        let entries = folder_entries(&home, &current).unwrap();
        let displays = entries
            .iter()
            .map(|entry| entry.display.as_str())
            .collect::<Vec<_>>();
        assert_eq!(displays, ["󰁞  ..", "Projects", "notes.txt"]);
        assert!(
            entries
                .iter()
                .all(|entry| !entry.display.contains('\u{2029}'))
        );
        fs::remove_dir_all(home).unwrap();
    }

    fn test_home(name: &str) -> PathBuf {
        let path = env::temp_dir().join(format!("rofi-filesearch-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }
}
