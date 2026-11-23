mod model;
mod supervisor;

#[cfg(test)]
mod tests;

use std::process::ExitCode;

use clap::{CommandFactory, Parser};

use crate::model::{Action, Config, Tools};
use crate::supervisor::Supervisor;

fn main() -> ExitCode {
    let config = Config::parse();
    let result = match config.action {
        Some(Action::Help) => Config::command().print_help(),
        None => {
            let mut supervisor = Supervisor::new(config, Tools::from_environment());
            supervisor.run()
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("webcam-crop: {error}");
            ExitCode::FAILURE
        }
    }
}
