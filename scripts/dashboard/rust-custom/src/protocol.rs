//! JSONL request and native notification schemas and event emission.

use std::io;
#[cfg(not(test))]
use std::io::Write;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[cfg(test)]
thread_local! {pub(crate) static EVENTS: std::cell::RefCell<Vec<Value>>=const{std::cell::RefCell::new(Vec::new())};}
#[cfg(test)]
pub(crate) fn emit(v: Value) -> io::Result<()> {
    EVENTS.with(|e| e.borrow_mut().push(v));
    Ok(())
}
#[cfg(not(test))]
pub(crate) fn emit(v: Value) -> io::Result<()> {
    writeln!(io::stdout().lock(), "{v}")
}
#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "camelCase", deny_unknown_fields)]
pub(crate) enum Request {
    Notify { command: Notice },
    Active { active: bool, generation: u64 },
    Timeout { minutes: u16 },
}
#[derive(Serialize, Deserialize)]
struct NativeAction {
    identifier: String,
    text: String,
}
#[derive(Serialize, Deserialize)]
pub(crate) struct Snapshot {
    app: String,
    summary: String,
    body: String,
    icon: String,
    urgency: u8,
    transient: bool,
    timeout: i32,
    actions: Vec<NativeAction>,
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub(crate) enum Notice {
    Init {
        dnd: bool,
        user: bool,
    },
    Received {
        native: u32,
        time: u64,
        token: String,
        row: Snapshot,
        restore: bool,
    },
    Resolved {
        token: String,
        reason: u32,
    },
    Dnd {
        value: bool,
    },
    SeedAck {
        value: bool,
    },
    Toggle,
    Hover {
        id: String,
        token: String,
        value: bool,
    },
    Dismiss {
        id: String,
        token: String,
    },
    Action {
        id: String,
        token: String,
        identifier: String,
    },
    Clear {
        #[serde(default)]
        instances: Vec<Instance>,
    },
}
#[derive(Deserialize)]
pub(crate) struct Instance {
    pub(crate) native: u32,
    pub(crate) token: String,
}
