//! The desktop's control centre: calendar, media and system readings in a
//! layer-shell panel, with Wayle's notification dropdown opened beside it.
//!
//! Wayle keeps the notifications. It is the notification daemon, and only the
//! daemon holds each entry's icon, actions and urgency, so re-drawing that
//! list here would lose them. Everything else Wayle's control centre used to
//! carry is drawn by this program instead, which is why the patches against
//! Wayle no longer reach into its widgets.

use std::cell::RefCell;
use std::env;
use std::process::ExitCode;
use std::rc::Rc;

use gtk::prelude::*;
use gtk::{gdk, glib};

mod calendar;
mod media;
mod ring;
mod system;
mod ui;
mod wayle;

/// One instance owns the panel; a second launch toggles the first.
const APP_ID: &str = "dev.raina.ControlCentre";

fn main() -> ExitCode {
    let mut arguments = env::args().skip(1);
    let output = match arguments.next().as_deref() {
        Some("toggle") | None => arguments.next().unwrap_or_default(),
        Some("help" | "--help" | "-h") => {
            println!(
                "control-centre\n\n\
                 Usage:\n  \
                 control-centre toggle [monitor]\n\n\
                 Opens or closes the panel. With no monitor, Wayle puts its\n\
                 notification dropdown on the output holding focus."
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
        let panel = held.get_or_insert_with(|| ui::Panel::build(application, output.clone()));
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
