//! Asynchronous NetworkManager refreshes, guarded operations, and scan scheduling.

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::time::{Duration, Instant};

use dbus::arg::{PropMap, Variant};
use dbus::channel::Sender;
use dbus::{Message, MessageType, Path};
use serde_json::{Value, json};

use crate::bus::{
    ACTIVE, CONNECTION, DBUS, DBUS_PATH, DEVICE, Failure, Managed, NM, NetResult, OBJECT_MANAGER,
    OBJECT_ROOT, Objects, READ_TIME, ROOT, Settings, WIRELESS, call, clone_settings, dbus_path,
    managed_json, settings_json, strip_secrets,
};
use crate::connection::{
    EntryGuard, ProfileGuard, Secret, SecretGuard, new_connection, set_password,
};
use crate::model::{
    Entry, SECURITY, Security, Snapshot, WIFI, WIRED, authentication_context, device_compatible,
    non_root, reconcile, state_json,
};
use crate::protocol::{Method, REQUEST_LIMIT, WireRequest, valid_id};

const MAX_OPERATIONS: usize = 32;
const MAX_PROFILES: usize = 512;
const REQUEST_TIME: Duration = Duration::from_secs(30);
const SCAN_TIME: Duration = Duration::from_secs(15);

struct ActivationWatch {
    path: String,
    requested_uuid: String,
    profile: Option<ProfileGuard>,
    added_profile: Option<String>,
    seen_target: bool,
    seen_progress: bool,
    seen_active: bool,
}
enum Phase {
    Start(Method),
    SecretStart(SecretGuard),
    AwaitSecret(SecretGuard),
    SecretFinish(SecretGuard, Secret),
    AwaitUpdate(ProfileGuard),
    ActivateSaved(ProfileGuard),
    AwaitActivation {
        requested_uuid: String,
        profile: Option<ProfileGuard>,
        added: bool,
    },
    WaitActivation(ActivationWatch),
    AwaitDisconnect {
        active: Option<String>,
        forget: Option<VecDeque<ProfileGuard>>,
    },
    VerifyDisconnect {
        active: Option<String>,
        forget: Option<VecDeque<ProfileGuard>>,
    },
    DeleteNext(VecDeque<ProfileGuard>),
    AwaitDelete(ProfileGuard, VecDeque<ProfileGuard>),
    VerifyDelete(ProfileGuard, VecDeque<ProfileGuard>),
    AwaitToggle(bool),
    VerifyToggle(bool),
}
struct Operation {
    id: Value,
    owner: String,
    guard: Option<EntryGuard>,
    mutation: bool,
    deadline: Instant,
    after_refresh: u64,
    phase: Phase,
}
enum PendingKind {
    Match,
    Owner,
    Objects(u64),
    Settings(u64, String),
    Operation(String),
    Scan(String, u64),
}
struct Pending {
    deadline: Instant,
    kind: PendingKind,
}
struct Refresh {
    id: u64,
    revision: u64,
    objects: Option<Objects>,
    settings: BTreeMap<String, Settings>,
    remaining: usize,
}
pub(crate) struct Engine {
    owner: String,
    snapshot: Snapshot,
    entries: Vec<Entry>,
    incarnations: BTreeMap<String, u64>,
    incarnation_serial: u64,
    reasons: BTreeMap<String, u32>,
    operations: BTreeMap<String, Operation>,
    pending: HashMap<u32, Pending>,
    busy: BTreeMap<String, String>,
    opened: bool,
    scans: BTreeMap<String, (u64, Instant)>,
    revision: u64,
    refresh_serial: u64,
    completed_refresh: u64,
    refresh: Option<Refresh>,
    dirty: bool,
    refresh_at: Instant,
    subscriptions: usize,
    owner_pending: bool,
    output: VecDeque<Value>,
    last_state: Value,
    stopped: bool,
}
impl Engine {
    pub(crate) fn new(now: Instant) -> Self {
        let mut engine = Self {
            owner: String::new(),
            snapshot: Snapshot::default(),
            entries: Vec::new(),
            incarnations: BTreeMap::new(),
            incarnation_serial: 0,
            reasons: BTreeMap::new(),
            operations: BTreeMap::new(),
            pending: HashMap::new(),
            busy: BTreeMap::new(),
            opened: false,
            scans: BTreeMap::new(),
            revision: 0,
            refresh_serial: 0,
            completed_refresh: 0,
            refresh: None,
            dirty: false,
            refresh_at: now,
            subscriptions: 0,
            owner_pending: false,
            output: VecDeque::new(),
            last_state: Value::Null,
            stopped: false,
        };
        engine.publish();
        engine
    }
    fn publish(&mut self) {
        if self.stopped {
            return;
        }
        let state = state_json(&self.snapshot, &self.entries, &self.busy);
        if state != self.last_state {
            self.last_state = state.clone();
            // Unsolicited snapshots are replaceable; request replies are not.
            self.output.retain(|value| value["type"] != "state");
            self.output.push_back(state);
        }
    }
    pub(crate) fn reply(&mut self, id: Value, result: NetResult<Value>) {
        if self.stopped {
            return;
        }
        self.output.push_back(match result {
            Ok(result) => json!({ "id": id, "result": result }),
            Err(error) => json!({ "id": id, "error": { "message": error.message, "code": error.code.to_ascii_lowercase() } }),
        });
    }
    fn mark_dirty(&mut self, now: Instant) {
        self.dirty = true;
        self.refresh_at = now;
    }
    fn send(
        &mut self,
        bus: &impl Sender,
        message: Message,
        kind: PendingKind,
        deadline: Instant,
    ) -> NetResult<()> {
        if self.stopped {
            return Err(Failure::cancelled());
        }
        let serial = bus.send(message).map_err(|_| Failure::unavailable())?;
        self.pending.insert(serial, Pending { deadline, kind });
        Ok(())
    }
    pub(crate) fn bootstrap(&mut self, bus: &impl Sender, now: Instant) -> NetResult<()> {
        for rule in [
            "type='signal',sender='org.freedesktop.DBus',path='/org/freedesktop/DBus',interface='org.freedesktop.DBus',member='NameOwnerChanged',arg0='org.freedesktop.NetworkManager'",
            "type='signal',sender='org.freedesktop.NetworkManager',path_namespace='/org/freedesktop'",
        ] {
            self.send(
                bus,
                call(DBUS, DBUS_PATH, DBUS, "AddMatch")?.append1(rule),
                PendingKind::Match,
                now + READ_TIME,
            )?;
            self.subscriptions += 1;
        }
        self.send(
            bus,
            call(DBUS, DBUS_PATH, DBUS, "GetNameOwner")?.append1(NM),
            PendingKind::Owner,
            now + READ_TIME,
        )?;
        self.owner_pending = true;
        Ok(())
    }
    pub(crate) fn enqueue(&mut self, line: &[u8], now: Instant) {
        if self.stopped {
            return;
        }
        if self.output.len() >= 128 {
            self.stop();
            return;
        }
        let request = if line.len() <= REQUEST_LIMIT {
            serde_json::from_slice::<WireRequest>(line).ok()
        } else {
            None
        };
        let Some(request) = request else {
            self.reply(Value::Null, Err(Failure::invalid()));
            return;
        };
        let id = request.id.filter(valid_id);
        if request.method == "setOpened" {
            if request.args.len() != 1 || !request.args[0].is_boolean() {
                self.reply(id.unwrap_or(Value::Null), Err(Failure::invalid()));
                return;
            }
            self.opened = request.args[0] == true;
            if !self.opened {
                self.stop_scans();
                // Already-sent calls cannot be undone; discard their continuations.
                let pending: Vec<_> = self.operations.keys().cloned().collect();
                for key in pending {
                    self.finish(&key, Err(Failure::cancelled()));
                }
            }
            if let Some(id) = id {
                self.reply(id, Ok(Value::Null));
            }
            return;
        }
        if request.method == "cancel" {
            if request.args.len() != 1 || !valid_id(&request.args[0]) {
                self.reply(id.unwrap_or(Value::Null), Err(Failure::invalid()));
                return;
            }
            self.finish(&request.args[0].to_string(), Err(Failure::cancelled()));
            if let Some(id) = id {
                self.reply(id, Ok(Value::Null));
            }
            return;
        }
        let Some(id) = id else {
            self.reply(Value::Null, Err(Failure::invalid()));
            return;
        };
        let operation_key = id.to_string();
        if self.operations.contains_key(&operation_key) {
            self.reply(
                id,
                Err(Failure {
                    message: "This network request ID is already in use.",
                    code: "DUPLICATE_ID",
                }),
            );
            return;
        }
        if self.operations.len() >= MAX_OPERATIONS {
            self.reply(
                id,
                Err(Failure {
                    message: "Too many network requests are pending.",
                    code: "BUSY",
                }),
            );
            return;
        }
        let method = match Method::parse(&request.method, request.args) {
            Ok(method) => method,
            Err(error) => {
                self.reply(id, Err(error));
                return;
            }
        };
        let guard = if let Some(key) = method.key() {
            let Some(entry) = self.entries.iter().find(|entry| entry.key == key) else {
                self.reply(
                    id,
                    Err(Failure::new(
                        "The selected network is no longer available. Refresh and try again.",
                    )),
                );
                return;
            };
            let sensitive = matches!(
                &method,
                Method::Forget(_)
                    | Method::ReadSecret(..)
                    | Method::ReadSavedSecret(..)
                    | Method::Activate {
                        password: Some(_),
                        ..
                    }
            );
            if sensitive {
                let stamp = self.last_state["rows"]
                    .as_array()
                    .and_then(|rows| rows.iter().find(|row| row["key"] == key))
                    .and_then(|row| row["selectionStamp"].as_str());
                if stamp.is_none() || stamp != request.expected_stamp.as_deref() {
                    self.reply(id, Err(Failure::changed()));
                    return;
                }
            }
            Some(EntryGuard::capture(entry, &self.snapshot))
        } else {
            None
        };
        if method.mutation()
            && let Some(guard) = &guard
            && self.busy.contains_key(&guard.device)
        {
            self.reply(
                id,
                Err(Failure {
                    message: "Wait for the current network operation to finish.",
                    code: "BUSY",
                }),
            );
            return;
        }
        if matches!(method, Method::ToggleWifi)
            && self.operations.values().any(|operation| {
                matches!(
                    operation.phase,
                    Phase::Start(Method::ToggleWifi)
                        | Phase::AwaitToggle(_)
                        | Phase::VerifyToggle(_)
                )
            })
        {
            self.reply(
                id,
                Err(Failure {
                    message: "Wait for the current network operation to finish.",
                    code: "BUSY",
                }),
            );
            return;
        }
        let mutation = method.mutation();
        let phase = match &method {
            Method::ReadSecret(key, identity) => SecretGuard::capture(
                &self.snapshot,
                &self.entries,
                key,
                Some(identity.clone()),
                None,
                &self.busy,
            )
            .map(Phase::SecretStart),
            Method::ReadSavedSecret(key, uuid) => SecretGuard::capture(
                &self.snapshot,
                &self.entries,
                key,
                None,
                Some(uuid),
                &self.busy,
            )
            .map(Phase::SecretStart),
            _ => Ok(Phase::Start(method)),
        };
        let phase = match phase {
            Ok(phase) => phase,
            Err(error) => {
                self.reply(id, Err(error));
                return;
            }
        };
        if matches!(&phase, Phase::Start(method) if method.mutation())
            && let Some(guard) = &guard
        {
            self.busy.insert(guard.device.clone(), guard.key.clone());
        }
        self.operations.insert(
            operation_key,
            Operation {
                id,
                owner: self.owner.clone(),
                guard,
                mutation,
                deadline: now + REQUEST_TIME,
                after_refresh: self.refresh_serial + 1,
                phase,
            },
        );
        self.mark_dirty(now);
        self.publish();
    }
    fn finish(&mut self, key: &str, result: NetResult<Value>) {
        let Some(operation) = self.operations.remove(key) else {
            return;
        };
        self.pending
            .retain(|_, pending| !matches!(&pending.kind, PendingKind::Operation(id) if id == key));
        if operation.mutation
            && let Some(guard) = &operation.guard
            && self.busy.get(&guard.device) == Some(&guard.key)
        {
            self.busy.remove(&guard.device);
        }
        self.reply(operation.id.clone(), result);
        self.publish();
    }
    fn stop_scans(&mut self) {
        self.scans.clear();
        self.pending
            .retain(|_, pending| !matches!(pending.kind, PendingKind::Scan(..)));
    }
    pub(crate) fn stop(&mut self) {
        self.stopped = true;
        self.opened = false;
        self.operations.clear();
        self.pending.clear();
        self.busy.clear();
        self.scans.clear();
        self.refresh = None;
        self.output.clear();
        self.entries.clear();
        self.snapshot = Snapshot::default();
    }
    pub(crate) fn set_owner(&mut self, owner: String, now: Instant) {
        if self.owner == owner {
            return;
        }
        let keys: Vec<_> = self.operations.keys().cloned().collect();
        for key in keys {
            self.finish(&key, Err(Failure::unavailable()));
        }
        self.pending
            .retain(|_, pending| matches!(pending.kind, PendingKind::Match));
        self.owner = owner;
        self.owner_pending = false;
        self.refresh = None;
        self.incarnations.clear();
        self.reasons.clear();
        self.snapshot = Snapshot::default();
        self.entries.clear();
        self.stop_scans();
        self.revision += 1;
        self.mark_dirty(now);
        self.publish();
    }
    fn start_refresh(&mut self, bus: &impl Sender, now: Instant) -> NetResult<()> {
        self.refresh_serial += 1;
        let id = self.refresh_serial;
        self.send(
            bus,
            call(
                &self.owner,
                OBJECT_ROOT,
                OBJECT_MANAGER,
                "GetManagedObjects",
            )?,
            PendingKind::Objects(id),
            now + READ_TIME,
        )?;
        self.refresh = Some(Refresh {
            id,
            revision: self.revision,
            objects: None,
            settings: BTreeMap::new(),
            remaining: 0,
        });
        Ok(())
    }
    fn complete_refresh(&mut self, now: Instant) -> NetResult<()> {
        let Some(refresh) = self.refresh.take() else {
            return Ok(());
        };
        if refresh.revision != self.revision {
            self.mark_dirty(now);
            return Ok(());
        }
        let objects = refresh.objects.ok_or_else(Failure::unavailable)?;
        let members: BTreeSet<String> = objects
            .iter()
            .filter(|(_, interfaces)| {
                interfaces.contains_key(DEVICE) || interfaces.contains_key(CONNECTION)
            })
            .map(|(path, _)| path.clone())
            .collect();
        self.incarnations.retain(|path, _| members.contains(path));
        for path in members {
            if !self.incarnations.contains_key(&path) {
                self.incarnation_serial += 1;
                self.incarnations.insert(path, self.incarnation_serial);
            }
        }
        self.reasons.retain(|path, _| {
            objects
                .get(path)
                .is_some_and(|interfaces| interfaces.contains_key(ACTIVE))
        });
        self.snapshot = Snapshot::build(
            self.owner.clone(),
            &objects,
            refresh.settings,
            &self.incarnations,
            &self.reasons,
        )?;
        self.entries = reconcile(&self.snapshot);
        self.completed_refresh = refresh.id;
        self.dirty = false;
        self.publish();
        Ok(())
    }
    fn refresh_failed(&mut self, now: Instant) {
        self.refresh = None;
        self.pending.retain(|_, pending| {
            !matches!(
                pending.kind,
                PendingKind::Objects(_) | PendingKind::Settings(..)
            )
        });
        let keys: Vec<_> = self.operations.keys().cloned().collect();
        for key in keys {
            self.finish(&key, Err(Failure::unavailable()));
        }
        self.snapshot = Snapshot::default();
        self.entries.clear();
        self.stop_scans();
        self.dirty = false;
        self.refresh_at = now;
        self.publish();
    }
    pub(crate) fn receive(&mut self, bus: &impl Sender, mut message: Message, now: Instant) {
        if self.stopped {
            return;
        }
        if message.msg_type() == MessageType::Signal {
            self.signal(&message, now);
            return;
        }
        let Some(pending) = message
            .get_reply_serial()
            .and_then(|serial| self.pending.remove(&serial))
        else {
            if let Some(reply) = dbus::channel::default_reply(&message) {
                let _ = bus.send(reply);
            }
            return;
        };
        let expected_sender = if matches!(pending.kind, PendingKind::Match | PendingKind::Owner) {
            DBUS
        } else {
            self.owner.as_str()
        };
        // A real bus always supplies an authenticated sender header. Synthetic
        // fixture replies may omit it, but cannot arrive over a bus socket.
        if message
            .sender()
            .is_some_and(|sender| &*sender != expected_sender)
        {
            self.pending
                .insert(message.get_reply_serial().unwrap(), pending);
            return;
        }
        let failure = Failure::native(&mut message);
        match pending.kind {
            PendingKind::Match => {
                self.subscriptions = self.subscriptions.saturating_sub(1);
                if failure.is_some() {
                    self.stop();
                }
            }
            PendingKind::Owner => {
                self.owner_pending = false;
                let owner = if failure.is_none() {
                    message.read1::<String>().unwrap_or_default()
                } else {
                    String::new()
                };
                self.set_owner(owner, now);
            }
            PendingKind::Objects(id) => {
                if !self
                    .refresh
                    .as_ref()
                    .is_some_and(|refresh| refresh.id == id)
                {
                    return;
                }
                let objects = if failure.is_none() {
                    message
                        .read1::<Managed>()
                        .ok()
                        .and_then(|objects| managed_json(objects).ok())
                } else {
                    None
                };
                let Some(objects) = objects else {
                    self.refresh_failed(now);
                    return;
                };
                let paths: Vec<_> = objects
                    .iter()
                    .filter(|(_, interfaces)| interfaces.contains_key(CONNECTION))
                    .map(|(path, _)| path.clone())
                    .collect();
                if paths.len() > MAX_PROFILES {
                    self.refresh_failed(now);
                    return;
                }
                let refresh = self.refresh.as_mut().unwrap();
                refresh.remaining = paths.len();
                refresh.objects = Some(objects);
                for path in &paths {
                    let result =
                        call(&self.owner, path, CONNECTION, "GetSettings").and_then(|message| {
                            self.send(
                                bus,
                                message,
                                PendingKind::Settings(id, path.clone()),
                                now + READ_TIME,
                            )
                        });
                    if result.is_err() {
                        self.refresh_failed(now);
                        return;
                    }
                }
                if paths.is_empty() && self.complete_refresh(now).is_err() {
                    self.refresh_failed(now);
                }
            }
            PendingKind::Settings(id, path) => {
                let Some(refresh) = self.refresh.as_mut().filter(|refresh| refresh.id == id) else {
                    return;
                };
                if failure.is_none()
                    && let Ok(mut settings) = message.read1::<Settings>()
                {
                    strip_secrets(&mut settings);
                    refresh.settings.insert(path, settings);
                }
                refresh.remaining = refresh.remaining.saturating_sub(1);
                if refresh.remaining == 0 && self.complete_refresh(now).is_err() {
                    self.refresh_failed(now);
                }
            }
            PendingKind::Operation(key) => {
                if let Some(failure) = failure {
                    self.finish(&key, Err(failure));
                    return;
                }
                let Some(mut operation) = self.operations.remove(&key) else {
                    return;
                };
                let result = self.operation_reply(&mut operation, &message, now);
                self.operations.insert(key.clone(), operation);
                if let Err(error) = result {
                    self.finish(&key, Err(error));
                }
            }
            PendingKind::Scan(_, _) => (),
        }
    }
    fn signal(&mut self, message: &Message, now: Instant) {
        let sender = message
            .sender()
            .map(|sender| sender.to_string())
            .unwrap_or_default();
        let interface = message
            .interface()
            .map(|interface| interface.to_string())
            .unwrap_or_default();
        let member = message
            .member()
            .map(|member| member.to_string())
            .unwrap_or_default();
        if sender == DBUS && interface == DBUS && member == "NameOwnerChanged" {
            if let Ok((name, _, owner)) = message.read3::<String, String, String>()
                && name == NM
            {
                self.set_owner(owner, now);
            }
            return;
        }
        if sender != self.owner || self.owner.is_empty() {
            return;
        }
        let path = message
            .path()
            .map(|path| path.to_string())
            .unwrap_or_default();
        if interface == ACTIVE
            && member == "StateChanged"
            && let Ok((_, reason)) = message.read2::<u32, u32>()
        {
            self.reasons.insert(path.clone(), reason);
        }
        if interface == OBJECT_MANAGER && member == "InterfacesRemoved" {
            if let Ok((path, interfaces)) = message.read2::<Path<'static>, Vec<String>>()
                && interfaces
                    .iter()
                    .any(|interface| matches!(interface.as_str(), DEVICE | CONNECTION))
            {
                self.incarnations.remove::<str>(&path);
            }
        } else if member == "DeviceRemoved" || member == "ConnectionRemoved" {
            if let Ok(path) = message.read1::<Path<'static>>() {
                self.incarnations.remove::<str>(&path);
            }
        } else if interface == CONNECTION && member == "Removed" {
            self.incarnations.remove(&path);
        }
        self.revision += 1;
        self.mark_dirty(now);
    }
    fn operation_reply(
        &mut self,
        operation: &mut Operation,
        message: &Message,
        now: Instant,
    ) -> NetResult<()> {
        let phase = std::mem::replace(&mut operation.phase, Phase::VerifyToggle(false));
        operation.phase = match phase {
            Phase::AwaitUpdate(profile) => Phase::ActivateSaved(profile),
            Phase::AwaitActivation {
                requested_uuid,
                profile,
                added,
            } => {
                let (added_profile, path) = if added {
                    let (profile, active) = message
                        .read2::<Path<'static>, Path<'static>>()
                        .map_err(|_| Failure::changed())?;
                    (Some(profile.to_string()), active.to_string())
                } else {
                    (
                        None,
                        message
                            .read1::<Path<'static>>()
                            .map_err(|_| Failure::changed())?
                            .to_string(),
                    )
                };
                if !non_root(&path) {
                    return Err(Failure::new(
                        "The connection could not be started. Try again.",
                    ));
                }
                Phase::WaitActivation(ActivationWatch {
                    path,
                    requested_uuid,
                    profile,
                    added_profile,
                    seen_target: false,
                    seen_progress: false,
                    seen_active: false,
                })
            }
            Phase::AwaitDisconnect { active, forget } => Phase::VerifyDisconnect { active, forget },
            Phase::AwaitDelete(profile, remaining) => Phase::VerifyDelete(profile, remaining),
            Phase::AwaitSecret(guard) => {
                let secrets = message.read1::<Settings>().map_err(|_| {
                    Failure::new(
                        "The saved password is not available. Check the saved network settings.",
                    )
                })?;
                let key = if guard.security == Security::Wep {
                    format!("wep-key{}", guard.key_index)
                } else {
                    "psk".to_owned()
                };
                let password = secrets.get(SECURITY).and_then(|security| security.get(&key)).and_then(|value| value.0.as_str())
                    .filter(|password| !password.is_empty() && password.len() <= REQUEST_LIMIT).ok_or_else(|| Failure::new("The saved password is not available. Check the saved network settings."))?;
                Phase::SecretFinish(guard, Secret(password.to_owned()))
            }
            Phase::AwaitToggle(enabled) => Phase::VerifyToggle(enabled),
            _ => return Err(Failure::changed()),
        };
        operation.after_refresh = self.refresh_serial + 1;
        self.mark_dirty(now);
        Ok(())
    }

    fn send_operation(
        &mut self,
        bus: &impl Sender,
        key: &str,
        operation: &Operation,
        message: Message,
    ) -> NetResult<()> {
        self.send(
            bus,
            message,
            PendingKind::Operation(key.to_owned()),
            operation.deadline,
        )
    }
    fn check_radios(&self, guard: &EntryGuard) -> NetResult<()> {
        if guard.wireless && (!self.snapshot.wifi_enabled || !self.snapshot.wifi_hardware_enabled) {
            return Err(Failure::new(
                "Enable Wi-Fi and check the hardware switch before connecting.",
            ));
        }
        Ok(())
    }
    fn activate_saved(
        &mut self,
        bus: &impl Sender,
        key: &str,
        operation: &mut Operation,
        profile_guard: ProfileGuard,
    ) -> NetResult<()> {
        let guard = operation.guard.as_ref().ok_or_else(Failure::changed)?;
        let entry = guard.entry(&self.snapshot, &self.entries)?;
        self.check_radios(guard)?;
        let profile = profile_guard.validate(&self.snapshot)?;
        if !entry
            .profiles
            .iter()
            .any(|index| self.snapshot.profiles[*index].path == profile.path)
        {
            return Err(Failure::changed());
        }
        let device = &self.snapshot.devices[entry.device];
        let ap = entry
            .best_ap(&self.snapshot, Some(profile))
            .map_or("/", |ap| ap.path.as_str());
        let message = call(&self.owner, ROOT, NM, "ActivateConnection")?.append3(
            dbus_path(&profile.path)?,
            dbus_path(&device.path)?,
            dbus_path(ap)?,
        );
        operation.phase = Phase::AwaitActivation {
            requested_uuid: profile.uuid.clone(),
            profile: Some(profile_guard),
            added: false,
        };
        self.send_operation(bus, key, operation, message)
    }
    fn step(
        &mut self,
        bus: &impl Sender,
        key: &str,
        operation: &mut Operation,
        now: Instant,
    ) -> NetResult<Option<Value>> {
        if operation.owner != self.owner || self.owner.is_empty() {
            return Err(Failure::unavailable());
        }
        if let Some(guard) = &operation.guard {
            guard.device(&self.snapshot)?;
        }
        let phase = std::mem::replace(&mut operation.phase, Phase::VerifyToggle(false));
        match phase {
            Phase::Start(Method::Activate {
                profile_uuid,
                password,
                ..
            }) => {
                let guard = operation.guard.as_ref().ok_or_else(Failure::changed)?;
                let entry = guard.entry(&self.snapshot, &self.entries)?;
                self.check_radios(guard)?;
                if entry.changing(&self.snapshot) || entry.connected(&self.snapshot) {
                    return Err(Failure::new(
                        "Wait for the current network operation to finish.",
                    ));
                }
                if password.is_some() && (guard.profiles.len() > 1 || entry.profiles.len() > 1) {
                    return Err(Failure::new(
                        "Multiple saved profiles match this network. Update the intended profile in a network connection editor.",
                    ));
                }
                let selected = if profile_uuid.is_empty() {
                    guard.profiles.first()
                } else {
                    Some(
                        guard
                            .profiles
                            .iter()
                            .find(|profile| profile.uuid == profile_uuid)
                            .ok_or_else(|| {
                                Failure::new("The selected saved profile is no longer available.")
                            })?,
                    )
                };
                if let Some(selected) = selected {
                    let profile = selected.validate(&self.snapshot)?;
                    if !entry
                        .profiles
                        .iter()
                        .any(|index| self.snapshot.profiles[*index].path == profile.path)
                    {
                        return Err(Failure::changed());
                    }
                    if let Some(password) = password {
                        let version = profile.version.filter(|version| *version != 0).ok_or_else(|| Failure::new("This NetworkManager version does not support guarded profile updates. Use a network connection editor."))?;
                        let mut settings = clone_settings(&profile.settings);
                        set_password(&mut settings, &password.0)?;
                        let mut expected = selected.clone();
                        expected.context = authentication_context(&settings_json(&settings));
                        let mut arguments = PropMap::new();
                        arguments.insert("version-id".to_owned(), Variant(Box::new(version)));
                        let message = call(&self.owner, &profile.path, CONNECTION, "Update2")?
                            .append3(settings, 1u32, arguments);
                        operation.phase = Phase::AwaitUpdate(expected);
                        self.send_operation(bus, key, operation, message)?;
                    } else {
                        self.activate_saved(bus, key, operation, selected.clone())?;
                    }
                } else {
                    if !entry.profiles.is_empty() {
                        return Err(Failure::changed());
                    }
                    let (uuid, settings) = new_connection(
                        &self.snapshot,
                        entry,
                        password.as_ref().map(|password| password.0.as_str()),
                    )?;
                    let device = &self.snapshot.devices[entry.device];
                    let ap = entry
                        .best_ap(&self.snapshot, None)
                        .map_or("/", |ap| ap.path.as_str());
                    let message = call(&self.owner, ROOT, NM, "AddAndActivateConnection")?.append3(
                        settings,
                        dbus_path(&device.path)?,
                        dbus_path(ap)?,
                    );
                    operation.phase = Phase::AwaitActivation {
                        requested_uuid: uuid,
                        profile: None,
                        added: true,
                    };
                    self.send_operation(bus, key, operation, message)?;
                }
            }
            Phase::ActivateSaved(profile) => {
                let guard = operation.guard.as_ref().ok_or_else(Failure::changed)?;
                let entry = guard.entry(&self.snapshot, &self.entries)?;
                if entry.profiles.len() != 1 {
                    return Err(Failure::new(
                        "The saved profile selection changed. Update the intended profile in a network connection editor.",
                    ));
                }
                self.activate_saved(bus, key, operation, profile)?;
            }
            Phase::WaitActivation(mut watch) => {
                let guard = operation.guard.as_ref().ok_or_else(Failure::changed)?;
                let device = guard.device(&self.snapshot)?;
                if guard.wireless
                    && (!self.snapshot.wifi_enabled || !self.snapshot.wifi_hardware_enabled)
                {
                    return Err(Failure::new(
                        "Wi-Fi was disabled during the connection request.",
                    ));
                }
                if let Some(active) = self.snapshot.active.get(&watch.path) {
                    watch.seen_active = true;
                    if active.uuid != watch.requested_uuid
                        || active.kind != if guard.wireless { WIFI } else { WIRED }
                        || !active.devices.contains(&guard.device)
                    {
                        return Err(Failure::new(
                            "The selected saved profile changed during activation. Refresh and try again.",
                        ));
                    }
                    if let Some(profile) = &watch.profile {
                        profile.validate(&self.snapshot)?;
                        if active.profile != profile.path {
                            return Err(Failure::changed());
                        }
                    } else if let Some(path) = &watch.added_profile {
                        let profile = self
                            .snapshot
                            .unique_profile(&watch.requested_uuid)
                            .filter(|profile| {
                                &profile.path == path
                                    && profile.kind == active.kind
                                    && profile.ssid == guard.ssid
                                    && device_compatible(device, profile)
                            })
                            .ok_or_else(Failure::changed)?;
                        watch.profile = Some(ProfileGuard::capture(profile));
                        if active.profile != *path {
                            return Err(Failure::changed());
                        }
                    }
                    let target = device
                        .active
                        .as_ref()
                        .is_some_and(|current| current.path == watch.path);
                    if target && device.changing() {
                        watch.seen_progress = true;
                    }
                    if matches!(active.state, 3 | 4)
                        || (target && watch.seen_progress && device.state == 120)
                    {
                        let message = "The connection failed. Check authentication and the saved network settings.";
                        return Err(
                            if matches!(device.reason, 7..=11) || matches!(active.reason, 9 | 10) {
                                Failure::authentication(message)
                            } else {
                                Failure::new(message)
                            },
                        );
                    }
                    if target {
                        watch.seen_target = true;
                    } else if device.active.is_some() && watch.seen_target {
                        return Err(Failure::new("Another connection replaced this request."));
                    }
                    if active.state == 2 && device.state == 100 && target {
                        let entry = guard.entry(&self.snapshot, &self.entries)?;
                        if !entry.targets(&self.snapshot) || !entry.connected(&self.snapshot) {
                            return Err(Failure::new(
                                "The selected network changed during activation.",
                            ));
                        }
                        return Ok(Some(Value::Null));
                    }
                } else if watch.seen_active {
                    return Err(Failure::new("Another connection replaced this request."));
                }
                operation.phase = Phase::WaitActivation(watch);
                operation.after_refresh = self.refresh_serial + 1;
                self.dirty = true;
                self.refresh_at = now + Duration::from_millis(250);
            }
            Phase::Start(Method::Disconnect(_)) | Phase::Start(Method::Forget(_)) => {
                let forget = matches!(phase, Phase::Start(Method::Forget(_)));
                let guard = operation.guard.as_ref().ok_or_else(Failure::changed)?;
                let entry = guard.entry(&self.snapshot, &self.entries)?;
                if entry.changing(&self.snapshot) {
                    return Err(Failure::new(
                        "Wait for the current network operation to finish.",
                    ));
                }
                let profiles =
                    if forget {
                        if guard.profiles.is_empty() {
                            return Err(Failure::new(
                                "The selected saved profile is no longer available.",
                            ));
                        }
                        for profile in &guard.profiles {
                            if self
                                .snapshot
                                .profiles
                                .iter()
                                .any(|current| current.uuid == profile.uuid)
                            {
                                let profile = profile.validate(&self.snapshot)?;
                                if !entry.profiles.iter().any(|index| {
                                    self.snapshot.profiles[*index].path == profile.path
                                }) {
                                    return Err(Failure::changed());
                                }
                            }
                        }
                        Some(VecDeque::from(guard.profiles.clone()))
                    } else {
                        None
                    };
                if entry.targets(&self.snapshot) {
                    guard.current_active(&self.snapshot)?;
                    let active = self.snapshot.devices[entry.device]
                        .active
                        .as_ref()
                        .map(|active| active.path.clone());
                    let message = call(&self.owner, &guard.device, DEVICE, "Disconnect")?;
                    operation.phase = Phase::AwaitDisconnect {
                        active,
                        forget: profiles,
                    };
                    self.send_operation(bus, key, operation, message)?;
                } else if let Some(profiles) = profiles {
                    operation.phase = Phase::DeleteNext(profiles);
                } else {
                    return Ok(Some(Value::Null));
                }
            }
            Phase::VerifyDisconnect { active, forget } => {
                let guard = operation.guard.as_ref().ok_or_else(Failure::changed)?;
                let device = guard.device(&self.snapshot)?;
                if device.active.as_ref().is_some_and(|current| {
                    Some(&current.path) != active.as_ref()
                        || Some(&current.uuid) != guard.active_uuid.as_ref()
                }) {
                    return Err(Failure::changed());
                }
                if let Some(profiles) = forget {
                    operation.phase = Phase::DeleteNext(profiles);
                } else {
                    return Ok(Some(Value::Null));
                }
            }
            Phase::DeleteNext(mut profiles) => {
                let guard = operation.guard.as_ref().ok_or_else(Failure::changed)?;
                guard.current_active(&self.snapshot)?;
                while let Some(profile_guard) = profiles.pop_front() {
                    if !self
                        .snapshot
                        .profiles
                        .iter()
                        .any(|profile| profile.uuid == profile_guard.uuid)
                    {
                        continue;
                    }
                    let entry = guard.entry(&self.snapshot, &self.entries)?;
                    let profile = profile_guard.validate(&self.snapshot)?;
                    if !entry
                        .profiles
                        .iter()
                        .any(|index| self.snapshot.profiles[*index].path == profile.path)
                    {
                        return Err(Failure::changed());
                    }
                    let message = call(&self.owner, &profile.path, CONNECTION, "Delete")?;
                    operation.phase = Phase::AwaitDelete(profile_guard, profiles);
                    self.send_operation(bus, key, operation, message)?;
                    return Ok(None);
                }
                return Ok(Some(Value::Null));
            }
            Phase::VerifyDelete(profile, profiles) => {
                if self
                    .snapshot
                    .profiles
                    .iter()
                    .any(|current| current.uuid == profile.uuid)
                {
                    return Err(Failure::changed());
                }
                operation.phase = Phase::DeleteNext(profiles);
            }
            Phase::SecretStart(guard) => {
                guard.validate(&self.snapshot, &self.entries, &self.busy)?;
                if guard.security == Security::Open {
                    operation.phase = Phase::SecretFinish(guard, Secret(String::new()));
                    operation.after_refresh = self.refresh_serial + 1;
                    self.mark_dirty(now);
                } else {
                    let message = call(&self.owner, &guard.profile.path, CONNECTION, "GetSecrets")?
                        .append1(SECURITY);
                    operation.phase = Phase::AwaitSecret(guard);
                    self.send_operation(bus, key, operation, message)?;
                }
            }
            Phase::SecretFinish(guard, password) => {
                guard.validate(&self.snapshot, &self.entries, &self.busy)?;
                return Ok(Some(guard.credentials(&password.0)));
            }
            Phase::Start(Method::ToggleWifi) => {
                if !self.snapshot.devices.iter().any(|device| device.wireless) {
                    return Err(Failure::new("No Wi-Fi device is available."));
                }
                if !self.snapshot.wifi_hardware_enabled {
                    return Err(Failure::new("Wi-Fi is blocked by the hardware switch."));
                }
                let enabled = !self.snapshot.wifi_enabled;
                let message = call(&self.owner, ROOT, "org.freedesktop.DBus.Properties", "Set")?
                    .append3(NM, "WirelessEnabled", Variant(enabled));
                operation.phase = Phase::AwaitToggle(enabled);
                self.send_operation(bus, key, operation, message)?;
            }
            Phase::VerifyToggle(enabled) => {
                if self.snapshot.wifi_enabled != enabled {
                    return Err(Failure::new("Unable to change the Wi-Fi radio state."));
                }
                return Ok(Some(Value::Null));
            }
            phase => {
                operation.phase = phase;
            }
        }
        Ok(None)
    }
    fn configure_scans(&mut self, bus: &impl Sender, now: Instant) {
        if !self.opened
            || !self.snapshot.wifi_enabled
            || !self.snapshot.wifi_hardware_enabled
            || self.owner.is_empty()
        {
            self.stop_scans();
            return;
        }
        let devices: Vec<_> = self
            .snapshot
            .devices
            .iter()
            .filter(|device| device.wireless)
            .map(|device| (device.path.clone(), device.incarnation))
            .collect();
        self.scans
            .retain(|path, (incarnation, _)| devices.contains(&(path.clone(), *incarnation)));
        self.pending.retain(|_, pending| match &pending.kind {
            PendingKind::Scan(path, incarnation) => devices.contains(&(path.clone(), *incarnation)),
            _ => true,
        });
        for (path, incarnation) in devices {
            let (previous, due) = self.scans.entry(path.clone()).or_insert((incarnation, now));
            if *previous != incarnation {
                *previous = incarnation;
                *due = now;
            }
            if *due > now || self.pending.values().any(|pending| matches!(&pending.kind, PendingKind::Scan(current, _) if current == &path)) { continue; }
            *due = now + SCAN_TIME;
            if let Ok(message) = call(&self.owner, &path, WIRELESS, "RequestScan") {
                let _ = self.send(
                    bus,
                    message.append1(PropMap::new()),
                    PendingKind::Scan(path, incarnation),
                    now + READ_TIME,
                );
            }
        }
    }
    pub(crate) fn tick(&mut self, bus: &impl Sender, now: Instant) {
        if self.stopped {
            return;
        }
        let expired: Vec<_> = self
            .pending
            .iter()
            .filter(|(_, pending)| pending.deadline <= now)
            .map(|(serial, _)| *serial)
            .collect();
        for serial in expired {
            if let Some(pending) = self.pending.remove(&serial) {
                match pending.kind {
                    PendingKind::Operation(key) => self.finish(&key, Err(Failure::timeout())),
                    PendingKind::Objects(_) | PendingKind::Settings(..) => self.refresh_failed(now),
                    PendingKind::Match => {
                        self.stop();
                        return;
                    }
                    PendingKind::Owner => self.owner_pending = false,
                    PendingKind::Scan(..) => (),
                }
            }
        }
        let expired: Vec<_> = self
            .operations
            .iter()
            .filter(|(_, operation)| operation.deadline <= now)
            .map(|(key, _)| key.clone())
            .collect();
        for key in expired {
            self.finish(&key, Err(Failure::timeout()));
        }
        if self.owner.is_empty() && !self.owner_pending && self.subscriptions == 0 {
            let keys: Vec<_> = self.operations.keys().cloned().collect();
            for key in keys {
                self.finish(&key, Err(Failure::unavailable()));
            }
        }
        if !self.dirty && self.refresh.is_none() && !self.snapshot.owner.is_empty() {
            self.configure_scans(bus, now);
            let keys: Vec<_> = self
                .operations
                .iter()
                .filter(|(_, operation)| operation.after_refresh <= self.completed_refresh)
                .map(|(key, _)| key.clone())
                .collect();
            for key in keys {
                let Some(mut operation) = self.operations.remove(&key) else {
                    continue;
                };
                let result = self.step(bus, &key, &mut operation, now);
                self.operations.insert(key.clone(), operation);
                match result {
                    Ok(Some(result)) => self.finish(&key, Ok(result)),
                    Err(error) => self.finish(&key, Err(error)),
                    _ => (),
                }
                if self.dirty {
                    break;
                }
            }
            if self
                .operations
                .values()
                .any(|operation| operation.after_refresh > self.completed_refresh)
                && !self.dirty
            {
                self.mark_dirty(now);
            }
        }
        if self.dirty
            && self.refresh.is_none()
            && now >= self.refresh_at
            && !self.owner.is_empty()
            && self.subscriptions == 0
            && self.start_refresh(bus, now).is_err()
        {
            self.refresh_failed(now);
        }
    }
}

impl Engine {
    pub(crate) fn is_stopped(&self) -> bool {
        self.stopped
    }

    pub(crate) fn pop_output(&mut self) -> Option<Value> {
        self.output.pop_front()
    }

    pub(crate) fn fail_unavailable(&mut self) {
        let ids: Vec<_> = self.operations.keys().cloned().collect();
        for id in ids {
            self.finish(&id, Err(Failure::unavailable()));
        }
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
