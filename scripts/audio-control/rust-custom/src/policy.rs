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

/// Resident protocol v1. JS publishes raw object snapshots and UI intents. Rust
/// publishes complete models and individually guarded native calls. There is no
/// JS policy fallback and no native mutation implementation in this process.
pub mod panel {
    use crate::{
        AppResult,
        audio::{self, Action, Backend, SelectionPlan, State},
        bluetooth,
        model::{AudioKind, Choice, Identity, NativeSnapshot, object_guard, object_key},
    };
    use serde::Deserialize;
    use serde_json::{Value, json};
    use std::{
        collections::{HashMap, HashSet, VecDeque},
        io::{self, BufRead, BufReader, Write},
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
            mpsc,
        },
        thread,
        time::{Duration, Instant},
    };

    const STEP: Duration = Duration::from_millis(100);
    const NATIVE_ATTEMPTS: usize = 30;

    pub trait Native {
        fn snapshot(&mut self) -> AppResult<NativeSnapshot>;
        fn execute(
            &mut self,
            operation: &str,
            target: &str,
            args: Value,
            guards: Vec<Value>,
            cleanup: bool,
        ) -> AppResult<NativeSnapshot>;
        fn pause(&mut self) {
            thread::sleep(STEP);
        }
    }

    fn on_drop(operation: &str, object: &Value, fields: &[&str]) -> Value {
        let mut guard = object_guard(object, fields);
        guard["held"] = true.into();
        json!({"type":"action","id":1,"operation":operation,"target":object["key"],"args":{},"guards":[guard],"cleanup":true})
    }

    pub fn call(
        native: &mut impl Native,
        operation: &str,
        object: &Value,
        args: Value,
        mut guards: Vec<Value>,
    ) -> AppResult<NativeSnapshot> {
        guards.push(object_guard(
            object,
            &["type", "id", "serial", "address", "adapter", "path"],
        ));
        native.execute(operation, object_key(object), args, guards, false)
    }

    fn wait(
        native: &mut impl Native,
        mut check: impl FnMut(&NativeSnapshot) -> AppResult<bool>,
    ) -> AppResult<NativeSnapshot> {
        for _ in 0..NATIVE_ATTEMPTS {
            native.pause();
            let snapshot = native.snapshot()?;
            if check(&snapshot)? {
                return Ok(snapshot);
            }
        }
        Err("The audio device did not accept the change.".into())
    }

    fn live<'a>(snapshot: &'a NativeSnapshot, original: &Value) -> AppResult<&'a Value> {
        let current = snapshot.object(object_key(original))?;
        if ["type", "id", "serial", "address", "adapter", "path"]
            .iter()
            .any(|field| current[*field] != original[*field])
        {
            return Err("The native object disappeared or was replaced".into());
        }
        Ok(current)
    }

    pub fn native_select(
        native: &mut impl Native,
        initial: &NativeSnapshot,
        kind: AudioKind,
        target: &Choice,
    ) -> AppResult<Value> {
        let SelectionPlan::Native { node, route } =
            audio::selection_plan(&initial.audio()?, target)?
        else {
            return Err("Selection requires supplemental policy".into());
        };
        let original = initial.endpoint(&node)?.clone();
        let snapshot = native.snapshot()?;
        let current = live(&snapshot, &original)?;
        if let Some(route) = route {
            let routes = current["routes"]
                .as_array()
                .ok_or("Missing native routes")?;
            if routes
                .iter()
                .filter(|item| {
                    item["index"] == route
                        && item["name"] == target.port_name
                        && item["available"] == true
                })
                .count()
                != 1
            {
                return Err("The selected audio port is no longer available".into());
            }
            call(
                native,
                "route",
                current,
                json!({"route":route}),
                vec![object_guard(current, &["routes", "cardId", "cardSerial"])],
            )?;
            wait(native, |snapshot| {
                Ok(live(snapshot, &original)?["routeName"] == target.port_name)
            })?;
        }
        let snapshot = native.snapshot()?;
        let current = live(&snapshot, &original)?;
        call(native, "default", current, json!({}), vec![])?;
        wait(native, |snapshot| {
            let current = live(snapshot, &original)?;
            Ok(current["isDefault"] == true && snapshot.default_key(kind) == object_key(&original))
        })?;
        Ok(original)
    }

    pub fn follow_output(native: &mut impl Native, endpoint: &Value) -> AppResult<()> {
        fn valid(snapshot: &NativeSnapshot, endpoint: &Value) -> AppResult<()> {
            live(snapshot, endpoint)?;
            if snapshot.default_output != object_key(endpoint) {
                return Err(
                    "The selected output is no longer the default; it changed elsewhere.".into(),
                );
            }
            Ok(())
        }
        let snapshot = native.snapshot()?;
        valid(&snapshot, endpoint)?;
        let streams: Vec<_> = snapshot.objects("stream").cloned().collect();
        let mut pending = Vec::new();
        let mut failures = Vec::new();
        for stream in streams {
            let snapshot = native.snapshot()?;
            valid(&snapshot, endpoint)?;
            if snapshot.object(object_key(&stream)).is_err() {
                continue;
            }
            let guards = vec![
                snapshot.root_guard(&["defaultOutput"]),
                object_guard(endpoint, &["type", "id", "serial"]),
            ];
            match call(native, "follow", &stream, json!({}), guards) {
                Ok(_) => pending.push(stream),
                Err(error) => failures.push(format!(
                    "{}: {error}",
                    stream["name"]
                        .as_str()
                        .filter(|name| !name.is_empty())
                        .unwrap_or("Playback")
                )),
            }
        }
        if !pending.is_empty() {
            wait(native, |snapshot| {
                valid(snapshot, endpoint)?;
                Ok(pending.iter().all(|stream| {
                    snapshot
                        .object(object_key(stream))
                        .ok()
                        .is_none_or(|current| current["targetSerial"] == -1)
                }))
            })
            .map_err(|error| {
                format!("Output changed, but playback could not follow it: {error}")
            })?;
        }
        if !failures.is_empty() {
            return Err(format!(
                "Output changed, but playback could not follow it: {}",
                failures.join("; ")
            )
            .into());
        }
        Ok(())
    }

    pub fn volume_action(
        snapshot: &NativeSnapshot,
        kind: AudioKind,
        key: &str,
        value: Option<f64>,
    ) -> AppResult<(Value, Vec<Value>)> {
        if key.is_empty() || snapshot.epoch(kind) != key {
            return Err("The default audio device changed. Try again.".into());
        }
        let node = snapshot.object(snapshot.default_key(kind))?.clone();
        let args = if let Some(value) = value {
            if !value.is_finite() {
                return Err("Invalid audio volume.".into());
            }
            json!({"volume":value.clamp(0.0, if kind == AudioKind::Output {1.5} else {1.0})})
        } else {
            json!({"muted":node["muted"] != true})
        };
        let fields = if kind == AudioKind::Output {
            ["defaultOutput", "outputEpoch"]
        } else {
            ["defaultInput", "inputEpoch"]
        };
        Ok((
            json!({"node":node,"args":args}),
            vec![snapshot.root_guard(&fields)],
        ))
    }

    pub fn pair_guards(
        snapshot: &NativeSnapshot,
        device: &Value,
        initial: &NativeSnapshot,
    ) -> AppResult<Vec<Value>> {
        let current = live(snapshot, device)?;
        let adapter = snapshot.object(&snapshot.adapter)?;
        if !snapshot.opened
            || snapshot.tab != 0
            || snapshot.adapter != initial.adapter
            || snapshot.pairing_epoch != initial.pairing_epoch
            || adapter["powered"] != true
            || current["adapter"] != adapter["path"]
        {
            return Err("Bluetooth pairing was cancelled or its native device changed".into());
        }
        Ok(vec![
            snapshot.root_guard(&["adapter", "opened", "tab", "pairingEpoch"]),
            object_guard(adapter, &["powered", "path"]),
        ])
    }

    pub fn paired_setup(
        native: &mut impl Native,
        initial: &NativeSnapshot,
        device: &Value,
    ) -> AppResult<()> {
        let snapshot = native.snapshot()?;
        let guards = pair_guards(&snapshot, device, initial)?;
        call(native, "trust", device, json!({"trusted":true}), guards)?;
        let snapshot = native.snapshot()?;
        let guards = pair_guards(&snapshot, device, initial)?;
        call(native, "connect", device, json!({}), guards)?;
        Ok(())
    }

    #[derive(Deserialize)]
    #[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
    enum Input {
        Snapshot {
            snapshot: NativeSnapshot,
        },
        Intent {
            command: String,
            kind: Option<AudioKind>,
            key: Option<String>,
            value: Option<f64>,
            answer: Option<String>,
        },
        Reply {
            id: u64,
            ok: bool,
            error: Option<String>,
            snapshot: NativeSnapshot,
        },
        Shutdown,
    }

    enum Event {
        Input(AppResult<Input>),
        Rpc {
            event: Value,
            reply: mpsc::SyncSender<AppResult<Value>>,
            cancelled: Arc<AtomicBool>,
        },
        Rows {
            kind: AudioKind,
            revision: u64,
            resolved: Vec<Identity>,
            result: AppResult<Vec<Choice>>,
        },
        Done {
            slot: String,
            result: AppResult<()>,
        },
        Control {
            owned: Option<Value>,
            detached: Option<Value>,
            signature: Value,
            result: AppResult<()>,
        },
        Prompt {
            id: u64,
            prompt: bluetooth::AgentPrompt,
        },
        Paired {
            id: u64,
        },
        PairDone {
            id: u64,
            result: AppResult<()>,
        },
    }

    #[derive(Clone)]
    struct Bridge {
        sender: mpsc::Sender<Event>,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    }
    impl Bridge {
        fn rpc(&self, event: Value) -> AppResult<Value> {
            if self.cancelled.load(Ordering::Relaxed) || Instant::now() >= self.deadline {
                return Err("Audio operation cancelled or timed out".into());
            }
            let (sender, receiver) = mpsc::sync_channel(1);
            let asynchronous_bluetooth =
                matches!(event["operation"].as_str(), Some("connect" | "disconnect"));
            self.sender.send(Event::Rpc {
                event,
                reply: sender,
                cancelled: self.cancelled.clone(),
            })?;
            // Astal's asynchronous BlueZ calls have their own native completion;
            // do not turn a slow connection into a global six-second failure.
            if asynchronous_bluetooth {
                receiver
                    .recv()
                    .map_err(|_| "Native Astal connection closed")?
            } else {
                receiver
                    .recv_timeout(Duration::from_secs(6))
                    .map_err(|_| "Native Astal reply timed out")?
            }
        }
    }
    impl Native for Bridge {
        fn snapshot(&mut self) -> AppResult<NativeSnapshot> {
            Ok(serde_json::from_value(
                self.rpc(json!({"type":"snapshot"}))?,
            )?)
        }
        fn execute(
            &mut self,
            operation: &str,
            target: &str,
            args: Value,
            guards: Vec<Value>,
            cleanup: bool,
        ) -> AppResult<NativeSnapshot> {
            Ok(serde_json::from_value(self.rpc(json!({"type":"action", "operation":operation,"target":target,"args":args,"guards":guards,"cleanup":cleanup}))?)?)
        }
    }

    struct AudioBridge {
        native: Bridge,
        kind: AudioKind,
        last: NativeSnapshot,
        bindings: HashMap<String, String>,
        selected: Option<Value>,
    }
    impl AudioBridge {
        fn check_bindings(&mut self, snapshot: &NativeSnapshot) -> AppResult<()> {
            for object in snapshot
                .objects
                .iter()
                .filter(|object| matches!(object["type"].as_str(), Some("node" | "card")))
            {
                let identity = format!("{}:{}", object["id"], object["serial"]);
                let key = object_key(object).to_owned();
                if self
                    .bindings
                    .get(&identity)
                    .is_some_and(|previous| *previous != key)
                {
                    return Err("The native audio object disappeared or was replaced".into());
                }
                self.bindings.insert(identity, key);
            }
            Ok(())
        }
    }
    impl Backend for AudioBridge {
        fn observe(&mut self) -> AppResult<State> {
            let snapshot = self.native.snapshot()?;
            self.check_bindings(&snapshot)?;
            self.last = snapshot;
            State::load(self.last.audio()?, self.kind)
        }
        fn action(&mut self, action: Action) -> AppResult<()> {
            let (operation, object, args) = match &action {
                Action::Profile { card, profile } => (
                    "profile",
                    self.last
                        .objects("card")
                        .find(|object| object["id"] == card.id && object["serial"] == card.serial)
                        .ok_or("The sound card disappeared")?
                        .clone(),
                    json!({"profile":profile}),
                ),
                Action::Route { node, route } => (
                    "route",
                    self.last.endpoint(node)?.clone(),
                    json!({"route":route}),
                ),
                Action::Default { node } => {
                    ("default", self.last.endpoint(node)?.clone(), json!({}))
                }
            };
            let mut guards = vec![object_guard(
                &object,
                &[
                    "activeProfile",
                    "profiles",
                    "routes",
                    "cardId",
                    "cardSerial",
                    "profileDevice",
                ],
            )];
            if operation == "default" {
                guards.push(self.last.root_guard(match self.kind {
                    AudioKind::Output => &["defaultOutput", "outputEpoch"],
                    AudioKind::Input => &["defaultInput", "inputEpoch"],
                }));
            }
            if let Some(card) = self.last.objects("card").find(|card| {
                object["cardId"] == card["id"] && object["cardSerial"] == card["serial"]
            }) {
                guards.push(object_guard(
                    card,
                    &["type", "id", "serial", "activeProfile"],
                ));
            }
            call(&mut self.native, operation, &object, args.clone(), guards)?;
            if operation == "profile" {
                wait(&mut self.native, |snapshot| {
                    Ok(live(snapshot, &object)?["activeProfile"] == args["profile"])
                })?;
            } else if operation == "default" {
                wait(&mut self.native, |snapshot| {
                    let current = live(snapshot, &object)?;
                    Ok(current["isDefault"] == true
                        && snapshot.default_key(self.kind) == object_key(&object))
                })?;
                self.selected = Some(object);
            }
            Ok(())
        }
    }

    #[derive(Default)]
    struct AudioSlot {
        entries: Vec<Choice>,
        resolved: Vec<Identity>,
        revision: u64,
        loading: bool,
        busy: bool,
        dirty: bool,
        error: String,
    }
    struct Pairing {
        id: u64,
        device: Value,
        initial: NativeSnapshot,
        cancel: Arc<AtomicBool>,
        cleanup_done: Arc<AtomicBool>,
        answer: Option<tokio::sync::oneshot::Sender<String>>,
        agent: bool,
    }
    struct Pending {
        reply: mpsc::SyncSender<AppResult<Value>>,
        deadline: Option<Instant>,
        cancelled: Arc<AtomicBool>,
        cleanup: bool,
    }
    type Work = Box<dyn FnOnce(&mut Bridge) -> AppResult<()> + Send>;

    #[derive(Default)]
    pub struct Panel {
        native: NativeSnapshot,
        slots: [AudioSlot; 2],
        refresh_due: Option<Instant>,
        poll_due: Option<Instant>,
        owned: Option<Value>,
        detached: Option<Value>,
        control_busy: bool,
        power: Option<(Value, bool)>,
        powering_off: String,
        busy: HashSet<String>,
        pairing: Option<Pairing>,
        notice: Option<bluetooth::PairingNotice>,
        bluetooth_error: String,
        serial: u64,
        closing: bool,
        queues: HashMap<String, VecDeque<Work>>,
        running: HashMap<String, Arc<AtomicBool>>,
        pair_keys: HashMap<u64, String>,
    }

    fn index(kind: AudioKind) -> usize {
        if kind == AudioKind::Output { 0 } else { 1 }
    }
    const KINDS: [AudioKind; 2] = [AudioKind::Output, AudioKind::Input];

    impl Panel {
        pub fn model(&self) -> AppResult<Value> {
            let audio = |kind| -> AppResult<Value> {
                let slot = &self.slots[index(kind)];
                let node = self.native.object(self.native.default_key(kind)).ok();
                let choices = audio::choices(&self.native, kind, &slot.entries, &slot.resolved)?;
                let name = node
                    .map(|node| {
                        ["nick", "description", "name"]
                            .into_iter()
                            .filter_map(|field| node[field].as_str())
                            .find(|value| !value.is_empty())
                            .unwrap_or("No device")
                    })
                    .unwrap_or("No device");
                let status = choices
                    .iter()
                    .find(|choice| choice["default"] == true)
                    .and_then(|choice| choice["label"].as_str())
                    .or_else(|| node.and_then(|node| node["description"].as_str()))
                    .unwrap_or_default();
                Ok(
                    json!({"nodeKey":self.native.epoch(kind),"available":node.is_some(),"name":name,"statusName":status,
                    "volume":node.and_then(|node| node["volume"].as_f64()).unwrap_or(0.0),"muted":node.is_some_and(|node| node["muted"] == true),
                    "choices":choices,"busy":slot.busy,"loading":slot.loading,"error":slot.error}),
                )
            };
            let adapter = self.native.object(&self.native.adapter).ok();
            Ok(
                json!({"output":audio(AudioKind::Output)?,"input":audio(AudioKind::Input)?,"bluetooth":{
                "available":adapter.is_some(),"powered":adapter.is_some_and(|adapter| adapter["powered"] == true),
                "devices":bluetooth::device_rows(&self.native, &self.busy),"pairing":self.pairing.as_ref().is_some_and(|pairing| pairing.agent),
                "pairingMessage":self.notice.as_ref().filter(|notice| !notice.dismissed),"error":self.bluetooth_error}}),
            )
        }

        fn bridge(sender: &mpsc::Sender<Event>, cancel: Arc<AtomicBool>, seconds: u64) -> Bridge {
            Bridge {
                sender: sender.clone(),
                cancelled: cancel,
                deadline: Instant::now() + Duration::from_secs(seconds),
            }
        }

        fn accept(
            &mut self,
            snapshot: NativeSnapshot,
            sender: &mpsc::Sender<Event>,
        ) -> AppResult<()> {
            snapshot.validate()?;
            let old_audio = serde_json::to_value(self.native.audio()?)?;
            let bindings = |native: &NativeSnapshot| {
                native
                    .objects
                    .iter()
                    .filter(|object| matches!(object["type"].as_str(), Some("node" | "card")))
                    .map(|object| object["key"].clone())
                    .collect::<Vec<_>>()
            };
            let changed = old_audio != serde_json::to_value(snapshot.audio()?)?
                || bindings(&self.native) != bindings(&snapshot);
            for slot in &mut self.slots {
                slot.resolved.retain(|identity| {
                    self.native.endpoint(identity).ok().map(object_key)
                        == snapshot.endpoint(identity).ok().map(object_key)
                });
                slot.entries.retain(|row| {
                    let binding = row.node_id.map(|id| Identity {
                        id,
                        serial: row.node_serial.clone(),
                    });
                    binding.is_none_or(|identity| {
                        self.native.endpoint(&identity).ok().map(object_key)
                            == snapshot.endpoint(&identity).ok().map(object_key)
                    })
                });
                if changed {
                    slot.revision += 1;
                }
            }
            let active = snapshot.opened && snapshot.tab == 1;
            let was_active = self.native.opened && self.native.tab == 1;
            self.native = snapshot;
            if changed && self.refresh_due.is_none() {
                self.refresh_due = Some(Instant::now() + Duration::from_millis(150));
            }
            if active && !was_active {
                self.poll_due = Some(Instant::now() + Duration::from_secs(2));
            } else if !active {
                self.poll_due = None;
            }
            if !self.native.opened
                || self.native.tab != 0
                || self
                    .native
                    .object(&self.native.adapter)
                    .ok()
                    .is_none_or(|adapter| adapter["powered"] != true)
            {
                self.powering_off.clear();
            }
            if self.pairing.as_ref().is_some_and(|pairing| {
                pair_guards(&self.native, &pairing.device, &pairing.initial).is_err()
            }) {
                self.cancel_pairing(sender);
            }
            Ok(())
        }

        fn refresh(&mut self, kind: AudioKind, sender: &mpsc::Sender<Event>) {
            let slot = &mut self.slots[index(kind)];
            if slot.loading || slot.busy {
                slot.dirty = true;
                return;
            }
            slot.loading = true;
            slot.dirty = false;
            let revision = slot.revision;
            let snapshot = self.native.audio();
            let sender = sender.clone();
            thread::spawn(move || {
                let resolved = snapshot
                    .as_ref()
                    .map(|snapshot| {
                        snapshot
                            .nodes
                            .iter()
                            .filter(|node| node.kind == kind)
                            .map(|node| node.identity())
                            .collect()
                    })
                    .unwrap_or_default();
                let result = snapshot
                    .and_then(|snapshot| State::load(snapshot, kind))
                    .and_then(|state| audio::rows(&state, kind));
                let _ = sender.send(Event::Rows {
                    kind,
                    revision,
                    resolved,
                    result,
                });
            });
        }

        fn task(
            &mut self,
            slot: String,
            sender: &mpsc::Sender<Event>,
            work: impl FnOnce(&mut Bridge) -> AppResult<()> + Send + 'static,
        ) {
            self.queues
                .entry(slot.clone())
                .or_default()
                .push_back(Box::new(work));
            self.next_task(&slot, sender);
        }

        fn next_task(&mut self, slot: &str, sender: &mpsc::Sender<Event>) {
            if self.running.contains_key(slot) || (self.closing && !slot.is_empty()) {
                return;
            }
            let Some(work) = self.queues.get_mut(slot).and_then(VecDeque::pop_front) else {
                return;
            };
            let cancel = Arc::new(AtomicBool::new(false));
            self.running.insert(slot.into(), cancel.clone());
            let sender = sender.clone();
            let slot = slot.to_owned();
            let mut native = Self::bridge(&sender, cancel, 45);
            thread::spawn(move || {
                let result = work(&mut native);
                let _ = sender.send(Event::Done { slot, result });
            });
        }

        fn select(
            &mut self,
            kind: AudioKind,
            key: &str,
            sender: &mpsc::Sender<Event>,
        ) -> AppResult<()> {
            let slot = &mut self.slots[index(kind)];
            if slot.busy {
                return Ok(());
            }
            let rows = audio::choices(&self.native, kind, &slot.entries, &slot.resolved)?;
            let mut target = rows
                .into_iter()
                .find(|row| row["key"] == key)
                .ok_or("The selected audio device is no longer available.")?;
            target
                .as_object_mut()
                .ok_or("Invalid selection")?
                .remove("default");
            let target: Choice = serde_json::from_value(target)?;
            let initial = self.native.clone();
            let plan = audio::selection_plan(&initial.audio()?, &target)?;
            slot.busy = true;
            slot.error.clear();
            slot.revision += 1;
            self.task(format!("select:{}", index(kind)), sender, move |native| {
                let endpoint = match plan {
                    SelectionPlan::Native { .. } => {
                        Some(native_select(native, &initial, kind, &target)?)
                    }
                    SelectionPlan::Policy => {
                        let mut backend = AudioBridge {
                            native: native.clone(),
                            kind,
                            last: initial.clone(),
                            bindings: HashMap::new(),
                            selected: None,
                        };
                        backend.check_bindings(&initial)?;
                        let state = State::load(initial.audio()?, kind)?;
                        audio::select(&mut backend, &state, kind, &target.key)?;
                        backend.selected
                    }
                };
                if kind == AudioKind::Output
                    && let Some(endpoint) = endpoint
                {
                    follow_output(native, &endpoint)?;
                }
                Ok(())
            });
            Ok(())
        }

        fn cancel_pairing(&mut self, sender: &mpsc::Sender<Event>) {
            let Some(pairing) = self.pairing.take() else {
                return;
            };
            pairing.cancel.store(true, Ordering::Relaxed);
            self.notice = None;
            let device = pairing.device;
            let done = pairing.cleanup_done;
            self.task(String::new(), sender, move |native| {
                let _ = native.execute(
                    "cancelPairing",
                    object_key(&device),
                    json!({}),
                    vec![json!({"key":device["key"],"held":true,"expect":{"type":"device","address":device["address"],"adapter":device["adapter"]}})],
                    true,
                );
                done.store(true, Ordering::Release);
                Ok(())
            });
        }

        fn pair(&mut self, device: Value, sender: &mpsc::Sender<Event>) -> AppResult<()> {
            let initial = self.native.clone();
            pair_guards(&initial, &device, &initial)?;
            let path = initial.object(&initial.adapter)?["path"]
                .as_str()
                .ok_or("The native Bluetooth adapter is no longer available.")?;
            let adapter = path
                .strip_prefix("/org/bluez/")
                .ok_or("The native Bluetooth adapter is no longer available.")?
                .to_owned();
            let address = device["address"]
                .as_str()
                .ok_or("Missing Bluetooth address")?
                .to_owned();
            bluetooth::pairing_target(&adapter, &address)?;
            self.serial += 1;
            let id = self.serial;
            let cancel = Arc::new(AtomicBool::new(false));
            let cleanup_done = Arc::new(AtomicBool::new(false));
            self.pairing = Some(Pairing {
                id,
                device: device.clone(),
                initial: initial.clone(),
                cancel: cancel.clone(),
                cleanup_done: cleanup_done.clone(),
                answer: None,
                agent: true,
            });
            self.notice = Some(bluetooth::PairingNotice::new(&device));
            self.busy.insert(object_key(&device).into());
            self.pair_keys.insert(id, object_key(&device).into());
            let sender = sender.clone();
            let runtime = tokio::runtime::Handle::current();
            thread::spawn(move || {
                let mut native = Self::bridge(&sender, cancel.clone(), 180);
                let result = (|| -> AppResult<()> {
                    let guards = pair_guards(&initial, &device, &initial)?;
                    call(
                        &mut native,
                        "retain",
                        &device,
                        json!({"onDrop":[on_drop("cancelPairing", &device, &["type", "address", "adapter"])]}),
                        guards,
                    )?;
                    let latest = native.snapshot()?;
                    pair_guards(&latest, &device, &initial)?;
                    let events = sender.clone();
                    let send = Arc::new(move |prompt| {
                        let _ = events.send(Event::Prompt { id, prompt });
                    });
                    runtime.block_on(async {
                        tokio::select! {
                            biased;
                            () = async { while !cancel.load(Ordering::Relaxed) { tokio::time::sleep(Duration::from_millis(25)).await; } } => Err("Bluetooth pairing cancelled".into()),
                            result = tokio::time::timeout(Duration::from_secs(180), bluetooth::pair_interactive(&adapter, &address, send)) => result.map_err(|_| "Bluetooth pairing timed out")?,
                        }
                    })?;
                    sender.send(Event::Paired { id })?;
                    native.deadline = Instant::now() + Duration::from_secs(45);
                    paired_setup(&mut native, &initial, &device)
                })();
                // Release remains possible after cancellation; native async callbacks
                // are still finished by JS even when their transaction is obsolete.
                if cancel.load(Ordering::Relaxed) {
                    let deadline = Instant::now() + Duration::from_secs(6);
                    while !cleanup_done.load(Ordering::Acquire) && Instant::now() < deadline {
                        thread::sleep(Duration::from_millis(5));
                    }
                }
                let mut cleanup = Self::bridge(&sender, Arc::new(AtomicBool::new(false)), 6);
                let _ = cleanup.execute("release", object_key(&device), json!({}), vec![], true);
                let _ = sender.send(Event::PairDone { id, result });
            });
            Ok(())
        }

        fn signature(&self) -> Value {
            json!([
                self.native.adapter,
                self.native.object(&self.native.adapter).ok(),
                self.native.opened,
                self.native.tab,
                self.powering_off,
                self.closing
            ])
        }

        fn reconcile_discovery(&mut self, sender: &mpsc::Sender<Event>) {
            if self.control_busy {
                return;
            }
            let adapter = self.native.object(&self.native.adapter).ok().cloned();
            let desired = adapter.clone().filter(|adapter| {
                !self.closing
                    && self.native.opened
                    && self.native.tab == 0
                    && adapter["powered"] == true
                    && self.powering_off != object_key(adapter)
            });
            let power = self.power.take();
            if self.owned.as_ref().map(object_key) == desired.as_ref().map(object_key)
                && !(self.closing && self.detached.is_some())
                && power.is_none()
            {
                return;
            }
            self.control_busy = true;
            if desired.is_some() && self.owned.is_none() {
                self.bluetooth_error.clear();
            }
            let owned = self.owned.take();
            let mut detached = self.detached.take();
            let signature = self.signature();
            let snapshot = self.native.clone();
            let sender = sender.clone();
            let mut native = Self::bridge(&sender, Arc::new(AtomicBool::new(false)), 20);
            thread::spawn(move || {
                let mut retained = None;
                let result = (|| -> AppResult<()> {
                    let stop = |native: &mut Bridge, owned: &Value| {
                        native.execute(
                            "stopDiscovery",
                            object_key(owned),
                            json!({}),
                            vec![json!({"key":owned["key"],"held":true,"expect":{"type":"adapter","path":owned["path"]}})],
                            true,
                        )
                    };
                    // One displaced lease may outlive its inventory entry. Retry on
                    // the next control transition (including shutdown), then drop.
                    if let Some(old) = detached.take() {
                        let _ = stop(&mut native, &old);
                        native.execute("release", object_key(&old), json!({}), vec![], true)?;
                    }
                    if let Some(owned) = owned {
                        let stopped = stop(&mut native, &owned);
                        if stopped.is_err() && snapshot.adapter != object_key(&owned) {
                            detached = Some(owned);
                        } else if stopped.is_err()
                            && snapshot.adapter == object_key(&owned)
                            && adapter
                                .as_ref()
                                .is_some_and(|adapter| adapter["powered"] == true)
                        {
                            retained = Some(owned);
                            if power.is_none() {
                                stopped?;
                            }
                        } else {
                            let _ = native.execute(
                                "release",
                                object_key(&owned),
                                json!({}),
                                vec![],
                                true,
                            );
                            stopped?;
                        }
                    }
                    if let Some((adapter, powered)) = power {
                        call(
                            &mut native,
                            "power",
                            &adapter,
                            json!({"powered":powered}),
                            vec![snapshot.root_guard(&["adapter"])],
                        )?;
                    } else if let Some(adapter) = desired {
                        call(
                            &mut native,
                            "retain",
                            &adapter,
                            json!({"onDrop":[on_drop("stopDiscovery", &adapter, &["type", "path"])]}),
                            vec![
                                snapshot.root_guard(&["adapter", "opened", "tab"]),
                                object_guard(&adapter, &["powered"]),
                            ],
                        )?;
                        match call(
                            &mut native,
                            "startDiscovery",
                            &adapter,
                            json!({}),
                            vec![
                                snapshot.root_guard(&["adapter", "opened", "tab"]),
                                object_guard(&adapter, &["powered"]),
                            ],
                        ) {
                            Ok(_) => retained = Some(adapter),
                            Err(error) => {
                                let _ = native.execute(
                                    "release",
                                    object_key(&adapter),
                                    json!({}),
                                    vec![],
                                    true,
                                );
                                return Err(error);
                            }
                        }
                    }
                    Ok(())
                })();
                let _ = sender.send(Event::Control {
                    owned: retained,
                    detached,
                    signature,
                    result,
                });
            });
        }

        fn intent(
            &mut self,
            command: &str,
            kind: Option<AudioKind>,
            key: Option<String>,
            value: Option<f64>,
            answer: Option<String>,
            sender: &mpsc::Sender<Event>,
        ) -> AppResult<()> {
            let valid_fields = match command {
                "select" | "mute" => {
                    kind.is_some() && key.is_some() && value.is_none() && answer.is_none()
                }
                "volume" => kind.is_some() && key.is_some() && value.is_some() && answer.is_none(),
                "device" | "forget" => {
                    kind.is_none() && key.is_some() && value.is_none() && answer.is_none()
                }
                "answer" => kind.is_none() && key.is_none() && value.is_none() && answer.is_some(),
                "active" | "bluetoothPower" | "cancelPairing" | "dismissPairing" => {
                    kind.is_none() && key.is_none() && value.is_none() && answer.is_none()
                }
                _ => false,
            };
            if !valid_fields {
                return Err("Invalid audio panel intent fields".into());
            }
            match command {
                "active" => {
                    if self.native.opened && self.native.tab == 1 {
                        self.refresh_due = Some(Instant::now());
                        self.poll_due = Some(Instant::now() + Duration::from_secs(2));
                    } else {
                        self.poll_due = None;
                    }
                    self.reconcile_discovery(sender);
                }
                "select" => self.select(
                    kind.ok_or("Missing audio kind")?,
                    key.as_deref().ok_or("Missing selection key")?,
                    sender,
                )?,
                "volume" | "mute" => {
                    let kind = kind.ok_or("Missing audio kind")?;
                    let key = key.unwrap_or_default();
                    let value = if command == "volume" {
                        Some(value.ok_or("Invalid audio volume.")?)
                    } else {
                        None
                    };
                    volume_action(&self.native, kind, &key, value)?;
                    let operation = command.to_owned();
                    self.task(format!("audio:{}", index(kind)), sender, move |native| {
                        let snapshot = native.snapshot()?;
                        let (action, guards) = volume_action(&snapshot, kind, &key, value)?;
                        call(
                            native,
                            &operation,
                            &action["node"],
                            action["args"].clone(),
                            guards,
                        )?;
                        Ok(())
                    });
                }
                "bluetoothPower" => {
                    if let Ok(adapter) = self.native.object(&self.native.adapter) {
                        let powered = adapter["powered"] != true;
                        self.power = Some((adapter.clone(), powered));
                        self.bluetooth_error.clear();
                        if !powered {
                            self.powering_off = self.native.adapter.clone();
                            self.cancel_pairing(sender);
                        }
                        self.reconcile_discovery(sender);
                    }
                }
                "device" | "forget" => {
                    let key = key.ok_or("Missing Bluetooth device key")?;
                    let device = self.native.object(&key)?.clone();
                    let adapter = self.native.object(&self.native.adapter)?.clone();
                    if device["type"] != "device" || device["adapter"] != adapter["path"] {
                        return Err("The Bluetooth device is no longer available.".into());
                    }
                    if self.busy.contains(&key) || self.pairing.is_some() {
                        return Ok(());
                    }
                    self.bluetooth_error.clear();
                    if command == "forget" && device["paired"] != true {
                        return Ok(());
                    }
                    if command == "device" && adapter["powered"] != true {
                        return Ok(());
                    }
                    if command == "device"
                        && device["paired"] != true
                        && device["connected"] != true
                    {
                        if self.native.opened && self.native.tab == 0 {
                            self.pair(device, sender)?;
                        }
                    } else {
                        let operation = if command == "forget" {
                            "forget"
                        } else if device["connected"] == true {
                            "disconnect"
                        } else {
                            "connect"
                        };
                        self.busy.insert(key.clone());
                        let guards = vec![
                            self.native.root_guard(&["adapter"]),
                            object_guard(&adapter, &["type", "path", "powered"]),
                            object_guard(&device, &["connected", "paired"]),
                        ];
                        self.task(format!("device:{key}"), sender, move |native| {
                            call(
                                native,
                                operation,
                                &device,
                                json!({"adapter":adapter["key"]}),
                                guards,
                            )?;
                            Ok(())
                        });
                    }
                }
                "answer" => {
                    let answer = bluetooth::pairing_answer(answer.as_deref().unwrap_or_default())?;
                    let pairing = self
                        .pairing
                        .as_mut()
                        .ok_or("No pairing request is waiting for an answer.")?;
                    if !pairing.agent {
                        return Err("Pairing has completed; connecting through Astal.".into());
                    }
                    pairing
                        .answer
                        .take()
                        .ok_or("No pairing request is waiting for an answer.")?
                        .send(answer)
                        .map_err(|_| "The pairing request stopped accepting input")?;
                    if let Some(notice) = &mut self.notice {
                        notice.clear_prompt();
                    }
                }
                "cancelPairing" => self.cancel_pairing(sender),
                "dismissPairing" => {
                    if let Some(notice) = &mut self.notice {
                        notice.dismiss(self.pairing.as_ref().is_some_and(|pairing| pairing.agent));
                    }
                }
                _ => return Err("Unknown audio panel intent".into()),
            }
            Ok(())
        }
    }

    pub fn run() -> AppResult<()> {
        serve(BufReader::new(io::stdin()), io::stdout())
    }

    pub fn serve<R: BufRead + Send + 'static, W: Write>(
        mut input: R,
        mut output: W,
    ) -> AppResult<()> {
        let (sender, receiver) = mpsc::channel();
        let reader = sender.clone();
        thread::spawn(move || {
            loop {
                let result = super::read_line(&mut input)
                    .and_then(|line| Ok(serde_json::from_slice(&line)?));
                let failed = result.is_err();
                if reader.send(Event::Input(result)).is_err() || failed {
                    break;
                }
            }
        });
        let mut panel = Panel::default();
        let mut pending: HashMap<u64, Pending> = HashMap::new();
        let mut rpc_id = 0_u64;
        let mut initialized = false;
        let mut last_model = Value::Null;
        let write = |output: &mut W, value: &Value| -> AppResult<()> {
            let bytes = serde_json::to_vec(value)?;
            if bytes.len() >= super::MAX_LINE_BYTES {
                return Err("Panel response exceeds 1 MiB".into());
            }
            output.write_all(&bytes)?;
            output.write_all(b"\n")?;
            output.flush()?;
            Ok(())
        };
        loop {
            match receiver.recv_timeout(Duration::from_millis(25)) {
                Ok(Event::Input(Ok(Input::Snapshot { snapshot }))) => {
                    let signature = panel.signature();
                    panel.accept(snapshot, &sender)?;
                    if !initialized {
                        initialized = true;
                        panel.refresh_due = Some(Instant::now());
                    }
                    if signature != panel.signature() {
                        panel.reconcile_discovery(&sender);
                    }
                }
                Ok(Event::Input(Ok(Input::Reply {
                    id,
                    ok,
                    error,
                    snapshot,
                }))) => {
                    let reply = pending
                        .remove(&id)
                        .ok_or("Unexpected native RPC reply id")?;
                    let signature = panel.signature();
                    let result =
                        panel
                            .accept(snapshot.clone(), &sender)
                            .and_then(|()| match (ok, error) {
                                (true, None) => Ok(serde_json::to_value(snapshot)?),
                                (false, Some(error)) => {
                                    crate::model::validate_text(&error)?;
                                    Err(error.into())
                                }
                                _ => Err("Invalid native acknowledgement".into()),
                            });
                    let _ = reply.reply.send(result);
                    if signature != panel.signature() {
                        panel.reconcile_discovery(&sender);
                    }
                }
                Ok(Event::Input(Ok(Input::Intent {
                    command,
                    kind,
                    key,
                    value,
                    answer,
                }))) if !panel.closing => {
                    if let Err(error) = panel.intent(&command, kind, key, value, answer, &sender) {
                        if let Some(kind) = kind {
                            panel.slots[index(kind)].error = error.to_string();
                        } else {
                            panel.bluetooth_error = error.to_string();
                        }
                    }
                }
                Ok(Event::Input(Ok(Input::Shutdown))) => {
                    panel.closing = true;
                    panel.power = None;
                    panel.cancel_pairing(&sender);
                    panel.reconcile_discovery(&sender);
                    for (slot, cancel) in &panel.running {
                        if !slot.is_empty() {
                            cancel.store(true, Ordering::Relaxed);
                        }
                    }
                    panel.queues.retain(|slot, _| slot.is_empty());
                    for request in pending.values() {
                        if !request.cleanup {
                            request.cancelled.store(true, Ordering::Relaxed);
                        }
                    }
                }
                Ok(Event::Input(Err(error))) => {
                    panel.cancel_pairing(&sender);
                    return Err(error);
                }
                Ok(Event::Input(Ok(Input::Intent { .. }))) => {}
                Ok(Event::Rpc {
                    mut event,
                    reply,
                    cancelled,
                }) => {
                    if cancelled.load(Ordering::Relaxed)
                        || (panel.closing && event["cleanup"] != true)
                    {
                        let _ = reply.send(Err("Audio operation cancelled.".into()));
                    } else {
                        if pending.len() >= 256 {
                            return Err("Too many pending native RPCs".into());
                        }
                        rpc_id = rpc_id
                            .checked_add(1)
                            .filter(|id| *id <= 9_007_199_254_740_991)
                            .ok_or("Native RPC id exhausted")?;
                        event["id"] = rpc_id.into();
                        write(&mut output, &event)?;
                        let deadline = if matches!(
                            event["operation"].as_str(),
                            Some("connect" | "disconnect")
                        ) {
                            None
                        } else {
                            Some(Instant::now() + Duration::from_secs(6))
                        };
                        pending.insert(
                            rpc_id,
                            Pending {
                                reply,
                                deadline,
                                cancelled,
                                cleanup: event["cleanup"] == true,
                            },
                        );
                    }
                }
                Ok(Event::Rows {
                    kind,
                    revision,
                    resolved,
                    result,
                }) => {
                    let slot = &mut panel.slots[index(kind)];
                    slot.loading = false;
                    if revision == slot.revision {
                        match result {
                            Ok(rows) => {
                                slot.entries = rows;
                                slot.resolved = resolved;
                                slot.error.clear();
                            }
                            Err(error) => slot.error = error.to_string(),
                        }
                    } else {
                        slot.dirty = true;
                    }
                    if slot.dirty && !panel.closing {
                        panel.refresh(kind, &sender);
                    }
                }
                Ok(Event::Done { slot, result }) => {
                    panel.running.remove(&slot);
                    panel.next_task(&slot, &sender);
                    if let Some(kind) = slot
                        .strip_prefix("select:")
                        .and_then(|kind| kind.parse::<usize>().ok())
                        .and_then(|kind| KINDS.get(kind))
                        .copied()
                    {
                        let slot = &mut panel.slots[index(kind)];
                        slot.busy = false;
                        if let Err(error) = result {
                            slot.error = error.to_string();
                        }
                        panel.refresh_due = Some(Instant::now() + Duration::from_millis(150));
                    } else if let Some(key) = slot.strip_prefix("device:") {
                        panel.busy.remove(key);
                        if panel.native.object(key).is_ok()
                            && let Err(error) = result
                        {
                            panel.bluetooth_error = error.to_string();
                        }
                    } else if let Some(kind) = slot
                        .strip_prefix("audio:")
                        .and_then(|kind| kind.parse::<usize>().ok())
                        && let Err(error) = result
                    {
                        panel.slots[kind].error = error.to_string();
                    }
                }
                Ok(Event::Control {
                    owned,
                    detached,
                    signature,
                    result,
                }) => {
                    panel.control_busy = false;
                    panel.owned = owned;
                    panel.detached = detached;
                    if let Err(error) = result {
                        panel.bluetooth_error = error.to_string();
                        if panel.power.is_none() {
                            panel.powering_off.clear();
                        }
                    }
                    if signature != panel.signature() || panel.power.is_some() {
                        panel.reconcile_discovery(&sender);
                    }
                }
                Ok(Event::Prompt { id, prompt }) => {
                    if let Some(pairing) = panel
                        .pairing
                        .as_mut()
                        .filter(|pairing| pairing.id == id && pairing.agent)
                    {
                        let text = prompt.message.replace(
                            pairing.device["address"].as_str().unwrap_or_default(),
                            &bluetooth::pair_label(&pairing.device),
                        );
                        if let Some(notice) = &mut panel.notice {
                            notice.set_prompt(text, prompt.answer.is_some());
                        }
                        pairing.answer = prompt.answer;
                    }
                }
                Ok(Event::Paired { id }) => {
                    if let Some(pairing) = panel.pairing.as_mut().filter(|pairing| pairing.id == id)
                    {
                        pairing.agent = false;
                        pairing.answer = None;
                        if let Some(notice) = &mut panel.notice {
                            notice.paired(&pairing.device);
                        }
                    }
                }
                Ok(Event::PairDone { id, result }) => {
                    if let Some(key) = panel.pair_keys.remove(&id) {
                        panel.busy.remove(&key);
                    }
                    if panel
                        .pairing
                        .as_ref()
                        .is_some_and(|pairing| pairing.id == id)
                    {
                        let pairing = panel.pairing.take().ok_or("Missing pairing session")?;
                        panel.busy.remove(object_key(&pairing.device));
                        if let Err(error) = result
                            && let Some(notice) = &mut panel.notice
                        {
                            notice.fail(&pairing.device, &error.to_string(), !pairing.agent);
                        }
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err("Panel event channel closed".into());
                }
            }
            let now = Instant::now();
            let expired: Vec<_> = pending
                .iter()
                .filter(|(_, request)| request.deadline.is_some_and(|deadline| now >= deadline))
                .map(|(id, _)| *id)
                .collect();
            if !expired.is_empty() {
                return Err("Native Astal reply timed out; stopping panel policy".into());
            }
            if panel.closing
                && !panel.control_busy
                && !pending.values().any(|request| request.cleanup)
                && !panel.running.contains_key("")
            {
                write(&mut output, &json!({"type":"closed"}))?;
                return Ok(());
            }
            if initialized && !panel.closing {
                if panel.poll_due.is_some_and(|due| now >= due) {
                    panel.poll_due = Some(now + Duration::from_secs(2));
                    panel.refresh_due = Some(now);
                }
                if panel.refresh_due.is_some_and(|due| now >= due) {
                    panel.refresh_due = None;
                    for kind in KINDS {
                        panel.refresh(kind, &sender);
                    }
                }
                let model = panel.model()?;
                if model != last_model {
                    write(&mut output, &json!({"type":"model","model":model}))?;
                    last_model = model;
                }
            }
        }
    }
}
