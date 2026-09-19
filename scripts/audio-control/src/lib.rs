//! Audio and Bluetooth control for this desktop.
//!
//! Wayle owns the visible audio UI. This is the small backend beside it:
//! Waybar status, Bluetooth power, and the profile-aware device selector Wayle
//! lacks — which keeps Speaker and Headphones separate even when the ALSA card
//! exposes them as mutually exclusive profiles.

use std::error::Error;

pub mod audio;
pub mod battery_provider;
pub mod bluetooth;
pub mod model;
pub mod waybar;
pub mod wayle;

pub type AppError = Box<dyn Error + Send + Sync>;
pub type AppResult<T> = Result<T, AppError>;
