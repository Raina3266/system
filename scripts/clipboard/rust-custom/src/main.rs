//! Clipboard capture and history policy. Only GTK/GDK publishes clipboard data.
//!
//! JSON snapshots add non-persisted `can_trash` and opaque `delete_token` fields;
//! `delete` requires the displayed token as `expected`. `observe` streams JSONL
//! `{ "pruneMissing": bool }` invalidations until its output closes or it is killed.
//! Trash uses CLIPBOARD_GIO (default: gio) with no shell or permanent fallback.

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
    Delete { id: u64, expected: String },
    Save { id: u64, base: String, text: String },
    Payload { id: u64 },
}

fn execute(store: &Store, request: Request) -> Result<Value> {
    match request {
        Request::Prepare {} => store.prepare(),
        Request::Snapshot {} => store.snapshot(),
        Request::Memo {} => store.memo(),
        Request::Pin { id } => store.pin(id),
        Request::Delete { id, expected } => store.delete(id, &expected),
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
        [command] if command == "observe" => observer::run(),
        [command, flag, mime] if command == "store" && flag == "--mime" => {
            ensure!(
                !mime.is_empty() && !mime.contains(['\n', '\r', '\0']),
                "Invalid MIME type"
            );
            capture::store_stdin(mime)
        }
        _ => bail!(
            "Usage: clipboard-custom [capture | store --mime MIME | observe]; without arguments, read one JSON request"
        ),
    }
}

// A panel-scoped, read-only inotify stream. No filesystem policy or debounce
// timers live in JS, and idle observation blocks in poll rather than waking up.
mod observer {
    use super::store::Store;
    use anyhow::{Context, Result, ensure};
    use std::collections::{BTreeMap, BTreeSet};
    use std::ffi::{CString, OsStr, OsString};
    use std::fs::File;
    use std::io::{Read, Write};
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::ffi::OsStrExt;
    use std::path::{Component, Path, PathBuf};
    use std::time::{Duration, Instant};

    const DEBOUNCE: Duration = Duration::from_millis(100);
    const SELF_EVENTS: u32 =
        libc::IN_DELETE_SELF | libc::IN_MOVE_SELF | libc::IN_UNMOUNT | libc::IN_IGNORED;
    const MASK: u32 = libc::IN_MODIFY
        | libc::IN_CLOSE_WRITE
        | libc::IN_CREATE
        | libc::IN_DELETE
        | libc::IN_MOVED_FROM
        | libc::IN_MOVED_TO
        | libc::IN_ATTRIB
        | SELF_EVENTS
        | libc::IN_ONLYDIR;

    #[derive(Clone, Debug, Default, PartialEq)]
    struct Interest {
        history: bool,
        files: BTreeSet<OsString>,
    }

    #[derive(Clone, Copy, Debug, Default, PartialEq)]
    struct Changes {
        history: bool,
        prune: bool,
    }
    impl Changes {
        fn merge(&mut self, other: Self) {
            self.history |= other.history;
            self.prune |= other.prune;
        }
        fn any(self) -> bool {
            self.history || self.prune
        }
    }

    #[derive(Default)]
    struct Debounce {
        changes: Changes,
        due: Option<Instant>,
    }
    impl Debounce {
        fn push(&mut self, changes: Changes, now: Instant) {
            if changes.any() {
                self.changes.merge(changes);
                self.due = Some(now + DEBOUNCE);
            }
        }
        fn timeout(&self, now: Instant) -> i32 {
            self.due.map_or(-1, |due| {
                due.saturating_duration_since(now)
                    .as_nanos()
                    .div_ceil(1_000_000)
                    .min(i32::MAX as u128) as i32
            })
        }
        fn take(&mut self, now: Instant) -> Option<Changes> {
            if self.due.is_some_and(|due| now >= due) {
                self.due = None;
                Some(std::mem::take(&mut self.changes))
            } else {
                None
            }
        }
    }

    // Match GIO's lexical directory grouping, without resolving or following a
    // source symlink. Destructive paths have a separate, stricter validation.
    fn normalized(path: &Path) -> PathBuf {
        let mut result = PathBuf::new();
        for part in path.components() {
            match part {
                Component::ParentDir => {
                    result.pop();
                }
                Component::CurDir => {}
                _ => result.push(part.as_os_str()),
            }
        }
        result
    }

    fn desired(root: &Path, sources: &[PathBuf]) -> BTreeMap<PathBuf, Interest> {
        let mut directories = BTreeMap::<_, Interest>::new();
        directories.entry(root.to_path_buf()).or_default().history = true;
        for source in sources {
            let path = normalized(source);
            if !path.is_absolute() || path.as_os_str().as_bytes().contains(&0) {
                continue;
            }
            if let (Some(parent), Some(name)) = (path.parent(), path.file_name()) {
                directories
                    .entry(parent.to_path_buf())
                    .or_default()
                    .files
                    .insert(name.to_owned());
            }
        }
        directories
    }

    fn classify(interest: &Interest, mask: u32, name: &OsStr) -> Changes {
        let own = mask & SELF_EVENTS != 0;
        Changes {
            history: interest.history && (own || name == "history.json"),
            prune: !interest.files.is_empty()
                && (own
                    || (mask & (libc::IN_DELETE | libc::IN_MOVED_FROM) != 0
                        && interest.files.contains(name))),
        }
    }

    struct Watcher {
        fd: File,
        root: PathBuf,
        directories: BTreeMap<PathBuf, i32>,
        interests: BTreeMap<i32, Interest>,
    }
    impl Watcher {
        fn new(root: &Path) -> Result<Self> {
            // SAFETY: inotify_init1 has no pointer arguments; successful return
            // is a fresh descriptor transferred exactly once to File.
            let fd = unsafe { libc::inotify_init1(libc::IN_CLOEXEC | libc::IN_NONBLOCK) };
            ensure!(fd >= 0, "Cannot create clipboard filesystem observer");
            Ok(Self {
                fd: unsafe { File::from_raw_fd(fd) },
                root: normalized(&std::path::absolute(root)?),
                directories: BTreeMap::new(),
                interests: BTreeMap::new(),
            })
        }
        fn sync(&mut self, sources: &[PathBuf]) -> Result<bool> {
            let wanted = desired(&self.root, sources);
            let mut changed_sources = false;
            let mut retained = BTreeMap::new();
            for (directory, interest) in &wanted {
                let descriptor = self.directories.get(directory).copied().or_else(|| {
                    let name = CString::new(directory.as_os_str().as_bytes()).ok()?;
                    // SAFETY: name is NUL-terminated and fd is owned by self.
                    let descriptor = unsafe {
                        libc::inotify_add_watch(self.fd.as_raw_fd(), name.as_ptr(), MASK)
                    };
                    (descriptor >= 0).then_some(descriptor)
                });
                if let Some(descriptor) = descriptor {
                    let previous = self.interests.get(&descriptor);
                    changed_sources |= interest
                        .files
                        .iter()
                        .any(|file| previous.is_none_or(|old| !old.files.contains(file)));
                    retained.insert(directory.clone(), descriptor);
                } else {
                    ensure!(!interest.history, "Cannot watch clipboard history");
                    // Optional inaccessible/missing source parents are retried
                    // on the next history snapshot, never on a polling timer.
                }
            }
            let kept: BTreeSet<_> = retained.values().copied().collect();
            for descriptor in self
                .interests
                .keys()
                .filter(|descriptor| !kept.contains(descriptor))
            {
                // SAFETY: removing an owned inotify watch; stale queued events
                // are ignored because its interests are removed below.
                unsafe {
                    libc::inotify_rm_watch(self.fd.as_raw_fd(), *descriptor);
                }
            }
            self.directories = retained;
            self.interests.clear();
            for (directory, descriptor) in &self.directories {
                let entry = self.interests.entry(*descriptor).or_default();
                let interest = &wanted[directory];
                entry.history |= interest.history;
                entry.files.extend(interest.files.iter().cloned());
            }
            Ok(changed_sources)
        }
        fn events(&mut self) -> Result<Changes> {
            let mut changes = Changes::default();
            let mut bytes = [0_u8; 64 * 1024];
            // Bound one drain even under heavy unrelated directory traffic.
            for _ in 0..16 {
                let count = match self.fd.read(&mut bytes) {
                    Ok(count) => count,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) => anyhow::bail!("Cannot read clipboard filesystem events"),
                };
                ensure!(count > 0, "Clipboard filesystem observer closed");
                let mut offset = 0;
                while offset + size_of::<libc::inotify_event>() <= count {
                    // SAFETY: the fixed header fits in initialized bytes. The
                    // byte buffer need not have inotify_event's alignment.
                    let event = unsafe {
                        bytes
                            .as_ptr()
                            .add(offset)
                            .cast::<libc::inotify_event>()
                            .read_unaligned()
                    };
                    let start = offset + size_of::<libc::inotify_event>();
                    let end = start + event.len as usize;
                    ensure!(end <= count, "Invalid clipboard filesystem event");
                    let name = &bytes[start..end];
                    let name = OsStr::from_bytes(
                        &name[..name
                            .iter()
                            .position(|byte| *byte == 0)
                            .unwrap_or(name.len())],
                    );
                    if event.mask & libc::IN_Q_OVERFLOW != 0 {
                        changes.merge(Changes {
                            history: true,
                            prune: true,
                        });
                    } else if let Some(interest) = self.interests.get(&event.wd) {
                        changes.merge(classify(interest, event.mask, name));
                        if event.mask & SELF_EVENTS != 0 {
                            self.directories
                                .retain(|_, descriptor| *descriptor != event.wd);
                            self.interests.remove(&event.wd);
                            // A moved directory still has an active kernel watch.
                            unsafe {
                                libc::inotify_rm_watch(self.fd.as_raw_fd(), event.wd);
                            }
                        }
                    }
                    offset = end;
                }
            }
            Ok(changes)
        }
        fn wait(&self, timeout: i32, output: Option<std::os::fd::RawFd>) -> Result<()> {
            let mut fds = [
                libc::pollfd {
                    fd: self.fd.as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                },
                libc::pollfd {
                    fd: output.unwrap_or(-1),
                    events: 0,
                    revents: 0,
                },
            ];
            // Also wake on a closed output pipe: a crashed UI must not leave an
            // idle observer orphaned forever. This adds no timer or polling loop.
            // SAFETY: fds contains two initialized pollfd values for the call.
            let result =
                unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, timeout) };
            if result < 0
                && std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted
            {
                anyhow::bail!("Cannot wait for clipboard filesystem events");
            }
            ensure!(
                fds.iter()
                    .all(|fd| fd.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) == 0),
                "Clipboard filesystem observer failed"
            );
            Ok(())
        }
    }

    fn emit(output: &mut impl Write, prune: bool) -> Result<()> {
        writeln!(output, "{}", serde_json::json!({ "pruneMissing": prune }))?;
        output.flush()?;
        Ok(())
    }

    pub(super) fn run() -> Result<()> {
        let store = Store::discover()?;
        // Discovery/migration and explicit directory creation precede watching.
        store.linked_sources()?;
        let mut watcher = Watcher::new(store.root())?;
        watcher.sync(&[])?;
        let sources = store.linked_sources()?;
        watcher.sync(&sources)?;
        let mut output = std::io::stdout().lock();
        // Reconcile both the initial UI snapshot and source-watch setup gaps.
        emit(&mut output, !sources.is_empty())?;
        let mut pending = Debounce::default();
        loop {
            watcher.wait(pending.timeout(Instant::now()), Some(output.as_raw_fd()))?;
            pending.push(watcher.events()?, Instant::now());
            if let Some(mut changes) = pending.take(Instant::now()) {
                // A bad snapshot must retain old source interests, not silently
                // drop watches or turn read failures into missing-file deletion.
                if let Ok(sources) = store.linked_sources() {
                    changes.prune |= watcher
                        .sync(&sources)
                        .context("Update clipboard observer")?;
                }
                emit(&mut output, changes.prune)?;
            }
        }
    }

    #[cfg(test)]
    crate::clipboard_unit_tests!(observer);
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
