//! A media panel for Waybar's centre button.
//!
//! This used to be a patch on Wayle's own media dropdown. It reads MPRIS off
//! the session bus directly, which is what lets the parts the spec is fussy
//! about be got right, and it lives here so a Wayle release cannot break it.
//!
//!   media-panel            open the panel, or toggle one already running
//!   media-panel <monitor>  the same, on the output the button was pressed on
//!   media-panel pause-all  ask every player to stop
//!   media-panel players    print what it reads off the bus

mod artwork;
mod ipc;
mod mpris;
mod pause;
mod ui;

use std::process::ExitCode;

use gtk::prelude::*;
use gtk::{gio, glib};

const APP_ID: &str = "dev.raina.MediaPanel";
const STYLE: &str = include_str!("style.css");

fn main() -> ExitCode {
    let argument = std::env::args().nth(1).unwrap_or_default();

    if argument == "pause-all" {
        return match pause::everything() {
            Ok(report) => {
                print!("{report}");
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("media-panel: {error}");
                ExitCode::FAILURE
            }
        };
    }

    if argument == "players" {
        return match mpris::Players::connect() {
            Ok(players) => {
                for player in players.snapshot() {
                    println!(
                        "{}\n    source : {}\n    track  : {} — {} — {}\n    state  : {:?} {}/{}\n    caps   : control={} seek={} prev={} next={}\n    art    : {:?}\n    url    : {:?}\n    also   : {:?}",
                        player.bus,
                        player.source,
                        player.title,
                        player.artist,
                        player.album,
                        player.status,
                        mpris::clock(player.position),
                        player
                            .length
                            .map_or_else(|| String::from("--:--"), mpris::clock),
                        player.can_control,
                        player.can_seek,
                        player.can_go_previous,
                        player.can_go_next,
                        player.art_url,
                        player.track_url,
                        player.also,
                    );
                }
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("media-panel: {error}");
                ExitCode::FAILURE
            }
        };
    }

    if argument == "--help" || argument == "-h" || argument == "help" {
        println!(
            "media-panel\n\n\
             Usage:\n  \
             media-panel [<monitor>]   open the panel, or toggle one already open\n  \
             media-panel pause-all     ask every MPRIS player to pause\n  \
             media-panel players       print what the panel reads off the bus\n"
        );
        return ExitCode::SUCCESS;
    }

    // A panel is already up: hand it the press and get out of the way.
    if ipc::toggle_running_panel(&argument) {
        return ExitCode::SUCCESS;
    }

    let (toggles, guard) = match ipc::listen() {
        Ok(listening) => listening,
        Err(error) => {
            eprintln!(
                "media-panel: could not claim {:?}: {error}",
                ipc::socket_path()
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
            ui::run(app, monitor.clone(), toggles);
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
