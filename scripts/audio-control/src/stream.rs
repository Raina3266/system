//! The Quickshell media panel's machine interface: move one playback stream
//! onto an output picked from `wayle-list output` rows.
//!
//! The move itself is `audio::move_stream`, so an inactive-profile destination
//! (laptop Speaker/Headphones) switches the ALSA card profile first and rolls
//! back on failure — the same path the Wayle device picker uses. Listing
//! streams and their current routes needs no helper: PipeWire clients read
//! nodes, properties and link groups natively.

use std::io;

use crate::{AppResult, audio, model::StreamEntry};

/// Moves one stream, selected by full key, bare PipeWire object serial, or
/// application name, to the destination a `wayle-list output` row's key names.
pub fn move_to(selector: &str, destination_key: &str) -> AppResult<()> {
    let entries = audio::streams()?;
    let entry = select_stream(&entries, selector)?;
    audio::move_stream(entry, destination_key)
}

/// Selection by full key, bare serial, bare PipeWire object id, or exact
/// application/node name. The id and serial are unique per stream, so they
/// win even when two tabs of one browser play at once; names are matched
/// exactly, ignoring ASCII case, and an ambiguous name is rejected rather
/// than routed to a guess.
pub(crate) fn select_stream<'a>(
    entries: &'a [StreamEntry],
    selector: &str,
) -> AppResult<&'a StreamEntry> {
    if let Some(entry) = entries.iter().find(|entry| {
        entry.key == selector || entry.serial == selector || entry.object_id == selector
    }) {
        return Ok(entry);
    }
    let mut matches = entries
        .iter()
        .filter(|entry| entry.application.eq_ignore_ascii_case(selector) || entry.name == selector);
    match (matches.next(), matches.next()) {
        (Some(entry), None) => Ok(entry),
        (Some(_), Some(_)) => Err(io::Error::other(format!(
            "More than one playback stream is named {selector:?}"
        ))
        .into()),
        (None, _) => Err(io::Error::other("This media stream is no longer available").into()),
    }
}
