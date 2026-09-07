//! Wayle's notification history, read and driven over the session bus.
//!
//! Wayle stays the daemon. It owns `org.freedesktop.Notifications`, keeps the
//! history, decides Do Not Disturb, and is the only party that may invoke an
//! action, because the sending application waits on an `ActionInvoked` signal
//! from the name it talked to. This panel only draws that list and asks Wayle
//! to act, so the notifications sit in the same window as everything else.

use std::io::{self, Write};
use std::thread;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use zbus::blocking::Connection;
use zbus::proxy::CacheProperties;
use zbus::zvariant::Type;

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

/// Wayle's notification history, carried whole by `notification-ipc.patch`.
///
/// `com.wayle.Notifications1` publishes an id and three strings, which is not
/// enough to draw a notification. This is the same history with the icon, the
/// image, the actions, the urgency and the timestamp still attached.
#[zbus::proxy(
    interface = "com.wayle.NotificationsExt1",
    default_service = "com.wayle.NotificationsExt1",
    default_path = "/com/wayle/NotificationsExt",
    gen_async = false
)]
trait WayleNotificationsExt {
    /// Every notification in history, newest first.
    fn list(&self) -> zbus::Result<Vec<Entry>>;

    /// Dismiss one notification.
    fn dismiss(&self, id: u32) -> zbus::Result<()>;

    /// Dismiss everything in history.
    fn dismiss_all(&self) -> zbus::Result<()>;

    /// Toggle Do Not Disturb.
    fn toggle_dnd(&self) -> zbus::Result<()>;

    #[zbus(property)]
    fn dnd(&self) -> zbus::Result<bool>;
}

/// One notification, exactly as `notification-ipc.patch` sends it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct Entry {
    pub id: u32,
    pub app_name: String,
    /// Icon name or path, whichever the sender supplied.
    pub app_icon: String,
    pub summary: String,
    pub body: String,
    /// Path to the notification's own image, empty when it has none.
    pub image_path: String,
    /// The desktop entry, which names the app more reliably than `app_name`.
    pub desktop_entry: String,
    /// 0 low, 1 normal, 2 critical.
    pub urgency: u32,
    /// Unix seconds, so a reader can say how long ago it arrived.
    pub timestamp: i64,
    /// Action id and label, in the order the sender listed them. Read but not
    /// drawn: running one is the daemon's to do and the panel does not offer
    /// it, so the field only keeps the wire format matching the patch.
    pub actions: Vec<(String, String)>,
}

impl Entry {
    /// The app a person would name, preferring what the sender called itself.
    pub fn app(&self) -> String {
        let name = if self.app_name.is_empty() {
            self.desktop_entry.as_str()
        } else {
            self.app_name.as_str()
        };
        if name.is_empty() {
            return String::from("Notifications");
        }

        let mut characters = name.chars();
        characters.next().map_or_else(String::new, |first| {
            first.to_uppercase().collect::<String>() + characters.as_str()
        })
    }

    /// The class its urgency colours the row by.
    pub fn urgency_class(&self) -> &'static str {
        match self.urgency {
            0 => "urgency-low",
            2 => "urgency-critical",
            _ => "urgency-normal",
        }
    }
}

/// How long ago a notification arrived, said the way a person would.
pub fn relative_time(timestamp: i64, now: i64) -> String {
    let seconds = (now - timestamp).max(0);
    match seconds {
        ..=44 => String::from("now"),
        45..=5399 => format!("{}m ago", (seconds + 30) / 60),
        5400..=86_399 => format!("{}h ago", (seconds + 1800) / 3600),
        _ => format!("{}d ago", (seconds + 43_200) / 86_400),
    }
}

/// A live handle on Wayle's notification history.
pub struct Notifications {
    proxy: WayleNotificationsExtProxy<'static>,
}

impl Notifications {
    /// Returns `None` when Wayle is not up; the panel then draws its other
    /// cards and says the list is unavailable rather than failing to open.
    pub fn connect() -> Option<Self> {
        let connection = Connection::session().ok()?;
        WayleNotificationsExtProxy::builder(&connection)
            .cache_properties(CacheProperties::No)
            .build()
            .ok()
            .map(|proxy| Notifications { proxy })
    }

    pub fn list(&self) -> Vec<Entry> {
        self.proxy.list().unwrap_or_default()
    }

    pub fn dnd(&self) -> bool {
        self.proxy.dnd().unwrap_or(false)
    }

    pub fn dismiss(&self, id: u32) {
        let _ = self.proxy.dismiss(id);
    }

    pub fn dismiss_all(&self) {
        let _ = self.proxy.dismiss_all();
    }

    pub fn toggle_dnd(&self) {
        let _ = self.proxy.toggle_dnd();
    }
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
