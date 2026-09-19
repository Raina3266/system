mod model;
mod supervisor;

#[cfg(test)]
mod tests;

use std::env;
use std::process::ExitCode;

use crate::model::{Action, Config, Tools};
use crate::supervisor::Supervisor;

fn print_help() {
    println!(
        "webcam-crop\n\n\
         Keep a selectable v4l2loopback webcam idle on a placeholder and switch\n\
         to a centre-cropped real camera feed while applications use it.\n\n\
         Usage: webcam-crop [options]\n\n\
         Options:\n\
           --source PATH          Real camera device (default: /dev/cam-raw)\n\
           --output PATH          v4l2loopback device (default: /dev/video10)\n\
           --input-format FORMAT  Input stream format (default: mjpeg)\n\
           --input-width PIXELS   Input width (default: 1280)\n\
           --input-height PIXELS  Input height (default: 720)\n\
           --framerate FPS        Capture/output framerate (default: 30)\n\
           --output-size PIXELS   Square output size (default: 720)\n\
           --idle-seconds SECONDS Release the real camera after this idle time (default: 3)\n\
           --warmup-seconds SEC   Let a consumer settle before switching (default: 2)\n\
           --poll-seconds SEC     Active consumer polling interval (default: 1)\n\
           -h, --help             Show this help"
    );
}

fn main() -> ExitCode {
    let action = match Config::parse(env::args().skip(1)) {
        Ok(action) => action,
        Err(error) => {
            eprintln!("webcam-crop: {error}\n");
            print_help();
            return ExitCode::from(2);
        }
    };

    let Action::Run(config) = action else {
        print_help();
        return ExitCode::SUCCESS;
    };

    let mut supervisor = Supervisor::new(config, Tools::from_environment());
    match supervisor.run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("webcam-crop: {error}");
            ExitCode::FAILURE
        }
    }
}
