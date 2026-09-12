//! The GTK panel Waybar's audio button opens.
//!
//! It draws the same four tabs the Rofi menu has, over the same `audio` and
//! `bluetooth` logic, so the two agree about everything — most importantly
//! about mutually exclusive ALSA card profiles.

pub mod ipc;
mod state;
mod ui;
mod worker;

pub use ui::run;
