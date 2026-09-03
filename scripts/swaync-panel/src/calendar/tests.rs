use super::*;

/// A day holding one of everything, in the order waybar-ycal wrote it.
const CACHE: &str = r#"{
  "2026-09-03": [
    "Standup 09:30-10:00",
    {"type": "task", "id": "1", "lid": "a", "title": "Pay rent & council tax", "done": false},
    "Dentist 15:00-15:45",
    {"type": "task", "id": "2", "lid": "a", "title": "Email <supervisor>", "done": true},
    {"type": "task", "id": "3", "lid": "a", "title": "Submit form", "done": false}
  ],
  "2026-09-04": ["Tomorrow only"]
}"#;

#[test]
fn open_tasks_come_first_then_events_then_what_is_done() {
    let entries = entries(CACHE, "2026-09-03");
    let titles: Vec<&str> = entries.iter().map(|entry| entry.title.as_str()).collect();
    assert_eq!(
        titles,
        [
            "Pay rent & council tax",
            "Submit form",
            "Standup 09:30-10:00",
            "Dentist 15:00-15:45",
            "Email <supervisor>",
        ]
    );
    assert_eq!(entries[0].icon, TASK_OPEN);
    assert_eq!(entries[2].icon, CALENDAR);
    assert!(entries[4].done);
}

#[test]
fn the_row_shows_three_entries_and_counts_the_rest() {
    let block = block(CACHE, "2026-09-03", false);
    let lines: Vec<&str> = block.lines().collect();
    assert_eq!(lines.len(), 4, "{block}");
    assert!(lines[3].ends_with("+ 2 more"), "{block}");
}

#[test]
fn a_day_that_fits_gets_no_count() {
    let block = block(CACHE, "2026-09-04", false);
    assert_eq!(block.lines().count(), 1);
    assert!(block.ends_with("Tomorrow only"), "{block}");
}

#[test]
fn a_quiet_day_says_so() {
    assert!(block(CACHE, "2026-09-09", false).contains("Nothing scheduled today"));
}

#[test]
fn a_missing_or_unreadable_cache_reads_as_a_quiet_day() {
    // A cache that has not been written yet, one that was truncated mid-write,
    // and one whose shape is not what waybar-ycal writes.
    for cache in ["", "{\"2026-09-03\": [\"Stan", "[]", "null"] {
        assert!(
            block(cache, "2026-09-03", false).contains("Nothing scheduled today"),
            "{cache:?}"
        );
    }
}

#[test]
fn an_entry_that_is_neither_an_event_nor_a_task_is_skipped() {
    let cache = r#"{"2026-09-03": [42, {"type": "task"}, "Real event"]}"#;
    let entries = entries(cache, "2026-09-03");
    assert_eq!(entries.len(), 1, "{entries:#?}");
    assert_eq!(entries[0].title, "Real event");
}

#[test]
fn markup_escapes_a_title_that_looks_like_markup() {
    let cache = r#"{"2026-09-03": ["Rock & <Roll>"]}"#;
    let block = block(cache, "2026-09-03", true);
    assert!(block.contains("Rock &amp; &lt;Roll&gt;"), "{block}");
    assert!(block.contains(colour::ICON), "{block}");
}

#[test]
fn a_finished_task_is_dimmed_and_an_open_one_is_not() {
    let cache = r#"{"2026-09-03": [
        {"title": "Done", "done": true},
        {"title": "Open", "done": false}
    ]}"#;
    let block = block(cache, "2026-09-03", true);
    let lines: Vec<&str> = block.lines().collect();
    assert!(lines[0].contains(colour::TEXT), "the open task: {block}");
    assert!(lines[1].contains(colour::NAME), "the finished one: {block}");
}

#[test]
fn plain_output_carries_the_glyph_but_no_markup() {
    let block = block(CACHE, "2026-09-04", false);
    assert!(block.starts_with(CALENDAR), "{block}");
    assert!(!block.contains("<span"), "{block}");
}
