//! Notification history, DND, popup lifetimes, and guarded native operations.

use std::{
    env, io,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use serde_json::{Value, json};

use crate::protocol::{Notice, emit};
use crate::storage::{home, limited, save};

pub(crate) struct Live {
    native: u32,
    token: String,
    row: Value,
    pub(crate) hover: bool,
    pub(crate) due: Option<Instant>,
}
impl Live {
    fn restart(&mut self) {
        let ms = self.row["timeout"].as_i64().unwrap_or(-1);
        self.due = (!self.hover && ms != 0 && self.row["urgency"] != 2)
            .then(|| Instant::now() + Duration::from_millis(if ms < 0 { 8000 } else { ms as u64 }));
    }
    fn operation(&self, op: &str, argument: &str) -> io::Result<()> {
        emit(
            json!({"event":"native","operation":op,"native":self.native,"token":self.token,"argument":argument}),
        )
    }
}
#[derive(Default)]
pub(crate) struct Center {
    pub(crate) history: Vec<Value>,
    pub(crate) popups: Vec<Value>,
    pub(crate) live: Vec<Live>,
    pub(crate) path: PathBuf,
    pub(crate) dnd: bool,
    saved: Vec<u8>,
    pending_seed: Option<bool>,
}
fn archived(mut v: Value) -> Value {
    v["closed"] = json!(true);
    v["actions"] = json!([]);
    if v["icon"]
        .as_str()
        .is_some_and(|s| s.starts_with("image://"))
    {
        v["icon"] = json!("")
    }
    v.as_object_mut().unwrap().remove("token");
    v
}
fn normalize(v: &Value) -> Option<Value> {
    let id = v.get("id")?.as_str()?;
    if id.is_empty() || v["transient"] == true {
        return None;
    }
    Some(archived(
        json!({"id":id,"app":v["app"].as_str()?,"summary":v["summary"].as_str()?,"body":v["body"].as_str()?,"icon":v["icon"].as_str().unwrap_or(""),"urgency":v["urgency"].as_u64().filter(|n|*n<=2).unwrap_or(1),"transient":false,"timeout":v["timeout"].as_i64().filter(|n|(-1..=2147483647).contains(n)).unwrap_or(-1)}),
    ))
}
impl Center {
    pub(crate) fn restore(
        &mut self,
        path: PathBuf,
        legacy: &Path,
        dnd: bool,
        user: bool,
    ) -> io::Result<()> {
        self.path = path;
        self.dnd = dnd;
        let mut first = false;
        let bytes = limited(&self.path, 8 * 1024 * 1024).or_else(|e| {
            if e.kind() != io::ErrorKind::NotFound {
                return Err(e);
            }
            first = true;
            limited(legacy, 8 * 1024 * 1024)
        });
        if let Some(v) = bytes
            .ok()
            .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
            .filter(|v| v["version"] == 1)
            && let Some(rows) = v["history"].as_array()
        {
            for row in rows.iter().filter_map(normalize) {
                if !self.history.iter().any(|r| r["id"] == row["id"]) {
                    self.history.push(row)
                }
                if self.history.len() == 200 {
                    break;
                }
            }
            if !user {
                self.pending_seed = if first {
                    Some(v["dnd"] == true)
                } else {
                    v["pendingSeed"].as_bool()
                };
                if let Some(value) = self.pending_seed {
                    self.dnd = value;
                    emit(json!({"event":"seed","value":value}))?;
                }
            }
        }
        Ok(())
    }
    pub(crate) fn publish(&mut self) -> io::Result<()> {
        let mut state = json!({"version":1,"dnd":self.dnd,"history":self.history.iter().cloned().map(archived).collect::<Vec<_>>()});
        if let Some(value) = self.pending_seed {
            state["pendingSeed"] = json!(value);
        }
        let archive = serde_json::to_vec(&state)?;
        if archive != self.saved {
            if archive.len() <= 8 * 1024 * 1024 && save(&self.path, &archive).is_ok() {
                self.saved = archive
            } else {
                emit(json!({"event":"error","code":"archive"}))?
            }
        }
        emit(json!({"event":"notifications","history":self.history,"popups":self.popups}))
    }
    pub(crate) fn tick(&mut self, now: Instant, mut delay: Duration) -> io::Result<Duration> {
        for l in &mut self.live {
            if let Some(t) = l.due {
                if t <= now {
                    l.operation("expire", "")?;
                    l.due = None
                } else {
                    delay = delay.min(t - now)
                }
            }
        }
        Ok(delay)
    }
    pub(crate) fn handle(&mut self, cmd: Notice) -> io::Result<()> {
        match cmd {
            Notice::Init { dnd, user } if self.path.as_os_str().is_empty() => {
                let path = env::var_os("NOTIFICATION_HISTORY")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| home().join("dashboard/notifications.json"));
                let legacy = env::var_os("LEGACY_NOTIFICATION_HISTORY")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| home().join("quickshell/notifications.json"));
                self.restore(path, &legacy, dnd, user)?;
            }
            _ if self.path.as_os_str().is_empty() => return Ok(()),
            Notice::Init { .. } => return Ok(()),
            Notice::Received {
                native,
                time,
                token,
                row,
                restore,
            } => {
                if token.len() > 64 || token.is_empty() {
                    return Ok(());
                }
                let mut row = serde_json::to_value(row)?;
                row["closed"] = json!(false);
                let prev = self
                    .live
                    .iter()
                    .position(|l| l.native == native)
                    .map(|i| self.live.remove(i));
                let mut l = Live {
                    native,
                    token: token.clone(),
                    row: Value::Null,
                    hover: prev.as_ref().is_some_and(|p| p.hover),
                    due: None,
                };
                if restore {
                    let prefix = format!("astal:{native}:");
                    let found = self.history.iter_mut().find(|r| {
                        let key = r["id"].as_str().unwrap_or("");
                        if !key
                            .strip_prefix(&prefix)
                            .and_then(|s| s.split(':').next()?.parse::<u64>().ok())
                            .is_some_and(|t| t <= time)
                        {
                            return false;
                        }
                        row["id"] = json!(key);
                        archived(row.clone()) == archived((*r).clone())
                    });
                    let Some(r) = found else {
                        return l.operation("expire", "");
                    };
                    if row["icon"]
                        .as_str()
                        .is_some_and(|s| s.starts_with("image://"))
                    {
                        row["icon"] = json!("");
                    }
                    row["token"] = json!(token);
                    *r = row.clone();
                } else {
                    row["id"] = prev
                        .map(|l| l.row["id"].clone())
                        .unwrap_or_else(|| json!(format!("astal:{native}:{time}:{token}")));
                    row["token"] = json!(token);
                    for (rows, admit, max) in [
                        (&mut self.history, row["transient"] != true, 200),
                        (&mut self.popups, !self.dnd, 5),
                    ] {
                        rows.retain(|r| r["id"] != row["id"]);
                        if admit {
                            rows.insert(0, row.clone());
                            rows.truncate(max)
                        }
                    }
                }
                l.row = row;
                l.restart();
                self.live.push(l)
            }
            Notice::Resolved { token, reason } => {
                let Some(i) = self.live.iter().position(|l| l.token == token) else {
                    return Ok(());
                };
                let id = self.live.remove(i).row["id"].clone();
                self.popups.retain(|r| r["id"] != id);
                if reason == 1 {
                    if let Some(r) = self.history.iter_mut().find(|r| r["id"] == id) {
                        *r = archived(r.clone())
                    }
                } else {
                    self.history.retain(|r| r["id"] != id)
                }
            }
            Notice::SeedAck { .. } if self.pending_seed.is_none() => return Ok(()),
            Notice::Dnd { value } | Notice::SeedAck { value } => {
                if matches!(cmd, Notice::SeedAck { .. }) {
                    self.pending_seed = None;
                }
                self.dnd = value;
                if value {
                    for l in &mut self.live {
                        if self.popups.iter().any(|r| r["token"] == l.token) {
                            l.hover = false;
                            l.restart()
                        }
                    }
                    self.popups.clear()
                }
            }
            Notice::Toggle => return emit(json!({"event":"toggle"})),
            Notice::Hover { id, token, value } => {
                if let Some(l) = self
                    .live
                    .iter_mut()
                    .find(|l| l.row["id"] == id && l.token == token)
                    && l.hover != value
                    && (!value || self.popups.iter().any(|r| r["id"] == id))
                {
                    l.hover = value;
                    l.restart()
                }
                return Ok(());
            }
            Notice::Action {
                id,
                token,
                identifier,
            } => {
                if let Some(l) = self
                    .live
                    .iter()
                    .find(|l| l.row["id"] == id && l.token == token)
                    && l.row["actions"]
                        .as_array()
                        .is_some_and(|a| a.iter().any(|a| a["identifier"] == identifier))
                {
                    l.operation("invoke", &identifier)?
                }
                return Ok(());
            }
            Notice::Dismiss { id, token } => {
                if let Some(i) = self.live.iter().position(|l| l.row["id"] == id) {
                    if self.live[i].token != token {
                        return Ok(());
                    }
                    self.live.remove(i).operation("dismiss", "")?
                } else if !token.is_empty() {
                    // A delayed click on a resolved instance is not a click on its archive row.
                    return Ok(());
                }
                self.history.retain(|r| r["id"] != id);
                self.popups.retain(|r| r["id"] != id)
            }
            Notice::Clear { instances } => {
                // Include native objects whose notified signal has not yet reached the adapter.
                for i in instances {
                    if !self
                        .live
                        .iter()
                        .any(|l| l.token == i.token && l.native == i.native)
                    {
                        emit(
                            json!({"event":"native","operation":"dismiss","native":i.native,"token":i.token,"argument":""}),
                        )?;
                    }
                }
                for l in self.live.drain(..) {
                    l.operation("dismiss", "")?
                }
                self.history.clear();
                self.popups.clear()
            }
        }
        // The byte budget also caps large archives with fewer than 200 rows.
        let mut bytes = 128;
        self.history.retain(|r| {
            bytes += r.to_string().len() + 32;
            bytes <= 8 * 1024 * 1024
        });
        let mut i = 0;
        while i < self.live.len() {
            let l = &mut self.live[i];
            let visible = self.popups.iter().any(|r| r["token"] == l.token);
            if !visible && !self.history.iter().any(|r| r["token"] == l.token) {
                self.live.remove(i).operation("expire", "")?;
                continue;
            }
            if !visible && l.hover {
                l.hover = false;
                l.restart()
            }
            i += 1
        }
        self.publish()
    }
}
