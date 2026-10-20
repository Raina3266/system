//! The Quickshell media panel's machine interface: move one playback stream
//! onto an output picked from `devices output` rows.
//!
//! The move itself is `audio::move_stream`, so an inactive-profile destination
//! (laptop Speaker/Headphones) switches the ALSA card profile first and rolls
//! back on failure — the same path the Quickshell device picker uses. Listing
//! streams and their current routes needs no helper: PipeWire clients read
//! nodes, properties and link groups natively.

use std::io;

use crate::{AppResult, audio, model::StreamEntry};

/// Moves one stream onto the destination a `devices output` row's key names.
/// Prefer `serial:N`: unlike a reusable node ID, a serial identifies one stream
/// lifetime. `node:N` remains an explicit instantaneous CLI selector.
pub fn move_to(selector: &str, destination_key: &str) -> AppResult<()> {
    let entries = audio::streams()?;
    let entry = select_stream(&entries, selector)?;
    audio::move_stream(entry, destination_key)
}

/// Typed identifiers cannot collide across the serial and node-id namespaces.
/// Keep legacy selectors, but reject any ambiguous identity or application name.
pub(crate) fn select_stream<'a>(
    entries: &'a [StreamEntry],
    selector: &str,
) -> AppResult<&'a StreamEntry> {
    if selector.trim().is_empty() {
        return Err(io::Error::other("This media stream is no longer available").into());
    }
    let explicit = selector
        .strip_prefix("serial:")
        .map(|value| (true, value))
        .or_else(|| selector.strip_prefix("node:").map(|value| (false, value)));
    if let Some((by_serial, value)) = explicit {
        if !value.bytes().all(|byte| byte.is_ascii_digit())
            || !value.parse::<u64>().is_ok_and(|id| id > 0)
        {
            return Err(io::Error::other(format!(
                "Invalid stream identifier {selector:?}; expected serial:<positive number> or node:<positive number>"
            ))
            .into());
        }
        return unique_match(
            entries.iter().filter(|entry| {
                if by_serial {
                    entry.serial == value
                } else {
                    entry.object_id == value
                }
            }),
            selector,
        );
    }
    unique_match(
        entries.iter().filter(|entry| {
            entry.key == selector
                || entry.serial == selector
                || entry.object_id == selector
                || entry.application.eq_ignore_ascii_case(selector)
                || entry.name.eq_ignore_ascii_case(selector)
        }),
        selector,
    )
}

fn unique_match<'a>(
    mut matches: impl Iterator<Item = &'a StreamEntry>,
    selector: &str,
) -> AppResult<&'a StreamEntry> {
    match (matches.next(), matches.next()) {
        (Some(entry), None) => Ok(entry),
        (Some(_), Some(_)) => Err(io::Error::other(format!(
            "More than one playback stream matches {selector:?}; use serial:<serial> or node:<id>"
        ))
        .into()),
        (None, _) => Err(io::Error::other("This media stream is no longer available").into()),
    }
}
