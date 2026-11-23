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

use anyhow::{Context, Result, ensure};
use clap::{CommandFactory, Parser, error::ErrorKind};
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

#[derive(Parser)]
#[command(
    name = "clipboard-custom",
    version,
    about = "Clipboard capture and history policy",
    after_help = "Without arguments, read one JSON history request from standard input.",
    disable_help_subcommand = true
)]
enum Cli {
    /// Capture the current Wayland clipboard.
    Capture,
    /// Stream history and linked-source invalidations as JSONL.
    Observe,
    /// Store clipboard data read from standard input.
    Store {
        #[arg(long, value_name = "MIME", allow_hyphen_values = true)]
        mime: String,
    },
}

fn run_cli(command: Cli) -> Result<()> {
    match command {
        Cli::Capture => capture::capture_clipboard(),
        Cli::Observe => observer::run(),
        Cli::Store { mime } => {
            ensure!(
                !mime.is_empty() && !mime.contains(['\n', '\r', '\0']),
                "Invalid MIME type"
            );
            capture::store_stdin(&mime)
        }
    }
}

// A panel-scoped, read-only inotify stream. No filesystem policy or debounce
// timers live in JS, and idle observation blocks in poll rather than waking up.
mod observer {
    use super::store::Store;
    use anyhow::{Context, Result, ensure};
    use inotify::{Inotify, WatchDescriptor, WatchMask};
    use std::collections::{BTreeMap, BTreeSet};
    use std::ffi::{OsStr, OsString};
    use std::io::Write;
    use std::os::fd::AsRawFd;
    use std::os::unix::ffi::OsStrExt;
    use std::path::{Component, Path, PathBuf};
    use std::time::{Duration, Instant};

    const DEBOUNCE: Duration = Duration::from_millis(100);
    const SELF_EVENTS: u32 =
        libc::IN_DELETE_SELF | libc::IN_MOVE_SELF | libc::IN_UNMOUNT | libc::IN_IGNORED;
    // Keep the Linux mask, including UNMOUNT/IGNORED bits not named by WatchMask.
    const MASK: WatchMask = WatchMask::from_bits_retain(
        libc::IN_MODIFY
            | libc::IN_CLOSE_WRITE
            | libc::IN_CREATE
            | libc::IN_DELETE
            | libc::IN_MOVED_FROM
            | libc::IN_MOVED_TO
            | libc::IN_ATTRIB
            | SELF_EVENTS
            | libc::IN_ONLYDIR,
    );

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
        inotify: Inotify,
        root: PathBuf,
        directories: BTreeMap<PathBuf, WatchDescriptor>,
        interests: BTreeMap<WatchDescriptor, Interest>,
    }
    impl Watcher {
        fn new(root: &Path) -> Result<Self> {
            Ok(Self {
                inotify: Inotify::init().context("Cannot create clipboard filesystem observer")?,
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
                let descriptor = self
                    .directories
                    .get(directory)
                    .cloned()
                    .or_else(|| self.inotify.watches().add(directory, MASK).ok());
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
            let kept: BTreeSet<_> = retained.values().cloned().collect();
            for descriptor in self
                .interests
                .keys()
                .filter(|descriptor| !kept.contains(descriptor))
            {
                // Stale queued events are ignored once interests are removed.
                let _ = self.inotify.watches().remove(descriptor.clone());
            }
            self.directories = retained;
            self.interests.clear();
            for (directory, descriptor) in &self.directories {
                let entry = self.interests.entry(descriptor.clone()).or_default();
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
                let events = match self.inotify.read_events(&mut bytes) {
                    Ok(events) => events,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => {
                        anyhow::bail!("Clipboard filesystem observer closed")
                    }
                    Err(_) => anyhow::bail!("Cannot read clipboard filesystem events"),
                };
                for event in events {
                    let mask = event.mask.bits();
                    if mask & libc::IN_Q_OVERFLOW != 0 {
                        changes.merge(Changes {
                            history: true,
                            prune: true,
                        });
                    } else if let Some(interest) = self.interests.get(&event.wd) {
                        changes.merge(classify(interest, mask, event.name.unwrap_or_default()));
                        if mask & SELF_EVENTS != 0 {
                            self.directories
                                .retain(|_, descriptor| *descriptor != event.wd);
                            self.interests.remove(&event.wd);
                            // A moved directory still has an active kernel watch.
                            let _ = self.inotify.watches().remove(event.wd);
                        }
                    }
                }
            }
            Ok(changes)
        }
        fn wait(&self, timeout: i32, output: Option<std::os::fd::RawFd>) -> Result<()> {
            let mut fds = [
                libc::pollfd {
                    fd: self.inotify.as_raw_fd(),
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
    if std::env::args_os().len() > 1 {
        let cli = Cli::try_parse().unwrap_or_else(|error| {
            if matches!(
                error.kind(),
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
            ) {
                error.exit();
            }
            // Clap diagnostics can echo arbitrary arguments, including private MIME
            // values. Render only static usage and retain the CLI's failure status.
            eprintln!("clipboard-custom: {}", Cli::command().render_usage());
            std::process::exit(1);
        });
        if let Err(error) = run_cli(cli) {
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
