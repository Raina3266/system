//! Waybar `custom/clipboard` backend.
//!
//! `rofi-clipboard status` is a long-running process that folds the old
//! systemd `wl-paste --watch rofi-clipboard capture` collector into the bar
//! module itself. A child `wl-paste --watch` keeps capturing every clipboard
//! change (so history stays event-driven and nothing is missed), while this
//! process emits one JSON status line whenever the history file changes, so
//! Waybar updates the glyph and tooltip live.

use std::io::Write;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result};

use crate::clipboard::{wl_copy_binary, wl_paste_binary};
use crate::model::{ClipboardItem, ItemKind, json_escape};
use crate::store::ClipboardStore;

const STATUS_POLL_INTERVAL: Duration = Duration::from_secs(1);
const WATCHER_RESTART_DELAY: Duration = Duration::from_secs(2);
const PREVIEW_LIMIT: usize = 60;

static SHOULD_EXIT: AtomicBool = AtomicBool::new(false);

extern "C" fn handle_signal(_signum: i32) {
    SHOULD_EXIT.store(true, Ordering::SeqCst);
}

unsafe extern "C" {
    fn signal(signum: i32, handler: extern "C" fn(i32)) -> usize;
}

const SIGTERM: i32 = 15;
const SIGINT: i32 = 2;
const SIGHUP: i32 = 1;

const PR_SET_PDEATHSIG: i32 = 1;
const SIGKILL: i32 = 9;

unsafe extern "C" {
    fn prctl(option: i32, ...) -> i32;
    fn getppid() -> i32;
    fn _exit(status: i32) -> !;
}

fn request_parent_death_signal() {
    let rc = unsafe { prctl(PR_SET_PDEATHSIG, SIGKILL as usize, 0, 0, 0) };
    if rc != 0 {
        return;
    }
    if unsafe { getppid() } == 1 {
        unsafe { _exit(0) };
    }
}

pub fn run_status() -> Result<()> {
    request_parent_death_signal();
    install_exit_handlers();

    // Emit an initial line so the module is not blank until the first change.
    let _ = print_status();

    let self_exe = std::env::current_exe().context("locate rofi-clipboard executable")?;
    let history = ClipboardStore::discover()?.history_file().to_path_buf();

    let mut last_mtime = file_mtime(&history);
    let mut child: Option<Child> = spawn_watcher(&self_exe);

    loop {
        if SHOULD_EXIT.load(Ordering::SeqCst) {
            break;
        }

        // Reap an exited watcher and respawn it, mirroring the old systemd
        // unit's Restart=on-failure / RestartSec=2 semantics.
        let exited = match child.as_mut() {
            Some(process) => process.try_wait().ok().flatten().is_some(),
            None => true,
        };
        if exited {
            std::thread::sleep(WATCHER_RESTART_DELAY);
            if SHOULD_EXIT.load(Ordering::SeqCst) {
                break;
            }
            child = spawn_watcher(&self_exe);
        }

        std::thread::sleep(STATUS_POLL_INTERVAL);

        if SHOULD_EXIT.load(Ordering::SeqCst) {
            break;
        }

        let mtime = file_mtime(&history);
        if mtime != last_mtime {
            last_mtime = mtime;
            let _ = print_status();
        }
    }

    // Reap the watcher so Waybar reloads do not leave orphaned `wl-paste`
    // processes behind. `kill` is harmless if the child already exited.
    if let Some(mut watcher) = child {
        let _ = watcher.kill();
        let _ = watcher.wait();
    }
    Ok(())
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

    let text = format!("<span size='large'>{}</span>", glyph(latest));
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

fn spawn_watcher(self_exe: &Path) -> Option<Child> {
    let mut command = Command::new(wl_paste_binary());
    command
        .arg("--watch")
        .arg(self_exe)
        .arg("capture")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit());
    unsafe {
        command.pre_exec(|| {
            request_parent_death_signal();
            Ok(())
        });
    }
    match command.spawn() {
        Ok(child) => Some(child),
        Err(error) => {
            eprintln!("rofi-clipboard: failed to spawn wl-paste watcher: {error}");
            None
        }
    }
}

fn file_mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).ok()?.modified().ok()
}

fn install_exit_handlers() {
    unsafe {
        signal(SIGTERM, handle_signal);
        signal(SIGINT, handle_signal);
        signal(SIGHUP, handle_signal);
    }
}

/// The module's single glyph, varying with the most recently captured item so
/// the bar reflects what was last copied at a glance.
fn glyph(latest: Option<&ClipboardItem>) -> &'static str {
    match latest {
        Some(item) if item.image_file.is_some() => "󰋩",
        Some(item) if item.kind == ItemKind::File => "󰈔",
        _ => "󰍩",
    }
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

fn tooltip(count: usize, latest: Option<&ClipboardItem>) -> String {
    let mut lines = vec![format!("Clipboard: {count} item{}", plural_s(count))];
    if let Some(item) = latest
        && let Some(preview) = preview_line(item)
    {
        lines.push(format!("Last: {preview}"));
    }
    lines.join("\n")
}

fn preview_line(item: &ClipboardItem) -> Option<String> {
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

fn plural_s(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glyph_reflects_last_copied_kind() {
        assert_eq!(glyph(None), "󰍩");

        let text = ClipboardItem {
            id: 1,
            kind: ItemKind::Text,
            text: Some("hi".to_owned()),
            image_file: None,
            name: None,
            mime: "text/plain;charset=utf-8".to_owned(),
            pinned: false,
            created_at: 1,
            digest: "d".to_owned(),
        };
        assert_eq!(glyph(Some(&text)), "󰍩");

        let file = ClipboardItem {
            id: 2,
            kind: ItemKind::File,
            text: None,
            image_file: None,
            name: Some("report.pdf".to_owned()),
            mime: "text/uri-list".to_owned(),
            pinned: false,
            created_at: 2,
            digest: "d2".to_owned(),
        };
        assert_eq!(glyph(Some(&file)), "󰈔");

        let image = ClipboardItem {
            id: 3,
            kind: ItemKind::File,
            text: None,
            image_file: Some("x.png".to_owned()),
            name: None,
            mime: "image/png".to_owned(),
            pinned: false,
            created_at: 3,
            digest: "d3".to_owned(),
        };
        assert_eq!(glyph(Some(&image)), "󰋩");
    }

    #[test]
    fn tooltip_collapses_multiline_text_into_one_preview_row() {
        let item = ClipboardItem {
            id: 1,
            kind: ItemKind::Text,
            text: Some("  hello\n  world  ".to_owned()),
            image_file: None,
            name: None,
            mime: "text/plain;charset=utf-8".to_owned(),
            pinned: false,
            created_at: 1,
            digest: "d".to_owned(),
        };
        assert_eq!(preview_line(&item).as_deref(), Some("hello world"));
        assert!(tooltip(1, Some(&item)).contains("Last: hello world"));
    }

    #[test]
    fn empty_text_yields_no_preview() {
        let item = ClipboardItem {
            id: 1,
            kind: ItemKind::Text,
            text: Some("   \n  ".to_owned()),
            image_file: None,
            name: None,
            mime: "text/plain;charset=utf-8".to_owned(),
            pinned: false,
            created_at: 1,
            digest: "d".to_owned(),
        };
        assert_eq!(preview_line(&item), None);
    }

    #[test]
    fn long_text_is_truncated_with_an_ellipsis() {
        let item = ClipboardItem {
            id: 1,
            kind: ItemKind::Text,
            text: Some("a".repeat(80)),
            image_file: None,
            name: None,
            mime: "text/plain;charset=utf-8".to_owned(),
            pinned: false,
            created_at: 1,
            digest: "d".to_owned(),
        };
        let preview = preview_line(&item).unwrap();
        assert_eq!(preview.chars().count(), PREVIEW_LIMIT + 1);
        assert!(preview.ends_with('…'));
    }

    #[test]
    fn pluralization_is_correct() {
        assert_eq!(plural_s(0), "s");
        assert_eq!(plural_s(1), "");
        assert_eq!(plural_s(2), "s");
    }
}
