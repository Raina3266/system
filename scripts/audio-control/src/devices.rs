//! JSON bridge used by Quickshell's physical device picker.
//!
//! Quickshell's native PipeWire service owns live defaults and volume. This
//! bridge supplies physical ports and mutually exclusive ALSA profiles (laptop
//! Speaker and Headphones) as stable rows, without patching Quickshell.

use std::io::{self, Write};

use crate::{
    AppResult, audio,
    model::{AudioEntry, AudioKind, json_escape},
};

pub(crate) fn kind(value: &str) -> AppResult<AudioKind> {
    match value {
        "output" => Ok(AudioKind::Output),
        "input" => Ok(AudioKind::Input),
        _ => Err(io::Error::other(format!(
            "unknown audio kind {value:?}; expected output or input"
        ))
        .into()),
    }
}

/// Prints a JSON array of objects with `key`, `label`, `description`, `name`,
/// boolean `default`, and boolean `port` (physical selection needs this bridge).
/// Inactive-profile rows have an empty `name`; pass their
/// stable `key` to `set-default` to activate them.
pub fn list(value: &str) -> AppResult<()> {
    let entries = audio::selections(kind(value)?)?;
    write_entries(io::stdout().lock(), &entries)
}

/// Re-resolves a stable key immediately before applying it. Inactive-profile
/// rows therefore survive the old sink disappearing and the new one arriving.
pub fn set_default(value: &str, key: &str) -> AppResult<()> {
    let entries = audio::selections(kind(value)?)?;
    let entry = entries
        .iter()
        .find(|entry| entry.key == key)
        .ok_or_else(|| io::Error::other("The selected audio device is no longer available"))?;
    audio::set_default(entry)
}

pub(crate) fn write_entries(mut output: impl Write, entries: &[AudioEntry]) -> AppResult<()> {
    output.write_all(b"[")?;
    for (index, entry) in entries.iter().enumerate() {
        if index > 0 {
            output.write_all(b",")?;
        }
        write!(
            output,
            "{{\"key\":\"{}\",\"label\":\"{}\",\"description\":\"{}\",\"name\":\"{}\",\"default\":{},\"port\":{}}}",
            json_escape(&entry.key),
            json_escape(&entry.label),
            json_escape(&entry.description),
            json_escape(&entry.name),
            entry.default,
            entry.port.is_some(),
        )?;
    }
    output.write_all(b"]\n")?;
    Ok(())
}
