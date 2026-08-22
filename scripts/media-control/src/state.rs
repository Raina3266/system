use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::model::PlayerState;
use crate::text::{hex_decode, hex_encode};

fn state_path() -> Option<PathBuf> {
    env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state")))
        .map(|root| root.join("media-control/state.tsv"))
}

pub(crate) fn read_state() -> HashMap<String, PlayerState> {
    let Some(path) = state_path() else {
        return HashMap::new();
    };
    let Ok(contents) = fs::read_to_string(path) else {
        return HashMap::new();
    };

    contents
        .lines()
        .filter_map(|line| {
            let mut fields = line.split('\t');
            let id = hex_decode(fields.next()?)?;
            let pinned = fields.next()? == "1";
            let was_playing = fields.next()? == "1";
            let activity = fields.next()?.parse::<u128>().ok()?;
            Some((
                id,
                PlayerState {
                    pinned,
                    was_playing,
                    activity,
                },
            ))
        })
        .collect()
}

pub(crate) fn write_state(state: &HashMap<String, PlayerState>) {
    let Some(path) = state_path() else {
        return;
    };
    let Some(parent) = path.parent() else {
        return;
    };
    if fs::create_dir_all(parent).is_err() {
        return;
    }

    let mut entries: Vec<_> = state.iter().collect();
    entries.sort_by(|left, right| right.1.activity.cmp(&left.1.activity));
    entries.truncate(128);

    let mut contents = String::new();
    for (id, value) in entries {
        contents.push_str(&format!(
            "{}\t{}\t{}\t{}\n",
            hex_encode(id),
            u8::from(value.pinned),
            u8::from(value.was_playing),
            value.activity
        ));
    }

    let temporary = path.with_extension("tmp");
    if fs::write(&temporary, contents).is_ok() {
        let _ = fs::rename(temporary, path);
    }
}

pub(crate) fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}
