//! Every test in the script, one mod per feature and one test per sub-feature.

mod clipboard {
    use std::env;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    use anyhow::Result;

    use crate::clipboard::*;
    use crate::model::{ClipboardItem, ItemKind};
    use crate::store::ClipboardStore;

    struct TestDirectory(PathBuf);

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn test_directory() -> TestDirectory {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        TestDirectory(env::temp_dir().join(format!(
            "rofi-clipboard-screenshot-test-{}-{unique}",
            std::process::id()
        )))
    }

    #[test]
    fn decodes_utf8_clipboard_url() {
        let value = decode_clipboard_text(b"https://example.com/image.png\n");

        assert_eq!(value, "https://example.com/image.png\n");
    }

    #[test]
    fn recognizes_standalone_urls_and_local_file_paths() {
        assert_eq!(
            standalone_file_source("https://example.com/docs/report.pdf\n").as_deref(),
            Some("https://example.com/docs/report.pdf")
        );
        assert_eq!(
            standalone_file_source("file:///home/raina/My%20Report.pdf").as_deref(),
            Some("/home/raina/My Report.pdf")
        );
        assert_eq!(
            standalone_file_source("/home/raina/My Report.pdf").as_deref(),
            Some("/home/raina/My Report.pdf")
        );
    }

    #[test]
    fn ordinary_text_that_mentions_a_url_stays_text() {
        assert!(standalone_file_source("See https://example.com for details").is_none());
        assert!(standalone_file_source("https://example.com\npage title").is_none());
    }

    #[test]
    fn standalone_url_text_is_stored_in_text_mode() -> Result<()> {
        let root = test_directory();
        let store = ClipboardStore::at(root.0.join("data"));

        store_text_or_file(
            &store,
            "https://example.com/report.pdf".to_owned(),
            "text/plain;charset=utf-8".to_owned(),
        )?;

        let history = store.load()?;
        assert_eq!(history.items.len(), 1);
        let item = &history.items[0];
        assert_eq!(item.kind, ItemKind::Text);
        assert_eq!(item.text.as_deref(), Some("https://example.com/report.pdf"));
        assert!(item.name.is_none());
        assert_eq!(item.mime, "text/plain;charset=utf-8");
        Ok(())
    }

    #[test]
    fn standalone_url_text_trims_surrounding_whitespace_into_text_mode() -> Result<()> {
        let root = test_directory();
        let store = ClipboardStore::at(root.0.join("data"));

        store_text_or_file(
            &store,
            "  https://example.com/page  ".to_owned(),
            "text/plain;charset=utf-8".to_owned(),
        )?;

        let item = &store.load()?.items[0];
        assert_eq!(item.kind, ItemKind::Text);
        assert_eq!(item.text.as_deref(), Some("https://example.com/page"));
        Ok(())
    }

    #[test]
    fn standalone_local_path_text_stays_in_file_mode() -> Result<()> {
        let root = test_directory();
        let store = ClipboardStore::at(root.0.join("data"));
        let path = root.0.join("share.png").to_string_lossy().into_owned();

        store_text_or_file(&store, path.clone(), "text/plain;charset=utf-8".to_owned())?;

        let item = &store.load()?.items[0];
        assert_eq!(item.kind, ItemKind::File);
        assert_eq!(item.name.as_deref(), Some(path.as_str()));
        Ok(())
    }

    #[test]
    fn text_that_only_mentions_a_url_is_stored_verbatim_as_text() -> Result<()> {
        let root = test_directory();
        let store = ClipboardStore::at(root.0.join("data"));
        let text = "See https://example.com for details".to_owned();

        store_text_or_file(&store, text.clone(), "text/plain;charset=utf-8".to_owned())?;

        let item = &store.load()?.items[0];
        assert_eq!(item.kind, ItemKind::Text);
        assert_eq!(item.text.as_deref(), Some(text.as_str()));
        assert!(item.name.is_none());
        Ok(())
    }

    #[test]
    fn builds_single_file_payloads_for_wayland_uri_targets() {
        assert_eq!(
            file_reference_payload(
                "text/uri-list",
                "file:///tmp/one.pdf\nfile:///tmp/two.pdf\n",
                "file:///tmp/two.pdf",
            ),
            "file:///tmp/two.pdf\n"
        );
        assert_eq!(
            file_reference_payload(
                "x-special/gnome-copied-files",
                "cut\nfile:///tmp/report.pdf\n",
                "file:///tmp/report.pdf",
            ),
            "cut\nfile:///tmp/report.pdf"
        );
    }

    #[test]
    fn local_paths_copy_back_as_uri_lists_for_dolphin() {
        let payload = local_file_copy_payload(Path::new("/home/raina/My Report #1.pdf"));

        assert_eq!(payload, b"file:///home/raina/My%20Report%20%231.pdf\n");
    }

    #[test]
    fn local_paths_use_uri_list_payloads_while_urls_stay_text() -> Result<()> {
        let root = test_directory();
        let store = ClipboardStore::at(root.0.join("data"));
        let mut item = ClipboardItem {
            id: 1,
            kind: ItemKind::File,
            text: Some("/home/raina/My Report.pdf".to_owned()),
            image_file: None,
            name: Some("/home/raina/My Report.pdf".to_owned()),
            mime: "text/plain;charset=utf-8".to_owned(),
            pinned: false,
            created_at: 0,
            digest: "digest".to_owned(),
        };

        assert_eq!(
            copy_payload(&store, &item)?,
            (
                TEXT_URI_LIST_MIME.to_owned(),
                b"file:///home/raina/My%20Report.pdf\n".to_vec()
            )
        );

        item.text = Some("https://example.com/report.pdf".to_owned());
        item.name = item.text.clone();
        assert_eq!(
            copy_payload(&store, &item)?,
            (
                "text/plain;charset=utf-8".to_owned(),
                b"https://example.com/report.pdf".to_vec()
            )
        );

        item.text = Some("file:///home/raina/My%20Report.pdf\n".to_owned());
        item.name = Some("/home/raina/My Report.pdf".to_owned());
        item.mime = "text/uri-list".to_owned();
        assert_eq!(
            copy_payload(&store, &item)?,
            (
                TEXT_URI_LIST_MIME.to_owned(),
                b"file:///home/raina/My%20Report.pdf\n".to_vec()
            )
        );

        item.text = Some("cut\nfile:///home/raina/My%20Report.pdf\n".to_owned());
        item.mime = GNOME_COPIED_FILES_MIME.to_owned();
        assert_eq!(
            copy_payload(&store, &item)?,
            (
                TEXT_URI_LIST_MIME.to_owned(),
                b"file:///home/raina/My%20Report.pdf\n".to_vec()
            )
        );
        Ok(())
    }

    #[test]
    fn decodes_bomless_utf16_little_endian_mozilla_url() {
        let mut bytes = Vec::new();
        for word in "https://example.com/image.png\nImage title".encode_utf16() {
            bytes.extend_from_slice(&word.to_le_bytes());
        }

        assert_eq!(
            decode_clipboard_text(&bytes),
            "https://example.com/image.png\nImage title"
        );
    }

    #[test]
    fn recognizes_chromium_and_mozilla_image_source_targets() {
        assert!(is_browser_image_source_mime("chromium/x-source-url"));
        assert!(is_browser_image_source_mime("text/x-moz-url"));
        assert!(is_browser_image_source_mime(
            "text/x-moz-url-data;charset=utf-8"
        ));
        assert!(!is_browser_image_source_mime("image/png"));
    }

    #[test]
    fn extracts_image_source_from_html() {
        let html =
            r#"<div><img alt="photo" src="https://example.com/image.png?a=1&amp;b=2"></div>"#;

        assert_eq!(
            image_source_from_html(html).as_deref(),
            Some("https://example.com/image.png?a=1&b=2")
        );
    }

    #[test]
    fn recognizes_niri_pathless_png_selection() {
        assert!(is_pathless_png("image/png\n"));
        assert!(!is_pathless_png("image/png\ntext/plain\n"));
        assert!(!is_pathless_png("image/jpeg\n"));
    }

    #[test]
    fn finds_screenshot_with_identical_png_bytes() {
        let directory = test_directory();
        fs::create_dir_all(&directory.0).unwrap();
        let matching = directory.0.join("Screenshot from 2026-08-11 12-00-00.png");
        fs::write(&matching, b"same PNG bytes").unwrap();
        fs::write(
            directory.0.join("Screenshot from 2026-08-11 12-00-01.png"),
            b"different bytes",
        )
        .unwrap();

        assert_eq!(
            matching_screenshot_path(&directory.0, b"same PNG bytes"),
            Some(matching)
        );
    }

    #[test]
    fn does_not_guess_a_screenshot_path_from_length_alone() {
        let directory = test_directory();
        fs::create_dir_all(&directory.0).unwrap();
        fs::write(
            directory.0.join("Screenshot from 2026-08-11 12-00-00.png"),
            b"different bytes",
        )
        .unwrap();

        assert_eq!(
            matching_screenshot_path(&directory.0, b"same byte count"),
            None
        );
    }
}

mod model {
    use std::path::Path;

    use crate::model::*;

    #[test]
    fn legacy_image_kind_loads_as_file() {
        assert_eq!(
            serde_json::from_str::<ItemKind>("\"image\"").unwrap(),
            ItemKind::File
        );
        assert_eq!(serde_json::to_string(&ItemKind::File).unwrap(), "\"file\"");
    }

    #[test]
    fn abbreviates_only_paths_inside_home() {
        let home = Path::new("/home/raina");

        assert_eq!(
            abbreviate_home_path_with("/home/raina/Documents/report.pdf", home),
            "~/Documents/report.pdf"
        );
        assert_eq!(abbreviate_home_path_with("/home/raina", home), "~");
        assert_eq!(
            abbreviate_home_path_with("/home/rainart/report.pdf", home),
            "/home/rainart/report.pdf"
        );
        assert_eq!(
            abbreviate_home_path_with("https://example.com/report.pdf", home),
            "https://example.com/report.pdf"
        );
    }

    #[test]
    fn url_value_recognizes_standalone_http_and_https_references() {
        assert_eq!(
            url_value("https://example.com/docs/report.pdf?a=1&b=2").as_deref(),
            Some("https://example.com/docs/report.pdf?a=1&b=2")
        );
        assert_eq!(
            url_value("http://example.com/image.png").as_deref(),
            Some("http://example.com/image.png")
        );
        assert_eq!(
            url_value("  https://example.com/page  ").as_deref(),
            Some("https://example.com/page")
        );
        assert_eq!(
            url_value("https://example.com/x?a=1&amp;b=2").as_deref(),
            Some("https://example.com/x?a=1&b=2")
        );
    }

    #[test]
    fn url_value_rejects_non_urls_and_urls_with_whitespace() {
        assert!(url_value("").is_none());
        assert!(url_value("/home/raina/report.pdf").is_none());
        assert!(url_value("file:///home/raina/report.pdf").is_none());
        assert!(url_value("See https://example.com for details").is_none());
        assert!(url_value("https://example.com\npage").is_none());
        assert!(url_value("ftp://example.com/resource").is_none());
    }
}

mod editor {
    use std::path::PathBuf;

    use rofi_preview_shared::panel_client::PanelContent;

    use crate::editor::*;
    use crate::model::{ClipboardItem, ItemKind};

    fn item(kind: ItemKind, text: Option<&str>, name: Option<&str>) -> ClipboardItem {
        ClipboardItem {
            id: 7,
            kind,
            text: text.map(str::to_owned),
            image_file: (kind == ItemKind::File).then(|| "7.png".to_owned()),
            name: name.map(str::to_owned),
            mime: match kind {
                ItemKind::Memo | ItemKind::Text => "text/plain",
                ItemKind::File => "image/png",
            }
            .to_owned(),
            pinned: false,
            created_at: 0,
            digest: "digest".to_owned(),
        }
    }

    #[test]
    fn editable_text_is_byte_for_byte_unchanged() {
        let original = "heading\r\n\t  repeated    spaces\n中文 👩🏽‍💻  \n";
        assert_eq!(
            panel_content(&item(ItemKind::Text, Some(original), None), None,),
            Some(PanelContent::EditableText(original.to_owned()))
        );
    }

    #[test]
    fn memo_content_uses_the_editable_text_panel() {
        assert_eq!(
            panel_content(&item(ItemKind::Memo, Some("draft memo"), None), None),
            Some(PanelContent::EditableText("draft memo".to_owned()))
        );
    }

    #[test]
    fn image_items_open_the_cached_image_preview() {
        let path = PathBuf::from("/home/raina/.local/share/rofi-clipboard/images/7.png");
        assert_eq!(
            panel_content(
                &item(
                    ItemKind::File,
                    None,
                    Some("/home/raina/Pictures/example.png"),
                ),
                Some(path.clone()),
            ),
            Some(PanelContent::Image(path))
        );
    }

    #[test]
    fn file_references_open_a_read_only_text_preview() {
        let mut file = item(
            ItemKind::File,
            Some("file:///home/raina/Documents/report.pdf\n"),
            Some("/home/raina/Documents/report.pdf"),
        );
        file.image_file = None;

        assert_eq!(
            panel_content(&file, None),
            Some(PanelContent::ReadOnlyText(
                "/home/raina/Documents/report.pdf".to_owned()
            ))
        );
    }

    #[test]
    fn unchanged_text_does_not_need_a_database_rewrite() {
        let item = item(ItemKind::Text, Some("typed text"), None);
        assert!(!text_is_changed(&item, "typed text").unwrap());
        assert!(text_is_changed(&item, "modified text").unwrap());
    }
}

mod rofi {
    use crate::model::{ClipboardItem, ItemKind};
    use crate::rofi::*;

    fn textual_item(id: u64, kind: ItemKind, text: &str, pinned: bool) -> ClipboardItem {
        ClipboardItem {
            id,
            kind,
            text: Some(text.to_owned()),
            image_file: None,
            name: None,
            mime: "text/plain".to_owned(),
            pinned,
            created_at: 0,
            digest: format!("digest-{id}"),
        }
    }

    fn image_item(name: Option<&str>) -> ClipboardItem {
        ClipboardItem {
            id: 1,
            kind: ItemKind::File,
            text: None,
            image_file: Some("1.png".to_owned()),
            name: name.map(str::to_owned),
            mime: "image/png".to_owned(),
            pinned: false,
            created_at: 0,
            digest: "digest".to_owned(),
        }
    }

    #[test]
    fn image_row_uses_internet_source_url() {
        let item = image_item(Some("https://example.com/images/photo.png"));

        assert_eq!(row_value(&item), "https://example.com/images/photo.png");
    }

    #[test]
    fn image_row_uses_local_source_path() {
        let item = image_item(Some("/home/raina/Pictures/photo.png"));

        assert_eq!(row_value(&item), "/home/raina/Pictures/photo.png");
    }

    #[test]
    fn image_row_falls_back_to_mime_for_entries_without_a_source() {
        let item = image_item(None);

        assert_eq!(row_value(&item), "Image · png");
    }

    #[test]
    fn memo_mode_excludes_pinned_clipboard_items() {
        let memo = textual_item(1, ItemKind::Memo, "memo", false);
        let pinned_text = textual_item(2, ItemKind::Text, "clipboard", true);
        let mut pinned_image = image_item(None);
        pinned_image.id = 3;
        pinned_image.pinned = true;

        assert!(Mode::Memo.includes(&memo));
        assert!(!Mode::Memo.includes(&pinned_text));
        assert!(!Mode::Memo.includes(&pinned_image));
        assert!(Mode::Text.includes(&pinned_text));
        assert!(Mode::Files.includes(&pinned_image));
    }

    #[test]
    fn text_mode_contains_urls_and_excludes_them_from_file_mode() {
        let url = textual_item(
            5,
            ItemKind::Text,
            "https://example.com/download.tar.zst",
            false,
        );

        assert!(Mode::Text.includes(&url));
        assert!(!Mode::Files.includes(&url));
        assert_eq!(row_value(&url), "https://example.com/download.tar.zst");
    }

    #[test]
    fn file_is_the_named_replacement_for_image_mode() {
        assert_eq!(Mode::parse("files").unwrap(), Mode::Files);
        assert_eq!(Mode::parse("images").unwrap(), Mode::Files);
        assert_eq!(Mode::Files.name(), "files");
        assert_eq!(Mode::Files.prompt(), "󰈔 Files");
    }

    #[test]
    fn memo_is_the_named_replacement_for_pinned_mode() {
        assert_eq!(Mode::parse("memo").unwrap(), Mode::Memo);
        assert_eq!(Mode::Memo.name(), "memo");
        assert_eq!(Mode::Memo.prompt(), "󰍩 Memo");
        assert!(Mode::parse("pinned").is_err());
    }

    #[test]
    fn empty_memo_has_a_visible_draft_label() {
        let memo = textual_item(4, ItemKind::Memo, "", false);

        assert_eq!(row_preview(&memo), "New memo");
        assert_eq!(row_value(&memo), "");
    }

    #[test]
    fn empty_memo_is_the_last_item_in_memo_mode() {
        let draft = textual_item(4, ItemKind::Memo, "", false);
        let newer = textual_item(3, ItemKind::Memo, "newer", false);
        let clipboard_text = textual_item(2, ItemKind::Text, "clipboard", false);
        let older = textual_item(1, ItemKind::Memo, "older", true);
        let history = vec![draft, newer, clipboard_text, older];

        let items = mode_items(&history, Mode::Memo);

        assert_eq!(
            items.iter().map(|item| item.id).collect::<Vec<_>>(),
            vec![3, 1, 4]
        );
    }

    #[test]
    fn text_row_preview_collapses_whitespace_to_one_line() {
        let item = textual_item(2, ItemKind::Text, "first line\nsecond\tline   third", false);

        assert_eq!(row_preview(&item), "first line second line third");
        assert_eq!(row_value(&item), "first line\nsecond\tline   third");
    }

    #[test]
    fn text_row_preview_truncates_long_text() {
        let text = "x".repeat(111);
        let item = textual_item(3, ItemKind::Text, &text, false);

        assert_eq!(row_preview(&item), format!("{}…", "x".repeat(110)));
    }

    #[test]
    fn deletion_selects_the_following_row_or_the_previous_row_at_the_end() {
        let first = textual_item(1, ItemKind::Text, "first", false);
        let second = textual_item(2, ItemKind::Text, "second", false);
        let third = textual_item(3, ItemKind::Text, "third", false);
        let items = vec![&first, &second, &third];

        assert_eq!(replacement_selection(&items, first.id), Some(second.id));
        assert_eq!(replacement_selection(&items, second.id), Some(third.id));
        assert_eq!(replacement_selection(&items, third.id), Some(second.id));
        assert_eq!(replacement_selection(&[&first], first.id), None);
    }

    #[test]
    fn initial_selection_prefers_the_first_unpinned_item_in_every_mode() {
        let first_pin = textual_item(1, ItemKind::Text, "first pin", true);
        let second_pin = textual_item(2, ItemKind::Text, "second pin", true);
        let first_unpinned = textual_item(3, ItemKind::Text, "first normal", false);
        let second_unpinned = textual_item(4, ItemKind::Text, "second normal", false);
        let items = vec![&first_pin, &second_pin, &first_unpinned, &second_unpinned];

        assert_eq!(preferred_selection(&items, None), Some(2));
        assert_eq!(preferred_selection(&items, Some(first_pin.id)), Some(0));
        assert_eq!(
            preferred_selection(&[&first_pin, &second_pin], None),
            Some(0)
        );
        assert_eq!(preferred_selection(&[], None), None);
    }
}

mod store {
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use anyhow::Result;

    use crate::model::{ClipboardItem, History, ItemKind};
    use crate::store::*;

    struct TestRoot(PathBuf);

    impl Drop for TestRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn test_root() -> TestRoot {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        TestRoot(std::env::temp_dir().join(format!(
            "rofi-clipboard-test-{}-{unique}",
            std::process::id()
        )))
    }

    fn item(id: u64, kind: ItemKind) -> ClipboardItem {
        ClipboardItem {
            id,
            kind,
            text: kind.is_textual().then(|| format!("text {id}")),
            image_file: (kind == ItemKind::File).then(|| format!("{id}.png")),
            name: None,
            mime: match kind {
                ItemKind::Memo | ItemKind::Text => "text/plain",
                ItemKind::File => "image/png",
            }
            .to_owned(),
            pinned: false,
            created_at: 0,
            digest: format!("digest-{id}"),
        }
    }

    #[test]
    fn editing_by_id_preserves_complete_text_and_does_not_modify_another_item() {
        let root = test_root();
        let store = ClipboardStore::at(root.0.clone());
        let editing_id = store
            .add_text("original".to_owned(), "text/plain".to_owned())
            .unwrap()
            .unwrap();
        let other_id = store
            .add_text("other selection".to_owned(), "text/plain".to_owned())
            .unwrap()
            .unwrap();
        let edited = "first line\n\tsecond  line\n中文 👩🏽‍💻  \n";

        assert!(store.edit_text(editing_id, edited.to_owned()).unwrap());

        let history = store.load().unwrap();
        assert_eq!(history.items[0].id, editing_id);
        assert_eq!(history.items[0].text.as_deref(), Some(edited));
        assert_eq!(
            history
                .items
                .iter()
                .find(|item| item.id == other_id)
                .and_then(|item| item.text.as_deref()),
            Some("other selection")
        );
    }

    #[test]
    fn file_reference_preserves_its_clipboard_payload_and_display_name() -> Result<()> {
        let root = test_root();
        let store = ClipboardStore::at(root.0.clone());
        let payload = "file:///home/raina/Documents/report.pdf\n";
        let id = store
            .add_file(
                payload.to_owned(),
                "text/uri-list".to_owned(),
                Some("/home/raina/Documents/report.pdf".to_owned()),
            )?
            .unwrap();

        let history = store.load()?;
        let item = history.items.iter().find(|item| item.id == id).unwrap();
        assert_eq!(item.kind, ItemKind::File);
        assert_eq!(item.text.as_deref(), Some(payload));
        assert_eq!(
            item.name.as_deref(),
            Some("/home/raina/Documents/report.pdf")
        );
        assert_eq!(store.item_bytes(item)?.as_slice(), payload.as_bytes());
        Ok(())
    }

    #[test]
    fn file_references_are_deduplicated_by_local_source_after_payload_conversion() -> Result<()> {
        let root = test_root();
        let store = ClipboardStore::at(root.0.clone());
        let source = "/home/raina/Documents/My Report.pdf";
        let id = store
            .add_file(
                source.to_owned(),
                "text/plain;charset=utf-8".to_owned(),
                Some(source.to_owned()),
            )?
            .unwrap();

        let second_id = store
            .add_file(
                "file:///home/raina/Documents/My%20Report.pdf\n".to_owned(),
                "text/uri-list".to_owned(),
                Some(source.to_owned()),
            )?
            .unwrap();

        assert_eq!(second_id, id);
        let history = store.load()?;
        assert_eq!(history.items.len(), 1);
        assert_eq!(history.items[0].mime, "text/uri-list");
        assert_eq!(
            history.items[0].text.as_deref(),
            Some("file:///home/raina/Documents/My%20Report.pdf\n")
        );
        Ok(())
    }

    #[test]
    fn pinning_moves_items_to_top_and_unpinning_moves_them_below_pins() -> Result<()> {
        let root = test_root();
        let store = ClipboardStore::at(root.0.clone());
        let older_text = store
            .add_text("older text".to_owned(), "text/plain".to_owned())?
            .unwrap();
        let image = store
            .add_image(b"image bytes", "image/png".to_owned())?
            .unwrap();
        let newer_text = store
            .add_text("newer text".to_owned(), "text/plain".to_owned())?
            .unwrap();

        assert!(store.pin(image)?);
        assert!(store.pin(older_text)?);
        assert_eq!(
            store
                .load()?
                .items
                .iter()
                .map(|item| item.id)
                .collect::<Vec<_>>(),
            vec![older_text, image, newer_text]
        );

        assert!(store.pin(image)?);
        let history = store.load()?;
        assert_eq!(
            history.items.iter().map(|item| item.id).collect::<Vec<_>>(),
            vec![older_text, image, newer_text]
        );
        assert!(history.items[0].pinned);
        assert!(!history.items[1].pinned);
        assert!(!history.items[2].pinned);
        Ok(())
    }

    #[test]
    fn memo_draft_is_unique_and_replaced_after_it_is_filled() -> Result<()> {
        let root = test_root();
        let store = ClipboardStore::at(root.0.clone());
        let text_id = store
            .add_text("same content".to_owned(), "text/plain".to_owned())?
            .unwrap();
        let memo_id = store.ensure_memo_draft()?;
        assert_eq!(store.ensure_memo_draft()?, memo_id);

        let draft = store
            .load()?
            .items
            .into_iter()
            .find(|item| item.id == memo_id)
            .unwrap();
        assert_eq!(draft.kind, ItemKind::Memo);
        assert_eq!(draft.text.as_deref(), Some(""));

        assert!(store.edit_text(memo_id, "same content".to_owned())?);
        let second_memo_id = store.ensure_memo_draft()?;
        assert_ne!(second_memo_id, memo_id);
        assert!(store.edit_text(second_memo_id, "same content".to_owned())?);
        let final_draft_id = store.ensure_memo_draft()?;
        assert_ne!(final_draft_id, second_memo_id);

        let history = store.load()?;
        assert_eq!(history.items.len(), 4);
        assert_eq!(
            history
                .items
                .iter()
                .filter(|item| item.is_empty_memo())
                .count(),
            1
        );
        assert!(
            history
                .items
                .iter()
                .any(|item| item.id == text_id && item.kind == ItemKind::Text)
        );
        assert!(history.items.iter().any(|item| {
            item.id == memo_id
                && item.kind == ItemKind::Memo
                && item.text.as_deref() == Some("same content")
        }));
        assert!(history.items.iter().any(|item| {
            item.id == second_memo_id
                && item.kind == ItemKind::Memo
                && item.text.as_deref() == Some("same content")
        }));
        assert!(history.items.iter().any(|item| {
            item.id == final_draft_id
                && item.kind == ItemKind::Memo
                && item.text.as_deref() == Some("")
                && !item.pinned
        }));
        Ok(())
    }

    #[test]
    fn pinning_a_memo_moves_it_above_other_memos() -> Result<()> {
        let root = test_root();
        let store = ClipboardStore::at(root.0.clone());
        let older = store.ensure_memo_draft()?;
        assert!(store.edit_text(older, "older".to_owned())?);
        let newer = store.ensure_memo_draft()?;
        assert!(store.edit_text(newer, "newer".to_owned())?);

        assert!(store.pin(older)?);
        let memo_ids: Vec<_> = store
            .load()?
            .items
            .iter()
            .filter(|item| item.kind == ItemKind::Memo && !item.is_empty_memo())
            .map(|item| item.id)
            .collect();
        assert_eq!(memo_ids, vec![older, newer]);
        Ok(())
    }

    #[test]
    fn clearing_a_memo_reuses_it_as_the_only_draft() -> Result<()> {
        let root = test_root();
        let store = ClipboardStore::at(root.0.clone());
        let memo_id = store.ensure_memo_draft()?;
        assert!(store.edit_text(memo_id, "keep me".to_owned())?);
        let old_draft_id = store.ensure_memo_draft()?;

        assert!(store.edit_text(memo_id, String::new())?);

        let history = store.load()?;
        let drafts: Vec<_> = history
            .items
            .iter()
            .filter(|item| item.is_empty_memo())
            .collect();
        assert_eq!(drafts.len(), 1);
        assert_eq!(drafts[0].id, memo_id);
        assert!(!history.items.iter().any(|item| item.id == old_draft_id));
        Ok(())
    }

    #[test]
    fn new_items_stay_below_pins_and_existing_history_is_repaired() -> Result<()> {
        let root = test_root();
        let store = ClipboardStore::at(root.0.clone());
        let history = History {
            next_id: 5,
            items: vec![
                item(1, ItemKind::Text),
                ClipboardItem {
                    pinned: true,
                    ..item(2, ItemKind::File)
                },
                item(3, ItemKind::File),
                ClipboardItem {
                    pinned: true,
                    ..item(4, ItemKind::Text)
                },
            ],
            ..History::default()
        };
        store.save_unlocked(&history)?;

        let new_id = store
            .add_text("new item".to_owned(), "text/plain".to_owned())?
            .unwrap();
        let history = store.load()?;

        assert_eq!(
            history.items.iter().map(|item| item.id).collect::<Vec<_>>(),
            vec![2, 4, new_id, 1, 3]
        );
        assert!(history.items[..2].iter().all(|item| item.pinned));
        assert!(history.items[2..].iter().all(|item| !item.pinned));
        Ok(())
    }

    #[test]
    fn removes_oldest_image_above_image_limit() {
        let mut history = History {
            items: (1..=101).rev().map(|id| item(id, ItemKind::File)).collect(),
            ..History::default()
        };

        let removed = trim_history(&mut history);

        assert_eq!(history.items.len(), MAX_IMAGE_ITEMS);
        assert_eq!(history.items.last().map(|item| item.id), Some(2));
        assert_eq!(removed, vec!["1.png"]);
    }

    #[test]
    fn file_references_do_not_count_toward_cached_image_limit() {
        let mut history = History {
            items: (1..=101)
                .rev()
                .map(|id| ClipboardItem {
                    id,
                    kind: ItemKind::File,
                    text: Some(format!("file:///tmp/{id}.pdf\n")),
                    image_file: None,
                    name: Some(format!("/tmp/{id}.pdf")),
                    mime: "text/uri-list".to_owned(),
                    pinned: false,
                    created_at: 0,
                    digest: format!("file-digest-{id}"),
                })
                .collect(),
            ..History::default()
        };

        let removed = trim_history(&mut history);

        assert_eq!(history.items.len(), 101);
        assert!(removed.is_empty());
    }

    #[test]
    fn image_limit_does_not_remove_text_items() {
        let mut history = History {
            items: (1..=251)
                .rev()
                .map(|id| {
                    let kind = if id >= 151 {
                        ItemKind::File
                    } else {
                        ItemKind::Text
                    };
                    item(id, kind)
                })
                .collect(),
            ..History::default()
        };

        let removed = trim_history(&mut history);

        assert_eq!(
            history
                .items
                .iter()
                .filter(|item| item.kind == ItemKind::Text)
                .count(),
            150
        );
        assert_eq!(
            history
                .items
                .iter()
                .filter(|item| item.image_file.is_some())
                .count(),
            MAX_IMAGE_ITEMS
        );
        assert_eq!(removed, vec!["151.png"]);
    }

    #[test]
    fn overall_history_limit_preserves_the_empty_memo_draft() {
        let mut history = History {
            next_id: MAX_HISTORY_ITEMS as u64 + 1,
            items: (1..=MAX_HISTORY_ITEMS as u64)
                .rev()
                .map(|id| ClipboardItem {
                    pinned: true,
                    ..item(id, ItemKind::Text)
                })
                .collect(),
            ..History::default()
        };

        let draft_id = ensure_memo_draft(&mut history);
        order_pinned_first(&mut history);
        let removed = trim_history(&mut history);

        assert_eq!(history.items.len(), MAX_HISTORY_ITEMS);
        assert!(history.items.iter().any(|item| item.id == draft_id));
        assert!(removed.is_empty());
    }

    #[test]
    fn adding_image_above_limit_deletes_oldest_cached_file() -> Result<()> {
        let root = test_root();
        let store = ClipboardStore::at(root.0.clone());
        fs::create_dir_all(&store.image_dir)?;

        let history = History {
            next_id: 101,
            items: (1..=100).rev().map(|id| item(id, ItemKind::File)).collect(),
            ..History::default()
        };
        store.save_unlocked(&history)?;

        let oldest_path = store.image_dir.join("1.png");
        fs::write(&oldest_path, b"oldest image")?;

        assert_eq!(
            store.add_image(b"new image", "image/png".to_owned())?,
            Some(101)
        );
        assert!(!oldest_path.exists());

        let history = store.load()?;
        assert_eq!(
            history
                .items
                .iter()
                .filter(|item| item.image_file.is_some())
                .count(),
            MAX_IMAGE_ITEMS
        );
        assert_eq!(history.items.first().map(|item| item.id), Some(101));
        assert!(store.image_dir.join("101.png").exists());
        Ok(())
    }

    #[test]
    fn pruning_missing_local_images_removes_rows_and_cached_files() -> Result<()> {
        let root = test_root();
        let store = ClipboardStore::at(root.0.join("data"));
        let existing_source = root.0.join("existing.png");
        let deleted_source = root.0.join("deleted.png");
        let deleted_pinned_source = root.0.join("deleted-pinned.png");
        fs::create_dir_all(&root.0)?;
        fs::write(&existing_source, b"existing source")?;
        fs::write(&deleted_source, b"deleted source")?;
        fs::write(&deleted_pinned_source, b"deleted pinned source")?;

        let existing_id = store
            .add_image_named(
                b"existing cache",
                "image/png".to_owned(),
                Some(existing_source.to_string_lossy().into_owned()),
            )?
            .unwrap();
        let deleted_id = store
            .add_image_named(
                b"deleted cache",
                "image/png".to_owned(),
                Some(deleted_source.to_string_lossy().into_owned()),
            )?
            .unwrap();
        let deleted_pinned_id = store
            .add_image_named(
                b"deleted pinned cache",
                "image/png".to_owned(),
                Some(deleted_pinned_source.to_string_lossy().into_owned()),
            )?
            .unwrap();
        assert!(store.pin(deleted_pinned_id)?);
        let url_id = store
            .add_image_named(
                b"url cache",
                "image/png".to_owned(),
                Some("https://example.com/image.png".to_owned()),
            )?
            .unwrap();
        let clipboard_only_id = store
            .add_image(b"clipboard cache", "image/png".to_owned())?
            .unwrap();
        let text_id = store
            .add_text("keep text".to_owned(), "text/plain".to_owned())?
            .unwrap();

        let history = store.load()?;
        let deleted_cache = store.image_path(
            history
                .items
                .iter()
                .find(|item| item.id == deleted_id)
                .unwrap(),
        );
        let deleted_pinned_cache = store.image_path(
            history
                .items
                .iter()
                .find(|item| item.id == deleted_pinned_id)
                .unwrap(),
        );
        fs::remove_file(&deleted_source)?;
        fs::remove_file(&deleted_pinned_source)?;

        assert_eq!(store.prune_missing_local_files()?, 2);

        let history = store.load()?;
        let retained_ids: Vec<_> = history.items.iter().map(|item| item.id).collect();
        assert!(!retained_ids.contains(&deleted_id));
        assert!(!retained_ids.contains(&deleted_pinned_id));
        assert!(retained_ids.contains(&existing_id));
        assert!(retained_ids.contains(&url_id));
        assert!(retained_ids.contains(&clipboard_only_id));
        assert!(retained_ids.contains(&text_id));
        assert!(!deleted_cache.unwrap().exists());
        assert!(!deleted_pinned_cache.unwrap().exists());
        assert_eq!(store.prune_missing_local_files()?, 0);
        Ok(())
    }

    #[test]
    fn pruning_missing_local_file_references_keeps_existing_paths_and_urls() -> Result<()> {
        let root = test_root();
        let store = ClipboardStore::at(root.0.join("data"));
        let existing_source = root.0.join("existing report.pdf");
        let deleted_source = root.0.join("deleted report.pdf");
        fs::create_dir_all(&root.0)?;
        fs::write(&existing_source, b"existing")?;
        fs::write(&deleted_source, b"deleted")?;

        let existing_id = store
            .add_file(
                format!("file://{}\n", existing_source.to_string_lossy()),
                "text/uri-list".to_owned(),
                Some(existing_source.to_string_lossy().into_owned()),
            )?
            .unwrap();
        let deleted_id = store
            .add_file(
                format!("file://{}\n", deleted_source.to_string_lossy()),
                "text/uri-list".to_owned(),
                Some(deleted_source.to_string_lossy().into_owned()),
            )?
            .unwrap();
        assert!(store.pin(deleted_id)?);
        let url_id = store
            .add_file(
                "https://example.com/report.pdf".to_owned(),
                "text/plain".to_owned(),
                Some("https://example.com/report.pdf".to_owned()),
            )?
            .unwrap();
        fs::remove_file(&deleted_source)?;

        assert_eq!(store.prune_missing_local_files()?, 1);

        let retained_ids: Vec<_> = store.load()?.items.iter().map(|item| item.id).collect();
        assert!(retained_ids.contains(&existing_id));
        assert!(!retained_ids.contains(&deleted_id));
        assert!(retained_ids.contains(&url_id));
        assert_eq!(store.prune_missing_local_files()?, 0);
        Ok(())
    }
}

mod waybar {
    use crate::model::{ClipboardItem, ItemKind};
    use crate::waybar::*;

    #[test]
    fn tooltip_collapses_multiline_text_into_one_preview_row() {
        let item = ClipboardItem {
            id: 1,
            kind: ItemKind::Text,
            text: Some("  hello\n  world  ".to_owned()),
            image_file: None,
            name: None,
            mime: "text/plain;charset=utf-8".to_owned(),
            pinned: false,
            created_at: 1,
            digest: "d".to_owned(),
        };
        assert_eq!(preview_line(&item).as_deref(), Some("hello world"));
        assert!(tooltip(1, Some(&item)).contains("Last: hello world"));
    }

    #[test]
    fn empty_text_yields_no_preview() {
        let item = ClipboardItem {
            id: 1,
            kind: ItemKind::Text,
            text: Some("   \n  ".to_owned()),
            image_file: None,
            name: None,
            mime: "text/plain;charset=utf-8".to_owned(),
            pinned: false,
            created_at: 1,
            digest: "d".to_owned(),
        };
        assert_eq!(preview_line(&item), None);
    }

    #[test]
    fn long_text_is_truncated_with_an_ellipsis() {
        let item = ClipboardItem {
            id: 1,
            kind: ItemKind::Text,
            text: Some("a".repeat(80)),
            image_file: None,
            name: None,
            mime: "text/plain;charset=utf-8".to_owned(),
            pinned: false,
            created_at: 1,
            digest: "d".to_owned(),
        };
        let preview = preview_line(&item).unwrap();
        assert_eq!(preview.chars().count(), PREVIEW_LIMIT + 1);
        assert!(preview.ends_with('…'));
    }

    #[test]
    fn pluralization_is_correct() {
        assert_eq!(plural_s(0), "s");
        assert_eq!(plural_s(1), "");
        assert_eq!(plural_s(2), "s");
    }
}
