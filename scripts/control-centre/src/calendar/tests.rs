use chrono::NaiveDate;

use super::build;

const CACHE: &str = r#"{
  "2026-09-07": ["Standup 09:30-09:45"],
  "2026-09-09": ["Video appointment with Johns, Brian (Mr) 19:00-19:30"],
  "2026-09-10": ["Teachers' Day", {"title": "Mom & Dad", "done": false},
                 {"title": "already done", "done": true}],
  "2026-09-13": ["曾璐's birthday"],
  "2026-09-20": ["outside the window"]
}"#;

fn start() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 9, 7).expect("valid date")
}

#[test]
fn the_range_covers_today_and_the_six_days_after_it() {
    assert_eq!(build(CACHE, start()).range, "7 Sep – 13 Sep");
}

#[test]
fn the_first_day_is_named_today_and_the_rest_are_dated() {
    let agenda = build(CACHE, start());
    let headings: Vec<&str> = agenda.days.iter().map(|day| day.heading.as_str()).collect();
    assert_eq!(
        headings,
        [
            "Today",
            "Wednesday 9 Sep",
            "Thursday 10 Sep",
            "Sunday 13 Sep"
        ]
    );
}

#[test]
fn days_with_nothing_on_them_are_left_out_entirely() {
    // The 8th, 11th and 12th are absent from the cache and from the agenda,
    // so the card has no blank rows to render.
    assert_eq!(build(CACHE, start()).days.len(), 4);
}

#[test]
fn a_day_beyond_the_seventh_is_not_shown() {
    let agenda = build(CACHE, start());
    assert!(!agenda
        .days
        .iter()
        .flat_map(|day| &day.entries)
        .any(|entry| entry.title == "outside the window"));
}

#[test]
fn unfinished_tasks_sort_above_events_and_finished_ones_below() {
    let agenda = build(CACHE, start());
    let thursday = &agenda.days[2];
    let titles: Vec<&str> = thursday
        .entries
        .iter()
        .map(|entry| entry.title.as_str())
        .collect();
    assert_eq!(titles, ["Mom & Dad", "Teachers' Day", "already done"]);
    assert_eq!(thursday.entries[0].icon, "󰄱");
    assert_eq!(thursday.entries[1].icon, "󰃭");
    assert_eq!(thursday.entries[2].icon, "󰄲");
}

#[test]
fn a_missing_or_unreadable_cache_is_an_empty_agenda_not_a_panic() {
    assert!(build("", start()).is_empty());
    assert!(build("not json at all", start()).is_empty());
    assert!(build("{}", start()).is_empty());
    // The range still reads correctly, so the card keeps its header.
    assert_eq!(build("", start()).range, "7 Sep – 13 Sep");
}
