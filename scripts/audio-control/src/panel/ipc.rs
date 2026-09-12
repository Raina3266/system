//! A single running panel, toggled by running the binary again.
//!
//! A near-copy of `media-panel`'s. The two cannot share it without a library
//! crate between them, and the packaging gives each workspace member only its
//! own directory, so seventy lines are duplicated rather than reshaping how
//! every crate here is built.
//!
//! The Waybar button runs `audio-panel <monitor>`. The first run finds
//! no socket, claims it, and becomes the panel; every run after that hands the
//! request to the one already running and exits. That keeps the button's
//! second press a close rather than a second window, without a service file to
//! install or a bus name to reserve.

use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread;
use std::time::Duration;

const SOCKET_NAME: &str = "audio-panel.sock";
const CONNECT_TIMEOUT: Duration = Duration::from_millis(500);

pub fn socket_path() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join(SOCKET_NAME)
}

/// Asks a panel that is already running to toggle. `false` if there is none.
pub fn toggle_running_panel(monitor: &str) -> bool {
    let Ok(mut stream) = UnixStream::connect(socket_path()) else {
        return false;
    };
    let _ = stream.set_write_timeout(Some(CONNECT_TIMEOUT));
    stream.write_all(monitor.as_bytes()).is_ok()
}

/// Removes the socket when the panel exits, so the next run can claim it.
pub struct SocketGuard(PathBuf);

impl Drop for SocketGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Claims the socket and listens on it. Each request yields the monitor the
/// button was pressed on.
pub fn listen() -> std::io::Result<(Receiver<String>, SocketGuard)> {
    let path = socket_path();
    // A socket left behind by a panel that was killed refuses to bind, and
    // nothing answers on it, so it is ours to clear.
    if UnixStream::connect(&path).is_err() {
        let _ = std::fs::remove_file(&path);
    }

    let listener = UnixListener::bind(&path)?;
    let (sender, receiver) = channel();
    thread::spawn(move || accept_loop(&listener, &sender));
    Ok((receiver, SocketGuard(path)))
}

fn accept_loop(listener: &UnixListener, sender: &Sender<String>) {
    for stream in listener.incoming().flatten() {
        let mut stream = stream;
        let _ = stream.set_read_timeout(Some(CONNECT_TIMEOUT));
        let mut monitor = String::new();
        if stream.read_to_string(&mut monitor).is_ok() && sender.send(monitor).is_err() {
            return;
        }
    }
}
