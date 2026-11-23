//! Bounded JSONL framing, nonblocking I/O, bus startup, and the polling loop.

use std::collections::VecDeque;
use std::io;
use std::time::{Duration, Instant};

use dbus::channel::{BusType, Channel};
use serde_json::Value;

use crate::bus::{Failure, READ_TIME};
use crate::engine::Engine;
use crate::protocol::REQUEST_LIMIT;

pub(crate) const OUTPUT_LIMIT: usize = 1024 * 1024;

struct Nonblocking {
    fd: i32,
    flags: i32,
}
impl Nonblocking {
    fn new(fd: i32) -> io::Result<Self> {
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self { fd, flags })
    }
}
impl Drop for Nonblocking {
    fn drop(&mut self) {
        unsafe {
            libc::fcntl(self.fd, libc::F_SETFL, self.flags);
        }
    }
}
#[derive(Default)]
pub(crate) struct Input {
    pub(crate) line: Vec<u8>,
    overlong: bool,
}
impl Input {
    pub(crate) fn feed(&mut self, bytes: &[u8], engine: &mut Engine, now: Instant) {
        for byte in bytes {
            if *byte == b'\n' {
                if self.overlong {
                    engine.reply(Value::Null, Err(Failure::invalid()));
                } else {
                    engine.enqueue(&self.line, now);
                }
                self.line.fill(0);
                self.line.clear();
                self.overlong = false;
            } else if !self.overlong {
                if self.line.len() == REQUEST_LIMIT {
                    self.line.fill(0);
                    self.line.clear();
                    self.overlong = true;
                } else {
                    self.line.push(*byte);
                }
            }
            if engine.is_stopped() {
                break;
            }
        }
    }
    fn read(&mut self, fd: i32, engine: &mut Engine, now: Instant) -> io::Result<bool> {
        let mut bytes = [0u8; 8192];
        for _ in 0..32 {
            let count = unsafe { libc::read(fd, bytes.as_mut_ptr().cast(), bytes.len()) };
            if count == 0 {
                self.line.fill(0);
                engine.stop();
                return Ok(false);
            }
            if count < 0 {
                let error = io::Error::last_os_error();
                if error.kind() == io::ErrorKind::WouldBlock {
                    return Ok(true);
                }
                if error.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(error);
            }
            self.feed(&bytes[..count as usize], engine, now);
            bytes.fill(0);
            if engine.is_stopped() {
                return Ok(false);
            }
        }
        Ok(true)
    }
}
#[derive(Default)]
pub(crate) struct Output {
    pub(crate) replies: VecDeque<Vec<u8>>,
    pub(crate) state: Option<Vec<u8>>,
    current: Option<(Vec<u8>, usize)>,
    bytes: usize,
}
impl Output {
    pub(crate) fn push(&mut self, value: Value) -> bool {
        let mut bytes = match serde_json::to_vec(&value) {
            Ok(bytes) => bytes,
            Err(_) => return false,
        };
        bytes.push(b'\n');
        if value["type"] == "state"
            && let Some(mut previous) = self.state.take()
        {
            self.bytes -= previous.len();
            previous.fill(0);
        }
        if self.bytes.saturating_add(bytes.len()) > OUTPUT_LIMIT {
            bytes.fill(0);
            return false;
        }
        self.bytes += bytes.len();
        if value["type"] == "state" {
            self.state = Some(bytes);
        } else {
            self.replies.push_back(bytes);
        }
        true
    }
    fn write(&mut self, fd: i32) -> io::Result<()> {
        let mut budget = 65536;
        while budget > 0 {
            if self.current.is_none() {
                self.current = self
                    .state
                    .take()
                    .or_else(|| self.replies.pop_front())
                    .map(|bytes| (bytes, 0));
            }
            let Some((bytes, offset)) = &mut self.current else {
                return Ok(());
            };
            let count = unsafe {
                libc::write(
                    fd,
                    bytes[*offset..].as_ptr().cast(),
                    (bytes.len() - *offset).min(budget),
                )
            };
            if count < 0 {
                let error = io::Error::last_os_error();
                if error.kind() == io::ErrorKind::WouldBlock {
                    return Ok(());
                }
                if error.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(error);
            }
            if count == 0 {
                return Err(io::Error::from(io::ErrorKind::WriteZero));
            }
            let count = count as usize;
            *offset += count;
            self.bytes -= count;
            budget -= count;
            if *offset == bytes.len() {
                bytes.fill(0);
                self.current = None;
            }
        }
        Ok(())
    }
}
impl Drop for Output {
    fn drop(&mut self) {
        if let Some((bytes, _)) = &mut self.current {
            bytes.fill(0);
        }
        if let Some(bytes) = &mut self.state {
            bytes.fill(0);
        }
        for bytes in &mut self.replies {
            bytes.fill(0);
        }
    }
}
pub(crate) fn run_loop(
    input_fd: i32,
    output_fd: i32,
    receiver: std::sync::mpsc::Receiver<Option<Channel>>,
) -> i32 {
    let _input_flags = match Nonblocking::new(input_fd) {
        Ok(flags) => flags,
        Err(_) => return 1,
    };
    let _output_flags = match Nonblocking::new(output_fd) {
        Ok(flags) => flags,
        Err(_) => return 1,
    };
    let started = Instant::now();
    let mut engine = Engine::new(started);
    let mut input = Input::default();
    let mut output = Output::default();
    let mut channel: Option<Channel> = None;
    let mut connecting = true;
    let mut output_progress = started;
    loop {
        let now = Instant::now();
        // Input is drained before dispatching any replies/continuations. A
        // cancelled request and an EOF can never schedule its next NM call.
        match input.read(input_fd, &mut engine, now) {
            Ok(false) => return 0,
            Err(_) => {
                engine.stop();
                return 1;
            }
            _ => (),
        }
        if engine.is_stopped() {
            return 1;
        }
        if connecting {
            match receiver.try_recv() {
                Ok(connection) => {
                    channel = connection;
                    connecting = false;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => connecting = false,
                _ if now.duration_since(started) >= READ_TIME => connecting = false,
                _ => (),
            }
            if let Some(channel) = &mut channel {
                channel.set_watch_enabled(true);
                if engine.bootstrap(channel, now).is_err() {
                    engine.stop();
                    return 1;
                }
            }
        }
        let mut bus_backlog = false;
        if let Some(bus) = &channel {
            if bus.read_write(Some(Duration::ZERO)).is_err() {
                engine.set_owner(String::new(), now);
                channel = None;
            } else {
                for index in 0..128 {
                    let Some(message) = bus.pop_message() else {
                        break;
                    };
                    engine.receive(bus, message, now);
                    if engine.is_stopped() {
                        return 1;
                    }
                    bus_backlog = index == 127;
                }
                if !bus_backlog {
                    engine.tick(bus, now);
                }
            }
        } else if !connecting {
            engine.fail_unavailable();
        }
        if output.bytes == 0 {
            output_progress = now;
        }
        while let Some(value) = engine.pop_output() {
            if !output.push(value) {
                engine.stop();
                return 1;
            }
        }
        let before_write = output.bytes;
        if output.write(output_fd).is_err() {
            engine.stop();
            return 1;
        }
        if output.bytes < before_write {
            output_progress = now;
        }
        if output.bytes != 0 && now.duration_since(output_progress) >= READ_TIME {
            engine.stop();
            return 1;
        }
        let watch = channel.as_ref().map(Channel::watch);
        let mut fds = [
            libc::pollfd {
                fd: input_fd,
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: if output.bytes == 0 { -1 } else { output_fd },
                events: libc::POLLOUT,
                revents: 0,
            },
            libc::pollfd {
                fd: watch.map_or(-1, |watch| watch.fd),
                events: watch.map_or(0, |watch| {
                    (if watch.read { libc::POLLIN } else { 0 })
                        | (if watch.write { libc::POLLOUT } else { 0 })
                }),
                revents: 0,
            },
        ];
        if unsafe {
            libc::poll(
                fds.as_mut_ptr(),
                fds.len() as libc::nfds_t,
                if bus_backlog { 0 } else { 50 },
            )
        } < 0
            && io::Error::last_os_error().kind() != io::ErrorKind::Interrupted
        {
            engine.stop();
            return 1;
        }
    }
}
pub(crate) fn run() -> i32 {
    // Bus authentication is the only blocking libdbus operation. Isolate it
    // from stdin/cancellation, bound startup, and never reconnect/retry calls.
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    if std::thread::Builder::new()
        .name("network-bus-connect".to_owned())
        .spawn(move || {
            let _ = sender.send(Channel::get_private(BusType::System).ok());
        })
        .is_err()
    {
        return 1;
    }
    run_loop(0, 1, receiver)
}
