//! Bounded newline-delimited JSON transport for the panel's native executor.
//!
//! `policy` accepts one {command, kind, key?, snapshot} request. Results and
//! snapshot/action RPCs are the only stdout records. Action acknowledgements are
//! native convergence acknowledgements, except routes, verified from raw Route.

use std::{
    io::{self, BufRead, BufReader, Read, Write},
    sync::mpsc,
    thread,
    time::Duration,
};

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};

use crate::{
    AppResult,
    audio::{self, Action, Backend, State},
    model::{AudioKind, Snapshot, validate_text},
};

pub(crate) const MAX_LINE_BYTES: usize = 1024 * 1024;
const REPLY_TIMEOUT: Duration = Duration::from_secs(6);
const MAX_RPCS: u64 = 256;

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum Command {
    Rows,
    Select,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    command: Command,
    kind: AudioKind,
    key: Option<String>,
    snapshot: Snapshot,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SnapshotReply {
    id: u64,
    snapshot: Snapshot,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ActionReply {
    id: u64,
    ok: bool,
    error: Option<String>,
}

struct Connection<W> {
    input: mpsc::Receiver<AppResult<Vec<u8>>>,
    output: W,
    next_id: u64,
    kind: AudioKind,
}

pub fn run() -> AppResult<()> {
    serve(BufReader::new(io::stdin()), io::stdout())
}

pub fn serve<R: BufRead + Send + 'static, W: Write>(mut input: R, output: W) -> AppResult<()> {
    let (sender, receiver) = mpsc::sync_channel(1);
    thread::spawn(move || {
        loop {
            let line = read_line(&mut input);
            let failed = line.is_err();
            if sender.send(line).is_err() || failed {
                break;
            }
        }
    });
    let mut connection = Connection {
        input: receiver,
        output,
        next_id: 1,
        kind: AudioKind::Output,
    };
    let result = (|| -> AppResult<Value> {
        let request: Request = connection.read()?;
        match request.command {
            Command::Rows if request.key.is_some() => {
                return Err("Rows requests must not include a selection key".into());
            }
            Command::Select => {
                let key = request
                    .key
                    .as_deref()
                    .ok_or("Select request key is missing")?;
                if key.is_empty() || key.len() > 65536 || key.contains('\0') {
                    return Err("Select request key is invalid or oversized".into());
                }
            }
            Command::Rows => {}
        }
        connection.kind = request.kind;
        let initial = State::load(request.snapshot, request.kind)?;
        match request.command {
            Command::Rows => {
                Ok(json!({"type": "result", "rows": audio::rows(&initial, request.kind)?}))
            }
            Command::Select => {
                audio::select(
                    &mut connection,
                    &initial,
                    request.kind,
                    request.key.as_deref().ok_or("Missing selection key")?,
                )?;
                Ok(json!({"type": "result", "ok": true}))
            }
        }
    })();
    let result = match result {
        Ok(value) => value,
        Err(error) => json!({"type": "result", "ok": false, "error": error.to_string()}),
    };
    if serde_json::to_vec(&result)?.len() >= MAX_LINE_BYTES {
        connection
            .write(&json!({"type":"result", "ok":false, "error":"Policy response exceeds 1 MiB"}))
    } else {
        connection.write(&result)
    }
}

pub(crate) fn read_line(reader: &mut impl BufRead) -> AppResult<Vec<u8>> {
    let mut bytes = Vec::new();
    reader
        .take((MAX_LINE_BYTES + 1) as u64)
        .read_until(b'\n', &mut bytes)?;
    if bytes.len() > MAX_LINE_BYTES {
        return Err("Policy JSON line exceeds 1 MiB".into());
    }
    if !bytes.ends_with(b"\n") {
        return Err("Policy input closed before a complete JSON line".into());
    }
    Ok(bytes)
}

impl<W: Write> Connection<W> {
    fn read<T: DeserializeOwned>(&self) -> AppResult<T> {
        let line = self
            .input
            .recv_timeout(REPLY_TIMEOUT)
            .map_err(|_| io::Error::other("Policy reply timed out or input closed"))??;
        Ok(serde_json::from_slice(&line)?)
    }

    fn write(&mut self, value: &impl Serialize) -> AppResult<()> {
        let bytes = serde_json::to_vec(value)?;
        if bytes.len() >= MAX_LINE_BYTES {
            return Err("Policy response exceeds 1 MiB".into());
        }
        self.output.write_all(&bytes)?;
        self.output.write_all(b"\n")?;
        self.output.flush()?;
        Ok(())
    }

    fn id(&mut self) -> AppResult<u64> {
        if self.next_id > MAX_RPCS {
            return Err("Policy transaction exceeded its RPC limit".into());
        }
        let id = self.next_id;
        self.next_id += 1;
        Ok(id)
    }
}

impl<W: Write> Backend for Connection<W> {
    fn observe(&mut self) -> AppResult<State> {
        let id = self.id()?;
        self.write(&json!({"type": "snapshot", "id": id}))?;
        let reply: SnapshotReply = self.read()?;
        if reply.id != id {
            return Err("Snapshot reply has the wrong RPC id".into());
        }
        State::load(reply.snapshot, self.kind)
    }

    fn action(&mut self, action: Action) -> AppResult<()> {
        let id = self.id()?;
        let mut event = serde_json::to_value(action)?;
        let event_object = event.as_object_mut().ok_or("Invalid policy action")?;
        event_object.insert("type".into(), "action".into());
        event_object.insert("id".into(), id.into());
        self.write(&event)?;
        let reply: ActionReply = self.read()?;
        if reply.id != id {
            return Err("Action reply has the wrong RPC id".into());
        }
        match (reply.ok, reply.error) {
            (true, None) => Ok(()),
            (false, Some(error)) => {
                validate_text(&error)?;
                Err(error.into())
            }
            _ => Err("Invalid action acknowledgement".into()),
        }
    }
}
