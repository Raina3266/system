use std::rc::Rc;

use gtk::prelude::*;
use gtk::{
    gio, Application, ApplicationWindow, PolicyType, ScrolledWindow, TextView, WrapMode,
};

use crate::cli::Options;

pub fn run(options: Options, text: String) {
    // NON_UNIQUE is important for launchers: every View action gets its own
    // document rather than activating an older window and losing new stdin.
    let application = Application::builder()
        .application_id("io.github.raina.PreviewPanel")
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    let options = Rc::new(options);
    let text = Rc::new(text);

    application.connect_activate(move |application| {
        build_window(application, options.as_ref(), text.as_str());
    });
    // main.rs has already parsed our CLI. Supplying a clean argv prevents
    // GApplication from trying to interpret --stdin, --title, or a file path.
    application.run_with_args(&["preview-panel"]);
}

fn build_window(application: &Application, options: &Options, text: &str) {
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
    window.set_child(Some(&scrolled_window));
    window.present();
    text_view.grab_focus();
}
