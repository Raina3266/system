//! The panel: a monitor-local layer surface holding one card per player.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::OnceLock;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread;
use std::time::Duration;

use gtk::gdk;
use gtk::gdk_pixbuf;
use gtk::glib;
use gtk::prelude::*;
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

use crate::artwork;
use crate::mpris::{Player, Players, clock};

/// The transport glyph: what the button will do, not what the player is doing.
const GLYPH_PLAY: &str = "\u{f040a}";
const GLYPH_PAUSE: &str = "\u{f03e4}";

/// How often the cards are re-read while the panel is open. Nothing is read
/// while it is closed: a hidden panel should cost nothing.
const TICK: Duration = Duration::from_millis(500);
/// How often the UI looks for a reading the worker has finished.
const DRAIN: Duration = Duration::from_millis(80);

/// Distance from the top of the screen: the height of Waybar, plus a gap.
const TOP_MARGIN: i32 = 46;

const PANEL_WIDTH: i32 = 486;
/// Cards past this many are reached by scrolling. Up to it, the panel is only
/// as tall as the cards it holds.
const VISIBLE_CARDS: i32 = 4;
const CARD_GAP: i32 = 7;
const ARTWORK_SIZE: i32 = 50;

/// What the worker thread is asked to do between refreshes.
pub enum Command {
    Refresh,
    PlayPause(Player),
    Next(Player),
    Previous(Player),
    Seek(Player, f64),
}

/// One card's widgets, kept so a refresh can update them in place rather than
/// rebuilding the list: rebuilding under the pointer loses a click.
struct Card {
    root: gtk::Box,
    source: gtk::Label,
    state: gtk::Label,
    lock: gtk::Label,
    artwork: gtk::Picture,
    artwork_fallback: gtk::Label,
    /// What `artwork` is currently showing, so a cover is decoded once per
    /// track rather than on every tick.
    art_path: RefCell<Option<PathBuf>>,
    title: gtk::Label,
    credits: gtk::Box,
    artist_icon: gtk::Label,
    artist: gtk::Label,
    album_icon: gtk::Label,
    album: gtk::Label,
    seek: gtk::Scale,
    seek_handler: glib::SignalHandlerId,
    position: gtk::Label,
    length: gtk::Label,
    play: gtk::Label,
    previous: gtk::Button,
    next: gtk::Button,
    player: Rc<RefCell<Player>>,
}

pub fn run(app: &gtk::Application, monitor: Option<String>, toggles: Receiver<String>) {
    let (commands, command_rx) = channel::<Command>();
    let (snapshot_tx, snapshots) = channel::<Vec<Player>>();
    spawn_worker(command_rx, snapshot_tx);

    let list = gtk::Box::new(gtk::Orientation::Vertical, CARD_GAP);
    list.add_css_class("media-list");

    let scroll = gtk::ScrolledWindow::new();
    scroll.add_css_class("media-scroll");
    scroll.set_hscrollbar_policy(gtk::PolicyType::Never);
    scroll.set_propagate_natural_height(true);
    scroll.set_child(Some(&list));

    let empty = empty_state();

    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    content.add_css_class("media-content");
    content.append(&scroll);
    content.append(&empty);

    let panel = gtk::Box::new(gtk::Orientation::Vertical, 0);
    panel.add_css_class("media-panel");
    panel.set_width_request(PANEL_WIDTH);
    panel.append(&header());
    panel.append(&content);

    let window = gtk::ApplicationWindow::new(app);
    window.set_decorated(false);
    window.set_resizable(false);
    window.add_css_class("media-window");
    window.set_child(Some(&panel));

    // Layer shell is a Wayland protocol. Guarding it keeps the panel runnable
    // under a plain X server, which is how it gets screenshotted in review.
    if wayland() {
        window.init_layer_shell();
        window.set_namespace(Some("media-panel"));
        window.set_layer(Layer::Overlay);
        window.set_keyboard_mode(KeyboardMode::OnDemand);
        window.set_exclusive_zone(0);
        window.set_anchor(Edge::Top, true);
        window.set_margin(Edge::Top, TOP_MARGIN);
        if let Some(monitor) = monitor.as_deref().filter(|name| !name.is_empty())
            && let Some(output) = monitor_named(monitor)
        {
            window.set_monitor(Some(&output));
        }
    }

    // Escape closes it, like every other dropdown on this desktop.
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

    let cards: Rc<RefCell<Vec<Card>>> = Rc::default();
    let drawing = commands.clone();
    glib::timeout_add_local(DRAIN, {
        let (list, scroll, empty, cards) =
            (list.clone(), scroll.clone(), empty.clone(), cards.clone());
        move || {
            // Only the newest reading matters; older ones are already stale.
            if let Some(players) = snapshots.try_iter().last() {
                let empty_list = players.is_empty();
                sync(&list, &cards, players, &drawing);
                scroll.set_visible(!empty_list);
                empty.set_visible(empty_list);
                scroll.set_max_content_height(list_cap(&cards.borrow()));
            }
            glib::ControlFlow::Continue
        }
    });

    // Only poll while something can see the result.
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

    // A second press of the Waybar button arrives on the socket. Moving the
    // surface to the monitor it came from means the panel follows the button
    // that opened it.
    let toggling = window.clone();
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
            }
        }
        glib::ControlFlow::Continue
    });
}

/// The bus is read on its own thread: a player that is slow to answer must not
/// stall the panel's own redraw.
fn spawn_worker(commands: Receiver<Command>, snapshots: Sender<Vec<Player>>) {
    thread::spawn(move || {
        let Ok(players) = Players::connect() else {
            return;
        };
        while let Ok(command) = commands.recv() {
            // One command can hold this thread for seconds — `set_playing`
            // waits on the player to agree — while the panel keeps asking for
            // a refresh twice a second. Taking everything queued in one pass
            // stops a click from landing behind that backlog, and collapses
            // the refreshes into the single snapshot below instead of paying
            // for one apiece.
            let batch = std::iter::once(command).chain(commands.try_iter());
            let started = std::time::Instant::now();
            let mut handled = 0usize;
            for command in batch {
                match command {
                    Command::Refresh => {}
                    Command::PlayPause(player) => {
                        players.set_playing(&player, !player.status.is_playing());
                    }
                    Command::Next(player) => players.next(&player),
                    Command::Previous(player) => players.previous(&player),
                    Command::Seek(player, fraction) => players.seek_to(&player, fraction),
                }
                handled += 1;
            }
            let acted = started.elapsed();
            let reading = std::time::Instant::now();
            let snapshot = players.snapshot();
            trace(handled, acted, reading.elapsed(), snapshot.len());
            if snapshots.send(snapshot).is_err() {
                return;
            }
        }
    });
}

/// Timings for the loop above, on stderr, when `MEDIA_PANEL_TRACE` is set.
/// The panel's responsiveness is entirely this thread's throughput, so this is
/// the one measurement worth having from a machine that feels slow.
fn trace(commands: usize, acted: Duration, read: Duration, players: usize) {
    static ON: OnceLock<bool> = OnceLock::new();
    if *ON.get_or_init(|| std::env::var_os("MEDIA_PANEL_TRACE").is_some()) {
        eprintln!(
            "media-panel: {commands} command(s) in {}ms, {players} player(s) read in {}ms",
            acted.as_millis(),
            read.as_millis()
        );
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

fn header() -> gtk::Box {
    let header = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    header.add_css_class("media-header");
    let icon = gtk::Label::new(Some("\u{f0387}"));
    icon.add_css_class("media-header-icon");
    header.append(&icon);
    let title = gtk::Label::new(Some("Now Playing"));
    title.add_css_class("media-header-title");
    header.append(&title);
    header
}

fn empty_state() -> gtk::Box {
    let empty = gtk::Box::new(gtk::Orientation::Vertical, 0);
    empty.add_css_class("media-empty");
    let title = gtk::Label::new(Some("No Media Playing"));
    title.add_css_class("media-empty-title");
    empty.append(&title);
    let body = gtk::Label::new(Some("Start playing media in any app to control it here"));
    body.add_css_class("media-empty-body");
    body.set_wrap(true);
    body.set_justify(gtk::Justification::Center);
    empty.append(&body);
    empty
}

/// The height of a full list: `VISIBLE_CARDS` cards and the gaps between them.
/// Every card is the same size, so one gives the height of all of them.
fn list_cap(cards: &[Card]) -> i32 {
    let card = cards
        .first()
        .map(|card| card.root.measure(gtk::Orientation::Vertical, -1).1)
        .filter(|height| *height > 0)
        .unwrap_or(124);
    card * VISIBLE_CARDS + CARD_GAP * (VISIBLE_CARDS - 1)
}

/// Brings the list in line with a fresh snapshot.
///
/// Cards are matched to players by bus name and updated in place. Rebuilding
/// the list on every tick would drop the click the pointer is in the middle of
/// and reset a slider mid-drag.
fn sync(
    list: &gtk::Box,
    cards: &Rc<RefCell<Vec<Card>>>,
    players: Vec<Player>,
    commands: &Sender<Command>,
) {
    let mut cards = cards.borrow_mut();

    cards.retain(|card| {
        let wanted = players
            .iter()
            .any(|player| player.bus == card.player.borrow().bus);
        if !wanted {
            list.remove(&card.root);
        }
        wanted
    });

    for (index, player) in players.into_iter().enumerate() {
        match cards
            .iter()
            .position(|card| card.player.borrow().bus == player.bus)
        {
            Some(existing) => {
                update(&cards[existing], player);
                let last = cards.len().saturating_sub(1);
                if existing != index {
                    cards.swap(existing, index.min(last));
                }
            }
            None => {
                let card = build_card(&player, commands);
                update(&card, player);
                list.append(&card.root);
                cards.push(card);
            }
        }
    }

    // Playing first, which is the order the snapshot arrived in.
    for (position, card) in cards.iter().enumerate() {
        #[allow(clippy::cast_possible_wrap)]
        list.reorder_child_after(
            &card.root,
            position
                .checked_sub(1)
                .and_then(|before| cards.get(before))
                .map(|card| &card.root),
        );
    }
}

fn build_card(player: &Player, commands: &Sender<Command>) -> Card {
    let state = Rc::new(RefCell::new(player.clone()));

    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.add_css_class("media-card");
    root.set_hexpand(true);

    // The source owns the top line, with how it is playing at the end of it.
    let source_row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    source_row.add_css_class("media-source-row");
    let source = gtk::Label::new(None);
    source.add_css_class("media-source");
    source.set_hexpand(true);
    source.set_halign(gtk::Align::Start);
    source.set_ellipsize(gtk::pango::EllipsizeMode::End);
    source_row.append(&source);
    // A player that answers CanControl=false is saying its buttons will do
    // nothing; saying so beats a transport row that silently refuses.
    let lock = gtk::Label::new(Some("\u{f033e}"));
    lock.set_css_classes(&["media-lock", "media-glyph"]);
    lock.set_tooltip_text(Some("This source does not accept playback commands"));
    source_row.append(&lock);
    let state_label = gtk::Label::new(None);
    source_row.append(&state_label);
    root.append(&source_row);

    let summary = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    summary.add_css_class("media-summary");

    // A fixed square. Nothing inside it expands, or the row's spare width
    // would grow the cover instead of the text beside it.
    let art_frame = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    art_frame.add_css_class("media-artwork");
    art_frame.set_size_request(ARTWORK_SIZE, ARTWORK_SIZE);
    art_frame.set_halign(gtk::Align::Start);
    art_frame.set_valign(gtk::Align::Center);
    art_frame.set_hexpand(false);
    art_frame.set_vexpand(false);
    art_frame.set_overflow(gtk::Overflow::Hidden);

    let artwork = gtk::Picture::new();
    // Fills the square from the middle of the art, so a cover that is not
    // square is cropped rather than stretched.
    artwork.set_content_fit(gtk::ContentFit::Cover);
    artwork.set_size_request(ARTWORK_SIZE, ARTWORK_SIZE);
    art_frame.append(&artwork);

    let artwork_fallback = gtk::Label::new(Some("\u{f075a}"));
    artwork_fallback.set_css_classes(&["media-artwork-fallback", "media-glyph"]);
    artwork_fallback.set_size_request(ARTWORK_SIZE, ARTWORK_SIZE);
    art_frame.append(&artwork_fallback);
    summary.append(&art_frame);

    let info = gtk::Box::new(gtk::Orientation::Vertical, 0);
    info.add_css_class("media-info");
    info.set_hexpand(true);
    info.set_valign(gtk::Align::Center);

    let title = gtk::Label::new(None);
    title.add_css_class("media-title");
    title.set_halign(gtk::Align::Fill);
    title.set_xalign(0.0);
    title.set_ellipsize(gtk::pango::EllipsizeMode::End);
    info.append(&title);

    // Artist and album share a line, an icon in front of each saying which.
    let credits = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    credits.add_css_class("media-credits");
    let artist_icon = gtk::Label::new(Some("\u{f036c}"));
    artist_icon.add_css_class("media-credit-icon");
    credits.append(&artist_icon);
    let artist = gtk::Label::new(None);
    artist.add_css_class("media-artist");
    artist.set_halign(gtk::Align::Start);
    artist.set_ellipsize(gtk::pango::EllipsizeMode::End);
    credits.append(&artist);
    let album_icon = gtk::Label::new(Some("\u{f00c2}"));
    album_icon.set_css_classes(&["media-credit-icon", "album"]);
    credits.append(&album_icon);
    let album = gtk::Label::new(None);
    album.add_css_class("media-album");
    album.set_hexpand(true);
    album.set_halign(gtk::Align::Start);
    album.set_ellipsize(gtk::pango::EllipsizeMode::End);
    credits.append(&album);
    info.append(&credits);

    let seek = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 1000.0, 1.0);
    seek.add_css_class("media-seek");
    seek.set_draw_value(false);
    let seek_handler = {
        let (commands, state) = (commands.clone(), state.clone());
        // `change-value` fires on the release of a drag and on a click, not on
        // every frame of a drag, so one gesture is one seek.
        seek.connect_change_value(move |_, _, value| {
            let _ = commands.send(Command::Seek(state.borrow().clone(), value / 1000.0));
            glib::Propagation::Proceed
        })
    };
    info.append(&seek);
    summary.append(&info);
    root.append(&summary);

    let controls = gtk::CenterBox::new();
    controls.add_css_class("media-controls");
    let position = gtk::Label::new(None);
    position.add_css_class("media-time");
    position.set_valign(gtk::Align::Center);
    controls.set_start_widget(Some(&position));

    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    buttons.add_css_class("media-buttons");
    let previous = transport_button("\u{f04ae}", "media-button");
    let play_button = gtk::Button::new();
    play_button.set_css_classes(&["media-button", "main"]);
    play_button.set_valign(gtk::Align::Center);
    let play = gtk::Label::new(None);
    play.add_css_class("media-glyph");
    play_button.set_child(Some(&play));
    let next = transport_button("\u{f04ad}", "media-button");
    buttons.append(&previous);
    buttons.append(&play_button);
    buttons.append(&next);
    controls.set_center_widget(Some(&buttons));

    let length = gtk::Label::new(None);
    length.add_css_class("media-time");
    length.set_valign(gtk::Align::Center);
    controls.set_end_widget(Some(&length));
    root.append(&controls);

    {
        let (commands, state, glyph) = (commands.clone(), state.clone(), play.clone());
        play_button.connect_clicked(move |_| {
            let player = state.borrow().clone();
            // Show the outcome at once. A player can take a second or more to
            // report a status change, and a button that only moves once the
            // next reading comes back round reads as a button that is stuck.
            // The next refresh corrects this if the player refused.
            glyph.set_label(if player.status.is_playing() {
                GLYPH_PLAY
            } else {
                GLYPH_PAUSE
            });
            let _ = commands.send(Command::PlayPause(player));
        });
    }
    for (button, forwards) in [(&previous, false), (&next, true)] {
        let (commands, state) = (commands.clone(), state.clone());
        button.connect_clicked(move |_| {
            let player = state.borrow().clone();
            let _ = commands.send(if forwards {
                Command::Next(player)
            } else {
                Command::Previous(player)
            });
        });
    }

    Card {
        root,
        source,
        state: state_label,
        lock,
        artwork,
        artwork_fallback,
        art_path: RefCell::new(None),
        title,
        credits,
        artist_icon,
        artist,
        album_icon,
        album,
        seek,
        seek_handler,
        position,
        length,
        play,
        previous,
        next,
        player: state,
    }
}

/// A centred square of the cover, at exactly the size the card draws it.
///
/// `Picture::set_filename` hands GTK the file at its own resolution, and a
/// widget's size request is a minimum rather than a maximum — so a 1280x720
/// video thumbnail took as much of the row as the title was willing to give up
/// and the card grew to match, while a small square cover looked correct. The
/// natural size is the drawn size now, so neither the row nor the card height
/// depends on what the artwork happens to be.
fn square_texture(path: &Path) -> Option<gdk::Texture> {
    let full = gdk_pixbuf::Pixbuf::from_file(path).ok()?;
    let (x, y, side) = centre_square(full.width(), full.height())?;
    let square = full.new_subpixbuf(x, y, side, side);
    let scaled =
        square.scale_simple(ARTWORK_SIZE, ARTWORK_SIZE, gdk_pixbuf::InterpType::Bilinear)?;
    Some(gdk::Texture::for_pixbuf(&scaled))
}

/// The largest centred square inside `width` x `height`, as `(x, y, side)`.
pub fn centre_square(width: i32, height: i32) -> Option<(i32, i32, i32)> {
    let side = width.min(height);
    (side > 0).then(|| ((width - side) / 2, (height - side) / 2, side))
}

fn transport_button(glyph: &str, class: &str) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class(class);
    button.set_valign(gtk::Align::Center);
    let label = gtk::Label::new(Some(glyph));
    label.add_css_class("media-glyph");
    button.set_child(Some(&label));
    button
}

/// Writes a fresh reading of a player into the card already on screen.
fn update(card: &Card, player: Player) {
    card.source.set_label(&player.source);
    card.state.set_label(player.status.label());
    card.state.set_css_classes(if player.status.is_playing() {
        &["media-state", "playing"]
    } else {
        &["media-state"]
    });
    card.lock.set_visible(!player.can_control);

    card.title
        .set_label(placeholder(&player.title, "Unknown Title"));
    // A chip with no value behind it is noise, so the icon goes with the text.
    // Web players routinely publish a title and nothing else.
    let artist = player.artist.trim();
    let album = player.album.trim();
    card.artist.set_label(artist);
    card.album.set_label(album);
    card.artist_icon.set_visible(!artist.is_empty());
    card.artist.set_visible(!artist.is_empty());
    card.album_icon.set_visible(!album.is_empty());
    card.album.set_visible(!album.is_empty());
    card.credits
        .set_visible(!artist.is_empty() || !album.is_empty());

    let cover = artwork::find_when_known(player.art_url.as_deref(), player.track_url.as_deref());
    if card.art_path.borrow().as_deref() != cover.as_deref() {
        let texture = cover.as_deref().and_then(square_texture);
        card.artwork.set_paintable(texture.as_ref());
        card.art_path.replace(cover);
    }
    let has_art = card.artwork.paintable().is_some();
    card.artwork.set_visible(has_art);
    card.artwork_fallback.set_visible(!has_art);

    card.play.set_label(if player.status.is_playing() {
        GLYPH_PAUSE
    } else {
        GLYPH_PLAY
    });
    card.previous.set_sensitive(player.can_go_previous);
    card.next.set_sensitive(player.can_go_next);

    card.position.set_label(&clock(player.position));
    // A publisher that omits mpris:length has not said the track is zero
    // seconds long, so do not claim it did.
    card.length
        .set_label(&player.length.map_or_else(|| String::from("--:--"), clock));

    card.seek
        .set_sensitive(player.can_seek && player.length.is_some());
    // Blocked, or writing the tick back would read as the user dragging.
    card.seek.block_signal(&card.seek_handler);
    card.seek.set_value(player.progress() * 1000.0);
    card.seek.unblock_signal(&card.seek_handler);

    card.player.replace(player);
}

fn placeholder<'a>(value: &'a str, fallback: &'a str) -> &'a str {
    if value.trim().is_empty() {
        fallback
    } else {
        value
    }
}

#[cfg(test)]
mod tests;
