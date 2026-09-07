use super::{badge, tooltip, Status};

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
