//! All clipboard policy, capture, migration, and real-binary CLI tests.

// Expand unit tests inside their production modules to retain private access.
macro_rules! clipboard_unit_tests {
    (store) => {
        mod tests {
            use super::*;
            use std::sync::atomic::{AtomicU64, Ordering};

            static SERIAL: AtomicU64 = AtomicU64::new(0);

            struct Fixture(Store);
            impl Fixture {
                fn new() -> Self {
                    Self(Store {
                        root: std::env::temp_dir().join(format!(
                            "clipboard-test-{}-{}",
                            std::process::id(),
                            SERIAL.fetch_add(1, Ordering::Relaxed)
                        )),
                    })
                }
                fn history(&self) -> History {
                    serde_json::from_slice(&fs::read(self.0.root.join("history.json")).unwrap()).unwrap()
                }

                fn write(&self, history: &History) {
                    fs::create_dir_all(&self.0.root).unwrap();
                    fs::write(
                        self.0.root.join("history.json"),
                        serde_json::to_vec(history).unwrap(),
                    )
                    .unwrap();
                }
            }
            impl Drop for Fixture {
                fn drop(&mut self) {
                    let _ = fs::remove_dir_all(&self.0.root);
                }
            }

            #[test]
            fn memo_save_creates_one_new_draft_and_keeps_ids_stable() {
                let fixture = Fixture::new();
                let id = fixture.0.memo().unwrap()["id"].as_u64().unwrap();
                assert_eq!(fixture.0.memo().unwrap()["id"], id);
                fixture.0.save(id, "", "memo\n你好".into()).unwrap();
                fixture.0.pin(id).unwrap();
                let history = fixture.history();
                assert_eq!(history.items.len(), 2);
                assert_eq!(history.items[0].id, id);
                assert!(history.items[0].pinned);
                assert_eq!(history.items.iter().filter(|item| item.draft()).count(), 1);
            }

            #[test]
            fn conflicting_save_and_deleted_item_do_not_overwrite_history() {
                let fixture = Fixture::new();
                let id = fixture.0.memo().unwrap()["id"].as_u64().unwrap();
                fixture.0.save(id, "", "external edit".into()).unwrap();
                assert!(fixture.0.save(id, "", "stale edit".into()).is_err());
                assert_eq!(
                    fixture
                        .history()
                        .items
                        .iter()
                        .find(|item| item.id == id)
                        .unwrap()
                        .text
                        .as_deref(),
                    Some("external edit")
                );
                fixture.0.delete(id).unwrap();
                assert!(
                    fixture
                        .0
                        .save(id, "external edit", "resurrect".into())
                        .is_err()
                );
            }

            #[test]
            fn old_image_alias_and_store_format_are_accepted() {
                let fixture = Fixture::new();
                fs::create_dir_all(fixture.0.root.join("images")).unwrap();
                fs::write(fixture.0.root.join("images/7.png"), b"image bytes").unwrap();
                fs::write(
                    fixture.0.root.join("history.json"),
                    r#"{"version":1,"next_id":8,"items":[
                    {"id":7,"kind":"image","text":null,"image_file":"7.png","mime":"image/png",
                     "pinned":false,"created_at":1,"digest":"old"}]}"#,
                )
                .unwrap();
                assert_eq!(fixture.0.snapshot().unwrap()["items"][0]["kind"], "file");
                assert_eq!(fixture.0.payload(7).unwrap()["mime"], "image/png");
                fixture.0.delete(7).unwrap();
                assert!(!fixture.0.root.join("images/7.png").exists());
            }

            #[test]
            fn invalid_images_and_future_versions_are_rejected() {
                let fixture = Fixture::new();
                fixture.0.memo().unwrap();
                let mut history = fixture.history();
                history.items[0].image_file = Some("../secret".into());
                fs::write(
                    fixture.0.root.join("history.json"),
                    serde_json::to_vec(&history).unwrap(),
                )
                .unwrap();
                assert!(fixture.0.snapshot().is_err());
                history.version = 2;
                fs::write(
                    fixture.0.root.join("history.json"),
                    serde_json::to_vec(&history).unwrap(),
                )
                .unwrap();
                assert!(fixture.0.memo().is_err());
            }

            #[test]
            fn concurrent_writers_share_the_history_flock() {
                let fixture = Fixture::new();
                let id = fixture.0.memo().unwrap()["id"].as_u64().unwrap();
                let threads: Vec<_> = (0..12)
                    .map(|_| {
                        let root = fixture.0.root.clone();
                        std::thread::spawn(move || Store { root }.pin(id).unwrap())
                    })
                    .collect();
                for thread in threads {
                    thread.join().unwrap();
                }
                assert!(!fixture.history().items[0].pinned);
            }

            #[test]
            fn file_uris_are_encoded_and_digests_remain_compatible() {
                assert_eq!(file_uri(Path::new("/tmp/a b#c")), "file:///tmp/a%20b%23c\n");
                assert_eq!(digest(b""), "cbf29ce484222325-0");
                assert_eq!(digest(b"hello"), "a430d84680aabd0b-5");
            }

            #[test]
            fn file_payload_does_not_trim_a_literal_source_filename() {
                let fixture = Fixture::new();
                fixture.0.memo().unwrap();
                let source = fixture.0.root.join("report ");
                fs::write(&source, b"intended file").unwrap();
                fs::write(fixture.0.root.join("report"), b"different file").unwrap();
                fixture
                    .0
                    .capture(vec![Captured::File {
                        text: file_uri(&source),
                        mime: "text/uri-list".into(),
                        name: Some(source.to_str().unwrap().into()),
                    }])
                    .unwrap();
                let id = fixture.history().items[0].id;
                assert_eq!(fixture.0.payload(id).unwrap()["text"], file_uri(&source));
            }

            #[test]
            fn text_deduplication_preserves_pins_and_rejects_empty_or_file_edits() {
                let fixture = Fixture::new();
                fixture.0.memo().unwrap();
                let mut history = fixture.history();
                let mut first = history.items[0].clone();
                first.kind = Kind::Text;
                first.text = Some("first".into());
                first.digest = digest(b"first");
                let mut duplicate = first.clone();
                duplicate.id = 2;
                duplicate.text = Some("duplicate".into());
                duplicate.digest = digest(b"duplicate");
                duplicate.pinned = true;
                history.next_id = 3;
                history.items = vec![first, duplicate];
                fixture.write(&history);
                fixture.0.save(1, "first", "duplicate".into()).unwrap();
                let history = fixture.history();
                assert_eq!(history.items.len(), 1);
                assert_eq!(history.items[0].id, 1);
                assert!(history.items[0].pinned);
                assert!(fixture.0.save(1, "duplicate", String::new()).is_err());
                let mut history = fixture.history();
                history.items[0].kind = Kind::File;
                fixture.write(&history);
                assert!(fixture.0.save(1, "duplicate", "file edit".into()).is_err());
            }

            #[test]
            fn prepare_prunes_only_missing_linked_files_and_their_image_cache() {
                let fixture = Fixture::new();
                fixture.0.memo().unwrap();
                let mut history = fixture.history();
                let mut missing = history.items[0].clone();
                missing.kind = Kind::File;
                missing.name = Some(fixture.0.root.join("missing.png").display().to_string());
                missing.image_file = Some("1.png".into());
                let mut remote = missing.clone();
                remote.id = 2;
                remote.name = Some("https://example.org/image.png".into());
                remote.image_file = Some("2.png".into());
                history.items = vec![missing, remote];
                fixture.write(&history);
                fs::create_dir_all(fixture.0.root.join("images")).unwrap();
                fs::write(fixture.0.root.join("images/1.png"), b"missing").unwrap();
                fs::write(fixture.0.root.join("images/2.png"), b"remote").unwrap();
                fixture.0.prepare().unwrap();
                let history = fixture.history();
                assert_eq!(history.items.len(), 2);
                assert!(history.items.iter().any(|item| item.id == 2));
                assert_eq!(history.items.iter().filter(|item| item.draft()).count(), 1);
                assert!(!fixture.0.root.join("images/1.png").exists());
                assert!(fixture.0.root.join("images/2.png").exists());
            }

            #[test]
            fn independent_image_limit_and_overall_limit_preserve_the_memo_draft() {
                let mut history = History::default();
                ensure_draft(&mut history);
                let draft = history.items[0].clone();
                for id in 2..=2200 {
                    let mut item = draft.clone();
                    item.id = id;
                    item.kind = Kind::File;
                    item.text = None;
                    item.image_file = Some(format!("{id}.png"));
                    history.items.insert(0, item);
                }
                let removed = trim(&mut history);
                assert_eq!(removed.len(), 2099);
                assert_eq!(history.items.len(), 101);
                assert!(history.items.iter().any(Item::draft));
                for id in 2201..=4400 {
                    let mut item = draft.clone();
                    item.id = id;
                    item.kind = Kind::Text;
                    item.text = Some(id.to_string());
                    history.items.insert(0, item);
                }
                trim(&mut history);
                assert_eq!(history.items.len(), 2000);
                assert!(history.items.iter().any(Item::draft));
            }

            #[test]
            fn request_protocol_rejects_unknown_fields_and_incomplete_saves() {
                use super::super::Request;
                assert!(serde_json::from_str::<Request>(r#"{"command":"snapshot","id":1}"#).is_err());
                assert!(serde_json::from_str::<Request>(r#"{"command":"save","id":1,"text":"x"}"#).is_err());
                assert!(serde_json::from_str::<Request>(r#"{"command":"delete","id":-1}"#).is_err());
                assert!(serde_json::from_str::<Request>(r#"{"command":"copy","id":1}"#).is_err());
                assert!(serde_json::from_str::<Request>(r#"{"command":"snapshot"}"#).is_ok());
            }

            #[test]
            fn unchanged_prepare_and_memo_do_not_replace_the_history_file() {
                use std::os::unix::fs::MetadataExt;
                let fixture = Fixture::new();
                let prepared = fixture.0.prepare().unwrap();
                assert_eq!(prepared["items"].as_array().unwrap().len(), 1);
                let path = fixture.0.root.join("history.json");
                let before = fs::metadata(&path).unwrap();
                let repeated = fixture.0.prepare().unwrap();
                let memo = fixture.0.memo().unwrap();
                let after = fs::metadata(&path).unwrap();
                assert_eq!(prepared, repeated);
                assert_eq!(memo["items"], prepared["items"]);
                assert_eq!(before.ino(), after.ino());
                assert_eq!(before.modified().unwrap(), after.modified().unwrap());
            }

            #[test]
            fn mutations_return_the_committed_snapshot_and_delete_replaces_the_draft() {
                let fixture = Fixture::new();
                let id = fixture.0.prepare().unwrap()["id"].as_u64().unwrap();
                let saved = fixture.0.save(id, "", "saved memo".into()).unwrap();
                assert_eq!(saved["items"], fixture.0.snapshot().unwrap()["items"]);
                let pinned = fixture.0.pin(id).unwrap();
                assert_eq!(pinned["items"][0]["pinned"], true);
                let draft = pinned["items"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|item| item["text"] == "")
                    .unwrap()["id"]
                    .as_u64()
                    .unwrap();
                let deleted = fixture.0.delete(draft).unwrap();
                assert_eq!(deleted["items"], fixture.0.snapshot().unwrap()["items"]);
                let history = fixture.history();
                assert_eq!(history.items.iter().filter(|item| item.draft()).count(), 1);
                assert!(!history.items.iter().any(|item| item.id == draft));
            }

            fn captured_text(text: &str) -> Captured {
                Captured::Text {
                    text: text.into(),
                    mime: "text/plain;charset=utf-8".into(),
                }
            }

            fn captured_image(bytes: &[u8], name: Option<&str>) -> Captured {
                Captured::Image {
                    bytes: bytes.into(),
                    mime: "image/png".into(),
                    name: name.map(str::to_owned),
                }
            }

            #[test]
            fn captured_text_reuses_ids_and_pins_without_deduplicating_memos() {
                let fixture = Fixture::new();
                let memo = fixture.0.memo().unwrap()["id"].as_u64().unwrap();
                fixture.0.save(memo, "", "hello".into()).unwrap();
                fixture.0.capture(vec![captured_text("hello")]).unwrap();
                let id = fixture
                    .history()
                    .items
                    .iter()
                    .find(|item| item.kind == Kind::Text)
                    .unwrap()
                    .id;
                fixture.0.pin(id).unwrap();
                fixture
                    .0
                    .capture(vec![
                        captured_text("newer"),
                        Captured::Text {
                            text: "hello".into(),
                            mime: "text/plain".into(),
                        },
                    ])
                    .unwrap();
                let history = fixture.history();
                assert_eq!(history.items.len(), 4);
                assert_eq!(history.items[0].id, id);
                assert!(history.items[0].pinned);
                assert_eq!(history.items[0].mime, "text/plain");
                assert_eq!(history.items[0].digest, digest(b"hello"));
                assert_eq!(
                    history
                        .items
                        .iter()
                        .filter(|item| item.kind == Kind::Memo)
                        .count(),
                    2
                );
                assert_eq!(
                    history
                        .items
                        .iter()
                        .filter(|item| item.kind == Kind::Text)
                        .count(),
                    2
                );
            }

            #[test]
            fn file_reference_deduplication_keeps_the_source_pin_and_converted_payload() {
                let fixture = Fixture::new();
                let file = |text: &str, mime: &str| Captured::File {
                    text: text.into(),
                    mime: mime.into(),
                    name: Some("/tmp/My Report.pdf".into()),
                };
                fixture
                    .0
                    .capture(vec![file("/tmp/My Report.pdf", "text/plain")])
                    .unwrap();
                let id = fixture.history().items[0].id;
                fixture.0.pin(id).unwrap();
                fixture
                    .0
                    .capture(vec![file(
                        "cut\nfile:///tmp/My%20Report.pdf",
                        "x-special/gnome-copied-files",
                    )])
                    .unwrap();
                let history = fixture.history();
                assert_eq!(history.items.len(), 1);
                assert_eq!(history.items[0].id, id);
                assert!(history.items[0].pinned);
                assert_eq!(
                    history.items[0].digest,
                    digest(b"cut\nfile:///tmp/My%20Report.pdf")
                );
            }

            #[test]
            fn image_deduplication_preserves_id_pin_source_and_repairs_a_missing_cache() {
                let fixture = Fixture::new();
                fixture
                    .0
                    .capture(vec![captured_image(
                        b"image",
                        Some("https://example.org/original.png"),
                    )])
                    .unwrap();
                let id = fixture.history().items[0].id;
                fixture.0.pin(id).unwrap();
                fixture
                    .0
                    .capture(vec![captured_image(b"image", None)])
                    .unwrap();
                assert_eq!(fixture.history().items.len(), 1);
                let path = fixture.0.root.join("images/1.png");
                fs::remove_file(&path).unwrap();
                fixture
                    .0
                    .capture(vec![captured_image(
                        b"image",
                        Some("https://example.org/new.png"),
                    )])
                    .unwrap();
                let history = fixture.history();
                assert_eq!(history.items[0].id, id);
                assert!(history.items[0].pinned);
                assert_eq!(
                    history.items[0].name.as_deref(),
                    Some("https://example.org/new.png")
                );
                assert_eq!(history.next_id, 2);
                assert_eq!(fs::read(path).unwrap(), b"image");
            }

            #[test]
            fn capture_retention_deletes_only_evicted_cache_files_and_keeps_pins_and_draft() {
                let fixture = Fixture::new();
                fixture.0.memo().unwrap();
                fixture
                    .0
                    .capture(vec![captured_image(b"pinned image", None)])
                    .unwrap();
                fixture.0.pin(2).unwrap();
                fixture
                    .0
                    .capture(
                        (0..101)
                            .map(|n| captured_image(n.to_string().as_bytes(), None))
                            .collect(),
                    )
                    .unwrap();
                let history = fixture.history();
                assert_eq!(
                    history
                        .items
                        .iter()
                        .filter(|item| item.image_file.is_some())
                        .count(),
                    100
                );
                assert_eq!(history.items[0].id, 2);
                assert_eq!(
                    fs::read_dir(fixture.0.root.join("images")).unwrap().count(),
                    100
                );
                assert!(fixture.0.root.join("images/2.png").exists());
                assert!(!fixture.0.root.join("images/3.png").exists());
                assert!(!fixture.0.root.join("images/4.png").exists());
                fixture
                    .0
                    .capture(
                        (0..2100)
                            .map(|n| captured_text(&format!("text {n}")))
                            .collect(),
                    )
                    .unwrap();
                let history = fixture.history();
                assert_eq!(history.items.len(), 2000);
                assert_eq!(history.items[0].id, 2);
                assert!(history.items.iter().any(Item::draft));
                assert_eq!(
                    fs::read_dir(fixture.0.root.join("images")).unwrap().count(),
                    1
                );
            }

            #[test]
            fn a_failed_batch_or_commit_leaves_history_and_existing_images_untouched() {
                let fixture = Fixture::new();
                fixture.0.memo().unwrap();
                let path = fixture.0.root.join("history.json");
                let before = fs::read(&path).unwrap();
                fs::create_dir(fixture.0.root.join("images")).unwrap();
                let orphan = fixture.0.root.join("images/3.png");
                fs::write(&orphan, b"existing orphan must not be overwritten").unwrap();
                assert!(
                    fixture
                        .0
                        .capture(vec![
                            captured_image(b"first", None),
                            captured_image(b"second", None)
                        ])
                        .is_err()
                );
                assert_eq!(fs::read(&path).unwrap(), before);
                assert!(!fixture.0.root.join("images/2.png").exists());
                assert_eq!(
                    fs::read(&orphan).unwrap(),
                    b"existing orphan must not be overwritten"
                );
                // A blocked atomic history write must roll back a newly created cache.
                let temporary = fixture
                    .0
                    .root
                    .join(format!(".clipboard-panel.{}.tmp", std::process::id()));
                fs::create_dir(&temporary).unwrap();
                assert!(
                    fixture
                        .0
                        .capture(vec![captured_image(b"new", None)])
                        .is_err()
                );
                assert_eq!(fs::read(&path).unwrap(), before);
                assert!(!fixture.0.root.join("images/2.png").exists());
                assert!(temporary.is_dir());
                fs::remove_dir(temporary).unwrap();
                let mut history = fixture.history();
                history.items[0].image_file = Some("../unsafe".into());
                fixture.write(&history);
                let before = fs::read(&path).unwrap();
                assert!(
                    fixture
                        .0
                        .capture(vec![captured_text("not committed")])
                        .is_err()
                );
                assert!(fixture.0.pin(1).is_err());
                assert_eq!(fs::read(path).unwrap(), before);
            }

            #[test]
            fn capture_and_panel_writers_do_not_lose_each_others_transactions() {
                let fixture = Fixture::new();
                let id = fixture.0.memo().unwrap()["id"].as_u64().unwrap();
                let threads: Vec<_> = (0..24)
                    .map(|n| {
                        let root = fixture.0.root.clone();
                        std::thread::spawn(move || {
                            let store = Store { root };
                            if n % 2 == 0 {
                                store.pin(id).unwrap();
                            } else {
                                store
                                    .capture(vec![captured_text(&format!("capture {n}"))])
                                    .unwrap();
                            }
                        })
                    })
                    .collect();
                for thread in threads {
                    thread.join().unwrap();
                }
                let history = fixture.history();
                assert_eq!(history.items.len(), 13);
                assert!(
                    !history
                        .items
                        .iter()
                        .find(|item| item.id == id)
                        .unwrap()
                        .pinned
                );
                assert_eq!(history.next_id, 14);
            }

            fn legacy_store(base: &Path) -> Store {
                let store = Store {
                    root: base.join("rofi-clipboard"),
                };
                store.memo().unwrap();
                store.save(1, "", "preserved memo 你好".into()).unwrap();
                store
                    .capture(vec![captured_image(b"preserved image", None)])
                    .unwrap();
                store.pin(3).unwrap();
                store
            }

            #[test]
            fn default_discovery_migrates_history_and_images_once_without_changing_the_source() {
                use std::os::unix::fs::PermissionsExt;
                let fixture = Fixture::new();
                let legacy = legacy_store(&fixture.0.root);
                let legacy_history = legacy.root.join("history.json");
                let bytes = String::from_utf8(fs::read(&legacy_history).unwrap())
                    .unwrap()
                    .replace("\"kind\": \"file\"", "\"kind\": \"image\"")
                    .into_bytes();
                fs::write(&legacy_history, &bytes).unwrap();
                let migrated = Store::at_data_home(&fixture.0.root).unwrap();
                assert_eq!(migrated.root, fixture.0.root.join("clipboard"));
                assert_eq!(fs::read(migrated.root.join("history.json")).unwrap(), bytes);
                assert_eq!(fs::read(legacy_history).unwrap(), bytes);
                assert_eq!(
                    fs::read(migrated.root.join("images/3.png")).unwrap(),
                    b"preserved image"
                );
                assert_eq!(
                    fs::read(legacy.root.join("images/3.png")).unwrap(),
                    b"preserved image"
                );
                assert!(
                    migrated.snapshot().unwrap()["items"][0]["pinned"]
                        .as_bool()
                        .unwrap()
                );
                assert_eq!(
                    fs::metadata(&migrated.root).unwrap().permissions().mode() & 0o777,
                    0o700
                );
                assert_eq!(
                    fs::metadata(migrated.root.join("history.json"))
                        .unwrap()
                        .permissions()
                        .mode()
                        & 0o777,
                    0o600
                );
                migrated
                    .capture(vec![captured_text("new history")])
                    .unwrap();
                let committed = fs::read(migrated.root.join("history.json")).unwrap();
                let again = Store::at_data_home(&fixture.0.root).unwrap();
                assert_eq!(
                    fs::read(again.root.join("history.json")).unwrap(),
                    committed
                );
            }

            #[test]
            fn existing_new_directory_is_never_replaced_or_merged_even_if_empty() {
                let fixture = Fixture::new();
                legacy_store(&fixture.0.root);
                fs::create_dir(fixture.0.root.join("clipboard")).unwrap();
                let store = Store::at_data_home(&fixture.0.root).unwrap();
                assert!(
                    store.snapshot().unwrap()["items"]
                        .as_array()
                        .unwrap()
                        .is_empty()
                );
                assert!(!store.root.join("history.json").exists());
            }

            #[test]
            fn invalid_legacy_history_and_unsafe_cache_fail_without_publishing_a_new_store() {
                use std::os::unix::fs::symlink;
                let fixture = Fixture::new();
                let legacy = legacy_store(&fixture.0.root);
                let path = legacy.root.join("history.json");
                let original = fs::read(&path).unwrap();
                for invalid in [
                    b"not JSON".to_vec(),
                    String::from_utf8(original.clone())
                        .unwrap()
                        .replace("\"version\": 1", "\"version\": 2")
                        .into_bytes(),
                ] {
                    fs::write(&path, &invalid).unwrap();
                    assert!(Store::at_data_home(&fixture.0.root).is_err());
                    assert_eq!(fs::read(&path).unwrap(), invalid);
                    assert!(!fixture.0.root.join("clipboard").exists());
                }
                fs::write(&path, &original).unwrap();
                let cache = legacy.root.join("images/3.png");
                fs::remove_file(&cache).unwrap();
                symlink(&path, &cache).unwrap();
                assert!(Store::at_data_home(&fixture.0.root).is_err());
                assert_eq!(fs::read(&path).unwrap(), original);
                assert!(!fixture.0.root.join("clipboard").exists());
                assert!(!fs::read_dir(&fixture.0.root).unwrap().any(|entry| {
                    entry
                        .unwrap()
                        .file_name()
                        .to_string_lossy()
                        .starts_with(".clipboard-migration-")
                }));
            }

            #[test]
            fn concurrent_discovery_and_capture_publish_one_complete_migrated_store() {
                let fixture = Fixture::new();
                legacy_store(&fixture.0.root);
                let threads: Vec<_> = (0..12)
                    .map(|n| {
                        let base = fixture.0.root.clone();
                        std::thread::spawn(move || {
                            let store = Store::at_data_home(&base).unwrap();
                            store
                                .capture(vec![captured_text(&format!("capture {n}"))])
                                .unwrap();
                        })
                    })
                    .collect();
                for thread in threads {
                    thread.join().unwrap();
                }
                let store = Store::at_data_home(&fixture.0.root).unwrap();
                let items = store.snapshot().unwrap()["items"]
                    .as_array()
                    .unwrap()
                    .clone();
                assert_eq!(items.len(), 15);
                assert_eq!(
                    fs::read(store.root.join("images/3.png")).unwrap(),
                    b"preserved image"
                );
                assert!(
                    items
                        .iter()
                        .any(|item| item["text"] == "preserved memo 你好")
                );
            }

            #[test]
            fn migration_waits_for_the_source_history_lock_before_reading() {
                let fixture = Fixture::new();
                let legacy = legacy_store(&fixture.0.root);
                let lock = OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(legacy.root.join("history.lock"))
                    .unwrap();
                // SAFETY: lock owns the valid descriptor until explicitly dropped below.
                assert_eq!(unsafe { flock(lock.as_raw_fd(), 2) }, 0);
                let (send, receive) = std::sync::mpsc::channel();
                let base = fixture.0.root.clone();
                let worker = std::thread::spawn(move || {
                    send.send(()).unwrap();
                    let store = Store::at_data_home(&base).unwrap();
                    send.send(()).unwrap();
                    store
                });
                receive.recv().unwrap();
                assert!(
                    receive
                        .recv_timeout(std::time::Duration::from_millis(50))
                        .is_err()
                );
                let path = legacy.root.join("history.json");
                let mut history: History = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
                let memo = history.items.iter_mut().find(|item| item.id == 1).unwrap();
                memo.text = Some("written under source lock".into());
                memo.digest = digest(b"written under source lock");
                fs::write(path, serde_json::to_vec(&history).unwrap()).unwrap();
                drop(lock);
                let migrated = worker.join().unwrap();
                assert!(
                    migrated.snapshot().unwrap()["items"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|item| item["text"] == "written under source lock")
                );
            }
        }
    };
    (capture) => {
        mod tests {
            use super::*;

            fn collect_without_targets(types: &str, bytes: &[u8]) -> Vec<Captured> {
                collect(
                    types,
                    bytes.to_vec(),
                    &mut |_| panic!("Unexpected target read"),
                    None,
                )
                .unwrap()
            }

            fn text(entry: &Captured) -> (&str, &str) {
                match entry {
                    Captured::Text { text, mime } => (text, mime),
                    _ => panic!("Expected text"),
                }
            }

            #[test]
            fn bounded_reads_accept_the_limit_and_stop_before_buffering_an_unbounded_stream() {
                assert_eq!(read_limited(&b"12345"[..], 5).unwrap(), b"12345");
                let mut stream = io::repeat(b'x');
                assert!(read_limited(&mut stream, 5).is_err());
            }

            #[test]
            fn text_policy_keeps_unicode_and_whitespace_but_normalizes_standalone_urls() {
                let entries = collect_without_targets("text/plain;charset=utf-8\ntext/html\n", b"  hello\n\n");
                assert_eq!(
                    text(&entries[0]),
                    ("  hello\n\n", "text/plain;charset=utf-8")
                );
                let entries = entries_for_mime("text/plain", "memo\n你好 🦀".as_bytes().to_vec()).unwrap();
                assert_eq!(text(&entries[0]).0, "memo\n你好 🦀");
                let entries = collect_without_targets("text/plain", b"  https://example.com/a?a=1&amp;b=2  ");
                assert_eq!(text(&entries[0]).0, "https://example.com/a?a=1&b=2");
                let entries = collect_without_targets("text/plain", b"See https://example.com for details\n");
                assert_eq!(text(&entries[0]).0, "See https://example.com for details\n");
                assert!(standalone_file_source("https://example.com\nPage title").is_none());
            }

            #[test]
            fn local_paths_stay_file_references_and_unsafe_uris_are_not_sources() {
                for source in [
                    "file:///tmp/My%20Report.pdf",
                    "file://localhost/tmp/My%20Report.pdf",
                    "/tmp/My Report.pdf",
                ] {
                    let entries = entries_for_mime("text/plain", source.as_bytes().to_vec()).unwrap();
                    assert!(
                        matches!(&entries[0], Captured::File { text, name: Some(name), .. }
                        if text == source && name == "/tmp/My Report.pdf")
                    );
                }
                for source in [
                    "file://remote/tmp/file",
                    "file:///tmp/a%00b",
                    "file:///tmp/%ZZ",
                    "/tmp/a\0b",
                ] {
                    assert!(local_file_path(source).is_none());
                }
            }

            #[test]
            fn literal_local_filenames_are_not_uri_or_html_decoded() {
                for path in [
                    "/tmp/report%20final.pdf",
                    "/tmp/a%ZZ.pdf",
                    "/tmp/a&amp;b.pdf",
                ] {
                    assert_eq!(local_file_path(path).as_deref(), Some(Path::new(path)));
                    let entries = entries_for_mime("text/plain", path.as_bytes().to_vec()).unwrap();
                    assert!(matches!(&entries[0], Captured::File { name: Some(name), .. } if name == path));
                }
                assert_eq!(
                    local_file_path("file:///tmp/report%2520final.pdf").as_deref(),
                    Some(Path::new("/tmp/report%20final.pdf"))
                );
                assert_eq!(
                    source_from_value("file:///tmp/report%20").as_deref(),
                    Some("/tmp/report ")
                );
                assert_eq!(
                    source_from_value("file:///tmp/a&amp;b.pdf").as_deref(),
                    Some("/tmp/a&amp;b.pdf")
                );
                assert!(source_from_value("file:///tmp/invalid-%FF.png").is_none());
                assert!(
                    entries_for_mime("text/uri-list", b"file:///tmp/invalid-%FF.png".to_vec())
                        .unwrap()
                        .is_empty()
                );
                assert_eq!(
                    image_source_from_html("<img src='file:///tmp/a&amp;b.png'>").as_deref(),
                    Some("/tmp/a&b.png")
                );
            }

            #[test]
            fn uri_and_gnome_targets_split_files_and_route_remote_urls_to_text() {
                let payload = "cut\n# comment\nfile:///tmp/one.pdf\nfile:///tmp/two.pdf\nhttps://example.com/report.pdf\n";
                let entries = collect(
                    "x-special/gnome-copied-files\ntext/plain",
                    b"ignored".to_vec(),
                    &mut |_| Ok(Some(payload.as_bytes().to_vec())),
                    None,
                )
                .unwrap();
                assert_eq!(entries.len(), 3);
                assert!(
                    matches!(&entries[0], Captured::File { text, mime, name: Some(name) }
                    if text == "cut\nfile:///tmp/one.pdf" && mime == GNOME_COPIED_FILES_MIME && name == "/tmp/one.pdf")
                );
                assert_eq!(
                    text(&entries[2]),
                    ("https://example.com/report.pdf", "text/plain")
                );
                assert_eq!(
                    file_reference_payload(TEXT_URI_LIST_MIME, "", "file:///tmp/a"),
                    "file:///tmp/a\n"
                );
                assert_eq!(
                    file_reference_payload(GNOME_COPIED_FILES_MIME, "", "file:///tmp/a"),
                    "copy\nfile:///tmp/a"
                );
                // An empty read may still use the watch payload; a failed read must not commit it.
                let entries = collect(
                    "text/uri-list",
                    b"file:///tmp/one.pdf".to_vec(),
                    &mut |_| Ok(None),
                    None,
                )
                .unwrap();
                assert_eq!(entries.len(), 1);
                assert!(
                    collect(
                        "text/uri-list",
                        b"file:///tmp/one.pdf".to_vec(),
                        &mut |_| anyhow::bail!("Target failed"),
                        None
                    )
                    .is_err()
                );
            }

            #[test]
            fn passwords_empty_invalid_utf8_and_binary_targets_are_not_captured() {
                for mime in [
                    "x-kde-passwordManagerHint",
                    "application/x-kde-passwordManagerHint",
                ] {
                    assert!(collect_without_targets(&format!("text/plain\n{mime}"), b"private").is_empty());
                    assert!(
                        entries_for_mime(mime, b"private".to_vec())
                            .unwrap()
                            .is_empty()
                    );
                }
                assert!(collect_without_targets("text/plain", b"").is_empty());
                assert!(collect_without_targets("text/plain", b"\xff").is_empty());
                assert!(collect_without_targets("application/octet-stream", b"valid UTF-8 binary").is_empty());
                assert!(collect_without_targets("", b"stale data").is_empty());
                assert!(entries_for_mime("text/plain", vec![0xff]).is_err());
            }

            #[test]
            fn image_preference_and_browser_sources_preserve_the_specific_image_origin() {
                let png = b"\x89PNG\r\n\x1a\nimage";
                let mut targets = Vec::new();
                let entries = collect(
                    "image/jpeg\nimage/png\ntext/html\nchromium/x-source-url",
                    png.to_vec(),
                    &mut |mime| {
                        targets.push(mime.to_owned());
                        Ok(Some(
                            br#"<div><img alt="photo" src="https://example.org/image.png?a=1&amp;b=2"></div>"#
                                .to_vec(),
                        ))
                    },
                    None,
                )
                .unwrap();
                assert_eq!(targets, ["text/html"]);
                assert!(
                    matches!(&entries[0], Captured::Image { bytes, mime, name: Some(name) }
                    if bytes == png && mime == "image/png" && name == "https://example.org/image.png?a=1&b=2")
                );
                let entries = collect(
                    "image/png\ntext/x-moz-url",
                    b"other watch target".to_vec(),
                    &mut |mime| {
                        Ok(Some(if mime == "image/png" {
                            png.to_vec()
                        } else {
                            "https://example.org/firefox.png\nTitle"
                                .encode_utf16()
                                .flat_map(u16::to_le_bytes)
                                .collect()
                        }))
                    },
                    None,
                )
                .unwrap();
                assert!(
                    matches!(&entries[0], Captured::Image { bytes, name: Some(name), .. }
                    if bytes == png && name == "https://example.org/firefox.png")
                );
                for mime in [
                    "chromium/x-source-url",
                    "text/x-moz-url",
                    "text/x-moz-url-data;charset=utf-8",
                    "application/x-moz-file-promise-url",
                ] {
                    assert!(is_browser_image_source_mime(mime));
                }
                assert!(!is_browser_image_source_mime("image/png"));
            }

            #[test]
            fn source_precedence_is_file_then_html_then_browser_then_plain_text() {
                let types = "image/png\ntext/uri-list\ntext/html\nchromium/x-source-url\ntext/plain";
                let mut reads = Vec::new();
                let source = clipboard_image_source(types, &mut |mime| {
                    reads.push(mime.to_owned());
                    Ok(Some(b"file:///tmp/image.png\n".to_vec()))
                })
                .unwrap();
                assert_eq!(source.as_deref(), Some("/tmp/image.png"));
                assert_eq!(reads, ["text/uri-list"]);
                let source = clipboard_image_source(types, &mut |mime| {
                    Ok((mime == "text/plain").then(|| b"https://example.org/plain.png".to_vec()))
                })
                .unwrap();
                assert_eq!(source.as_deref(), Some("https://example.org/plain.png"));
                assert!(clipboard_image_source(types, &mut |_| anyhow::bail!("Failed source read")).is_err());
            }

            #[test]
            fn decodes_utf16_endianness_boms_and_terminated_source_targets() {
                let value = "https://example.org/你好.png\nTitle";
                for bom in [true, false] {
                    for big_endian in [true, false] {
                        let mut bytes = if bom {
                            if big_endian {
                                vec![0xfe, 0xff]
                            } else {
                                vec![0xff, 0xfe]
                            }
                        } else {
                            vec![]
                        };
                        for word in value.encode_utf16().chain([0]) {
                            bytes.extend_from_slice(&if big_endian {
                                word.to_be_bytes()
                            } else {
                                word.to_le_bytes()
                            });
                        }
                        assert_eq!(decode_clipboard_text(&bytes), value);
                    }
                }
                assert_eq!(
                    decode_clipboard_text(b"https://example.org\0"),
                    "https://example.org"
                );
                assert_eq!(
                    image_source_from_html("<IMG data-src='wrong' SRC = '/tmp/a.png'>").as_deref(),
                    Some("/tmp/a.png")
                );
            }

            #[test]
            fn screenshot_paths_require_an_exact_match_and_a_pathless_png_offer() {
                let directory =
                    std::env::temp_dir().join(format!("clipboard-capture-test-{}", std::process::id()));
                fs::create_dir(&directory).unwrap();
                let result = std::panic::catch_unwind(|| {
                    let png = b"\x89PNG\r\n\x1a\nscreenshot";
                    let path = directory.join("shot.png");
                    fs::write(&path, png).unwrap();
                    fs::write(directory.join("wrong.png"), b"same length not same bytes").unwrap();
                    assert_eq!(
                        matching_screenshot_path(&directory, png),
                        Some(path.clone())
                    );
                    assert!(matching_screenshot_path(&directory, b"different screenshot").is_none());
                    assert!(is_pathless_png("image/png\n"));
                    assert!(!is_pathless_png("image/png\ntext/plain\n"));
                    assert!(!is_pathless_png("image/jpeg"));
                    let entries = collect(
                        "image/png",
                        png.to_vec(),
                        &mut |_| panic!(),
                        Some(&directory),
                    )
                    .unwrap();
                    assert!(
                        matches!(&entries[0], Captured::Image { name: Some(name), .. } if Path::new(name) == path)
                    );
                    assert!(screenshot_image_source("image/png\ntext/plain", png, Some(&directory)).is_none());
                    let uri = format!("file://{}\n", path.display());
                    let entries = entries_for_mime("text/uri-list", uri.into_bytes()).unwrap();
                    assert!(
                        matches!(&entries[0], Captured::Image { bytes, name: Some(name), .. }
                        if bytes == png && Path::new(name) == path)
                    );
                    let invalid_name = directory.join(OsString::from_vec(b"invalid-\xff.png".to_vec()));
                    fs::rename(&path, invalid_name).unwrap();
                    assert!(screenshot_image_source("image/png", png, Some(&directory)).is_none());
                });
                fs::remove_dir_all(directory).unwrap();
                result.unwrap();
            }
        }
    };
}
pub(crate) use clipboard_unit_tests;

// Compile the real request type and modules without widening private APIs.
#[path = "main.rs"]
#[allow(dead_code, unused_imports, unused_macros)]
mod app;

mod cli {
    use serde_json::{Value, json};
    use std::fs;
    use std::io::Write;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;
    use std::process::{Command, Output, Stdio};
    use std::sync::atomic::{AtomicU64, Ordering};

    static SERIAL: AtomicU64 = AtomicU64::new(0);
    const HELPER: &str = env!("CARGO_BIN_EXE_clipboard-custom");

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "clipboard-cli-test-{}-{}",
                std::process::id(),
                SERIAL.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&root).unwrap();
            fs::create_dir(root.join("home")).unwrap();
            Self(root)
        }
        fn command(&self) -> Command {
            let mut command = Command::new(HELPER);
            command
                .env_remove("CLIPBOARD_STATE")
                .env_remove("CLIPBOARD_DATA_DIR")
                .env_remove("CLIPBOARD_WL_PASTE")
                .env("HOME", self.0.join("home"))
                .env("XDG_DATA_HOME", self.0.join("data"))
                .env("CLIPBOARD_SCREENSHOT_DIR", self.0.join("screenshots"))
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            command
        }
        fn request(&self, request: Value) -> Value {
            let output = run(self.command(), &[], request.to_string().as_bytes());
            assert!(output.status.success(), "{:?}", output);
            serde_json::from_slice(&output.stdout).unwrap()
        }
        fn store(&self, mime: &str, bytes: &[u8]) {
            let output = run(self.command(), &["store", "--mime", mime], bytes);
            assert!(output.status.success(), "{:?}", output);
            assert!(output.stdout.is_empty());
            assert!(output.stderr.is_empty());
        }
        fn install_paste(&self) -> PathBuf {
            let path = self.0.join("fake-wl-paste");
            fs::write(
                &path,
                r#"#!/bin/sh
    case "$*" in
        --list-types) cat "$CLIPBOARD_FAKE_DIR/types" ;;
        '--no-newline --type image/png') cat "$CLIPBOARD_FAKE_DIR/image" ;;
        '--no-newline --type text/html') cat "$CLIPBOARD_FAKE_DIR/html" ;;
        '--no-newline --type text/uri-list') cat "$CLIPBOARD_FAKE_DIR/uris" ;;
        *) exit 12 ;;
    esac
    "#,
            )
            .unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
            path
        }
        fn capture(&self, watched: &[u8]) -> Output {
            let mut command = self.command();
            command
                .env("CLIPBOARD_WL_PASTE", self.install_paste())
                .env("CLIPBOARD_FAKE_DIR", &self.0);
            run(command, &["capture"], watched)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn run(mut command: Command, args: &[&str], bytes: &[u8]) -> Output {
        let mut child = command.args(args).spawn().unwrap();
        child.stdin.take().unwrap().write_all(bytes).unwrap();
        child.wait_with_output().unwrap()
    }

    #[test]
    fn no_argument_protocol_and_store_cli_coexist_and_preserve_payloads() {
        let fixture = Fixture::new();
        let prepared = fixture.request(json!({ "command": "prepare" }));
        assert!(prepared["id"].is_u64());
        fixture.store("text/plain;charset=utf-8", "  typed 你好\n\n".as_bytes());
        let snapshot = fixture.request(json!({ "command": "snapshot" }));
        let item = &snapshot["items"][0];
        assert_eq!(item["kind"], "text");
        let payload = fixture.request(json!({ "command": "payload", "id": item["id"] }));
        assert_eq!(payload["text"], "  typed 你好\n\n");
        assert!(fixture.0.join("data/clipboard/history.json").is_file());
        let before = fs::read(fixture.0.join("data/clipboard/history.json")).unwrap();
        for args in [
            &["store"][..],
            &["store", "--mime", "text/plain", "extra"],
            &["capture", "extra"],
            &["unknown"],
        ] {
            let output = run(fixture.command(), args, b"private clipboard contents");
            assert!(!output.status.success());
            assert!(output.stdout.is_empty());
            assert!(
                !String::from_utf8_lossy(&output.stderr).contains("private clipboard contents")
            );
        }
        assert!(fixture.request(json!({ "command": "no-such-command" }))["error"].is_string());
        assert_eq!(
            fs::read(fixture.0.join("data/clipboard/history.json")).unwrap(),
            before
        );
    }

    #[test]
    fn capture_cli_handles_images_browser_origins_text_and_file_references_without_output() {
        let fixture = Fixture::new();
        let png = b"\x89PNG\r\n\x1a\nimage payload";
        fs::write(
            fixture.0.join("types"),
            "image/png\ntext/html\ntext/plain\n",
        )
        .unwrap();
        fs::write(fixture.0.join("image"), png).unwrap();
        fs::write(
            fixture.0.join("html"),
            "<img src='https://example.org/image.png'>",
        )
        .unwrap();
        let output = fixture.capture(b"watch selected a text target");
        assert!(output.status.success(), "{:?}", output);
        assert!(output.stdout.is_empty());
        assert!(output.stderr.is_empty());
        let snapshot = fixture.request(json!({ "command": "snapshot" }));
        assert_eq!(
            snapshot["items"][0]["name"],
            "https://example.org/image.png"
        );
        assert_eq!(
            fs::read(snapshot["items"][0]["image_path"].as_str().unwrap()).unwrap(),
            png
        );
        assert!(fixture.capture(png).status.success());
        assert_eq!(
            fixture.request(json!({ "command": "snapshot" }))["items"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        fs::write(fixture.0.join("types"), "text/plain;charset=utf-8\n").unwrap();
        assert!(
            fixture
                .capture("exact watched text\n你好".as_bytes())
                .status
                .success()
        );
        fs::write(fixture.0.join("types"), "text/uri-list\ntext/plain\n").unwrap();
        fs::write(
            fixture.0.join("uris"),
            "file:///tmp/one.pdf\nfile:///tmp/two.pdf\nhttps://example.org/page\n",
        )
        .unwrap();
        assert!(fixture.capture(b"fallback watch bytes").status.success());
        let snapshot = fixture.request(json!({ "command": "snapshot" }));
        let items = snapshot["items"].as_array().unwrap();
        assert_eq!(items.len(), 5);
        assert!(
            items
                .iter()
                .any(|item| item["text"] == "exact watched text\n你好")
        );
        assert!(items.iter().any(|item| item["name"] == "/tmp/two.pdf"));
        assert!(
            items
                .iter()
                .any(|item| item["kind"] == "text" && item["text"] == "https://example.org/page")
        );
    }

    #[test]
    fn sensitive_states_password_hints_and_target_failures_never_change_history() {
        let fixture = Fixture::new();
        fixture.store("text/plain", b"keep this");
        let history = fixture.0.join("data/clipboard/history.json");
        let before = fs::read(&history).unwrap();
        for state in ["sensitive", "nil"] {
            let mut command = fixture.command();
            command
                .env("CLIPBOARD_STATE", state)
                .env("CLIPBOARD_WL_PASTE", "/does/not/exist");
            let output = run(command, &["capture"], b"private clipboard contents");
            assert!(output.status.success());
            assert!(output.stdout.is_empty());
            assert!(output.stderr.is_empty());
        }
        fs::write(
            fixture.0.join("types"),
            "text/plain\napplication/x-kde-passwordManagerHint\n",
        )
        .unwrap();
        assert!(
            fixture
                .capture(b"private clipboard contents")
                .status
                .success()
        );
        fs::write(fixture.0.join("types"), "image/png\n").unwrap();
        // The image target is unavailable: no partial history mutation is allowed.
        let output = fixture.capture(b"not an image");
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(!String::from_utf8_lossy(&output.stderr).contains("not an image"));
        fs::remove_file(fixture.0.join("types")).unwrap();
        assert!(!fixture.capture(b"stale watch bytes").status.success());
        assert_eq!(fs::read(history).unwrap(), before);
    }

    #[test]
    fn custom_store_and_home_fallback_are_isolated_from_default_legacy_history() {
        let fixture = Fixture::new();
        fs::create_dir_all(fixture.0.join("data/rofi-clipboard")).unwrap();
        fs::write(
            fixture.0.join("data/rofi-clipboard/history.json"),
            b"invalid legacy history",
        )
        .unwrap();
        let mut command = fixture.command();
        command.env("CLIPBOARD_DATA_DIR", fixture.0.join("custom"));
        let output = run(command, &["store", "--mime", "text/plain"], b"custom only");
        assert!(output.status.success(), "{:?}", output);
        assert!(fixture.0.join("custom/history.json").is_file());
        assert!(!fixture.0.join("data/clipboard").exists());
        let mut command = fixture.command();
        command.env_remove("XDG_DATA_HOME");
        let output = run(command, &["store", "--mime", "text/plain"], b"home only");
        assert!(output.status.success(), "{:?}", output);
        assert!(
            fixture
                .0
                .join("home/.local/share/clipboard/history.json")
                .is_file()
        );
    }

    #[test]
    fn json_discovery_migrates_the_default_legacy_store_before_returning_paths() {
        let fixture = Fixture::new();
        let legacy = fixture.0.join("data/rofi-clipboard");
        let mut command = fixture.command();
        command.env("CLIPBOARD_DATA_DIR", &legacy);
        let output = run(command, &["store", "--mime", "image/png"], b"image payload");
        assert!(output.status.success(), "{:?}", output);
        let bytes = fs::read(legacy.join("history.json")).unwrap();
        let snapshot = fixture.request(json!({ "command": "snapshot" }));
        assert!(snapshot.get("error").is_none(), "{snapshot}");
        let image = fixture.0.join("data/clipboard/images/1.png");
        assert_eq!(snapshot["items"][0]["image_path"], image.to_str().unwrap());
        assert_eq!(fs::read(image).unwrap(), b"image payload");
        assert_eq!(
            fs::read(fixture.0.join("data/clipboard/history.json")).unwrap(),
            bytes
        );
        assert_eq!(fs::read(legacy.join("history.json")).unwrap(), bytes);
        assert_eq!(
            fs::read(legacy.join("images/1.png")).unwrap(),
            b"image payload"
        );
    }

    #[test]
    fn uri_capture_does_not_block_on_fifos_or_read_oversized_files() {
        unsafe extern "C" {
            fn mkfifo(path: *const std::ffi::c_char, mode: u32) -> i32;
        }
        let fixture = Fixture::new();
        let fifo = fixture.0.join("pipe");
        let path = std::ffi::CString::new(fifo.as_os_str().as_bytes()).unwrap();
        // SAFETY: path is a valid, NUL-terminated string for the duration of the call.
        assert_eq!(unsafe { mkfifo(path.as_ptr(), 0o600) }, 0);
        let oversized = fixture.0.join("large.png");
        let mut file = fs::File::create(&oversized).unwrap();
        file.write_all(b"\x89PNG\r\n\x1a\n").unwrap();
        file.set_len(64 * 1024 * 1024 + 1).unwrap();
        let mut child = fixture
            .command()
            .args(["store", "--mime", "text/uri-list"])
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(
                format!(
                    "file://{}\nfile://{}\n",
                    fifo.display(),
                    oversized.display()
                )
                .as_bytes(),
            )
            .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while child.try_wait().unwrap().is_none() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let timed_out = child.try_wait().unwrap().is_none();
        if timed_out {
            child.kill().unwrap();
        }
        let output = child.wait_with_output().unwrap();
        assert!(!timed_out, "URI capture blocked on a nonregular file");
        assert!(output.status.success(), "{:?}", output);
        let snapshot = fixture.request(json!({ "command": "snapshot" }));
        let items = snapshot["items"].as_array().unwrap();
        assert_eq!(items.len(), 2);
        assert!(
            items
                .iter()
                .all(|item| item["kind"] == "file" && item["image_path"].is_null())
        );
        assert!(!fixture.0.join("data/clipboard/images").exists());
    }

    #[test]
    fn oversized_paste_target_fails_without_exposing_bytes_or_changing_history() {
        let fixture = Fixture::new();
        fixture.store("text/plain", b"keep this history");
        let history = fixture.0.join("data/clipboard/history.json");
        let before = fs::read(&history).unwrap();
        fs::write(fixture.0.join("types"), "image/png\n").unwrap();
        let mut file = fs::File::create(fixture.0.join("image")).unwrap();
        file.write_all(b"private clipboard target").unwrap();
        file.set_len(64 * 1024 * 1024 + 1).unwrap();
        let output = fixture.capture(b"not an image");
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("too large"), "{stderr}");
        assert!(!stderr.contains("private clipboard target"));
        assert!(!stderr.contains("not an image"));
        assert_eq!(fs::read(history).unwrap(), before);
    }

    #[test]
    fn local_file_capture_preserves_literal_source_names_and_real_png_bytes() {
        let fixture = Fixture::new();
        let source = fixture.0.join("report%20final&amp;notes.pdf");
        fs::write(&source, b"file reference fixture").unwrap();
        fs::write(
            fixture.0.join("report final&notes.pdf"),
            b"must not select this file",
        )
        .unwrap();
        let png = b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR\x00\x00\x00 \x00\x00\x00 \x08\x02\x00\x00\x00\xfc\x18\xed\xa3\x00\x00\x00*IDATx\x9c\xed\xcd\xb1\t\x00\x00\x08\x03\xb0>\xea\xff\x0f8\x8bW8\x08\x81\xecI\xf5\x9c\x8a@ \x10\x08\x04\x02\x81@ \xf8\x12,\x8f\xe4<\x88J\xecH/\x00\x00\x00\x00IEND\xaeB`\x82";
        let image = fixture.0.join("image%20literal.png");
        fs::write(&image, png).unwrap();
        fs::write(fixture.0.join("types"), "text/uri-list\ntext/plain\n").unwrap();
        fs::write(
            fixture.0.join("uris"),
            format!("{}\n{}\n", source.display(), image.display()),
        )
        .unwrap();
        let output = fixture.capture(b"unused watched target");
        assert!(output.status.success(), "{:?}", output);
        assert!(output.stdout.is_empty());
        assert!(output.stderr.is_empty());
        let snapshot = fixture.request(json!({ "command": "snapshot" }));
        let items = snapshot["items"].as_array().unwrap();
        let reference = items
            .iter()
            .find(|item| item["name"] == source.to_str().unwrap())
            .unwrap();
        let payload = fixture.request(json!({ "command": "payload", "id": reference["id"] }));
        assert_eq!(payload["mime"], "text/uri-list");
        assert!(
            payload["text"]
                .as_str()
                .unwrap()
                .ends_with("/report%2520final%26amp%3Bnotes.pdf\n")
        );
        let cached = items
            .iter()
            .find(|item| item["name"] == image.to_str().unwrap())
            .unwrap();
        assert_eq!(cached["mime"], "image/png");
        let cache = cached["image_path"].as_str().unwrap();
        assert_eq!(fs::read(cache).unwrap(), png);
        for path in [
            fixture.0.join("data/clipboard"),
            fixture.0.join("data/clipboard/images"),
        ] {
            assert_eq!(
                fs::metadata(path).unwrap().permissions().mode() & 0o777,
                0o700
            );
        }
        for path in [
            PathBuf::from(cache),
            fixture.0.join("data/clipboard/history.json"),
            fixture.0.join("data/clipboard/history.lock"),
        ] {
            assert_eq!(
                fs::metadata(path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }

    #[test]
    fn screenshot_environment_is_used_without_a_display_or_clipboard_publisher() {
        let fixture = Fixture::new();
        let png = b"\x89PNG\r\n\x1a\nscreenshot payload";
        fs::write(fixture.0.join("types"), "image/png\n").unwrap();
        fs::create_dir(fixture.0.join("screenshots")).unwrap();
        let screenshot = fixture.0.join("screenshots/shot.png");
        fs::write(&screenshot, png).unwrap();
        let mut command = fixture.command();
        command
            .env_remove("DISPLAY")
            .env_remove("WAYLAND_DISPLAY")
            .env("CLIPBOARD_WL_PASTE", fixture.install_paste())
            .env("CLIPBOARD_FAKE_DIR", &fixture.0);
        let output = run(command, &["capture"], png);
        assert!(output.status.success(), "{:?}", output);
        assert!(output.stdout.is_empty());
        let snapshot = fixture.request(json!({ "command": "snapshot" }));
        assert_eq!(snapshot["items"][0]["name"], screenshot.to_str().unwrap());
    }
}
