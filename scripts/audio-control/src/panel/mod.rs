//! The GTK panel Waybar's audio button opens.
//!
//! It draws four tabs over the same `audio` and `bluetooth` logic the command
//! line uses, so the two agree about everything — most importantly about
//! mutually exclusive ALSA card profiles.

pub mod ipc;
mod state;
mod ui;
mod worker;

pub use ui::run;
