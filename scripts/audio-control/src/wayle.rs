//! Small machine interface used by Wayle's device picker.
//!
//! The regular UI and this bridge deliberately use the same `selections` and
//! `set_default` functions. That keeps mutually exclusive ALSA card profiles
//! (notably the laptop Speaker and Headphones profiles) behaving identically
//! in Rofi and Wayle.

use std::io::{self, Write};

use crate::{
    AppResult, audio,
    model::{AudioEntry, AudioKind},
};

const FIELD_SEPARATOR: u8 = 0;

fn kind(value: &str) -> AppResult<AudioKind> {
    match value {
        "output" => Ok(AudioKind::Output),
        "input" => Ok(AudioKind::Input),
        _ => Err(io::Error::other(format!(
            "unknown Wayle audio kind {value:?}; expected output or input"
        ))
        .into()),
    }
}

/// Prints five NUL-terminated UTF-8 fields for every selectable row:
/// stable key, compact label, full description, live node name, and default
/// state (`1` or `0`). NUL cannot occur in data originating from PulseAudio's
/// C strings, so no escaping or extra serialization dependency is needed.
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

fn write_entries(mut output: impl Write, entries: &[AudioEntry]) -> AppResult<()> {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(default: bool) -> AudioEntry {
        AudioEntry {
            key: "card-output:01:port:02".into(),
            kind: AudioKind::Output,
            name: "alsa_output.pci".into(),
            card: Some("alsa_card.pci".into()),
            description: "Alder Lake Controller — Headphones".into(),
            label: "Headphones".into(),
            volume: 50,
            muted: false,
            default,
            port: Some("[Out] Headphones".into()),
        }
    }

    #[test]
    fn bridge_rows_have_fixed_nul_delimited_fields() {
        let mut bytes = Vec::new();
        write_entries(&mut bytes, &[entry(true), entry(false)]).unwrap();
        let mut fields: Vec<_> = bytes.split(|byte| *byte == 0).collect();
        assert_eq!(fields.pop(), Some(&[][..]));
        assert_eq!(fields.len(), 10);
        assert_eq!(fields[0], b"card-output:01:port:02");
        assert_eq!(fields[1], b"Headphones");
        assert_eq!(fields[4], b"1");
        assert_eq!(fields[9], b"0");
    }

    #[test]
    fn only_device_tabs_are_accepted() {
        assert_eq!(kind("output").unwrap(), AudioKind::Output);
        assert_eq!(kind("input").unwrap(), AudioKind::Input);
        assert!(kind("playback").is_err());
    }
}
