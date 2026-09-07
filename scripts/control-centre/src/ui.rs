//! The panel: a layer-shell surface holding the calendar, media and system
//! cards, with Wayle's notification dropdown toggled alongside it.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use gtk::glib;
use gtk::prelude::*;
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

use crate::media::{clock, Players};
use crate::ring::{Colour, Ring};
use crate::wayle::{relative_time, Entry, Notifications};
use crate::{calendar, media, system};

/// How often the media rows and system dials are re-read while open.
const TICK: Duration = Duration::from_millis(750);

/// Distance from the top of the screen: the height of Waybar.
const TOP_MARGIN: i32 = 40;

/// Distance from the right edge of the screen.
const RIGHT_MARGIN: i32 = 6;

/// How tall the notification list may grow before it scrolls, so a busy day
/// does not push the panel off the bottom of the screen.
const NOTIFICATION_HEIGHT: i32 = 260;

pub struct Panel {
    window: gtk::Window,
    players: Option<Players>,
    notifications: Option<Notifications>,
    monitor: RefCell<system::Monitor>,
    calendar_body: gtk::Box,
    media_body: gtk::Box,
    notification_body: gtk::Box,
    dnd_toggle: gtk::Switch,
    /// Set while the switch is being written to, so reacting to the change
    /// does not ask Wayle to toggle what it just reported.
    syncing_dnd: Cell<bool>,
    rings: Rings,
}

struct Rings {
    cpu: Ring,
    memory: Ring,
    disk: Ring,
    temperature: Ring,
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
        let media_body = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let notification_body = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let dnd_toggle = gtk::Switch::new();
        let clear_all = gtk::Button::with_label("Clear All");
        // The palette from niri/wayle/default.nix, so the dials match the
        // notification dropdown sitting beside them.
        let rings = Rings {
            cpu: Ring::new("CPU", Colour(0.478, 0.988, 1.0)),
            memory: Ring::new("RAM", Colour(1.0, 0.494, 0.859)),
            disk: Ring::new("DISK", Colour(0.996, 0.871, 0.365)),
            temperature: Ring::new("TEMP", Colour(1.0, 0.431, 0.431)),
        };

        // Two columns, as Wayle's control centre had: the agenda reads as a
        // tall block, and the readings and players stack beside it.
        let left = gtk::Box::new(gtk::Orientation::Vertical, 0);
        left.add_css_class("column");
        left.append(&calendar_card(&calendar_body));
        left.append(&notification_card(
            &notification_body,
            &dnd_toggle,
            &clear_all,
        ));

        let right = gtk::Box::new(gtk::Orientation::Vertical, 0);
        right.add_css_class("column");
        right.append(&system_card(&rings));
        right.append(&media_card(&media_body));

        let columns = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        columns.add_css_class("columns");
        columns.append(&left);
        columns.append(&right);

        let panel = gtk::Box::new(gtk::Orientation::Vertical, 0);
        panel.add_css_class("panel");
        panel.set_halign(gtk::Align::End);
        panel.set_valign(gtk::Align::Start);
        panel.set_margin_top(TOP_MARGIN);
        panel.set_margin_end(right_margin());
        panel.append(&columns);

        let backdrop = gtk::Box::new(gtk::Orientation::Vertical, 0);
        backdrop.set_hexpand(true);
        backdrop.set_vexpand(true);
        backdrop.append(&panel);
        window.set_child(Some(&backdrop));

        let this = Rc::new(Panel {
            window,
            players: Players::connect().ok(),
            notifications: Notifications::connect(),
            monitor: RefCell::new(system::Monitor::new()),
            calendar_body,
            media_body,
            notification_body,
            dnd_toggle,
            syncing_dnd: Cell::new(false),
            rings,
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
                this.refresh_live();
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
        self.refresh_live();
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
            body.set_lines(3);
            body.set_ellipsize(gtk::pango::EllipsizeMode::End);
            row.append(&body);
        }

        if !entry.actions.is_empty() {
            row.append(&self.actions_widget(entry));
        }
        row
    }

    /// The sender's own buttons. Wayle runs them: the application is waiting
    /// on a signal from the daemon, not from this panel.
    fn actions_widget(self: &Rc<Self>, entry: &Entry) -> gtk::Box {
        let actions = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        actions.add_css_class("notification-actions");

        for (id, label) in &entry.actions {
            let button = gtk::Button::with_label(label);
            button.add_css_class("notification-action");
            button.connect_clicked({
                let this = Rc::clone(self);
                let notification = entry.id;
                let action = id.clone();
                move |_| {
                    if let Some(notifications) = this.notifications.as_ref() {
                        notifications.invoke(notification, &action);
                    }
                    this.refresh_notifications();
                }
            });
            actions.append(&button);
        }
        actions
    }

    fn refresh_live(self: &Rc<Self>) {
        let stats = self.monitor.borrow_mut().sample();
        self.rings.cpu.set(stats.cpu.fraction, &stats.cpu.text);
        self.rings
            .memory
            .set(stats.memory.fraction, &stats.memory.text);
        self.rings.disk.set(stats.disk.fraction, &stats.disk.text);
        self.rings
            .temperature
            .set(stats.temperature.fraction, &stats.temperature.text);

        self.refresh_media();
    }

    fn refresh_media(self: &Rc<Self>) {
        let Some(players) = self.players.as_ref() else {
            return;
        };
        let snapshot = players.snapshot();

        clear(&self.media_body);
        if snapshot.is_empty() {
            let empty = gtk::Label::new(Some("Nothing playing"));
            empty.add_css_class("media-empty");
            empty.set_xalign(0.0);
            self.media_body.append(&empty);
            return;
        }

        for player in snapshot {
            self.media_body.append(&self.media_row(&player));
        }
    }

    fn media_row(self: &Rc<Self>, player: &media::Player) -> gtk::Box {
        let row = gtk::Box::new(gtk::Orientation::Vertical, 2);
        row.add_css_class("media-row");

        let title = gtk::Label::new(Some(&player.title));
        title.add_css_class("media-track");
        title.set_xalign(0.0);
        title.set_ellipsize(gtk::pango::EllipsizeMode::End);
        row.append(&title);

        let meta = if player.artist.is_empty() {
            player.source.clone()
        } else {
            format!("{}  ·  {}", player.source, player.artist)
        };
        let meta = gtk::Label::new(Some(&meta));
        meta.add_css_class("media-meta");
        meta.set_xalign(0.0);
        meta.set_ellipsize(gtk::pango::EllipsizeMode::End);
        row.append(&meta);

        row.append(&self.transport(player));
        if player.has_length() {
            row.append(&self.seek(player));
        }
        row
    }

    fn transport(self: &Rc<Self>, player: &media::Player) -> gtk::Box {
        let controls = gtk::Box::new(gtk::Orientation::Horizontal, 2);
        controls.set_halign(gtk::Align::Start);

        for (glyph, action) in [
            ("󰒮", Action::Previous),
            (player.status.icon(), Action::PlayPause),
            ("󰒭", Action::Next),
        ] {
            let button = gtk::Button::with_label(glyph);
            button.add_css_class("media-button");
            button.connect_clicked({
                let bus = player.bus.clone();
                let this = Rc::clone(self);
                move |_| this.act(&bus, action)
            });
            controls.append(&button);
        }
        controls
    }

    fn seek(self: &Rc<Self>, player: &media::Player) -> gtk::Box {
        let scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 1.0, 0.001);
        scale.add_css_class("media-seek");
        scale.set_draw_value(false);
        scale.set_hexpand(true);
        scale.set_value(player.progress());
        scale.connect_change_value({
            let bus = player.bus.clone();
            let length = player.length;
            let this = Rc::clone(self);
            move |_, _, value| {
                if let Some(players) = this.players.as_ref() {
                    players.seek_to(&bus, value, length);
                }
                glib::Propagation::Proceed
            }
        });

        let elapsed = gtk::Label::new(Some(&clock(player.position)));
        elapsed.add_css_class("media-clock");
        let total = gtk::Label::new(Some(&clock(player.length)));
        total.add_css_class("media-clock");

        let bar = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        bar.append(&elapsed);
        bar.append(&scale);
        bar.append(&total);
        bar
    }

    fn act(self: &Rc<Self>, bus: &str, action: Action) {
        let Some(players) = self.players.as_ref() else {
            return;
        };
        match action {
            Action::Previous => players.previous(bus),
            Action::PlayPause => players.play_pause(bus),
            Action::Next => players.next(bus),
        }
        self.refresh_media();
    }
}

#[derive(Clone, Copy)]
enum Action {
    Previous,
    PlayPause,
    Next,
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

fn card(title: &str, with_range: bool, body: &gtk::Box) -> gtk::Box {
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

    let scroll = gtk::ScrolledWindow::new();
    scroll.add_css_class("notification-scroll");
    scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroll.set_max_content_height(NOTIFICATION_HEIGHT);
    scroll.set_propagate_natural_height(true);
    scroll.set_child(Some(body));

    let card = gtk::Box::new(gtk::Orientation::Vertical, 0);
    card.set_css_classes(&["card", "card-notifications"]);
    card.append(&header);
    card.append(&dnd_row);
    card.append(&scroll);
    card
}

fn media_card(body: &gtk::Box) -> gtk::Box {
    card("Media players", false, body)
}

fn system_card(rings: &Rings) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    row.set_homogeneous(true);
    row.append(&rings.cpu.widget);
    row.append(&rings.memory.widget);
    row.append(&rings.disk.widget);
    row.append(&rings.temperature.widget);

    // The dials name themselves, so this card goes without a heading.
    let card = gtk::Box::new(gtk::Orientation::Vertical, 0);
    card.set_css_classes(&["card", "card-system"]);
    card.append(&row);
    card
}
