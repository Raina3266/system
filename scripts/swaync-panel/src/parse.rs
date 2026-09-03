//! Pure parsers for the `/proc`, `/sys` and `df` text this program reads.
//!
//! Nothing here touches the filesystem, so every format quirk is covered by the
//! unit tests in `tests.rs` instead of by whatever the running machine happens
//! to report.

/// Cumulative CPU jiffies since boot, from the aggregate `cpu` line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CpuTimes {
    pub busy: u64,
    pub total: u64,
}

/// Memory and swap totals, in kibibytes, as `/proc/meminfo` reports them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MemoryKib {
    pub total: u64,
    pub available: u64,
    pub swap_total: u64,
    pub swap_free: u64,
}

/// Cumulative interface byte counters since boot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NetCounters {
    pub rx: u64,
    pub tx: u64,
}

/// A filesystem's size, as reported by `df -P -B1`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Disk {
    pub total: u64,
    pub used: u64,
    pub available: u64,
}

/// Parse the aggregate `cpu` line of `/proc/stat`.
///
/// `total` sums the first eight fields; `guest` and `guest_nice` are omitted
/// because the kernel already counts them inside `user` and `nice`. `busy` is
/// everything except `idle` and `iowait`, which is what a CPU meter shows.
pub fn cpu_times(proc_stat: &str) -> Option<CpuTimes> {
    let line = proc_stat
        .lines()
        .find(|line| line.split_whitespace().next() == Some("cpu"))?;

    let fields: Vec<u64> = line
        .split_whitespace()
        .skip(1)
        .take(8)
        .map(|field| field.parse().unwrap_or(0))
        .collect();
    if fields.len() < 4 {
        return None;
    }

    let total: u64 = fields.iter().sum();
    let idle = fields[3] + fields.get(4).copied().unwrap_or(0);
    Some(CpuTimes {
        busy: total.saturating_sub(idle),
        total,
    })
}

/// Average the per-core `cpu MHz` lines of `/proc/cpuinfo`.
///
/// Not every architecture reports the field; callers treat `None` as "no
/// frequency to show" rather than as an error.
pub fn cpu_mhz(proc_cpuinfo: &str) -> Option<f64> {
    let mut sum = 0.0;
    let mut count = 0u32;
    for line in proc_cpuinfo.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        if key.trim() != "cpu MHz" {
            continue;
        }
        if let Ok(mhz) = value.trim().parse::<f64>() {
            sum += mhz;
            count += 1;
        }
    }
    (count > 0).then(|| sum / f64::from(count))
}

/// Read `MemTotal`, `MemAvailable` and the swap pair out of `/proc/meminfo`.
///
/// `MemAvailable` is the kernel's own estimate of what a new allocation could
/// get, so `total - available` is the "used" figure a user recognises; the
/// alternative (`total - free`) counts reclaimable page cache as used.
pub fn memory_kib(proc_meminfo: &str) -> Option<MemoryKib> {
    let field = |name: &str| -> Option<u64> {
        proc_meminfo.lines().find_map(|line| {
            let (key, value) = line.split_once(':')?;
            (key.trim() == name)
                .then(|| value.split_whitespace().next()?.parse().ok())
                .flatten()
        })
    };

    let total = field("MemTotal")?;
    Some(MemoryKib {
        total,
        available: field("MemAvailable").unwrap_or_else(|| field("MemFree").unwrap_or(0)),
        swap_total: field("SwapTotal").unwrap_or(0),
        swap_free: field("SwapFree").unwrap_or(0),
    })
}

/// Pick the interface carrying the default route out of `/proc/net/route`.
///
/// A machine can hold several default routes at once — a Wi-Fi link and a VPN,
/// say — so the lowest metric wins, which is the one the kernel actually uses.
pub fn default_interface(proc_net_route: &str) -> Option<String> {
    let mut best: Option<(u64, String)> = None;
    for line in proc_net_route.lines().skip(1) {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 7 || fields[1] != "00000000" {
            continue;
        }
        let metric = fields[6].parse().unwrap_or(u64::MAX);
        if best.as_ref().is_none_or(|(best, _)| metric < *best) {
            best = Some((metric, fields[0].to_owned()));
        }
    }
    best.map(|(_, iface)| iface)
}

/// Sum one interface's `/proc/net/dev` byte counters, or every real interface
/// when `iface` is `None`.
///
/// The loopback and the virtual bridges that container runtimes and VPN
/// clients leave behind would otherwise inflate the totals, so the fallback
/// path skips them by name.
pub fn net_counters(proc_net_dev: &str, iface: Option<&str>) -> Option<NetCounters> {
    let mut total = NetCounters { rx: 0, tx: 0 };
    let mut matched = false;

    for line in proc_net_dev.lines().skip(2) {
        let Some((name, counters)) = line.split_once(':') else {
            continue;
        };
        let name = name.trim();
        match iface {
            Some(wanted) if name != wanted => continue,
            None if !is_physical_interface(name) => continue,
            _ => {}
        }

        let fields: Vec<u64> = counters
            .split_whitespace()
            .map(|field| field.parse().unwrap_or(0))
            .collect();
        if fields.len() < 9 {
            continue;
        }
        matched = true;
        total.rx += fields[0];
        total.tx += fields[8];
    }

    matched.then_some(total)
}

/// Whether an interface counts towards the "all interfaces" fallback total.
fn is_physical_interface(name: &str) -> bool {
    const VIRTUAL_PREFIXES: [&str; 8] = [
        "lo", "docker", "veth", "br-", "virbr", "tun", "tap", "podman",
    ];
    !VIRTUAL_PREFIXES
        .iter()
        .any(|prefix| name.starts_with(prefix))
}

/// Convert the contents of a `temp*_input` or `thermal_zone*/temp` file.
///
/// Both report millidegrees Celsius.
pub fn millidegrees(raw: &str) -> Option<f64> {
    raw.trim().parse::<f64>().ok().map(|value| value / 1000.0)
}

/// Parse the single data row of `df -P -B1 <path>`.
///
/// Fields are read from the right because a filesystem's device name can
/// contain spaces while the trailing five columns never vary in count.
pub fn disk(df_output: &str) -> Option<Disk> {
    let row = df_output.lines().nth(1)?;
    let fields: Vec<&str> = row.split_whitespace().collect();
    if fields.len() < 6 {
        return None;
    }
    let from_end = |offset: usize| -> Option<u64> { fields[fields.len() - offset].parse().ok() };
    Some(Disk {
        total: from_end(5)?,
        used: from_end(4)?,
        available: from_end(3)?,
    })
}

#[cfg(test)]
mod tests;
