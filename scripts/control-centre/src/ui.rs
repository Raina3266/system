//! The panel: a layer-shell surface holding the calendar, media and system
//! cards, with Wayle's notification dropdown toggled alongside it.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use gtk::glib;
use gtk::prelude::*;
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

use crate::media::{clock, Players};
use crate::ring::{Colour, Ring};
use crate::{calendar, media, system, wayle};

/// How often the media rows and system dials are re-read while open.
const TICK: Duration = Duration::from_millis(750);

/// Distance from the top of the screen: the height of Waybar.
const TOP_MARGIN: i32 = 40;

/// Room kept on the right for Wayle's notification dropdown, so the two sit
/// side by side rather than on top of one another. Override with
/// `CONTROL_CENTRE_RIGHT_MARGIN` if Wayle's dropdown is a different width.
const RIGHT_MARGIN: i32 = 306;

/// Passed to Wayle so its dropdown clears Waybar by the same distance.
const WAYLE_OFFSET: i32 = TOP_MARGIN;

pub struct Panel {
    window: gtk::Window,
    players: Option<Players>,
    monitor: RefCell<system::Monitor>,
    calendar_body: gtk::Box,
    media_body: gtk::Box,
    rings: Rings,
    /// Which output Wayle should open its dropdown on; empty lets it choose.
    output: String,
}

struct Rings {
    cpu: Ring,
    memory: Ring,
    disk: Ring,
    temperature: Ring,
}

impl Panel {
    pub fn build(application: &gtk::Application, output: String) -> Rc<Self> {
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
        // The palette from niri/wayle/default.nix, so the dials match the
        // notification dropdown sitting beside them.
        let rings = Rings {
            cpu: Ring::new("CPU", Colour(0.478, 0.988, 1.0)),
            memory: Ring::new("RAM", Colour(1.0, 0.494, 0.859)),
            disk: Ring::new("DISK", Colour(0.996, 0.871, 0.365)),
            temperature: Ring::new("TEMP", Colour(1.0, 0.431, 0.431)),
        };

        let panel = gtk::Box::new(gtk::Orientation::Vertical, 0);
        panel.add_css_class("panel");
        panel.set_halign(gtk::Align::End);
        panel.set_valign(gtk::Align::Start);
        panel.set_margin_top(TOP_MARGIN);
        panel.set_margin_end(right_margin());
        panel.append(&calendar_card(&calendar_body));
        panel.append(&media_card(&media_body));
        panel.append(&system_card(&rings));

        let backdrop = gtk::Box::new(gtk::Orientation::Vertical, 0);
        backdrop.set_hexpand(true);
        backdrop.set_vexpand(true);
        backdrop.append(&panel);
        window.set_child(Some(&backdrop));

        let this = Rc::new(Panel {
            window,
            players: Players::connect().ok(),
            monitor: RefCell::new(system::Monitor::new()),
            calendar_body,
            media_body,
            rings,
            output,
        });

        this.connect_dismissal(&backdrop, &panel);
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
            }
            glib::ControlFlow::Continue
        });
    }

    pub fn is_open(&self) -> bool {
        self.window.is_visible()
    }

    /// Open the panel, and bring Wayle's notification dropdown up beside it.
    pub fn show(self: &Rc<Self>) {
        self.refresh_calendar();
        self.refresh_live();
        self.window.present();
        self.toggle_wayle();
    }

    /// Close the panel, and put Wayle's dropdown away with it.
    pub fn hide(&self) {
        if !self.window.is_visible() {
            return;
        }
        self.window.set_visible(false);
        self.toggle_wayle();
    }

    pub fn toggle(self: &Rc<Self>) {
        if self.is_open() {
            self.hide();
        } else {
            self.show();
        }
    }

    /// Wayle not running is a panel without its notification column, not a
    /// failure: the calendar, media and system cards are this program's.
    fn toggle_wayle(&self) {
        if let Err(error) = wayle::toggle_notifications(&self.output, WAYLE_OFFSET) {
            eprintln!("control-centre: {error}");
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
    card("System", false, &row)
}
