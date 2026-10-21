//! Custom media policy for the native media frontend.
//!
//! The frontend supplies MPRIS/audio snapshots and executes native operations.
//! This helper owns grouping, browser filtering, stream ownership, and metadata
//! that those native services do not provide. It has no GUI or D-Bus backend.

// The explicit integration target owns all tests; keep library test builds valid.
#[cfg(test)]
macro_rules! media_unit_tests {
    ($module:ident) => {};
}

pub mod artwork;
pub mod credit;
pub mod model;
pub mod playback;

pub type AppResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
