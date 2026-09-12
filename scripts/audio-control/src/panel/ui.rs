//! The panel: four tabs on a monitor-local layer surface.
//!
//! The tabs and what they hold are the ones the Rofi menu has always had, so
//! `Mode` decides both. Everything drawn here comes from one `Snapshot`, and
//! every press goes back as one `Command`; the UI itself knows nothing about
//! PulseAudio or BlueZ.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

use gtk::gdk;
use gtk::glib;
use gtk::prelude::*;
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

use super::state::{Command, Snapshot, TABS, View};
use super::worker;
use crate::model::{AudioEntry, BluetoothEntry, Mode, StreamEntry};

/// How often the panel re-reads while it is open. Nothing is read while it is
/// hidden: PulseAudio and BlueZ should cost nothing when nobody is looking.
const TICK: Duration = Duration::from_millis(1500);
/// How often the UI looks for a reading the worker has finished.
const DRAIN: Duration = Duration::from_millis(80);
/// Distance from the top of the screen: the height of Waybar, plus a gap.
const TOP_MARGIN: i32 = 46;
const PANEL_WIDTH: i32 = 420;
const LIST_MAX_HEIGHT: i32 = 460;

pub fn run(app: &gtk::Application, monitor: Option<String>, toggles: Receiver<String>) {
    let (commands, command_rx) = channel::<Command>();
    let (snapshot_tx, snapshots) = channel::<Snapshot>();
    worker::spawn(command_rx, snapshot_tx);

    let view: Rc<RefCell<View>> = Rc::default();
    let latest: Rc<RefCell<Snapshot>> = Rc::default();
    let drawn: Rc<RefCell<String>> = Rc::default();

    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    content.add_css_class("audio-content");

    let scroll = gtk::ScrolledWindow::new();
    scroll.add_css_class("audio-scroll");
    scroll.set_hscrollbar_policy(gtk::PolicyType::Never);
    scroll.set_propagate_natural_height(true);
    scroll.set_max_content_height(LIST_MAX_HEIGHT);
    scroll.set_child(Some(&content));

    let tabs = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    tabs.add_css_class("audio-tabs");

    let panel = gtk::Box::new(gtk::Orientation::Vertical, 0);
    panel.add_css_class("audio-panel");
    panel.set_width_request(PANEL_WIDTH);
    panel.append(&tabs);
    panel.append(&scroll);

    let window = gtk::ApplicationWindow::new(app);
    window.set_decorated(false);
    window.set_resizable(false);
    window.add_css_class("audio-window");
    window.set_child(Some(&panel));
    layer_shell(&window, monitor.as_deref());

    let dismiss = gtk::EventControllerKey::new();
    let hiding = window.clone();
    dismiss.connect_key_pressed(move |_, key, _, _| {
        if key == gdk::Key::Escape {
            hiding.set_visible(false);
            return glib::Propagation::Stop;
        }
        glib::Propagation::Proceed
    });
    window.add_controller(dismiss);

    build_tabs(&tabs, &view, &drawn, &commands);

    let redraw = {
        let (content, view, latest, drawn, commands, tabs) = (
            content.clone(),
            view.clone(),
            latest.clone(),
            drawn.clone(),
            commands.clone(),
            tabs.clone(),
        );
        move || {
            let snapshot = latest.borrow().clone();
            let current = view.borrow().clone();
            let signature = signature(&snapshot, &current);
            if *drawn.borrow() == signature {
                return;
            }
            drawn.replace(signature);
            mark_tabs(&tabs, current.tab());
            clear(&content);
            draw(&content, &snapshot, &current, &view, &commands, &drawn);
        }
    };

    let pump = redraw.clone();
    glib::timeout_add_local(DRAIN, {
        let latest = latest.clone();
        move || {
            if let Some(snapshot) = snapshots.try_iter().last() {
                latest.replace(snapshot);
                pump();
            }
            glib::ControlFlow::Continue
        }
    });

    let ticking = commands.clone();
    let visible = window.clone();
    glib::timeout_add_local(TICK, move || {
        if visible.is_visible() {
            let _ = ticking.send(Command::Refresh);
        }
        glib::ControlFlow::Continue
    });

    let _ = commands.send(Command::Refresh);
    window.present();

    let toggling = window.clone();
    let waking = commands.clone();
    glib::timeout_add_local(DRAIN, move || {
        while let Ok(monitor) = toggles.try_recv() {
            if toggling.is_visible() {
                toggling.set_visible(false);
            } else {
                if wayland()
                    && let Some(output) = monitor_named(monitor.trim())
                {
                    toggling.set_monitor(Some(&output));
                }
                toggling.present();
                let _ = waking.send(Command::Refresh);
            }
        }
        glib::ControlFlow::Continue
    });
}

fn layer_shell(window: &gtk::ApplicationWindow, monitor: Option<&str>) {
    // Layer shell is a Wayland protocol. Guarding it keeps the panel runnable
    // under a plain X server, which is how it gets screenshotted in review.
    if !wayland() {
        return;
    }
    window.init_layer_shell();
    window.set_namespace(Some("audio-panel"));
    window.set_layer(Layer::Overlay);
    window.set_keyboard_mode(KeyboardMode::OnDemand);
    window.set_exclusive_zone(0);
    window.set_anchor(Edge::Top, true);
    window.set_anchor(Edge::Right, true);
    window.set_margin(Edge::Top, TOP_MARGIN);
    window.set_margin(Edge::Right, 6);
    if let Some(name) = monitor.filter(|name| !name.is_empty())
        && let Some(output) = monitor_named(name)
    {
        window.set_monitor(Some(&output));
    }
}

fn wayland() -> bool {
    gdk::Display::default().is_some_and(|display| display.backend().is_wayland())
}

fn monitor_named(name: &str) -> Option<gdk::Monitor> {
    let monitors = gdk::Display::default()?.monitors();
    (0..monitors.n_items())
        .filter_map(|index| monitors.item(index))
        .filter_map(|object| object.downcast::<gdk::Monitor>().ok())
        .find(|monitor| monitor.connector().as_deref() == Some(name))
}

fn build_tabs(
    tabs: &gtk::Box,
    view: &Rc<RefCell<View>>,
    drawn: &Rc<RefCell<String>>,
    commands: &Sender<Command>,
) {
    for mode in TABS {
        let button = gtk::Button::with_label(mode.prompt());
        button.set_css_classes(&["audio-tab"]);
        button.set_hexpand(true);
        let (view, drawn, commands) = (view.clone(), drawn.clone(), commands.clone());
        button.connect_clicked(move |_| {
            let mut current = view.borrow_mut();
            current.mode = Some(mode);
            // Leaving the Play tab's device picker behind when the tab changes.
            current.routing = None;
            drop(current);
            // Force the next drain to redraw even though nothing on the bus
            // moved: the tab did.
            drawn.replace(String::new());
            let _ = commands.send(Command::Refresh);
        });
        tabs.append(&button);
    }
}

fn mark_tabs(tabs: &gtk::Box, active: Mode) {
    let mut child = tabs.first_child();
    for mode in TABS {
        let Some(button) = child else { break };
        if mode == active {
            button.add_css_class("active");
        } else {
            button.remove_css_class("active");
        }
        child = button.next_sibling();
    }
}

fn clear(container: &gtk::Box) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
}

/// Everything the panel currently shows, as one string.
///
/// Redrawing rebuilds the rows, which loses whatever the pointer was on, so it
/// only happens when something actually changed.
fn signature(snapshot: &Snapshot, view: &View) -> String {
    let mut out = format!("{:?}|{:?}|", view.tab(), view.routing.as_ref().map(|s| &s.key));
    out.push_str(&format!("{}{}", snapshot.powered, snapshot.scanning));
    if let Some(notice) = &snapshot.notice {
        out.push_str(notice);
    }
    if let Some(notice) = &snapshot.bluetooth_notice {
        out.push_str(notice);
    }
    if let Some(prompt) = &snapshot.prompt {
        out.push_str(&prompt.message);
    }
    match view.tab() {
        Mode::Bluetooth => {
            for device in &snapshot.devices {
                out.push_str(&format!(
                    "{}{}{}{:?};",
                    device.key, device.connected, device.paired, device.battery
                ));
            }
        }
        Mode::Playback => {
            for stream in &snapshot.streams {
                out.push_str(&format!(
                    "{}{:?}{}{};",
                    stream.key, stream.volume, stream.muted, stream.device_label
                ));
            }
            for entry in &snapshot.outputs {
                out.push_str(&format!("{};", entry.key));
            }
        }
        mode => {
            for entry in worker::entries(snapshot, mode) {
                out.push_str(&format!(
                    "{}{}{}{};",
                    entry.key, entry.volume, entry.muted, entry.default
                ));
            }
        }
    }
    out
}

fn draw(
    content: &gtk::Box,
    snapshot: &Snapshot,
    view: &View,
    view_cell: &Rc<RefCell<View>>,
    commands: &Sender<Command>,
    drawn: &Rc<RefCell<String>>,
) {
    if let Some(notice) = &snapshot.notice {
        content.append(&notice_row(notice));
    }
    match view.tab() {
        Mode::Bluetooth => bluetooth_tab(content, snapshot, commands),
        Mode::Playback => playback_tab(content, snapshot, view, view_cell, commands, drawn),
        mode => device_tab(content, snapshot, mode, commands),
    }
}

fn notice_row(text: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    label.add_css_class("audio-notice");
    label.set_wrap(true);
    label.set_xalign(0.0);
    label
}

fn placeholder(text: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    label.add_css_class("audio-placeholder");
    label.set_wrap(true);
    label
}

fn section(title: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(title));
    label.add_css_class("audio-section");
    label.set_xalign(0.0);
    label
}

// ---------------------------------------------------------------------------
// Output and Input
// ---------------------------------------------------------------------------

/// The default device's volume and mute, then every device to switch to.
///
/// The rows are `selections`, the same list the Rofi menu and Wayle's picker
/// use, so a laptop's Speaker and Headphones stay separate destinations even
/// though their ALSA profiles are mutually exclusive.
fn device_tab(content: &gtk::Box, snapshot: &Snapshot, mode: Mode, commands: &Sender<Command>) {
    let entries = worker::entries(snapshot, mode);
    let Some(default) = entries.iter().find(|entry| entry.default) else {
        content.append(&placeholder("No devices"));
        return;
    };

    content.append(&volume_card(default, commands));
    content.append(&section(if mode == Mode::Input {
        "Microphones"
    } else {
        "Outputs"
    }));

    for entry in entries {
        content.append(&device_row(entry, commands));
    }
}

fn volume_card(entry: &AudioEntry, commands: &Sender<Command>) -> gtk::Box {
    let card = gtk::Box::new(gtk::Orientation::Vertical, 0);
    card.set_css_classes(&["audio-card", "audio-volume-card"]);

    let title = gtk::Label::new(Some(&entry.label));
    title.add_css_class("audio-card-title");
    title.set_xalign(0.0);
    title.set_ellipsize(gtk::pango::EllipsizeMode::End);
    card.append(&title);

    let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    row.add_css_class("audio-volume-row");

    let mute = gtk::Button::with_label(if entry.muted { "\u{f075f}" } else { "\u{f057e}" });
    mute.set_css_classes(if entry.muted {
        &["audio-icon-button", "audio-glyph", "muted"]
    } else {
        &["audio-icon-button", "audio-glyph"]
    });
    let (sender, muted_entry) = (commands.clone(), entry.clone());
    mute.connect_clicked(move |_| {
        let _ = sender.send(Command::ToggleMute(muted_entry.clone()));
    });
    row.append(&mute);

    let slider = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 100.0, 1.0);
    slider.add_css_class("audio-slider");
    slider.set_hexpand(true);
    slider.set_draw_value(false);
    slider.set_value(f64::from(entry.volume));
    slider.set_sensitive(!entry.muted);
    let (sender, slider_entry) = (commands.clone(), entry.clone());
    // `change-value` fires on release and on a click, not on every frame of a
    // drag, so one gesture is one volume change.
    slider.connect_change_value(move |_, _, value| {
        let target = value.clamp(0.0, 100.0).round() as u8;
        let _ = sender.send(Command::SetVolume(slider_entry.clone(), target));
        glib::Propagation::Proceed
    });
    row.append(&slider);

    let reading = gtk::Label::new(Some(&format!("{}%", entry.volume)));
    reading.add_css_class("audio-volume-reading");
    row.append(&reading);

    card.append(&row);
    card
}

fn device_row(entry: &AudioEntry, commands: &Sender<Command>) -> gtk::Button {
    let button = gtk::Button::new();
    button.set_css_classes(if entry.default {
        &["audio-row", "active"]
    } else {
        &["audio-row"]
    });

    let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let text = gtk::Box::new(gtk::Orientation::Vertical, 0);
    text.set_hexpand(true);

    let label = gtk::Label::new(Some(&entry.label));
    label.add_css_class("audio-row-title");
    label.set_xalign(0.0);
    label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    text.append(&label);

    // The full PulseAudio description is long and mostly repeats the label, so
    // it stays in the tooltip and only the distinguishing part is drawn.
    button.set_tooltip_text(Some(&entry.description));
    row.append(&text);

    if entry.default {
        let tick = gtk::Label::new(Some("\u{f012c}"));
        tick.set_css_classes(&["audio-tick", "audio-glyph"]);
        row.append(&tick);
    }
    button.set_child(Some(&row));

    let (sender, chosen) = (commands.clone(), entry.clone());
    button.connect_clicked(move |_| {
        let _ = sender.send(Command::SetDefault(chosen.clone()));
    });
    button
}

// ---------------------------------------------------------------------------
// Play
// ---------------------------------------------------------------------------

/// Per-application volume, and a picker for where each stream goes.
fn playback_tab(
    content: &gtk::Box,
    snapshot: &Snapshot,
    view: &View,
    view_cell: &Rc<RefCell<View>>,
    commands: &Sender<Command>,
    drawn: &Rc<RefCell<String>>,
) {
    if let Some(stream) = &view.routing {
        route_picker(content, snapshot, stream, view_cell, commands, drawn);
        return;
    }

    if snapshot.streams.is_empty() {
        content.append(&placeholder("Nothing is playing"));
        return;
    }

    for stream in &snapshot.streams {
        content.append(&stream_card(stream, view_cell, commands, drawn));
    }
}

fn stream_card(
    stream: &StreamEntry,
    view_cell: &Rc<RefCell<View>>,
    commands: &Sender<Command>,
    drawn: &Rc<RefCell<String>>,
) -> gtk::Box {
    let card = gtk::Box::new(gtk::Orientation::Vertical, 0);
    card.set_css_classes(&["audio-card"]);

    let header = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let title = gtk::Label::new(Some(&stream.application));
    title.add_css_class("audio-card-title");
    title.set_hexpand(true);
    title.set_xalign(0.0);
    title.set_ellipsize(gtk::pango::EllipsizeMode::End);
    header.append(&title);

    // Moving one stream, rather than changing the system default, so the rest
    // of the desktop keeps playing where it was.
    let route = gtk::Button::with_label(&stream.device_label);
    route.set_css_classes(&["audio-route"]);
    route.set_tooltip_text(Some("Send this application somewhere else"));
    let (routing, cell, sender, mark) = (
        stream.clone(),
        view_cell.clone(),
        commands.clone(),
        drawn.clone(),
    );
    route.connect_clicked(move |_| {
        cell.borrow_mut().routing = Some(routing.clone());
        mark.replace(String::new());
        let _ = sender.send(Command::Refresh);
    });
    header.append(&route);
    card.append(&header);

    let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let mute = gtk::Button::with_label(if stream.muted { "\u{f075f}" } else { "\u{f057e}" });
    mute.set_css_classes(if stream.muted {
        &["audio-icon-button", "audio-glyph", "muted"]
    } else {
        &["audio-icon-button", "audio-glyph"]
    });
    let (sender, muted) = (commands.clone(), stream.clone());
    mute.connect_clicked(move |_| {
        let _ = sender.send(Command::ToggleStreamMute(muted.clone()));
    });
    row.append(&mute);

    let slider = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 100.0, 1.0);
    slider.add_css_class("audio-slider");
    slider.set_hexpand(true);
    slider.set_draw_value(false);
    slider.set_value(f64::from(stream.volume.unwrap_or(0)));
    // A stream that reports no volume has none to set.
    slider.set_sensitive(stream.volume.is_some() && !stream.muted);
    let (sender, sliding) = (commands.clone(), stream.clone());
    slider.connect_change_value(move |_, _, value| {
        let target = value.clamp(0.0, 100.0).round() as u8;
        let _ = sender.send(Command::SetStreamVolume(sliding.clone(), target));
        glib::Propagation::Proceed
    });
    row.append(&slider);

    let reading = gtk::Label::new(Some(&match stream.volume {
        Some(volume) => format!("{volume}%"),
        None => String::from("--"),
    }));
    reading.add_css_class("audio-volume-reading");
    row.append(&reading);
    card.append(&row);
    card
}

fn route_picker(
    content: &gtk::Box,
    snapshot: &Snapshot,
    stream: &StreamEntry,
    view_cell: &Rc<RefCell<View>>,
    commands: &Sender<Command>,
    drawn: &Rc<RefCell<String>>,
) {
    let back = gtk::Button::with_label("\u{f004d}  Back");
    back.set_css_classes(&["audio-row", "audio-back"]);
    let (cell, sender, mark) = (view_cell.clone(), commands.clone(), drawn.clone());
    back.connect_clicked(move |_| {
        cell.borrow_mut().routing = None;
        mark.replace(String::new());
        let _ = sender.send(Command::Refresh);
    });
    content.append(&back);
    content.append(&section(&format!("Send {} to", stream.application)));

    for entry in &snapshot.outputs {
        let button = gtk::Button::new();
        button.set_css_classes(if entry.name == stream.device_name {
            &["audio-row", "active"]
        } else {
            &["audio-row"]
        });
        let label = gtk::Label::new(Some(&entry.label));
        label.add_css_class("audio-row-title");
        label.set_xalign(0.0);
        label.set_hexpand(true);
        label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        button.set_child(Some(&label));

        let (sender, moving, key, cell, mark) = (
            commands.clone(),
            stream.clone(),
            entry.key.clone(),
            view_cell.clone(),
            drawn.clone(),
        );
        button.connect_clicked(move |_| {
            let _ = sender.send(Command::MoveStream(moving.clone(), key.clone()));
            cell.borrow_mut().routing = None;
            mark.replace(String::new());
        });
        content.append(&button);
    }
}

// ---------------------------------------------------------------------------
// Pair
// ---------------------------------------------------------------------------

/// The adapter switch, the scan button, the pairing prompt when one is up, and
/// every paired or discovered device.
fn bluetooth_tab(content: &gtk::Box, snapshot: &Snapshot, commands: &Sender<Command>) {
    content.append(&adapter_row(snapshot, commands));

    if let Some(notice) = &snapshot.bluetooth_notice {
        content.append(&notice_row(notice));
    }

    if let Some(prompt) = &snapshot.prompt {
        content.append(&pairing_card(prompt, commands));
    }

    if !snapshot.powered {
        content.append(&placeholder("Bluetooth is off"));
        return;
    }
    if snapshot.devices.is_empty() {
        content.append(&placeholder(if snapshot.scanning {
            "Scanning…"
        } else {
            "No devices. Press Scan to look for one."
        }));
        return;
    }

    let (paired, nearby): (Vec<_>, Vec<_>) =
        snapshot.devices.iter().partition(|device| device.paired);
    if !paired.is_empty() {
        content.append(&section("Paired"));
        for device in paired {
            content.append(&bluetooth_row(device, commands));
        }
    }
    if !nearby.is_empty() {
        content.append(&section("Nearby"));
        for device in nearby {
            content.append(&bluetooth_row(device, commands));
        }
    }
}

fn adapter_row(snapshot: &Snapshot, commands: &Sender<Command>) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    row.set_css_classes(&["audio-card", "audio-adapter"]);

    let label = gtk::Label::new(Some("Bluetooth"));
    label.add_css_class("audio-card-title");
    label.set_hexpand(true);
    label.set_xalign(0.0);
    row.append(&label);

    let scan = gtk::Button::with_label(if snapshot.scanning { "Scanning…" } else { "Scan" });
    scan.set_css_classes(&["audio-scan"]);
    scan.set_sensitive(!snapshot.scanning);
    let sender = commands.clone();
    scan.connect_clicked(move |_| {
        let _ = sender.send(Command::Scan);
    });
    row.append(&scan);

    let power = gtk::Switch::new();
    power.add_css_class("audio-switch");
    power.set_valign(gtk::Align::Center);
    power.set_active(snapshot.powered);
    let sender = commands.clone();
    // `state-set` rather than `notify::active`, or setting the switch from a
    // fresh reading would look like the user having flicked it.
    power.connect_state_set(move |_, on| {
        let _ = sender.send(Command::SetPowered(on));
        glib::Propagation::Proceed
    });
    row.append(&power);
    row
}

fn bluetooth_row(device: &BluetoothEntry, commands: &Sender<Command>) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    row.set_css_classes(if device.connected {
        &["audio-row", "audio-bt-row", "active"]
    } else {
        &["audio-row", "audio-bt-row"]
    });

    let text = gtk::Box::new(gtk::Orientation::Vertical, 0);
    text.set_hexpand(true);
    let name = gtk::Label::new(Some(&device.name));
    name.add_css_class("audio-row-title");
    name.set_xalign(0.0);
    name.set_ellipsize(gtk::pango::EllipsizeMode::End);
    text.append(&name);

    let mut detail = String::from(if device.connected {
        "Connected"
    } else if device.paired {
        "Paired"
    } else {
        "Not paired"
    });
    if let Some(battery) = device.battery {
        detail.push_str(&format!(" · {battery}%"));
    }
    let subtitle = gtk::Label::new(Some(&detail));
    subtitle.add_css_class("audio-row-detail");
    subtitle.set_xalign(0.0);
    text.append(&subtitle);
    row.append(&text);

    let action = gtk::Button::with_label(if device.connected {
        "Disconnect"
    } else {
        "Connect"
    });
    action.set_css_classes(&["audio-action"]);
    let (sender, target, connected) = (commands.clone(), device.clone(), device.connected);
    action.connect_clicked(move |_| {
        let _ = sender.send(if connected {
            Command::Disconnect(target.clone())
        } else {
            Command::Connect(target.clone())
        });
    });
    row.append(&action);

    if device.paired {
        let forget = gtk::Button::with_label("\u{f01b4}");
        forget.set_css_classes(&["audio-icon-button", "audio-glyph"]);
        forget.set_tooltip_text(Some("Forget this device"));
        let (sender, target) = (commands.clone(), device.clone());
        forget.connect_clicked(move |_| {
            let _ = sender.send(Command::Forget(target.clone()));
        });
        row.append(&forget);
    }
    row
}

/// The prompt BlueZ raises mid-pairing: a PIN or passkey to type, or a code to
/// confirm. An informational one has no answer to send, so it only gets a
/// dismiss.
fn pairing_card(prompt: &super::state::Prompt, commands: &Sender<Command>) -> gtk::Box {
    let card = gtk::Box::new(gtk::Orientation::Vertical, 0);
    card.set_css_classes(&["audio-card", "audio-pairing"]);

    let message = gtk::Label::new(Some(&prompt.message));
    message.add_css_class("audio-card-title");
    message.set_wrap(true);
    message.set_xalign(0.0);
    card.append(&message);

    let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let entry = gtk::Entry::new();
    entry.add_css_class("audio-entry");
    entry.set_hexpand(true);
    let needs_code = prompt.kind.is_some();
    entry.set_visible(needs_code);
    if let Some(kind) = prompt.kind {
        entry.set_placeholder_text(Some(match kind {
            crate::model::CodeKind::Pin => "PIN",
            crate::model::CodeKind::Passkey => "6-digit passkey",
        }));
    }
    row.append(&entry);

    let confirm = gtk::Button::with_label(if needs_code { "Pair" } else { "OK" });
    confirm.set_css_classes(&["audio-action", "primary"]);
    let (sender, field) = (commands.clone(), entry.clone());
    confirm.connect_clicked(move |_| {
        let answer = needs_code.then(|| field.text().to_string());
        let _ = sender.send(Command::AnswerPrompt(answer));
    });
    row.append(&confirm);

    if needs_code {
        let cancel = gtk::Button::with_label("Cancel");
        cancel.set_css_classes(&["audio-action"]);
        let sender = commands.clone();
        cancel.connect_clicked(move |_| {
            let _ = sender.send(Command::AnswerPrompt(None));
        });
        row.append(&cancel);
        // Enter is what a person types after a PIN.
        let (sender, field) = (commands.clone(), entry.clone());
        entry.connect_activate(move |_| {
            let _ = sender.send(Command::AnswerPrompt(Some(field.text().to_string())));
        });
    }
    card.append(&row);
    card
}
