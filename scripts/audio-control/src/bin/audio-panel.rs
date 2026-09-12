//! The audio and Bluetooth panel Waybar's right-hand button opens.
//!
//!   audio-panel            open the panel, or toggle one already running
//!   audio-panel <monitor>  the same, on the output the button was pressed on

use std::process::ExitCode;

use audio_control::panel;
use gtk::prelude::*;
use gtk::{gio, glib};

const APP_ID: &str = "dev.raina.AudioPanel";
const STYLE: &str = include_str!("../panel/style.css");

fn main() -> ExitCode {
    let argument = std::env::args().nth(1).unwrap_or_default();

    if argument == "--help" || argument == "-h" || argument == "help" {
        println!(
            "audio-panel\n\n\
             Usage:\n  \
             audio-panel [<monitor>]   open the panel, or toggle one already open\n"
        );
        return ExitCode::SUCCESS;
    }

    // A panel is already up: hand it the press and get out of the way.
    if panel::ipc::toggle_running_panel(&argument) {
        return ExitCode::SUCCESS;
    }

    let (toggles, guard) = match panel::ipc::listen() {
        Ok(listening) => listening,
        Err(error) => {
            eprintln!(
                "audio-panel: could not claim {:?}: {error}",
                panel::ipc::socket_path()
            );
            return ExitCode::FAILURE;
        }
    };

    let monitor = (!argument.is_empty()).then_some(argument);
    let app = gtk::Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();

    let toggles = std::cell::RefCell::new(Some(toggles));
    app.connect_startup(|_| load_style());
    app.connect_activate(move |app| {
        if let Some(toggles) = toggles.borrow_mut().take() {
            panel::run(app, monitor.clone(), toggles);
        }
    });

    let status = app.run_with_args::<&str>(&[]);
    drop(guard);
    if status == glib::ExitCode::SUCCESS {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn load_style() {
    let Some(display) = gtk::gdk::Display::default() else {
        return;
    };
    let provider = gtk::CssProvider::new();
    provider.load_from_data(STYLE);
    gtk::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}
