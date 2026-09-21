pub mod launcher;

pub mod panel {
    pub use crate::cli::{Action, HELP, Options, Side, WindowOverrides, parse_from};
    pub use crate::document::load;
    pub use crate::ipc::bind;
    pub use crate::ui::run;
}

mod cli;
mod config;
mod document;
mod ipc;
mod panel_state;
mod ui;

#[cfg(test)]
mod tests;
