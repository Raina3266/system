//! Media backend for the Astal frontend.
//!
//! Astal supplies native MPRIS/audio snapshots and executes guarded native calls.
//! Rust owns grouping, selection, timing, action transactions, private-player
//! discovery and D-Bus recovery, native playback-status reconciliation, stream
//! ownership, artwork, and credit lookups.
//! The resident service has no GUI; the pure model command remains available.

// Integration suites run only when tests.rs is the Cargo test target.
#[cfg(test)]
macro_rules! skip_integration_tests {
    (integration) => {};
}
#[cfg(test)]
pub(crate) use skip_integration_tests as script_tests;

#[cfg(test)]
#[macro_use]
mod tests;

pub mod artwork;
pub mod credit;
pub mod model;
pub mod playback;

pub type AppResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
