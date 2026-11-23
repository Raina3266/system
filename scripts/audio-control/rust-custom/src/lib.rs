//! Audio/Bluetooth policy, queues, scheduling and transactions for the Astal UI.
//!
//! Canonical inventory and defaults arrive from Astal. Only missing SPA
//! relationships and per-endpoint route state are read from `pw-dump -N`.
//! All audio changes, including recovery, are RPC requests to the native owner.
//! Rust also supplies request-local pairing agents and the HID++ battery provider.

use std::error::Error;

pub mod audio;
pub mod bluetooth;
pub mod devices;
pub mod model;
pub mod policy;

pub type AppError = Box<dyn Error + Send + Sync>;
pub type AppResult<T> = Result<T, AppError>;

#[cfg(test)]
mod unit_tests;
