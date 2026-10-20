//! Audio and Bluetooth control for this desktop.
//!
//! Quickshell owns the visible audio UI. This is the small backend beside it:
//! Waybar status, Bluetooth power, and the profile-aware device selector Quickshell
//! lacks — which keeps Speaker and Headphones separate even when the ALSA card
//! exposes them as mutually exclusive profiles.

use std::error::Error;

pub mod artwork;
pub mod audio;
pub mod bluetooth;
pub mod devices;
pub mod model;
pub mod stream;
pub mod waybar;
pub mod wayle;

#[cfg(test)]
mod tests;

pub type AppError = Box<dyn Error + Send + Sync>;
pub type AppResult<T> = Result<T, AppError>;
