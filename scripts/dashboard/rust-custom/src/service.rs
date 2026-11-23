//! Bounded stdin framing and the dashboard service event loop.

use std::{
    env,
    io::{self, BufRead, Read},
    sync::mpsc,
    time::{Duration, Instant},
};

use serde_json::json;

use crate::idle::Idle;
use crate::metrics::{Sampler, disk, sample};
use crate::notifications::Center;
use crate::protocol::{Request, emit};
use crate::storage::home;

const FRAME: usize = 128 * 1024;

fn serve() -> io::Result<()> {
    let (tx, rx) = mpsc::sync_channel(8);
    let closed = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let eof = closed.clone();
    std::thread::spawn(move || {
        let mut input = io::stdin().lock();
        loop {
            let mut b = Vec::new();
            if input
                .by_ref()
                .take(FRAME as u64 + 1)
                .read_until(b'\n', &mut b)
                .is_err()
                || b.len() > FRAME
                || b.last() != Some(&b'\n')
            {
                break;
            }
            if tx.send(serde_json::from_slice::<Request>(&b)).is_err() {
                break;
            }
        }
        eof.store(true, std::sync::atomic::Ordering::Release);
    });
    let path = home().join("dashboard/screen-timeout.json");
    let mut idle = Idle::restore(
        path,
        env::var_os("SWAYIDLE").unwrap_or_else(|| "swayidle".into()),
        env::var("SYSTEMCTL").unwrap_or_else(|_| "systemctl".into()),
    );
    let report = |minutes| emit(json!({"event":"timeout","minutes":minutes}));
    report(idle.minutes)?;
    let mut center = Center::default();
    let mut sampler = Sampler::new(Instant::now());
    loop {
        if closed.load(std::sync::atomic::Ordering::Acquire) {
            break;
        }
        let now = Instant::now();
        if let Some(event) = sampler.tick(now, disk, sample) {
            emit(event)?;
        }
        if idle.exited()? {
            report(0)?
        }
        let now = Instant::now();
        let mut delay = sampler.wait(now);
        if idle.child.is_some() {
            delay = delay.min(Duration::from_secs(1))
        }
        let delay = center.tick(now, delay)?;
        match rx.recv_timeout(delay) {
            _ if closed.load(std::sync::atomic::Ordering::Acquire) => break,
            Ok(Ok(Request::Notify { command })) => center.handle(command)?,
            Ok(Ok(Request::Active {
                active: a,
                generation: g,
            })) => {
                sampler.activate(a, g, Instant::now());
            }
            Ok(Ok(Request::Timeout { minutes })) => match idle.set(minutes) {
                Ok(true) => report(minutes)?,
                Ok(false) => (),
                Err(_) => emit(json!({"event":"error","code":"timeout"}))?,
            },
            Ok(Err(_)) => emit(json!({"event":"error","code":"request"}))?,
            Err(mpsc::RecvTimeoutError::Timeout) => (),
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    Ok(())
}
pub(crate) fn run() -> i32 {
    if serve().is_ok() { 0 } else { 1 }
}
