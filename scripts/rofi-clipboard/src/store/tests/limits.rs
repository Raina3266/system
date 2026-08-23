use super::*;

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
