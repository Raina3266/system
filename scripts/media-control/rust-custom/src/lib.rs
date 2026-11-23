//! Media backend for the Astal frontend.
//!
//! Astal supplies native MPRIS/audio snapshots and executes guarded native calls.
//! Rust owns grouping, selection, timing, action transactions, private-player
//! discovery and D-Bus recovery, stream ownership, artwork, and credit lookups.
//! The resident service has no GUI; the pure model command remains available.

#[cfg(test)]
#[macro_use]
mod unit_tests;

pub mod artwork;
pub mod credit;
pub mod model;
pub mod playback;

pub type AppResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
