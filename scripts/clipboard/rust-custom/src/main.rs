//! History policy only. GTK/GDK owns clipboard publication; the existing
//! rofi-clipboard collector owns background Wayland capture.
mod store;

use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::{Value, json};
use std::io::{self, Read};
use store::Store;

#[derive(Debug, Deserialize)]
#[serde(tag = "command", rename_all = "kebab-case", deny_unknown_fields)]
enum Request {
    Prepare {},
    Snapshot {},
    Memo {},
    Pin { id: u64 },
    Delete { id: u64 },
    Save { id: u64, base: String, text: String },
    Payload { id: u64 },
}

fn execute(store: &Store, request: Request) -> Result<Value> {
    match request {
        Request::Prepare {} => {
            store.prepare()?;
            store.snapshot()
        }
        Request::Snapshot {} => store.snapshot(),
        Request::Memo {} => store.memo(),
        Request::Pin { id } => store.pin(id),
        Request::Delete { id } => store.delete(id),
        Request::Save { id, base, text } => store.save(id, &base, text),
        Request::Payload { id } => store.payload(id),
    }
}

fn run() -> Result<Value> {
    let mut input = String::new();
    io::stdin()
        .take(64 * 1024 * 1024 + 1)
        .read_to_string(&mut input)?;
    anyhow::ensure!(input.len() <= 64 * 1024 * 1024, "Request is too large");
    execute(
        &Store::discover()?,
        serde_json::from_str(&input).context("Invalid history request")?,
    )
}

fn main() {
    // Errors are structured too; never print clipboard contents to the journal.
    let reply = match run() {
        Ok(reply) => reply,
        Err(error) => json!({ "error": error.to_string() }),
    };
    println!("{reply}");
}
