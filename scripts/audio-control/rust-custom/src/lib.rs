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

#[cfg(test)]
audio_unit_tests!(library);
