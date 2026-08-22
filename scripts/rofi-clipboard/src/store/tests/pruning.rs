use super::*;

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
