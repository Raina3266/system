//! Linux telemetry and visibility-gated sampling with generation tracking.

use std::{
    path::Path,
    time::{Duration, Instant},
};

use procfs::{CpuTime, Current, CurrentSI, KernelStats, Meminfo};
use serde_json::{Value, json};

use crate::storage::limited;

pub(crate) fn cpu(c: &CpuTime) -> Option<(u64, u64)> {
    // Guest time is already included in user/nice; iowait counts as idle.
    let total = [
        c.user,
        c.nice,
        c.system,
        c.idle,
        c.iowait.unwrap_or(0),
        c.irq.unwrap_or(0),
        c.softirq.unwrap_or(0),
        c.steal.unwrap_or(0),
    ]
    .into_iter()
    .try_fold(0u64, u64::checked_add)?;
    Some((total, c.idle.checked_add(c.iowait.unwrap_or(0))?))
}
pub(crate) fn percent(prev: Option<(u64, u64)>, next: Option<(u64, u64)>) -> Option<f64> {
    let (t, i) = prev?;
    let (n, j) = next?;
    let dt = n.checked_sub(t)?;
    let busy = dt.checked_sub(j.checked_sub(i)?)?;
    (dt > 0).then(|| (100. * busy as f64 / dt as f64).round())
}
pub(crate) fn memory(m: &Meminfo) -> Option<[f64; 3]> {
    let t = m.mem_total;
    if t == 0 {
        return None;
    }
    let a = m
        .mem_available
        .or_else(|| m.mem_free.checked_add(m.buffers)?.checked_add(m.cached))?;
    // procfs normalizes meminfo's kB values to bytes.
    let u = t.saturating_sub(a) as f64;
    Some([
        (100. * u / t as f64).round(),
        u / 1073741824.,
        t as f64 / 1073741824.,
    ])
}
pub(crate) fn disk() -> Option<[f64; 3]> {
    let mut s = std::mem::MaybeUninit::uninit(); // statvfs initializes the structure on success.
    if unsafe { libc::statvfs(c"/".as_ptr(), s.as_mut_ptr()) } != 0 {
        return None;
    }
    let s = unsafe { s.assume_init() };
    disk_usage(s.f_blocks, s.f_bfree, s.f_bavail, s.f_frsize)
}
pub(crate) fn disk_usage(blocks: u64, free: u64, avail: u64, size: u64) -> Option<[f64; 3]> {
    let used = blocks.checked_sub(free)? as f64;
    let avail = used + avail as f64;
    let scale = size as f64 / 1073741824.;
    (blocks > 0 && avail > 0. && size > 0).then(|| {
        [
            (100. * used / avail).ceil(),
            used * scale,
            blocks as f64 * scale,
        ]
    })
}
pub(crate) fn temperature(s: &str) -> Option<f64> {
    let s = s.trim();
    if !s.bytes().all(|b| b.is_ascii_digit() || b == b'.') {
        return None;
    }
    let t = s.parse::<f64>().ok()?;
    (t.is_finite() && t > 0.).then(|| if t > 1000. { t / 1000. } else { t })
}
pub(crate) fn sample(prev: &mut Option<(u64, u64)>, d: [f64; 3]) -> Value {
    let n = KernelStats::current().ok().and_then(|s| cpu(&s.total));
    let c = percent(*prev, n);
    *prev = n;
    let m = Meminfo::current()
        .ok()
        .and_then(|m| memory(&m))
        .unwrap_or([-1.; 3]);
    let temp = limited(
        Path::new("/sys/class/thermal/thermal_zone0/temp"),
        1024 * 1024,
    )
    .ok()
    .and_then(|b| String::from_utf8(b).ok())
    .and_then(|s| temperature(&s));
    json!({"event":"metrics","data":{"cpuPercent":c.unwrap_or(-1.),"memoryPercent":m[0],"memoryUsedGiB":m[1],"memoryTotalGiB":m[2],"diskPercent":d[0],"diskUsedGiB":d[1],"diskTotalGiB":d[2],"cpuTemperatureC":temp.unwrap_or(-1.)}})
}
pub(crate) struct Sampler {
    active: bool,
    generation: u64,
    next: Instant,
    disk_due: Instant,
    disk: [f64; 3],
    previous: Option<(u64, u64)>,
}
impl Sampler {
    pub(crate) fn new(now: Instant) -> Self {
        Self {
            active: false,
            generation: 0,
            next: now,
            disk_due: now,
            disk: [-1.; 3],
            previous: None,
        }
    }
    pub(crate) fn activate(&mut self, active: bool, generation: u64, now: Instant) {
        self.generation = generation;
        if self.active != active {
            self.active = active;
            self.next = now;
            self.disk_due = now;
            self.previous = None;
        }
    }
    pub(crate) fn tick(
        &mut self,
        now: Instant,
        read_disk: impl FnOnce() -> Option<[f64; 3]>,
        read_metrics: impl FnOnce(&mut Option<(u64, u64)>, [f64; 3]) -> Value,
    ) -> Option<Value> {
        if !self.active || now < self.next {
            return None;
        }
        if now >= self.disk_due {
            self.disk = read_disk().unwrap_or([-1.; 3]);
            self.disk_due = now + Duration::from_secs(30);
        }
        let mut event = read_metrics(&mut self.previous, self.disk);
        event["generation"] = json!(self.generation);
        self.next = now + Duration::from_millis(2500);
        Some(event)
    }
    pub(crate) fn wait(&self, now: Instant) -> Duration {
        if self.active {
            self.next.saturating_duration_since(now)
        } else {
            Duration::from_secs(3600)
        }
    }
}
