//! The system readings row: CPU, memory, temperature, disk and network.
//!
//! The readings that used to live in the Waybar `group/hardware` drawer, as one
//! Pango markup block for SwayNC's patched `label` widget.
//!
//! The row is rendered once per refresh rather than by a daemon, so the
//! counters needed for CPU load and network throughput are carried between runs
//! in a small state file. See `state.rs`.

use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::format::{self, Level, Row};
use crate::state::{self, State};
use crate::{non_empty, parse, path_from_environment};

/// How long the seeding run waits before taking the reading it reports.
///
/// Only the very first run after login pays this: every later run differences
/// against the sample its predecessor stored.
const SEED_DELAY: Duration = Duration::from_millis(250);

/// Below this gap two runs are treated as the same moment, and the previously
/// derived rates are repeated instead of dividing by a near-zero interval.
const MIN_INTERVAL_SECONDS: f64 = 0.25;

/// The glyph that names each row, now that the rows carry no words.
mod icon {
    pub const CPU: &str = "󰻠";
    pub const MEMORY: &str = "󰍛";
    pub const SWAP: &str = "󰓡";
    pub const TEMPERATURE: &str = "󰄏";
    pub const TEMPERATURE_HOT: &str = "󰄅";
    pub const DISK: &str = "󰋊";
    pub const WIRELESS: &str = "󰖩";
    pub const WIRED: &str = "󰈀";
    pub const TUNNEL: &str = "󰛳";
    pub const DISCONNECTED: &str = "󰖪";
}

/// Print the block, as Pango markup unless `markup` is false.
pub(crate) fn render(markup: bool) {
    let config = Config::from_environment();
    println!("{}", format::block(&collect(&config), markup));
}

/// Everything the program reads from the environment, resolved once.
pub(crate) struct Config {
    proc_root: PathBuf,
    sys_root: PathBuf,
    df: OsString,
    disk_path: OsString,
    thermal_zone: Option<String>,
    interface: Option<String>,
    state_path: PathBuf,
}

impl Config {
    fn from_environment() -> Self {
        Config {
            proc_root: path_from_environment("SWAYNC_PANEL_PROC", "/proc"),
            sys_root: path_from_environment("SWAYNC_PANEL_SYS", "/sys"),
            df: env::var_os("SWAYNC_PANEL_DF").unwrap_or_else(|| OsString::from("df")),
            thermal_zone: non_empty("SWAYNC_PANEL_THERMAL_ZONE"),
            disk_path: env::var_os("SWAYNC_PANEL_DISK").unwrap_or_else(|| OsString::from("/")),
            interface: non_empty("SWAYNC_PANEL_INTERFACE"),
            state_path: state_path(),
        }
    }

    fn read_proc(&self, name: &str) -> Option<String> {
        fs::read_to_string(self.proc_root.join(name)).ok()
    }
}

/// The state file lives in the runtime directory so it disappears at logout;
/// `/tmp` is only a fallback for sessions that do not set one.
pub(crate) fn state_path() -> PathBuf {
    if let Some(path) = env::var_os("SWAYNC_PANEL_STATE").filter(|value| !value.is_empty()) {
        return PathBuf::from(path);
    }
    let directory = env::var_os("XDG_RUNTIME_DIR")
        .filter(|value| !value.is_empty())
        .map_or_else(|| PathBuf::from("/tmp"), PathBuf::from);
    directory.join("swaync-sysmon").join("state")
}

pub(crate) fn now_seconds() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_secs_f64())
        .unwrap_or(0.0)
}

/// Build every row the machine can currently supply, in display order.
pub(crate) fn collect(config: &Config) -> Vec<Row> {
    collect_with_clock(config, now_seconds)
}

/// Allow tests to control sample times independently of filesystem latency.
pub(crate) fn collect_with_clock(config: &Config, clock: impl Fn() -> f64) -> Vec<Row> {
    // A machine with no readable routing table has no networking to report.
    // One that has a routing table but no default route is disconnected, which
    // is worth a row of its own.
    let route = config.read_proc("net/route");
    let interface = config
        .interface
        .clone()
        .or_else(|| route.as_deref().and_then(parse::default_interface));

    let mut previous = state::load(&config.state_path);
    if previous.is_empty() {
        // No history: take one reading, wait, and let the reading below
        // difference against it, so the first panel that opens is not blank.
        previous = sample(config, interface.as_deref(), &State::default(), clock());
        thread::sleep(SEED_DELAY);
    }

    let current = sample(config, interface.as_deref(), &previous, clock());
    if let Err(error) = state::store(&config.state_path, &current) {
        eprintln!(
            "swaync-sysmon: could not write {}: {error}",
            config.state_path.display()
        );
    }

    let mut rows = Vec::new();
    rows.extend(cpu_row(config, &current));
    rows.extend(temperature_row(config));
    rows.extend(memory_rows(config));
    rows.extend(disk_row(config));
    if route.is_some() || interface.is_some() {
        rows.extend(network_row(&current, interface.as_deref()));
    }
    rows
}

/// Read the counters and derive the rates against `previous`.
fn sample(config: &Config, interface: Option<&str>, previous: &State, timestamp: f64) -> State {
    let cpu = config
        .read_proc("stat")
        .as_deref()
        .and_then(parse::cpu_times)
        .unwrap_or(parse::CpuTimes { busy: 0, total: 0 });
    let net = config
        .read_proc("net/dev")
        .as_deref()
        .and_then(|text| parse::net_counters(text, interface))
        .unwrap_or(parse::NetCounters { rx: 0, tx: 0 });

    let elapsed = timestamp - previous.timestamp;
    let fresh = !previous.is_empty() && elapsed >= MIN_INTERVAL_SECONDS;

    State {
        timestamp,
        cpu_busy: cpu.busy,
        cpu_total: cpu.total,
        net_rx: net.rx,
        net_tx: net.tx,
        cpu_percent: if fresh {
            state::cpu_percent(previous, cpu.busy, cpu.total).unwrap_or(previous.cpu_percent)
        } else {
            previous.cpu_percent
        },
        rx_rate: if fresh {
            state::rate(previous.net_rx, net.rx, elapsed).unwrap_or(previous.rx_rate)
        } else {
            previous.rx_rate
        },
        tx_rate: if fresh {
            state::rate(previous.net_tx, net.tx, elapsed).unwrap_or(previous.tx_rate)
        } else {
            previous.tx_rate
        },
    }
}

fn cpu_row(config: &Config, current: &State) -> Option<Row> {
    if current.cpu_total == 0 {
        return None;
    }
    let percent = current.cpu_percent.clamp(0.0, 100.0);
    let row = Row::new(icon::CPU, format!("{percent:.0}%"))
        .level(Level::from_thresholds(percent, 80.0, 95.0));
    Some(
        match config
            .read_proc("cpuinfo")
            .as_deref()
            .and_then(parse::cpu_mhz)
        {
            Some(mhz) => row.detail(format!("{:.2}GHz", mhz / 1000.0)),
            None => row,
        },
    )
}

/// Memory always produces a row; swap only appears once something is in it.
fn memory_rows(config: &Config) -> Vec<Row> {
    let Some(memory) = config
        .read_proc("meminfo")
        .as_deref()
        .and_then(parse::memory_kib)
    else {
        return Vec::new();
    };
    if memory.total == 0 {
        return Vec::new();
    }

    let used = memory.total.saturating_sub(memory.available);
    let percent = (used as f64 / memory.total as f64) * 100.0;
    let mut rows = vec![
        Row::new(
            icon::MEMORY,
            format!(
                "{}/{}",
                format::gibibytes(used),
                format::gibibytes(memory.total)
            ),
        )
        .detail(format!("{percent:.0}%"))
        .level(Level::from_thresholds(percent, 80.0, 90.0)),
    ];

    let swap_used = memory.swap_total.saturating_sub(memory.swap_free);
    if memory.swap_total > 0 && swap_used > 0 {
        let swap_percent = (swap_used as f64 / memory.swap_total as f64) * 100.0;
        rows.push(
            Row::new(
                icon::SWAP,
                format!(
                    "{}/{}",
                    format::gibibytes(swap_used),
                    format::gibibytes(memory.swap_total)
                ),
            )
            .detail(format!("{swap_percent:.0}%"))
            .level(Level::from_thresholds(swap_percent, 50.0, 80.0)),
        );
    }
    rows
}

fn temperature_row(config: &Config) -> Option<Row> {
    let celsius = temperature_source(config)
        .and_then(|path| fs::read_to_string(path).ok())
        .as_deref()
        .and_then(parse::millidegrees)?;
    // The same thresholds the Waybar `temperature` module used.
    let level = Level::from_thresholds(celsius, 55.0, 80.0);
    let glyph = if level == Level::Critical {
        icon::TEMPERATURE_HOT
    } else {
        icon::TEMPERATURE
    };
    Some(Row::new(glyph, format!("{celsius:.0}°C")).level(level))
}

/// Resolve which sensor to read.
///
/// An explicit setting wins; otherwise the package-wide CPU sensor is
/// preferred over the chassis one, because that is what a CPU temperature
/// readout is expected to mean.
fn temperature_source(config: &Config) -> Option<PathBuf> {
    if let Some(zone) = &config.thermal_zone {
        return Some(if zone.chars().all(|c| c.is_ascii_digit()) {
            config
                .sys_root
                .join("class/thermal")
                .join(format!("thermal_zone{zone}"))
                .join("temp")
        } else {
            PathBuf::from(zone)
        });
    }

    const PREFERRED_ZONES: [&str; 5] = [
        "x86_pkg_temp",
        "TCPU",
        "cpu-thermal",
        "soc_thermal",
        "acpitz",
    ];
    let zones = entries(&config.sys_root.join("class/thermal"), "thermal_zone");
    for wanted in PREFERRED_ZONES {
        for zone in &zones {
            let kind = fs::read_to_string(zone.join("type")).unwrap_or_default();
            if kind.trim() == wanted {
                return Some(zone.join("temp"));
            }
        }
    }

    const PREFERRED_HWMON: [&str; 4] = ["coretemp", "k10temp", "zenpower", "cpu_thermal"];
    let monitors = entries(&config.sys_root.join("class/hwmon"), "hwmon");
    for wanted in PREFERRED_HWMON {
        for monitor in &monitors {
            let name = fs::read_to_string(monitor.join("name")).unwrap_or_default();
            if name.trim() == wanted {
                return Some(monitor.join("temp1_input"));
            }
        }
    }

    zones.first().map(|zone| zone.join("temp"))
}

/// The children of `directory` whose names start with `prefix`, sorted so the
/// choice does not depend on the order the filesystem happens to return.
fn entries(directory: &Path, prefix: &str) -> Vec<PathBuf> {
    let Ok(listing) = fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = listing
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(prefix))
        })
        .collect();
    paths.sort();
    paths
}

fn disk_row(config: &Config) -> Option<Row> {
    let output = Command::new(&config.df)
        .arg("-P")
        .arg("-B1")
        .arg(&config.disk_path)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let disk = parse::disk(&String::from_utf8_lossy(&output.stdout))?;
    if disk.total == 0 {
        return None;
    }
    let percent = (disk.used as f64 / disk.total as f64) * 100.0;
    Some(
        Row::new(
            icon::DISK,
            format!("{} free", format::bytes(disk.available)),
        )
        .detail(format!("{percent:.0}%"))
        .level(Level::from_thresholds(percent, 80.0, 90.0)),
    )
}

pub(crate) fn network_row(current: &State, interface: Option<&str>) -> Option<Row> {
    let Some(interface) = interface else {
        return Some(Row::new(icon::DISCONNECTED, "Disconnected".to_owned()).level(Level::Warning));
    };
    let glyph = if interface.starts_with("wl") {
        icon::WIRELESS
    } else if interface.starts_with("en") || interface.starts_with("eth") {
        icon::WIRED
    } else {
        icon::TUNNEL
    };
    Some(
        Row::new(
            glyph,
            format!(
                "↓{} ↑{}",
                format::rate(current.rx_rate),
                format::rate(current.tx_rate)
            ),
        )
        // The wired/wireless/tunnel glyph already says which link this is, and
        // a cell that also spells out the interface name does not fit beside a
        // second column.
        .level(Level::Normal),
    )
}

#[cfg(test)]
mod tests;
