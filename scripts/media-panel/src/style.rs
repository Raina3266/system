//! Live CSS loading. Styling is configuration, not a compiled program input.

use std::path::PathBuf;
use std::time::Duration;

use gtk::glib;

fn path() -> PathBuf {
    if let Some(path) = std::env::var_os("MEDIA_PANEL_CSS") {
        return path.into();
    }
    if let Some(config) = std::env::var_os("XDG_CONFIG_HOME") {
        return PathBuf::from(config).join("media-panel/style.css");
    }
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default()
        .join(".config/media-panel/style.css")
}

pub fn install() {
    let Some(display) = gtk::gdk::Display::default() else {
        return;
    };

    let provider = gtk::CssProvider::new();
    gtk::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );

    let path = path();
    let mut active = None;
    if let Ok(css) = std::fs::read_to_string(&path) {
        provider.load_from_data(&css);
        active = Some(css);
    }

    // Reusing the provider updates every existing widget without recreating
    // the panel. Invalid/partial saves leave the last readable CSS in place.
    glib::timeout_add_local(Duration::from_millis(250), move || {
        if let Ok(css) = std::fs::read_to_string(&path)
            && active.as_ref() != Some(&css)
        {
            provider.load_from_data(&css);
            active = Some(css);
        }
        glib::ControlFlow::Continue
    });
}
