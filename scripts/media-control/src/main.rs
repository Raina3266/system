use std::env;
use std::process::ExitCode;

mod model;
mod mpris;
mod state;
mod text;
mod ui;

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("menu") => ui::launch_menu(),
        Some("rofi") => ui::rofi_mode(),
        Some("waybar") => ui::waybar(&args[1..]),
        Some("toggle") => ui::toggle(),
        Some("pause-all") => mpris::pause_all(),
        Some("list") => mpris::print_list(),
        Some("help" | "--help" | "-h") | None => {
            print_help();
            Ok(())
        }
        Some(other) => Err(format!("unknown command: {other}")),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("media-control: {message}");
            ExitCode::FAILURE
        }
    }
}

fn print_help() {
    println!(
        "media-control\n\n\
         Usage:\n\
           media-control menu\n\
           media-control waybar --watch [--interval-ms 750]\n\
           media-control toggle\n\
           media-control pause-all\n\
           media-control list"
    );
}

#[cfg(test)]
mod tests;
