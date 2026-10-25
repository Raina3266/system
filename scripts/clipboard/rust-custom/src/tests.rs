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
fn concurrent_writers_share_the_legacy_flock() {
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
fn file_uris_are_encoded_and_digests_match_the_collector() {
    assert_eq!(file_uri(Path::new("/tmp/a b#c")), "file:///tmp/a%20b%23c\n");
    assert_eq!(digest(b""), "cbf29ce484222325-0");
    assert_eq!(digest(b"hello"), "a430d84680aabd0b-5");
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
    assert_eq!(fixture.history().items.len(), 1);
    assert_eq!(fixture.history().items[0].id, 2);
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
    use crate::Request;
    assert!(serde_json::from_str::<Request>(r#"{"command":"snapshot","id":1}"#).is_err());
    assert!(serde_json::from_str::<Request>(r#"{"command":"save","id":1,"text":"x"}"#).is_err());
    assert!(serde_json::from_str::<Request>(r#"{"command":"delete","id":-1}"#).is_err());
    assert!(serde_json::from_str::<Request>(r#"{"command":"copy","id":1}"#).is_err());
    assert!(serde_json::from_str::<Request>(r#"{"command":"snapshot"}"#).is_ok());
}
