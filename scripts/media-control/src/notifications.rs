//! Wayle's notification count, shared with the media label in Waybar.
//!
//! The locally patched `wayle notify status --watch` prints one JSON line per
//! snapshot and flushes it. Following that stream costs one long-lived child
//! rather than a new D-Bus round trip and process on every Waybar tick.

use std::env;
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crate::mpris::executable;

/// How long to wait before following a daemon that went away again.
const RETRY_DELAY: Duration = Duration::from_secs(3);

/// What the bar button needs to know about the notification daemon.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Notifications {
    pub(crate) count: u32,
    pub(crate) dnd: bool,
}

/// A live view of the daemon's state.
pub(crate) struct Subscription {
    state: Arc<Mutex<Notifications>>,
}

impl Subscription {
    /// Start following the daemon. Returns immediately: until the first line
    /// arrives the bar shows a quiet bell, which is also what it shows when
    /// there is genuinely nothing waiting.
    pub(crate) fn start() -> Self {
        let state = Arc::new(Mutex::new(Notifications::default()));
        let worker = Arc::clone(&state);
        thread::spawn(move || follow(&worker));
        Subscription { state }
    }

    pub(crate) fn get(&self) -> Notifications {
        *self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

fn store(state: &Mutex<Notifications>, value: Notifications) {
    *state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = value;
}

/// Read the subscription until it ends, then start it again.
///
/// Follow Wayle until it restarts, then reconnect.
fn follow(state: &Mutex<Notifications>) {
    loop {
        let Some(mut child) = spawn_client() else {
            return;
        };
        if let Some(stdout) = child.stdout.take() {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if let Some(update) = parse(&line) {
                    store(state, update);
                }
            }
        }
        let _ = child.wait();

        // The daemon restarted or exited; its count is no longer known.
        store(state, Notifications::default());
        thread::sleep(RETRY_DELAY);
    }
}

fn spawn_client() -> Option<Child> {
    let client = executable("MEDIA_CONTROL_WAYLE", "wayle");

    // Run the subscriber behind the repository's parent-death guard, the same
    // way Waybar runs this program, so it goes when this process does.
    let mut command = match env::var("MEDIA_CONTROL_WITH_PARENT_DEATH")
        .ok()
        .filter(|guard| !guard.is_empty())
    {
        Some(guard) => {
            let mut command = Command::new(guard);
            command.arg(&client);
            command
        }
        None => Command::new(&client),
    };

    command
        .args(["notify", "status", "--watch"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()
}

/// Read one `wayle notify status --watch` line.
///
/// The line is small and its shape fixed — `{ "count": 3, "dnd": false }` —
/// so it is scanned directly rather than through a JSON dependency this
/// program does not otherwise need.
pub(crate) fn parse(line: &str) -> Option<Notifications> {
    Some(Notifications {
        count: number_after(line, "\"count\"")?,
        dnd: flag_after(line, "\"dnd\"").unwrap_or(false),
    })
}

fn value_after<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let rest = line.split_once(key)?.1.trim_start();
    Some(rest.strip_prefix(':')?.trim_start())
}

fn number_after(line: &str, key: &str) -> Option<u32> {
    value_after(line, key)?
        .chars()
        .take_while(|character| character.is_ascii_digit())
        .collect::<String>()
        .parse()
        .ok()
}

fn flag_after(line: &str, key: &str) -> Option<bool> {
    let value = value_after(line, key)?;
    if value.starts_with("true") {
        Some(true)
    } else if value.starts_with("false") {
        Some(false)
    } else {
        None
    }
}
