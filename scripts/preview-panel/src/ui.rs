use std::rc::Rc;
use std::sync::mpsc::Receiver;
use std::time::Duration;

use gtk::prelude::*;
use gtk::{
    gdk, gio, glib, Application, ApplicationWindow, CssProvider, PolicyType, ScrolledWindow,
    TextView, WrapMode,
};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

use crate::cli::Options;
use crate::ipc::Message;

const PANEL_CSS: &str = r#"
window.preview-panel {
    background: rgba(14, 6, 22, 0.95);
    border: 1px solid rgba(255, 126, 219, 0.55);
    border-radius: 15px;
}

textview.preview-text,
textview.preview-text text {
    background: transparent;
    color: #cbe3e7;
    caret-color: #7afcff;
    font-family: "JetBrains Mono Nerd Font", monospace;
    font-size: 11pt;
}

scrollbar slider {
    min-width: 4px;
    min-height: 24px;
    background: #ff7edb;
    border-radius: 4px;
}
"#;

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

    let window = ApplicationWindow::builder()
        .application(application)
        .default_height(options.height)
        .default_width(options.width)
        .title(&options.title)
        .build();
    window.add_css_class("preview-panel");
    window.set_decorated(!options.panel);
    window.set_child(Some(&scrolled_window));

    if options.panel {
        configure_companion_panel(&window, options);
    }
    if let Some(receiver) = receiver {
        connect_live_updates(application, &window, &text_view, receiver);
    }

    window.present();
    if !options.panel {
        text_view.grab_focus();
    }
}

fn install_css() {
    let provider = CssProvider::new();
    provider.load_from_data(PANEL_CSS);
    gtk::style_context_add_provider_for_display(
        &gdk::Display::default().expect("GTK display is available"),
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}

fn configure_companion_panel(window: &ApplicationWindow, options: &Options) {
    window.init_layer_shell();
    window.set_layer(Layer::Overlay);
    window.set_namespace(Some("rofi-preview-panel"));
    window.set_keyboard_mode(KeyboardMode::OnDemand);

    let panel_width = options.width;
    let companion_width = options.companion_width;
    let gap = options.gap;
    window.connect_map(move |window| {
        let Some(surface) = window.surface() else {
            return;
        };
        let Some(monitor) = window.display().monitor_at_surface(&surface) else {
            return;
        };
        let monitor_width = monitor.geometry().width();
        let left_margin = ((monitor_width - companion_width) / 2 - panel_width - gap).max(0);
        window.set_anchor(Edge::Left, true);
        window.set_margin(Edge::Left, left_margin);
    });
}

fn connect_live_updates(
    application: &Application,
    window: &ApplicationWindow,
    text_view: &TextView,
    receiver: Receiver<Message>,
) {
    let application = application.clone();
    let window = window.clone();
    let text_view = text_view.clone();
    let mut latest_serial = 0;
    glib::timeout_add_local(Duration::from_millis(16), move || {
        while let Ok(message) = receiver.try_recv() {
            match message {
                Message::Update { serial, text } => {
                    if !accept_serial(&mut latest_serial, serial) {
                        continue;
                    }
                    let buffer = text_view.buffer();
                    buffer.set_text(&text);
                    let mut start = buffer.start_iter();
                    buffer.place_cursor(&start);
                    text_view.scroll_to_iter(&mut start, 0.0, false, 0.0, 0.0);
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
    use super::accept_serial;

    #[test]
    fn rapid_updates_cannot_restore_an_older_selection() {
        let mut latest = 0;
        assert!(accept_serial(&mut latest, 8));
        assert!(!accept_serial(&mut latest, 6));
        assert_eq!(latest, 8);
        assert!(accept_serial(&mut latest, 9));
    }
}
