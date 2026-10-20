//! Custom media policy for the native Quickshell frontend.
//!
//! Quickshell supplies MPRIS/PipeWire snapshots and executes native operations.
//! This helper owns grouping, browser filtering, stream ownership, and metadata
//! that those native services do not provide. It has no GUI or D-Bus backend.

pub mod artwork;
pub mod credit;
pub mod model;
pub mod playback;

pub type AppResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
