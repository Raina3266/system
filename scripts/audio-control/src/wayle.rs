//! Small machine interface used by Wayle's device picker.
//!
//! Only for the profile-aware selection Wayle's audio service lacks: the same
//! `selections` and `set_default` the Waybar backend uses, keeping mutually
//! exclusive ALSA profiles (laptop Speaker and Headphones) as stable rows.

use std::io::{self, Write};

use crate::{
    AppResult, audio,
    model::{AudioEntry, AudioKind},
};

const FIELD_SEPARATOR: u8 = 0;

pub(crate) fn kind(value: &str) -> AppResult<AudioKind> {
    match value {
        "output" => Ok(AudioKind::Output),
        "input" => Ok(AudioKind::Input),
        _ => Err(io::Error::other(format!(
            "unknown Wayle audio kind {value:?}; expected output or input"
        ))
        .into()),
    }
}

/// Prints five NUL-terminated fields per row: stable key, compact label, full
/// description, live node name, default state. NUL cannot occur in PulseAudio's
/// C strings, so nothing needs escaping.
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
    for entry in entries {
        for field in [
            entry.key.as_str(),
            entry.label.as_str(),
            entry.description.as_str(),
            entry.name.as_str(),
            if entry.default { "1" } else { "0" },
        ] {
            if field.as_bytes().contains(&FIELD_SEPARATOR) {
                return Err(io::Error::other("Audio metadata contains a NUL byte").into());
            }
            output.write_all(field.as_bytes())?;
            output.write_all(&[FIELD_SEPARATOR])?;
        }
    }
    Ok(())
}
