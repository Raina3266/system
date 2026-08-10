use std::env;
use std::ffi::OsStr;
use std::fs;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc::Receiver;
use std::time::Duration;

use gtk::prelude::*;
use gtk::{
    gdk, gio, glib, Application, ApplicationWindow, CssProvider, PolicyType, ScrolledWindow,
    Picture, Stack, TextView, WrapMode,
};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

use crate::cli::{Options, Side};
use crate::ipc::Message;

const DEFAULT_CSS: &str = include_str!("../style.css");
const CSS_RELOAD_INTERVAL: Duration = Duration::from_millis(250);

pub fn run(options: Options, text: String, receiver: Option<Receiver<Message>>) {
    // NON_UNIQUE is important for launchers: every View action gets its own
    // document rather than activating an older window and losing new stdin.
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
    install_css();

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

    let window = ApplicationWindow::builder()
        .application(application)
        .default_height(options.height)
        .default_width(options.width)
        .title(&options.title)
        .build();
    window.add_css_class("preview-panel");
    window.set_decorated(!options.panel);
    window.set_child(Some(&stack));

    if options.panel {
        configure_companion_panel(&window, options);
    }
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

fn install_css() {
    let provider = CssProvider::new();
    let css_path = configured_css_path();
    let mut loaded_css = css_path
        .as_deref()
        .and_then(|path| fs::read_to_string(path).ok())
        .unwrap_or_else(|| DEFAULT_CSS.to_owned());

    provider.load_from_data(&loaded_css);
    gtk::style_context_add_provider_for_display(
        &gdk::Display::default().expect("GTK display is available"),
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );

    let Some(css_path) = css_path else {
        return;
    };
    glib::timeout_add_local(CSS_RELOAD_INTERVAL, move || {
        if let Ok(css) = fs::read_to_string(&css_path)
            && css != loaded_css
        {
            provider.load_from_data(&css);
            loaded_css = css;
        }
        glib::ControlFlow::Continue
    });
}

fn configured_css_path() -> Option<PathBuf> {
    css_path_from(
        env::var_os("PREVIEW_PANEL_CSS").as_deref(),
        env::var_os("XDG_CONFIG_HOME").as_deref(),
        env::var_os("HOME").as_deref(),
    )
}

fn css_path_from(
    override_path: Option<&OsStr>,
    xdg_config_home: Option<&OsStr>,
    home: Option<&OsStr>,
) -> Option<PathBuf> {
    if let Some(path) = override_path.filter(|path| !path.is_empty()) {
        return Some(PathBuf::from(path));
    }

    let config_home = xdg_config_home
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            home.filter(|path| !path.is_empty())
                .map(|path| PathBuf::from(path).join(".config"))
        })?;
    Some(config_home.join("preview-panel/style.css"))
}

fn configure_companion_panel(window: &ApplicationWindow, options: &Options) {
    window.init_layer_shell();
    window.set_layer(Layer::Overlay);
    window.set_namespace(Some("rofi-preview-panel"));
    window.set_keyboard_mode(KeyboardMode::OnDemand);

    let (edge, opposite_edge) = match options.side {
        Side::Left => (Edge::Left, Edge::Right),
        Side::Right => (Edge::Right, Edge::Left),
    };
    // Layer-shell geometry must be established before the window is mapped.
    // Applying the anchor from the map handler lets the compositor commit the
    // first surface in the centre and some compositors never reposition it.
    window.set_anchor(edge, true);
    window.set_anchor(opposite_edge, false);

    let panel_width = options.width;
    let companion_width = options.companion_width;
    let gap = options.gap;
    window.connect_realize(move |window| {
        update_companion_margin(window, edge, companion_width, panel_width, gap);
    });
    window.connect_map(move |window| {
        // Recalculate once the compositor has assigned the final output. This
        // matters when the focused output is not GDK's initial output.
        update_companion_margin(window, edge, companion_width, panel_width, gap);
    });
}

fn update_companion_margin(
    window: &ApplicationWindow,
    edge: Edge,
    companion_width: i32,
    panel_width: i32,
    gap: i32,
) {
    let Some(surface) = window.surface() else {
        return;
    };
    let Some(monitor) = gtk::prelude::RootExt::display(window).monitor_at_surface(&surface) else {
        return;
    };
    window.set_margin(
        edge,
        companion_margin(
            monitor.geometry().width(),
            companion_width,
            panel_width,
            gap,
        ),
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
    use std::ffi::OsStr;
    use std::path::PathBuf;

    use super::{accept_serial, companion_margin, css_path_from};

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
    fn explicit_css_path_takes_priority() {
        let path = css_path_from(
            Some(OsStr::new("/tmp/custom.css")),
            Some(OsStr::new("/tmp/config")),
            Some(OsStr::new("/home/raina")),
        );
        assert_eq!(path, Some(PathBuf::from("/tmp/custom.css")));
    }

    #[test]
    fn css_path_uses_xdg_then_home_fallback() {
        assert_eq!(
            css_path_from(None, Some(OsStr::new("/tmp/config")), None),
            Some(PathBuf::from("/tmp/config/preview-panel/style.css"))
        );
        assert_eq!(
            css_path_from(None, None, Some(OsStr::new("/home/raina"))),
            Some(PathBuf::from(
                "/home/raina/.config/preview-panel/style.css"
            ))
        );
    }
}
