//! Clipboard capture and history policy. Only GTK/GDK publishes clipboard data.

// The explicit integration target owns all tests; keep binary test builds valid.
#[cfg(test)]
macro_rules! clipboard_unit_tests {
    ($module:ident) => {};
}
#[cfg(test)]
pub(crate) use clipboard_unit_tests;

mod capture;
mod store;

use anyhow::{Context, Result, bail, ensure};
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
        Request::Prepare {} => store.prepare(),
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

fn run_cli(args: &[String]) -> Result<()> {
    match args {
        [command] if command == "capture" => capture::capture_clipboard(),
        [command, flag, mime] if command == "store" && flag == "--mime" => {
            ensure!(
                !mime.is_empty() && !mime.contains(['\n', '\r', '\0']),
                "Invalid MIME type"
            );
            capture::store_stdin(mime)
        }
        _ => bail!(
            "Usage: clipboard-custom [capture | store --mime MIME]; without arguments, read one JSON request"
        ),
    }
}

fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if !args.is_empty() {
        if let Err(error) = run_cli(&args) {
            // Never echo clipboard bytes, MIME lists, or subprocess stderr.
            eprintln!("clipboard-custom: {error}");
            std::process::exit(1);
        }
        return;
    }
    // Preserve the no-argument JSON request/reply protocol, including errors.
    let reply = match run() {
        Ok(reply) => reply,
        Err(error) => json!({ "error": error.to_string() }),
    };
    println!("{reply}");
}
