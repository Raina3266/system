//! The counters carried between runs.
//!
//! CPU load and network throughput are both differences between two readings
//! of a monotonically increasing counter, but the widget runs this program as a
//! short-lived command. The previous reading therefore lives in a small file
//! under `$XDG_RUNTIME_DIR`, which the kernel clears at logout.

use std::fs;
use std::io;
use std::path::Path;

/// One complete sample, plus the rates that were derived from it.
///
/// Keeping the derived rates means a run that happens too soon after the last
/// one — opening the control center a moment after a timer tick — can repeat
/// the previous figures instead of dividing by a near-zero interval.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct State {
    pub timestamp: f64,
    pub cpu_busy: u64,
    pub cpu_total: u64,
    pub net_rx: u64,
    pub net_tx: u64,
    pub cpu_percent: f64,
    pub rx_rate: f64,
    pub tx_rate: f64,
}

impl State {
    /// Parse the `key=value` lines written by [`State::serialise`].
    ///
    /// A field that is missing or unparsable falls back to zero: a truncated
    /// file from a killed run should cost one stale tick, not an error.
    pub fn parse(text: &str) -> Self {
        let mut state = State::default();
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            match key.trim() {
                "timestamp" => state.timestamp = value.trim().parse().unwrap_or(0.0),
                "cpu_busy" => state.cpu_busy = value.trim().parse().unwrap_or(0),
                "cpu_total" => state.cpu_total = value.trim().parse().unwrap_or(0),
                "net_rx" => state.net_rx = value.trim().parse().unwrap_or(0),
                "net_tx" => state.net_tx = value.trim().parse().unwrap_or(0),
                "cpu_percent" => state.cpu_percent = value.trim().parse().unwrap_or(0.0),
                "rx_rate" => state.rx_rate = value.trim().parse().unwrap_or(0.0),
                "tx_rate" => state.tx_rate = value.trim().parse().unwrap_or(0.0),
                _ => {}
            }
        }
        state
    }

    pub fn serialise(&self) -> String {
        format!(
            "timestamp={}\ncpu_busy={}\ncpu_total={}\nnet_rx={}\nnet_tx={}\n\
             cpu_percent={}\nrx_rate={}\ntx_rate={}\n",
            self.timestamp,
            self.cpu_busy,
            self.cpu_total,
            self.net_rx,
            self.net_tx,
            self.cpu_percent,
            self.rx_rate,
            self.tx_rate,
        )
    }

    /// A state with no usable history, so the first run shows placeholders
    /// rather than a rate computed against boot time.
    pub fn is_empty(&self) -> bool {
        self.timestamp <= 0.0
    }
}

pub fn load(path: &Path) -> State {
    fs::read_to_string(path)
        .map(|text| State::parse(&text))
        .unwrap_or_default()
}

/// Write the state through a temporary file so a concurrent reader never sees
/// a half-written sample.
pub fn store(path: &Path, state: &State) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension("tmp");
    fs::write(&temporary, state.serialise())?;
    fs::rename(&temporary, path)
}

/// The share of CPU time spent busy between two samples, as a percentage.
///
/// Returns `None` when the counters did not advance, which happens on the
/// first run and whenever two runs land inside the same kernel tick.
pub fn cpu_percent(previous: &State, busy: u64, total: u64) -> Option<f64> {
    let busy_delta = busy.checked_sub(previous.cpu_busy)?;
    let total_delta = total.checked_sub(previous.cpu_total)?;
    (total_delta > 0).then(|| (busy_delta as f64 / total_delta as f64) * 100.0)
}

/// Bytes per second between two counter readings.
pub fn rate(previous: u64, current: u64, seconds: f64) -> Option<f64> {
    if seconds <= 0.0 {
        return None;
    }
    // A counter that went backwards means the interface was replaced or the
    // counter wrapped; there is no meaningful rate to report for that gap.
    let delta = current.checked_sub(previous)?;
    Some(delta as f64 / seconds)
}

#[cfg(test)]
mod tests;
