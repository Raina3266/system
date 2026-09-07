//! Wayle's notification dropdown, opened beside this panel.
//!
//! Notifications stay Wayle's job: it is the daemon, and only the daemon has
//! the icons, actions and urgency each entry carries. This panel shows the
//! calendar, media and system cards, and asks Wayle to bring its notification
//! dropdown up alongside, so the two read as one control centre.

use zbus::blocking::Connection;

/// The dropdown holding notification history.
const HISTORY_DROPDOWN: &str = "notification";

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
