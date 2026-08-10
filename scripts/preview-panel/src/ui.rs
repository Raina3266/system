use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::mpsc::Receiver;
use std::time::Duration;

use gtk::prelude::*;
use gtk::{
    gdk, gio, glib, Application, ApplicationWindow, CssProvider, PolicyType, ScrolledWindow,
    Picture, Stack, TextView, WrapMode,
};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

use crate::cli::{Options, Side, WindowOverrides};
use crate::config::{self, Config, WindowConfig};
use crate::ipc::Message;

const CONFIG_RELOAD_INTERVAL: Duration = Duration::from_millis(250);

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
    let config_path = config::configured_path();
    let (observed_config, loaded_config) = load_initial_config(config_path.as_deref());
    let css_provider = install_css(&loaded_config.css);
    let panel_config = loaded_config
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
    text_view.set_pixels_above_lines(1);
    text_view.set_pixels_below_lines(1);
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

    let stack = Stack::new();
    stack.set_hexpand(true);
    stack.set_vexpand(true);
    stack.add_named(&scrolled_window, Some("text"));
    stack.add_named(&picture, Some("image"));
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

    let panel_geometry = options
        .panel
        .then(|| configure_companion_panel(&window, panel_config));
    watch_config(
        &window,
        config_path,
        observed_config,
        css_provider,
        options.window_overrides,
        panel_geometry,
    );
    if let Some(receiver) = receiver {
        connect_live_updates(
            application,
            &window,
            &stack,
            &text_view,
            &picture,
            receiver,
        );
    }

    window.present();
    if !options.panel {
        text_view.grab_focus();
    }
}

fn load_initial_config(path: Option<&Path>) -> (Option<String>, Config) {
    let Some(path) = path else {
        return (None, config::embedded());
    };

    match fs::read_to_string(path) {
        Ok(source) => match config::parse(&source) {
            Ok(config) => (Some(source), config),
            Err(error) => {
                eprintln!(
                    "preview-panel: ignoring invalid config {}: {error}",
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
                "preview-panel: cannot read config {}: {error}",
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

fn watch_config(
    window: &ApplicationWindow,
    config_path: Option<PathBuf>,
    mut observed_config: Option<String>,
    css_provider: CssProvider,
    window_overrides: WindowOverrides,
    panel_geometry: Option<Rc<RefCell<WindowConfig>>>,
) {
    let Some(config_path) = config_path else {
        return;
    };
    let window = window.clone();
    glib::timeout_add_local(CONFIG_RELOAD_INTERVAL, move || {
        let Ok(source) = fs::read_to_string(&config_path) else {
            return glib::ControlFlow::Continue;
        };
        if observed_config.as_deref() == Some(source.as_str()) {
            return glib::ControlFlow::Continue;
        }
        observed_config = Some(source.clone());

        match config::parse(&source) {
            Ok(config) => {
                css_provider.load_from_data(&config.css);
                if let Some(panel_geometry) = panel_geometry.as_ref() {
                    let geometry = config.window.with_overrides(window_overrides);
                    *panel_geometry.borrow_mut() = geometry.clone();
                    apply_companion_geometry(&window, &geometry);
                }
            }
            Err(error) => eprintln!(
                "preview-panel: ignoring invalid config {}: {error}",
                config_path.display()
            ),
        }
        glib::ControlFlow::Continue
    });
}

fn configure_companion_panel(
    window: &ApplicationWindow,
    geometry: WindowConfig,
) -> Rc<RefCell<WindowConfig>> {
    window.init_layer_shell();
    window.set_layer(Layer::Overlay);
    window.set_namespace(Some("rofi-preview-panel"));
    window.set_keyboard_mode(KeyboardMode::OnDemand);

    let geometry = Rc::new(RefCell::new(geometry));
    apply_companion_geometry(window, &geometry.borrow());

    let realize_geometry = Rc::clone(&geometry);
    window.connect_realize(move |window| {
        apply_companion_geometry(window, &realize_geometry.borrow());
    });
    let map_geometry = Rc::clone(&geometry);
    window.connect_map(move |window| {
        // Recalculate once the compositor has assigned the final output. This
        // matters when the focused output is not GDK's initial output.
        apply_companion_geometry(window, &map_geometry.borrow());
    });
    geometry
}

fn apply_companion_geometry(window: &ApplicationWindow, geometry: &WindowConfig) {
    window.set_default_size(geometry.width, geometry.height);
    window.set_size_request(geometry.width, geometry.height);

    let (edge, opposite_edge) = match geometry.side {
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
    update_companion_margins(window, edge, geometry);
    window.queue_resize();
}

fn update_companion_margins(
    window: &ApplicationWindow,
    edge: Edge,
    geometry: &WindowConfig,
) {
    let Some(surface) = window.surface() else {
        return;
    };
    let Some(monitor) = gtk::prelude::RootExt::display(window).monitor_at_surface(&surface) else {
        return;
    };
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

fn connect_live_updates(
    application: &Application,
    window: &ApplicationWindow,
    stack: &Stack,
    text_view: &TextView,
    picture: &Picture,
    receiver: Receiver<Message>,
) {
    let application = application.clone();
    let window = window.clone();
    let stack = stack.clone();
    let text_view = text_view.clone();
    let picture = picture.clone();
    let mut latest_serial = 0;
    glib::timeout_add_local(Duration::from_millis(16), move || {
        while let Ok(message) = receiver.try_recv() {
            match message {
                Message::UpdateText { serial, text } => {
                    if !accept_serial(&mut latest_serial, serial) {
                        continue;
                    }
                    let buffer = text_view.buffer();
                    buffer.set_text(&text);
                    let mut start = buffer.start_iter();
                    buffer.place_cursor(&start);
                    text_view.scroll_to_iter(&mut start, 0.0, false, 0.0, 0.0);
                    stack.set_visible_child_name("text");
                }
                Message::UpdateImage { serial, path } => {
                    if !accept_serial(&mut latest_serial, serial) {
                        continue;
                    }
                    picture.set_filename(Some(path.as_path()));
                    stack.set_visible_child_name("image");
                }
                Message::SaveAndClose { reply } => {
                    let buffer = text_view.buffer();
                    let text = buffer
                        .text(&buffer.start_iter(), &buffer.end_iter(), true)
                        .to_string();
                    let _ = reply.send(text);
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

fn accept_serial(latest: &mut u64, candidate: u64) -> bool {
    if candidate < *latest {
        return false;
    }
    *latest = candidate;
    true
}

#[cfg(test)]
mod tests {
    use super::{
        accept_serial, companion_margin, horizontal_margin, vertical_margin,
    };
    use crate::cli::Side;

    #[test]
    fn rapid_updates_cannot_restore_an_older_selection() {
        let mut latest = 0;
        assert!(accept_serial(&mut latest, 8));
        assert!(!accept_serial(&mut latest, 6));
        assert_eq!(latest, 8);
        assert!(accept_serial(&mut latest, 9));
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
