use std::cell::RefCell;
use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::rc::Rc;
use std::sync::mpsc::Receiver;
use std::time::Duration;

use gtk::prelude::*;
use gtk::{
    Align, Application, ApplicationWindow, Box as GtkBox, CssProvider, EventControllerMotion,
    Image, Orientation, Picture, PolicyType, ScrolledWindow, Stack, TextView, WrapMode, gdk, gio,
    glib,
};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

use crate::cli::{Options, Side, WindowOverrides};
use crate::config::{self, Config, WindowConfig};
use crate::ipc::{ContentSnapshot, Message, SwitchReply};
use crate::panel_state::{ContentKind, CurrentItem, LiveState, SwitchDisposition};

const THEME_RELOAD_INTERVAL: Duration = Duration::from_millis(250);
const NETWORK_QR_SIZE: i32 = 50;

#[derive(Clone, Copy)]
enum PanelKeyboardState {
    Browsing,
    EditorArmed,
}

struct PanelGeometry {
    config: WindowConfig,
    monitor: Option<gdk::Monitor>,
}

struct LiveWidgets {
    application: Application,
    window: ApplicationWindow,
    stack: Stack,
    text_view: TextView,
    picture: Picture,
    network_text_view: TextView,
    network_picture: Image,
}

impl PanelKeyboardState {
    fn mode(self) -> KeyboardMode {
        match self {
            Self::Browsing => KeyboardMode::None,
            Self::EditorArmed => KeyboardMode::OnDemand,
        }
    }
}

pub fn run(options: Options, text: String, receiver: Option<Receiver<Message>>) {
    // NON_UNIQUE is important for launchers: every Edit action gets its own
    // item rather than activating an older panel and losing the new content.
    let application = Application::builder()
        .application_id("io.github.raina.PreviewPanel")
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    let options = Rc::new(options);
    let text = Rc::new(text);
    let receiver = Rc::new(std::cell::RefCell::new(receiver));

    application.connect_activate(move |application| {
        build_window(
            application,
            options.as_ref(),
            text.as_str(),
            receiver.borrow_mut().take(),
        );
    });
    // main.rs has already parsed our CLI. Supplying a clean argv prevents
    // GApplication from trying to interpret --stdin, --title, or a file path.
    application.run_with_args(&["preview-panel"]);
}

fn build_window(
    application: &Application,
    options: &Options,
    text: &str,
    receiver: Option<Receiver<Message>>,
) {
    let theme_path = config::configured_path();
    let (observed_theme, loaded_theme) = load_initial_theme(theme_path.as_deref());
    let css_provider = install_css(&loaded_theme.css);
    let panel_config = loaded_theme
        .window
        .with_overrides(options.window_overrides);

    let text_view = TextView::new();
    text_view.buffer().set_text(text);
    text_view.set_accepts_tab(true);
    text_view.set_cursor_visible(true);
    text_view.set_editable(options.editable);
    text_view.set_hexpand(true);
    text_view.set_left_margin(14);
    text_view.set_monospace(true);
    text_view.set_pixels_above_lines(2);
    text_view.set_pixels_below_lines(2);
    text_view.set_right_margin(14);
    text_view.set_top_margin(12);
    text_view.set_bottom_margin(12);
    text_view.set_vexpand(true);
    text_view.set_wrap_mode(if options.wrap {
        WrapMode::WordChar
    } else {
        WrapMode::None
    });
    text_view.add_css_class("preview-text");
    connect_persistent_clipboard(&text_view);

    let scrolled_window = ScrolledWindow::new();
    scrolled_window.set_child(Some(&text_view));
    scrolled_window.set_has_frame(true);
    scrolled_window.set_hexpand(true);
    scrolled_window.set_kinetic_scrolling(true);
    scrolled_window.set_policy(
        if options.wrap {
            PolicyType::Never
        } else {
            PolicyType::Automatic
        },
        PolicyType::Automatic,
    );
    scrolled_window.set_vexpand(true);

    let picture = Picture::new();
    picture.set_can_shrink(true);
    picture.set_hexpand(true);
    picture.set_vexpand(true);
    picture.set_margin_start(14);
    picture.set_margin_end(14);
    picture.set_margin_top(12);
    picture.set_margin_bottom(12);
    picture.add_css_class("preview-image");

    let network_text_view = TextView::new();
    network_text_view.set_cursor_visible(false);
    network_text_view.set_editable(false);
    network_text_view.set_hexpand(true);
    network_text_view.set_left_margin(14);
    network_text_view.set_monospace(true);
    network_text_view.set_pixels_above_lines(2);
    network_text_view.set_pixels_below_lines(2);
    network_text_view.set_right_margin(14);
    network_text_view.set_top_margin(12);
    network_text_view.set_bottom_margin(12);
    network_text_view.set_vexpand(true);
    network_text_view.set_wrap_mode(WrapMode::WordChar);
    network_text_view.add_css_class("network-details");
    connect_persistent_clipboard(&network_text_view);

    let network_scrolled = ScrolledWindow::new();
    network_scrolled.set_child(Some(&network_text_view));
    network_scrolled.set_has_frame(true);
    network_scrolled.set_hexpand(true);
    network_scrolled.set_kinetic_scrolling(true);
    network_scrolled.set_policy(PolicyType::Never, PolicyType::Automatic);
    network_scrolled.set_vexpand(true);

    let network_picture = Image::new();
    network_picture.set_halign(Align::Center);
    network_picture.set_pixel_size(NETWORK_QR_SIZE);
    network_picture.set_size_request(NETWORK_QR_SIZE, NETWORK_QR_SIZE);
    network_picture.set_vexpand(false);
    network_picture.set_visible(false);
    network_picture.add_css_class("network-qr");

    let network_box = GtkBox::new(Orientation::Vertical, 10);
    network_box.set_hexpand(true);
    network_box.set_vexpand(true);
    network_box.append(&network_scrolled);
    network_box.append(&network_picture);

    let stack = Stack::new();
    stack.set_hexpand(true);
    stack.set_vexpand(true);
    stack.add_named(&scrolled_window, Some("text"));
    stack.add_named(&picture, Some("image"));
    stack.add_named(&network_box, Some("network"));
    stack.set_visible_child_name("text");

    let (window_width, window_height) = if options.panel {
        (panel_config.width, panel_config.height)
    } else {
        (options.width, options.height)
    };
    let window = ApplicationWindow::builder()
        .application(application)
        .default_height(window_height)
        .default_width(window_width)
        .title(&options.title)
        .build();
    window.add_css_class("preview-panel");
    window.set_decorated(!options.panel);
    window.set_child(Some(&stack));

    let panel_geometry = options.panel.then(|| {
        let geometry = configure_companion_panel(&window, panel_config);
        if options.editable {
            connect_panel_editor_focus(&window, &text_view);
        }
        geometry
    });
    watch_theme(
        &window,
        theme_path,
        observed_theme,
        css_provider,
        options.window_overrides,
        panel_geometry,
    );
    if let Some(receiver) = receiver {
        connect_live_updates(
            LiveWidgets {
                application: application.clone(),
                window: window.clone(),
                stack: stack.clone(),
                text_view: text_view.clone(),
                picture: picture.clone(),
                network_text_view: network_text_view.clone(),
                network_picture: network_picture.clone(),
            },
            receiver,
        );
    }

    window.present();
    if !options.panel {
        text_view.grab_focus();
    }
}

fn connect_persistent_clipboard(text_view: &TextView) {
    let text_view_for_copy = text_view.clone();
    // Run after GTK's default handler so wl-copy becomes the final clipboard
    // owner and keeps serving the selection after this panel exits.
    text_view.connect_local("copy-clipboard", true, move |_| {
        let buffer = text_view_for_copy.buffer();
        let (start, end) = buffer.selection_bounds()?;
        let text = buffer.text(&start, &end, true);
        if let Err(error) = copy_text_to_wayland(&text) {
            eprintln!("preview-panel: copy selection to Wayland clipboard: {error}");
        }
        None
    });
}

fn copy_text_to_wayland(text: &str) -> io::Result<()> {
    let mut child = Command::new(wl_copy_binary())
        .args(["--type", "text/plain;charset=utf-8"])
        .stdin(Stdio::piped())
        .spawn()?;
    let mut input = child
        .stdin
        .take()
        .ok_or_else(|| io::Error::other("wl-copy standard input is unavailable"))?;
    input.write_all(text.as_bytes())?;
    drop(input);

    let status = child.wait()?;
    if !status.success() {
        return Err(io::Error::other(format!("wl-copy exited with {status}")));
    }
    Ok(())
}

fn wl_copy_binary() -> PathBuf {
    env::var_os("ROFI_CLIPBOARD_WL_COPY")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new("wl-copy").to_path_buf())
}

fn load_initial_theme(path: Option<&Path>) -> (Option<String>, Config) {
    let Some(path) = path else {
        return (None, config::embedded());
    };

    match fs::read_to_string(path) {
        Ok(source) => match config::parse(&source) {
            Ok(config) => (Some(source), config),
            Err(error) => {
                eprintln!(
                    "preview-panel: ignoring invalid CSS theme {}: {error}",
                    path.display()
                );
                (Some(source), config::embedded())
            }
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            (None, config::embedded())
        }
        Err(error) => {
            eprintln!(
                "preview-panel: cannot read CSS theme {}: {error}",
                path.display()
            );
            (None, config::embedded())
        }
    }
}

fn install_css(css: &str) -> CssProvider {
    let provider = CssProvider::new();
    provider.load_from_data(css);
    gtk::style_context_add_provider_for_display(
        &gdk::Display::default().expect("GTK display is available"),
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
    provider
}

fn watch_theme(
    window: &ApplicationWindow,
    theme_path: Option<PathBuf>,
    mut observed_theme: Option<String>,
    css_provider: CssProvider,
    window_overrides: WindowOverrides,
    panel_geometry: Option<Rc<RefCell<PanelGeometry>>>,
) {
    let Some(theme_path) = theme_path else {
        return;
    };
    let window = window.clone();
    glib::timeout_add_local(THEME_RELOAD_INTERVAL, move || {
        let Ok(source) = fs::read_to_string(&theme_path) else {
            return glib::ControlFlow::Continue;
        };
        if observed_theme.as_deref() == Some(source.as_str()) {
            return glib::ControlFlow::Continue;
        }
        observed_theme = Some(source.clone());

        match config::parse(&source) {
            Ok(config) => {
                css_provider.load_from_data(&config.css);
                if let Some(panel_geometry) = panel_geometry.as_ref() {
                    let mut geometry = panel_geometry.borrow_mut();
                    geometry.config = config.window.with_overrides(window_overrides);
                    apply_companion_geometry(&window, &geometry);
                }
            }
            Err(error) => eprintln!(
                "preview-panel: ignoring invalid CSS theme {}: {error}",
                theme_path.display()
            ),
        }
        glib::ControlFlow::Continue
    });
}

fn configure_companion_panel(
    window: &ApplicationWindow,
    geometry: WindowConfig,
) -> Rc<RefCell<PanelGeometry>> {
    window.init_layer_shell();
    window.set_layer(Layer::Overlay);
    window.set_namespace(Some("rofi-preview-panel"));
    // Rofi keeps keyboard focus while the pointer is in the list. The editor
    // becomes focusable just before its first click instead of stealing focus
    // as soon as the companion panel is mapped.
    window.set_keyboard_mode(PanelKeyboardState::Browsing.mode());

    let geometry = Rc::new(RefCell::new(PanelGeometry {
        config: geometry,
        monitor: None,
    }));
    apply_companion_geometry(window, &geometry.borrow());

    let realize_geometry = Rc::clone(&geometry);
    window.connect_realize(move |window| {
        let Some(surface) = window.surface() else {
            return;
        };

        let window = window.clone();
        let geometry = Rc::clone(&realize_geometry);
        surface.connect_enter_monitor(move |_, monitor| {
            // The compositor has now assigned the layer surface to an output.
            // Keep that monitor so config reloads can recalculate margins.
            let mut geometry = geometry.borrow_mut();
            geometry.monitor = Some(monitor.clone());
            apply_companion_geometry(&window, &geometry);
        });
    });
    geometry
}

fn connect_panel_editor_focus(window: &ApplicationWindow, text_view: &TextView) {
    let motion = EventControllerMotion::new();
    let window_for_enter = window.clone();
    let text_view_for_enter = text_view.clone();
    motion.connect_enter(move |_, _, _| {
        // Wayland decides the keyboard target before delivering the click.
        // Arm on pointer entry so that click can focus this layer surface and
        // GTK can then route key events to the TextView.
        window_for_enter.set_keyboard_mode(PanelKeyboardState::EditorArmed.mode());
        text_view_for_enter.grab_focus();
    });
    text_view.add_controller(motion);
}

fn apply_companion_geometry(window: &ApplicationWindow, geometry: &PanelGeometry) {
    let config = &geometry.config;

    window.set_default_size(config.width, config.height);
    window.set_size_request(config.width, config.height);

    let (edge, opposite_edge) = match config.side {
        Side::Left => (Edge::Left, Edge::Right),
        Side::Right => (Edge::Right, Edge::Left),
    };
    // Anchor one horizontal edge and the top edge so both position offsets can
    // be expressed as layer-shell margins and changed while the panel is open.
    window.set_anchor(opposite_edge, false);
    window.set_margin(opposite_edge, 0);
    window.set_anchor(edge, true);
    window.set_anchor(Edge::Bottom, false);
    window.set_margin(Edge::Bottom, 0);
    window.set_anchor(Edge::Top, true);

    if let Some(monitor) = geometry.monitor.as_ref() {
        update_companion_margins(window, monitor, edge, config);
    }

    window.queue_resize();
}

fn update_companion_margins(
    window: &ApplicationWindow,
    monitor: &gdk::Monitor,
    edge: Edge,
    geometry: &WindowConfig,
) {
    let monitor = monitor.geometry();
    window.set_margin(
        edge,
        horizontal_margin(
            monitor.width(),
            geometry.companion_width,
            geometry.width,
            geometry.gap,
            geometry.side,
            geometry.x,
        ),
    );
    window.set_margin(
        Edge::Top,
        vertical_margin(monitor.height(), geometry.height, geometry.y),
    );
}

fn companion_margin(
    monitor_width: i32,
    companion_width: i32,
    panel_width: i32,
    gap: i32,
) -> i32 {
    ((monitor_width - companion_width) / 2 - panel_width - gap).max(0)
}

fn horizontal_margin(
    monitor_width: i32,
    companion_width: i32,
    panel_width: i32,
    gap: i32,
    side: Side,
    x: i32,
) -> i32 {
    let automatic = companion_margin(monitor_width, companion_width, panel_width, gap);
    let adjusted = match side {
        Side::Left => automatic + x,
        Side::Right => automatic - x,
    };
    clamp_margin(adjusted, monitor_width, panel_width)
}

fn vertical_margin(monitor_height: i32, panel_height: i32, y: i32) -> i32 {
    let centered = (monitor_height - panel_height) / 2;
    clamp_margin(centered + y, monitor_height, panel_height)
}

fn clamp_margin(margin: i32, monitor_size: i32, panel_size: i32) -> i32 {
    margin.clamp(0, (monitor_size - panel_size).max(0))
}

fn connect_live_updates(widgets: LiveWidgets, receiver: Receiver<Message>) {
    let LiveWidgets {
        application,
        window,
        stack,
        text_view,
        picture,
        network_text_view,
        network_picture,
    } = widgets;
    let mut state = LiveState::default();
    glib::timeout_add_local(Duration::from_millis(16), move || {
        while let Ok(message) = receiver.try_recv() {
            match message {
                Message::UpdateText { serial, id, text } => {
                    if !state.apply_update(serial, id, ContentKind::Text) {
                        continue;
                    }
                    let buffer = text_view.buffer();
                    buffer.set_text(&text);
                    let mut start = buffer.start_iter();
                    buffer.place_cursor(&start);
                    text_view.scroll_to_iter(&mut start, 0.0, false, 0.0, 0.0);
                    stack.set_visible_child_name("text");
                }
                Message::UpdateImage { serial, id, path } => {
                    if !state.apply_update(serial, id, ContentKind::Image) {
                        continue;
                    }
                    picture.set_filename(Some(path.as_path()));
                    stack.set_visible_child_name("image");
                }
                Message::UpdateNetwork {
                    serial,
                    id,
                    details,
                    png,
                } => {
                    if !state.apply_update(serial, id, ContentKind::Network) {
                        continue;
                    }
                    let buffer = network_text_view.buffer();
                    buffer.set_text(&details);
                    let mut start = buffer.start_iter();
                    buffer.place_cursor(&start);
                    network_text_view.scroll_to_iter(&mut start, 0.0, false, 0.0, 0.0);

                    if png.is_empty() {
                        network_picture.clear();
                        network_picture.set_visible(false);
                    } else {
                        let bytes = glib::Bytes::from_owned(png);
                        match gdk::Texture::from_bytes(&bytes) {
                            Ok(texture) => {
                                network_picture.set_from_paintable(Some(&texture));
                                network_picture.set_visible(true);
                            }
                            Err(error) => {
                                eprintln!("preview-panel: decode network QR code: {error}");
                                network_picture.clear();
                                network_picture.set_visible(false);
                            }
                        }
                    }
                    stack.set_visible_child_name("network");
                }
                Message::PrepareSwitch {
                    serial,
                    target_id,
                    reply,
                } => {
                    let response = match state.prepare_switch(serial, target_id) {
                        SwitchDisposition::Rejected => SwitchReply::Rejected,
                        SwitchDisposition::SameItem => SwitchReply::SameItem,
                        SwitchDisposition::Ready => {
                            SwitchReply::Ready(current_snapshot(&state, &text_view))
                        }
                    };
                    let _ = reply.send(response);
                }
                Message::SaveAndClose { reply } => {
                    let _ = reply.send(current_snapshot(&state, &text_view));
                    window.close();
                    application.quit();
                    return glib::ControlFlow::Break;
                }
                Message::Close => {
                    window.close();
                    application.quit();
                    return glib::ControlFlow::Break;
                }
            }
        }
        glib::ControlFlow::Continue
    });
}

fn current_snapshot(state: &LiveState, text_view: &TextView) -> Option<ContentSnapshot> {
    match state.current? {
        CurrentItem {
            id,
            kind: ContentKind::Text,
        } => {
            let buffer = text_view.buffer();
            let text = buffer
                .text(&buffer.start_iter(), &buffer.end_iter(), true)
                .to_string();
            Some(ContentSnapshot::Text { id, text })
        }
        CurrentItem {
            id,
            kind: ContentKind::Image,
        } => Some(ContentSnapshot::Image { id }),
        CurrentItem {
            kind: ContentKind::Network,
            ..
        } => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{PanelKeyboardState, companion_margin, horizontal_margin, vertical_margin};
    use crate::cli::Side;
    use gtk4_layer_shell::KeyboardMode;

    #[test]
    fn panel_only_accepts_keyboard_focus_after_the_editor_is_armed() {
        assert!(matches!(
            PanelKeyboardState::Browsing.mode(),
            KeyboardMode::None
        ));
        assert!(matches!(
            PanelKeyboardState::EditorArmed.mode(),
            KeyboardMode::OnDemand
        ));
    }

    #[test]
    fn companion_margin_places_the_panel_beside_a_centered_window() {
        assert_eq!(companion_margin(1920, 400, 480, 10), 270);
        assert_eq!(companion_margin(1280, 400, 480, 10), 0);
    }

    #[test]
    fn x_offset_moves_in_the_same_screen_direction_on_both_sides() {
        assert_eq!(
            horizontal_margin(1920, 400, 480, 10, Side::Left, 25),
            295
        );
        assert_eq!(
            horizontal_margin(1920, 400, 480, 10, Side::Right, 25),
            245
        );
    }

    #[test]
    fn y_offset_moves_from_center_and_stays_on_screen() {
        assert_eq!(vertical_margin(1080, 615, 0), 232);
        assert_eq!(vertical_margin(1080, 615, 40), 272);
        assert_eq!(vertical_margin(1080, 615, -500), 0);
        assert_eq!(vertical_margin(1080, 615, 1000), 465);
    }
}
