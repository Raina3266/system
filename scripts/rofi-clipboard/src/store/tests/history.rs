use super::*;

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
