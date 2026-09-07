use super::{badge, relative_time, tooltip, Entry, Status};

#[test]
fn nothing_waiting_is_a_quiet_bell() {
    let (text, class) = badge(Status::default());
    assert_eq!(text, "\u{f009c}");
    assert_eq!(class, "quiet");
}

#[test]
fn waiting_notifications_are_counted_beside_the_bell() {
    let (text, class) = badge(Status {
        count: 3,
        dnd: false,
    });
    assert_eq!(text, "\u{f009a} 3");
    assert_eq!(class, "notification");
}

#[test]
fn do_not_disturb_silences_the_bell_and_drops_the_count() {
    // The count is not worth showing when nothing will be shown anyway.
    let (text, class) = badge(Status {
        count: 7,
        dnd: true,
    });
    assert_eq!(text, "\u{f009b}");
    assert_eq!(class, "dnd");
}

#[test]
fn the_tooltip_says_what_the_glyph_cannot() {
    assert_eq!(tooltip(Status::default()), "No notifications");
    assert_eq!(
        tooltip(Status {
            count: 3,
            dnd: false
        }),
        "3 waiting"
    );
    assert_eq!(
        tooltip(Status {
            count: 7,
            dnd: true
        }),
        "Do Not Disturb"
    );
}

fn entry(app_name: &str, desktop_entry: &str, urgency: u32) -> Entry {
    Entry {
        id: 1,
        app_name: app_name.to_owned(),
        app_icon: String::new(),
        summary: String::from("Something happened"),
        body: String::new(),
        image_path: String::new(),
        desktop_entry: desktop_entry.to_owned(),
        urgency,
        timestamp: 0,
        actions: Vec::new(),
    }
}

#[test]
fn an_app_is_named_by_what_it_called_itself() {
    assert_eq!(
        entry("google chrome", "google-chrome", 1).app(),
        "Google chrome"
    );
    // Falling back to the desktop entry when the sender gave no name.
    assert_eq!(entry("", "notify-send", 1).app(), "Notify-send");
    // And to something sayable when it gave neither.
    assert_eq!(entry("", "", 1).app(), "Notifications");
}

#[test]
fn urgency_picks_the_class_that_colours_the_row() {
    assert_eq!(entry("a", "a", 0).urgency_class(), "urgency-low");
    assert_eq!(entry("a", "a", 1).urgency_class(), "urgency-normal");
    assert_eq!(entry("a", "a", 2).urgency_class(), "urgency-critical");
    // Anything unexpected reads as normal rather than as nothing.
    assert_eq!(entry("a", "a", 9).urgency_class(), "urgency-normal");
}

#[test]
fn an_age_is_said_the_way_a_person_would() {
    let now = 1_000_000;
    assert_eq!(relative_time(now, now), "now");
    assert_eq!(relative_time(now - 44, now), "now");
    assert_eq!(relative_time(now - 60, now), "1m ago");
    assert_eq!(relative_time(now - 22 * 60, now), "22m ago");
    assert_eq!(relative_time(now - 3 * 3600, now), "3h ago");
    assert_eq!(relative_time(now - 51 * 3600, now), "2d ago");
}

#[test]
fn a_clock_skew_reads_as_now_rather_than_as_the_future() {
    let now = 1_000_000;
    assert_eq!(relative_time(now + 500, now), "now");
}

/// Talks to a real `com.wayle.NotificationsExt1` on the session bus, so the
/// method names, the signature of an entry and the property all have to match
/// what `notification-ipc.patch` serves. Ignored by default because it needs a
/// daemon; run it with `cargo test -- --ignored` inside a session that has one.
#[test]
#[ignore]
fn the_history_client_matches_the_interface_wayle_serves() {
    let notifications = super::Notifications::connect().expect("a daemon on the session bus");

    let entries = notifications.list();
    assert!(!entries.is_empty(), "the daemon offered no notifications");

    let first = &entries[0];
    assert!(!first.summary.is_empty());
    assert!(!first.app().is_empty());

    // Every call the panel's buttons make, so a wrong name or signature fails
    // here rather than silently doing nothing under the pointer.
    let with_actions = entries
        .iter()
        .find(|entry| !entry.actions.is_empty())
        .expect("an entry carrying its actions");
    notifications.invoke(with_actions.id, &with_actions.actions[0].0);
    notifications.dismiss(first.id);
    notifications.toggle_dnd();
    let _ = notifications.dnd();
    notifications.dismiss_all();

    // DismissAll emptied the mock's history, which proves the reads are live
    // rather than cached.
    assert!(notifications.list().is_empty());
}
