//! The seven-day agenda, read from the cache `waybar-ycal` keeps.
//!
//! Wayle used to render this list itself, from a patch that reached into its
//! notification dropdown. The cache is the same file; only the reader moved.

use std::env;
use std::fs;
use std::path::PathBuf;

use chrono::{Duration, Local, NaiveDate};
use serde_json::Value;

/// How many days ahead the agenda looks, today included.
const DAYS: i64 = 7;

/// One line under a day: a calendar event, or a task that may be done.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// Sorts unfinished tasks first and finished ones last.
    order: u8,
    pub icon: &'static str,
    pub title: String,
}

/// A day that has something on it. Days with nothing are left out entirely.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Day {
    pub heading: String,
    pub entries: Vec<Entry>,
}

/// What the card shows: the range it covers, and the days within it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Agenda {
    pub range: String,
    pub days: Vec<Day>,
}

impl Agenda {
    pub fn is_empty(&self) -> bool {
        self.days.is_empty()
    }
}

fn cache_path() -> PathBuf {
    if let Some(path) = env::var_os("CONTROL_CENTRE_CALENDAR_CACHE").filter(|v| !v.is_empty()) {
        return PathBuf::from(path);
    }

    env::var_os("HOME")
        .map_or_else(PathBuf::new, PathBuf::from)
        .join(".cache/waybar-ycal/events.json")
}

/// Read the agenda for the seven days starting today.
pub fn read() -> Agenda {
    let cache = fs::read_to_string(cache_path()).unwrap_or_default();
    build(&cache, Local::now().date_naive())
}

/// Build the agenda from cache text, so tests can fix both it and the date.
pub fn build(cache: &str, start: NaiveDate) -> Agenda {
    let end = start + Duration::days(DAYS - 1);
    let range = format!("{} – {}", start.format("%-d %b"), end.format("%-d %b"));
    let parsed = serde_json::from_str::<Value>(cache).unwrap_or(Value::Null);

    let mut days = Vec::new();
    for offset in 0..DAYS {
        let date = start + Duration::days(offset);
        let entries = entries_for(&parsed, &date.format("%Y-%m-%d").to_string());
        if entries.is_empty() {
            continue;
        }

        days.push(Day {
            heading: if offset == 0 {
                String::from("Today")
            } else {
                date.format("%A %-d %b").to_string()
            },
            entries,
        });
    }

    Agenda { range, days }
}

fn entries_for(parsed: &Value, date: &str) -> Vec<Entry> {
    let Some(Value::Array(items)) = parsed.get(date) else {
        return Vec::new();
    };

    let mut entries = items
        .iter()
        .filter_map(|item| match item {
            Value::String(title) => Some(Entry {
                order: 1,
                icon: "󰃭",
                title: title.clone(),
            }),
            Value::Object(task) => {
                let done = task.get("done").and_then(Value::as_bool).unwrap_or(false);
                Some(Entry {
                    order: if done { 2 } else { 0 },
                    icon: if done { "󰄲" } else { "󰄱" },
                    title: task.get("title").and_then(Value::as_str)?.to_owned(),
                })
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    entries.sort_by_key(|entry| entry.order);
    entries
}

#[cfg(test)]
mod tests;
