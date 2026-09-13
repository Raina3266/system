//! Audio and Bluetooth control for this desktop.
//!
//! Wayle owns the visible audio UI and uses its native widgets and reactive
//! audio service wherever possible. This library is the small backend beside
//! it: Waybar status/Bluetooth power plus the profile-aware device selector
//! Wayle calls for behaviour its own audio service does not expose. In
//! particular, Speaker and Headphones can remain separate destinations even
//! when the ALSA card exposes them through mutually exclusive profiles.

use std::error::Error;

pub mod audio;
pub mod bluetooth;
pub mod model;
pub mod waybar;
pub mod wayle;

pub type AppError = Box<dyn Error + Send + Sync>;
pub type AppResult<T> = Result<T, AppError>;
