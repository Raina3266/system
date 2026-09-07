//! Wayle's notification dropdown, opened beside this panel.
//!
//! Notifications stay Wayle's job: it is the daemon, and only the daemon has
//! the icons, actions and urgency each entry carries. This panel shows the
//! calendar, media and system cards, and asks Wayle to bring its notification
//! dropdown up alongside, so the two read as one control centre.

use std::io::{self, Write};
use std::thread;
use std::time::Duration;

use zbus::blocking::Connection;
use zbus::proxy::CacheProperties;

/// The dropdown holding notification history.
const HISTORY_DROPDOWN: &str = "notification";

/// How often the badge re-reads the daemon. Wayle emits no `PropertiesChanged`
/// for these, so they are polled; this matches Waybar's own tick.
const POLL_INTERVAL: Duration = Duration::from_millis(750);

/// How long to wait before looking for a daemon that went away again.
const RETRY_DELAY: Duration = Duration::from_secs(3);

/// Wayle's notification extensions, which name their interface after their bus
/// name. Only the two readings the bar badge shows are declared.
#[zbus::proxy(
    interface = "com.wayle.Notifications1",
    default_service = "com.wayle.Notifications1",
    default_path = "/com/wayle/Notifications",
    gen_async = false
)]
trait WayleNotifications {
    /// Number of notifications in history.
    #[zbus(property)]
    fn count(&self) -> zbus::Result<u32>;

    /// Whether Do Not Disturb is on.
    #[zbus(property)]
    fn dnd(&self) -> zbus::Result<bool>;
}

/// Wayle's shell IPC. `DropdownToggle` is the one call `waybar-dropdown.patch`
/// adds, so something other than Wayle's own bar can open a dropdown.
#[zbus::proxy(
    interface = "com.wayle.Shell1",
    default_service = "com.wayle.Shell1",
    default_path = "/com/wayle/Shell",
    gen_async = false
)]
trait WayleShell {
    /// Toggles `name` on `monitor`, `offset` logical pixels from the screen
    /// edge. An empty monitor leaves the output to Wayle.
    fn dropdown_toggle(&self, name: &str, monitor: &str, offset: i32) -> zbus::Result<()>;
}

/// Toggle Wayle's notification dropdown.
///
/// Errors are returned rather than raised: Wayle not running is a reason to
/// show this panel without its notification column, not a reason to fail.
pub fn toggle_notifications(monitor: &str, offset: i32) -> Result<(), String> {
    let connection = Connection::session()
        .map_err(|error| format!("could not reach the session bus: {error}"))?;
    WayleShellProxy::new(&connection)
        .map_err(|error| format!("could not reach Wayle: {error}"))?
        .dropdown_toggle(HISTORY_DROPDOWN, monitor, offset)
        .map_err(|error| format!("could not toggle the notification dropdown: {error}"))
}

/// What the bar badge shows about the daemon.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Status {
    pub count: u32,
    pub dnd: bool,
}

/// The badge glyphs: nothing waiting, something waiting, and silenced.
const BELL_QUIET: &str = "\u{f009c}";
const BELL_ACTIVE: &str = "\u{f009a}";
const BELL_SILENCED: &str = "\u{f009b}";

/// Stream Waybar JSON for the notification badge until stdout closes.
///
/// This replaces `wayle notify status --watch` piped through `jq`: one
/// process, no shell, and no CLI carried by a patch against Wayle.
pub fn watch_badge() -> Result<(), String> {
    loop {
        if let Some(proxy) = notifications() {
            while let Some(status) = read(&proxy) {
                if !print_badge(status) {
                    return Ok(());
                }
                thread::sleep(POLL_INTERVAL);
            }
        }

        // Wayle restarted, or has not claimed its name yet; either way the
        // count is no longer known, so the badge falls back to a quiet bell.
        if !print_badge(Status::default()) {
            return Ok(());
        }
        thread::sleep(RETRY_DELAY);
    }
}

fn notifications() -> Option<WayleNotificationsProxy<'static>> {
    let connection = Connection::session().ok()?;
    WayleNotificationsProxy::builder(&connection)
        // Wayle never signals these properties, so a caching proxy would keep
        // answering with whatever was true when it was built.
        .cache_properties(CacheProperties::No)
        .build()
        .ok()
}

fn read(proxy: &WayleNotificationsProxy<'_>) -> Option<Status> {
    Some(Status {
        count: proxy.count().ok()?,
        dnd: proxy.dnd().unwrap_or(false),
    })
}

/// Returns false once Waybar has gone away and there is nothing to write to.
fn print_badge(status: Status) -> bool {
    let (text, class) = badge(status);
    println!(
        "{{\"text\":\"{text}\",\"class\":\"{class}\",\"alt\":\"{class}\",\"tooltip\":\"{}\"}}",
        tooltip(status),
    );
    io::stdout().flush().is_ok()
}

/// What hovering the badge explains. None of these need escaping: the count is
/// a number and the rest is fixed text.
pub fn tooltip(status: Status) -> String {
    if status.dnd {
        String::from("Do Not Disturb")
    } else if status.count > 0 {
        format!("{} waiting", status.count)
    } else {
        String::from("No notifications")
    }
}

/// The badge's label and the class `themes/waybar.css` colours it by.
pub fn badge(status: Status) -> (String, &'static str) {
    if status.dnd {
        (BELL_SILENCED.to_owned(), "dnd")
    } else if status.count > 0 {
        (format!("{BELL_ACTIVE} {}", status.count), "notification")
    } else {
        (BELL_QUIET.to_owned(), "quiet")
    }
}

#[cfg(test)]
mod tests;
