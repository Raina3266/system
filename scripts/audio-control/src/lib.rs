//! Audio and Bluetooth control for this desktop.
//!
//! The logic lives here so the three front ends over it — the GTK panel, the
//! Waybar status line, and Wayle's device picker — all make the same
//! decisions. That matters most for mutually exclusive ALSA card profiles:
//! Speaker and Headphones stay separate destinations in every one of them
//! because they all go through the same `selections` and `set_default`.

use std::error::Error;

pub mod audio;
pub mod bluetooth;
pub mod model;
pub mod panel;
pub mod waybar;
pub mod wayle;

pub type AppError = Box<dyn Error + Send + Sync>;
pub type AppResult<T> = Result<T, AppError>;
