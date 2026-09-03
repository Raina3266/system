//! Today's calendar events and tasks.
//!
//! waybar-ycal's popup already fetches Google Calendar and Google Tasks and
//! caches them, so this row reads that cache rather than starting Python and a
//! set of API calls of its own. The cache is keyed by ISO date; a day's entries
//! are a mix of plain strings (events, already carrying their time range) and
//! objects (tasks, carrying a done flag).

use std::fs;
use std::path::PathBuf;

use chrono::Local;
use serde_json::Value;

use crate::format::{colour, span};
use crate::non_empty;

/// How many entries the row shows before it starts counting the rest.
const LIMIT: usize = 3;

const CALENDAR: &str = "󰃭";
const TASK_OPEN: &str = "󰄱";
const TASK_DONE: &str = "󰄲";

/// One line of the row.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Entry {
    /// Open tasks first: they are the part that still needs doing. Then
    /// events, then whatever is already ticked off.
    order: u8,
    icon: &'static str,
    done: bool,
    title: String,
}

pub(crate) fn render(markup: bool) {
    let today = Local::now().format("%Y-%m-%d").to_string();
    let cache = fs::read_to_string(cache_path()).unwrap_or_default();
    println!("{}", block(&cache, &today, markup));
}

fn cache_path() -> PathBuf {
    if let Some(path) = non_empty("SWAYNC_PANEL_CALENDAR") {
        return PathBuf::from(path);
    }
    let home = std::env::var_os("HOME").map_or_else(PathBuf::new, PathBuf::from);
    home.join(".cache/waybar-ycal/events.json")
}

/// Render one day of the cache.
///
/// A cache that is missing, unparsable or simply has nothing for today all
/// render the same way: there is no useful difference between "no events" and
/// "no events yet known", and neither is worth an error in a status panel.
pub(crate) fn block(cache: &str, today: &str, markup: bool) -> String {
    let entries = entries(cache, today);
    if entries.is_empty() {
        return line(CALENDAR, "Nothing scheduled today", true, markup);
    }

    let mut lines: Vec<String> = entries
        .iter()
        .take(LIMIT)
        .map(|entry| line(entry.icon, &entry.title, entry.done, markup))
        .collect();
    if entries.len() > LIMIT {
        let more = format!("+ {} more", entries.len() - LIMIT);
        lines.push(if markup {
            span(colour::NAME, &more)
        } else {
            more
        });
    }
    lines.join("\n")
}

/// The day's entries, in the order the row shows them.
fn entries(cache: &str, today: &str) -> Vec<Entry> {
    let Ok(Value::Object(days)) = serde_json::from_str::<Value>(cache) else {
        return Vec::new();
    };
    let Some(Value::Array(items)) = days.get(today) else {
        return Vec::new();
    };

    let mut entries: Vec<Entry> = items
        .iter()
        .filter_map(|item| match item {
            // An event: the cache stores it as its finished label.
            Value::String(title) => Some(Entry {
                order: 1,
                icon: CALENDAR,
                done: false,
                title: title.clone(),
            }),
            // A task, which is the only entry that can be finished.
            Value::Object(task) => {
                let done = task.get("done").and_then(Value::as_bool).unwrap_or(false);
                Some(Entry {
                    order: if done { 2 } else { 0 },
                    icon: if done { TASK_DONE } else { TASK_OPEN },
                    done,
                    title: task.get("title").and_then(Value::as_str)?.to_owned(),
                })
            }
            _ => None,
        })
        .collect();
    // A stable sort keeps each group in the order the cache listed it.
    entries.sort_by_key(|entry| entry.order);
    entries
}

fn line(icon: &str, title: &str, dim: bool, markup: bool) -> String {
    if !markup {
        return format!("{icon}  {title}");
    }
    let text = if dim { colour::NAME } else { colour::TEXT };
    format!("{}  {}", span(colour::ICON, icon), span(text, title))
}

#[cfg(test)]
mod tests;
