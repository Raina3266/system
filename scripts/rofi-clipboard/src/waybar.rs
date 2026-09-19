//! Waybar `custom/clipboard` backend.
//!
//! `rofi-clipboard status` follows the history file and updates Waybar.
//! A single systemd collector captures clipboard changes independently of
//! the number of monitors and survives Waybar reloads.

use std::io::Write;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result};

use crate::clipboard::wl_copy_binary;
use crate::model::{ClipboardItem, ItemKind, json_escape};
use crate::store::ClipboardStore;

const STATUS_POLL_INTERVAL: Duration = Duration::from_secs(1);
pub(crate) const PREVIEW_LIMIT: usize = 60;

pub fn run_status() -> Result<()> {
    // Emit an initial line so the module is not blank until the first change.
    print_status()?;

    let history = ClipboardStore::discover()?.history_file().to_path_buf();

    let mut last_mtime = file_mtime(&history);

    loop {
        std::thread::sleep(STATUS_POLL_INTERVAL);
        let mtime = file_mtime(&history);
        if mtime != last_mtime {
            last_mtime = mtime;
            print_status()?;
        }
    }
}

/// `on-click-right` for the Waybar module: clears the current Wayland
/// clipboard selection. Stored history is untouched.
pub fn clear_selection() -> Result<()> {
    let status = Command::new(wl_copy_binary())
        .arg("--clear")
        .status()
        .context("run wl-copy --clear")?;
    if !status.success() {
        anyhow::bail!("wl-copy --clear exited with {status}");
    }
    Ok(())
}

fn print_status() -> Result<()> {
    let history = ClipboardStore::discover()
        .and_then(|store| store.load())
        .unwrap_or_default();

    let count = history
        .items
        .iter()
        .filter(|item| !item.is_empty_memo())
        .count();
    let latest = history.items.iter().max_by_key(|item| item.created_at);

    let text = format!("<span size='large'>{}</span>", "󰍩");
    let tooltip = tooltip(count, latest);
    let class = class(latest);

    let stdout = std::io::stdout();
    let mut handle = stdout.lock();
    writeln!(
        handle,
        "{{\"text\":\"{}\",\"tooltip\":\"{}\",\"class\":\"{}\"}}",
        json_escape(&text),
        json_escape(&tooltip),
        class,
    )?;
    handle.flush()?;
    Ok(())
}

fn file_mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).ok()?.modified().ok()
}

/// Styling hook for `waybar.css`, mirroring the audio module's class-based
/// recoloring: the glyph can be recolored per last-copied kind without
/// changing this program.
fn class(latest: Option<&ClipboardItem>) -> &'static str {
    match latest {
        None => "clipboard-empty",
        Some(item) if item.image_file.is_some() => "clipboard-image",
        Some(item) if item.kind == ItemKind::File => "clipboard-file",
        Some(_) => "clipboard-text",
    }
}

pub(crate) fn tooltip(count: usize, latest: Option<&ClipboardItem>) -> String {
    let mut lines = vec![format!("Clipboard: {count} item{}", plural_s(count))];
    if let Some(item) = latest
        && let Some(preview) = preview_line(item)
    {
        lines.push(format!("Last: {preview}"));
    }
    lines.join("\n")
}

pub(crate) fn preview_line(item: &ClipboardItem) -> Option<String> {
    if let Some(text) = item.text.as_ref() {
        // Collapse whitespace so the "Last:" line stays a single tooltip row.
        let collapsed: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
        if collapsed.is_empty() {
            return None;
        }
        return Some(truncate_chars(&collapsed, PREVIEW_LIMIT));
    }
    if item.image_file.is_some() {
        return Some("image".to_owned());
    }
    item.name
        .as_ref()
        .map(|name| truncate_chars(name, PREVIEW_LIMIT))
}

fn truncate_chars(value: &str, maximum: usize) -> String {
    let mut chars = value.chars();
    let mut result: String = chars.by_ref().take(maximum).collect();
    if chars.next().is_some() {
        result.push('…');
    }
    result
}

pub(crate) fn plural_s(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
}
