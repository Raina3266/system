use super::*;

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
