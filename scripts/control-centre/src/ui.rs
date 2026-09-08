//! The calendar and notification panel shown from Waybar.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use gtk::glib;
use gtk::prelude::*;
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

use crate::calendar;
use crate::wayle::{relative_time, Entry, Notifications};

/// How often notification state is refreshed while the panel is open.
const TICK: Duration = Duration::from_millis(750);

/// Distance from the top of the screen: the height of Waybar.
const TOP_MARGIN: i32 = 40;

/// Distance from the right edge of the screen.
const RIGHT_MARGIN: i32 = 6;

/// How tall the notification list may grow before it scrolls.
const NOTIFICATION_HEIGHT: i32 = 360;

/// Width kept clear on the right of a scrolling list. GTK draws the scrollbar
/// over the content, so without this it sits on the dismiss buttons.
const SCROLLBAR_LANE: i32 = 10;

/// How wide a wrapping label may ask to be. A label that wraps still reports
/// its *unwrapped* width as the width it would like, so without this the
/// longest calendar entry or notification body decides the panel's width.
const WRAP_CHARS: i32 = 24;

pub struct Panel {
    window: gtk::Window,
    notifications: Option<Notifications>,
    calendar_body: gtk::Box,
    notification_body: gtk::Box,
    dnd_toggle: gtk::Switch,
    /// Set while the switch is being written to, so reacting to the change
    /// does not ask Wayle to toggle what it just reported.
    syncing_dnd: Cell<bool>,
}

impl Panel {
    pub fn build(application: &gtk::Application) -> Rc<Self> {
        let window = gtk::Window::new();
        window.set_application(Some(application));
        window.add_css_class("control-centre");

        // The surface covers the whole output so the transparent area beside
        // the panel can catch a click and dismiss it, the same way Wayle's
        // own dropdown does.
        window.init_layer_shell();
        window.set_layer(Layer::Overlay);
        window.set_namespace(Some("control-centre"));
        for edge in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
            window.set_anchor(edge, true);
        }
        window.set_keyboard_mode(KeyboardMode::OnDemand);

        let calendar_body = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let notification_body = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let dnd_toggle = gtk::Switch::new();
        let clear_all = gtk::Button::with_label("Clear All");

        let column = gtk::Box::new(gtk::Orientation::Vertical, 0);
        column.add_css_class("column");
        column.append(&calendar_card(&calendar_body));
        column.append(&notification_card(
            &notification_body,
            &dnd_toggle,
            &clear_all,
        ));

        let panel = gtk::Box::new(gtk::Orientation::Vertical, 0);
        panel.add_css_class("panel");
        panel.set_halign(gtk::Align::End);
        panel.set_valign(gtk::Align::Start);
        panel.set_margin_top(TOP_MARGIN);
        panel.set_margin_end(right_margin());
        panel.append(&column);

        let backdrop = gtk::Box::new(gtk::Orientation::Vertical, 0);
        backdrop.set_hexpand(true);
        backdrop.set_vexpand(true);
        backdrop.append(&panel);
        window.set_child(Some(&backdrop));

        let this = Rc::new(Panel {
            window,
            notifications: Notifications::connect(),
            calendar_body,
            notification_body,
            dnd_toggle,
            syncing_dnd: Cell::new(false),
        });

        this.connect_dismissal(&backdrop, &panel);
        this.connect_dnd();
        this.connect_clear_all(&clear_all);
        this.start_ticking();
        this
    }

    /// A click anywhere outside the panel closes it, as does Escape.
    fn connect_dismissal(self: &Rc<Self>, backdrop: &gtk::Box, panel: &gtk::Box) {
        let click = gtk::GestureClick::new();
        click.connect_pressed({
            let this = Rc::clone(self);
            let panel = panel.clone();
            move |gesture, _, x, y| {
                // Only a press that missed the panel itself dismisses it.
                let Some(widget) = gesture.widget() else {
                    return;
                };
                if let Some((px, py)) = widget
                    .compute_point(&panel, &gtk::graphene::Point::new(x as f32, y as f32))
                    .map(|point| (f64::from(point.x()), f64::from(point.y())))
                {
                    let inside = px >= 0.0
                        && py >= 0.0
                        && px <= f64::from(panel.width())
                        && py <= f64::from(panel.height());
                    if inside {
                        return;
                    }
                }
                this.hide();
            }
        });
        backdrop.add_controller(click);

        let keys = gtk::EventControllerKey::new();
        keys.connect_key_pressed({
            let this = Rc::clone(self);
            move |_, key, _, _| {
                if key == gtk::gdk::Key::Escape {
                    this.hide();
                    return glib::Propagation::Stop;
                }
                glib::Propagation::Proceed
            }
        });
        self.window.add_controller(keys);
    }

    /// Refresh while open, and not at all while closed.
    fn start_ticking(self: &Rc<Self>) {
        let this = Rc::clone(self);
        glib::timeout_add_local(TICK, move || {
            if this.window.is_visible() {
                this.refresh_notifications();
            }
            glib::ControlFlow::Continue
        });
    }

    pub fn is_open(&self) -> bool {
        self.window.is_visible()
    }

    /// Open the panel, with everything it shows read fresh.
    pub fn show(self: &Rc<Self>) {
        self.refresh_calendar();
        self.refresh_notifications();
        self.window.present();
    }

    pub fn hide(&self) {
        self.window.set_visible(false);
    }

    pub fn toggle(self: &Rc<Self>) {
        if self.is_open() {
            self.hide();
        } else {
            self.show();
        }
    }

    fn refresh_calendar(&self) {
        clear(&self.calendar_body);
        let agenda = calendar::read();

        if let Some(range) = self.calendar_body.parent().and_then(range_label) {
            range.set_label(&agenda.range);
        }

        if agenda.is_empty() {
            let empty = gtk::Label::new(Some("Nothing scheduled in the next 7 days"));
            empty.add_css_class("calendar-empty");
            empty.set_xalign(0.0);
            empty.set_wrap(true);
            empty.set_max_width_chars(WRAP_CHARS);
            self.calendar_body.append(&empty);
            return;
        }

        for day in agenda.days {
            let heading = gtk::Label::new(Some(&day.heading));
            heading.add_css_class("calendar-day");
            heading.set_xalign(0.0);
            self.calendar_body.append(&heading);

            for entry in day.entries {
                let line = gtk::Label::new(Some(&format!("{}  {}", entry.icon, entry.title)));
                line.add_css_class("calendar-entry");
                line.set_xalign(0.0);
                line.set_wrap(true);
                line.set_wrap_mode(gtk::pango::WrapMode::WordChar);
                line.set_max_width_chars(WRAP_CHARS);
                self.calendar_body.append(&line);
            }
        }
    }

    /// Follow Do Not Disturb both ways: the switch asks Wayle to toggle, and
    /// a change made anywhere else moves the switch.
    fn connect_dnd(self: &Rc<Self>) {
        let this = Rc::clone(self);
        self.dnd_toggle.connect_state_set(move |_, _| {
            if !this.syncing_dnd.get() {
                if let Some(notifications) = this.notifications.as_ref() {
                    notifications.toggle_dnd();
                }
            }
            glib::Propagation::Proceed
        });
    }

    fn connect_clear_all(self: &Rc<Self>, button: &gtk::Button) {
        let this = Rc::clone(self);
        button.connect_clicked(move |_| {
            if let Some(notifications) = this.notifications.as_ref() {
                notifications.dismiss_all();
            }
            this.refresh_notifications();
        });
    }

    fn refresh_notifications(self: &Rc<Self>) {
        let Some(notifications) = self.notifications.as_ref() else {
            clear(&self.notification_body);
            self.notification_body
                .append(&placeholder("Wayle is not running", "notification-empty"));
            return;
        };

        self.syncing_dnd.set(true);
        self.dnd_toggle.set_active(notifications.dnd());
        self.syncing_dnd.set(false);

        clear(&self.notification_body);
        let entries = notifications.list();
        if entries.is_empty() {
            self.notification_body
                .append(&placeholder("No notifications", "notification-empty"));
            return;
        }

        let now = chrono::Local::now().timestamp();
        for (app, group) in group_by_app(entries) {
            self.notification_body
                .append(&self.group_widget(&app, &group, now));
        }
    }

    /// One app's notifications: a header naming it and counting them, then a
    /// row each. Grouping matches how Wayle's own list reads.
    fn group_widget(self: &Rc<Self>, app: &str, group: &[Entry], now: i64) -> gtk::Box {
        let name = gtk::Label::new(Some(app));
        name.add_css_class("notification-app");
        name.set_xalign(0.0);
        name.set_hexpand(true);
        name.set_halign(gtk::Align::Start);

        let count = gtk::Label::new(Some(&format!("({})", group.len())));
        count.add_css_class("notification-count");

        let header = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        header.add_css_class("notification-group-header");
        header.append(&name);
        header.append(&count);

        let widget = gtk::Box::new(gtk::Orientation::Vertical, 0);
        widget.add_css_class("notification-group");
        widget.append(&header);
        for entry in group {
            widget.append(&self.entry_widget(entry, now));
        }
        widget
    }

    fn entry_widget(self: &Rc<Self>, entry: &Entry, now: i64) -> gtk::Box {
        let summary = gtk::Label::new(Some(&entry.summary));
        summary.add_css_class("notification-summary");
        summary.set_xalign(0.0);
        summary.set_hexpand(true);
        summary.set_halign(gtk::Align::Start);
        summary.set_ellipsize(gtk::pango::EllipsizeMode::End);
        summary.set_max_width_chars(WRAP_CHARS);

        let age = gtk::Label::new(Some(&relative_time(entry.timestamp, now)));
        age.add_css_class("notification-age");

        let close = gtk::Button::with_label("\u{00d7}");
        close.add_css_class("notification-close");
        close.connect_clicked({
            let this = Rc::clone(self);
            let id = entry.id;
            move |_| {
                if let Some(notifications) = this.notifications.as_ref() {
                    notifications.dismiss(id);
                }
                this.refresh_notifications();
            }
        });

        let top = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        top.append(&summary);
        top.append(&age);
        top.append(&close);

        let row = gtk::Box::new(gtk::Orientation::Vertical, 2);
        row.set_css_classes(&["notification-row", entry.urgency_class()]);
        row.append(&top);

        if !entry.body.is_empty() {
            let body = gtk::Label::new(Some(&entry.body));
            body.add_css_class("notification-body");
            body.set_xalign(0.0);
            body.set_wrap(true);
            body.set_wrap_mode(gtk::pango::WrapMode::WordChar);
            body.set_max_width_chars(WRAP_CHARS);
            body.set_lines(3);
            body.set_ellipsize(gtk::pango::EllipsizeMode::End);
            row.append(&body);
        }

        row
    }
}

fn right_margin() -> i32 {
    std::env::var("CONTROL_CENTRE_RIGHT_MARGIN")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(RIGHT_MARGIN)
}

/// Notifications in the order they arrived, gathered under the app that sent
/// them. Groups keep the order of their first entry, so the newest app is top.
fn group_by_app(entries: Vec<Entry>) -> Vec<(String, Vec<Entry>)> {
    let mut groups: Vec<(String, Vec<Entry>)> = Vec::new();
    for entry in entries {
        let app = entry.app();
        match groups.iter_mut().find(|(name, _)| name == &app) {
            Some((_, group)) => group.push(entry),
            None => groups.push((app, vec![entry])),
        }
    }
    groups
}

fn placeholder(text: &str, class: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    label.add_css_class(class);
    label.set_xalign(0.0);
    label
}

/// A list that scrolls once it outgrows `height`, with room on the right for
/// the scrollbar so it does not sit on top of the content.
fn scrolling(body: &gtk::Box, height: i32) -> gtk::ScrolledWindow {
    body.set_margin_end(SCROLLBAR_LANE);

    let scroll = gtk::ScrolledWindow::new();
    scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroll.set_max_content_height(height);
    scroll.set_propagate_natural_height(true);
    scroll.set_child(Some(body));
    scroll
}

fn clear(container: &gtk::Box) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
}

/// The range label lives in the calendar card's header, beside its title.
fn range_label(card: gtk::Widget) -> Option<gtk::Label> {
    let header = card.first_child()?;
    let mut child = header.first_child();
    while let Some(widget) = child {
        if widget.has_css_class("card-range") {
            return widget.downcast::<gtk::Label>().ok();
        }
        child = widget.next_sibling();
    }
    None
}

fn card(title: &str, with_range: bool, body: &impl IsA<gtk::Widget>) -> gtk::Box {
    let heading = gtk::Label::new(Some(title));
    heading.add_css_class("card-title");
    heading.set_xalign(0.0);
    heading.set_hexpand(true);
    heading.set_halign(gtk::Align::Start);

    let header = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    header.add_css_class("card-header");
    header.append(&heading);

    if with_range {
        let range = gtk::Label::new(Some(""));
        range.add_css_class("card-range");
        range.set_halign(gtk::Align::End);
        header.append(&range);
    }

    let card = gtk::Box::new(gtk::Orientation::Vertical, 0);
    card.add_css_class("card");
    card.append(&header);
    card.append(body);
    card
}

fn calendar_card(body: &gtk::Box) -> gtk::Box {
    let card = card("Calendar", true, body);
    card.add_css_class("card-calendar");
    card
}

/// The notification card: a heading, the Do Not Disturb switch beside it, and
/// the list under both, scrolling once it outgrows its share of the column.
fn notification_card(body: &gtk::Box, dnd: &gtk::Switch, clear_all: &gtk::Button) -> gtk::Box {
    let heading = gtk::Label::new(Some("Notifications"));
    heading.add_css_class("card-title");
    heading.set_xalign(0.0);
    heading.set_hexpand(true);
    heading.set_halign(gtk::Align::Start);

    clear_all.add_css_class("notification-clear-all");
    clear_all.set_valign(gtk::Align::Center);

    let header = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    header.add_css_class("card-header");
    header.append(&heading);
    header.append(clear_all);

    let dnd_label = gtk::Label::new(Some("Do Not Disturb"));
    dnd_label.add_css_class("dnd-label");
    dnd_label.set_hexpand(true);
    dnd_label.set_halign(gtk::Align::Start);
    dnd.set_valign(gtk::Align::Center);

    // Its own row: the heading, "Clear All", a caption and a switch do not fit
    // across one column of this width.
    let dnd_row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    dnd_row.add_css_class("dnd-row");
    dnd_row.append(&dnd_label);
    dnd_row.append(dnd);

    let scroll = scrolling(body, NOTIFICATION_HEIGHT);

    let card = gtk::Box::new(gtk::Orientation::Vertical, 0);
    card.set_css_classes(&["card", "card-notifications"]);
    card.append(&header);
    card.append(&dnd_row);
    card.append(&scroll);
    card
}
