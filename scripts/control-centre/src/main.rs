//! The desktop's calendar and notification panel.
//!
//! Wayle remains the notification daemon and exposes its notification state to
//! this small layer-shell client. Media lives in Wayle's native media panel.

use std::cell::RefCell;
use std::env;
use std::process::ExitCode;
use std::rc::Rc;

use gtk::prelude::*;
use gtk::{gdk, glib};

mod calendar;
mod media_badge;
mod ui;
mod wayle;

/// One instance owns the panel; a second launch toggles the first.
const APP_ID: &str = "dev.raina.ControlCentre";

fn main() -> ExitCode {
    let mut arguments = env::args().skip(1);
    match arguments.next().as_deref() {
        // The bar badge needs no window, so it never starts GTK.
        Some("waybar") => {
            return match wayle::watch_badge() {
                Ok(()) => ExitCode::SUCCESS,
                Err(error) => {
                    eprintln!("control-centre: {error}");
                    ExitCode::FAILURE
                }
            };
        }
        Some("media-waybar") => {
            return match media_badge::watch() {
                Ok(()) => ExitCode::SUCCESS,
                Err(error) => {
                    eprintln!("control-centre: {error}");
                    ExitCode::FAILURE
                }
            };
        }
        Some("toggle") | None => {}
        Some("help" | "--help" | "-h") => {
            println!(
                "control-centre\n\n\
                 Usage:\n  \
                 control-centre toggle\n  \
                 control-centre waybar\n  \
                 control-centre media-waybar\n\n\
                 `toggle` opens or closes the panel; `waybar` streams the\n\
                 notification badge; `media-waybar` streams the active title."
            );
            return ExitCode::SUCCESS;
        }
        Some(other) => {
            eprintln!("control-centre: unknown command: {other}");
            return ExitCode::FAILURE;
        }
    };

    let application = gtk::Application::builder().application_id(APP_ID).build();
    let panel: Rc<RefCell<Option<Rc<ui::Panel>>>> = Rc::new(RefCell::new(None));

    application.connect_startup(|_| load_css());
    application.connect_activate(move |application| {
        let mut held = panel.borrow_mut();
        // The first launch builds the panel; every later one toggles it, so
        // reopening costs no process start and no GTK setup.
        let panel = held.get_or_insert_with(|| ui::Panel::build(application));
        panel.toggle();
    });

    // GTK's own argument parsing has nothing to do here.
    let status = application.run_with_args::<&str>(&[]);
    if status == glib::ExitCode::SUCCESS {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn load_css() {
    let provider = gtk::CssProvider::new();
    provider.load_from_data(include_str!("style.css"));

    if let Some(display) = gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}
