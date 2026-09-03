use super::*;

use std::os::unix::fs::PermissionsExt;
use std::sync::atomic::{AtomicU32, Ordering};

/// A throwaway `/proc` + `/sys` tree, so the assertions below describe fixed
/// readings instead of whatever the machine running the tests is doing.
struct Fixture {
    root: PathBuf,
}

static NEXT_FIXTURE: AtomicU32 = AtomicU32::new(0);

const SAMPLE_TIME: f64 = 1_000.0;

impl Fixture {
    fn new() -> Self {
        let root = env::temp_dir().join(format!(
            "swaync-sysmon-{}-{}",
            std::process::id(),
            NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
        ));
        let fixture = Fixture { root };

        fixture.write(
            "proc/stat",
            "cpu  1000 200 300 8000 500 0 100 0 0 0\ncpu0 500 100 150 4000 250 0 50 0 0 0\n",
        );
        fixture.write("proc/cpuinfo", "processor\t: 0\ncpu MHz\t\t: 2410.000\n");
        fixture.write(
            "proc/meminfo",
            "MemTotal:       33554432 kB\nMemAvailable:   25165824 kB\n\
             SwapTotal:      26214400 kB\nSwapFree:       26214400 kB\n",
        );
        fixture.write(
            "proc/net/dev",
            "Inter-|   Receive                                                |  Transmit\n\
             \x20face |bytes    packets errs drop fifo frame compressed multicast|bytes    packets errs drop fifo colls carrier compressed\n\
             \x20\x20\x20\x20lo:  100000     100    0    0    0     0          0         0   100000     100    0    0    0     0       0          0\n\
             \x20wlan0: 5002048    5000    0    0    0     0          0         0  2001024    2000    0    0    0     0       0          0\n",
        );
        fixture.write(
            "proc/net/route",
            "Iface\tDestination\tGateway\tFlags\tRefCnt\tUse\tMetric\tMask\tMTU\tWindow\tIRTT\n\
             wlan0\t00000000\t0101A8C0\t0003\t0\t0\t600\t00000000\t0\t0\t0\n",
        );
        fixture.write("sys/class/thermal/thermal_zone0/type", "acpitz\n");
        fixture.write("sys/class/thermal/thermal_zone0/temp", "39000\n");
        fixture.write("sys/class/thermal/thermal_zone1/type", "x86_pkg_temp\n");
        fixture.write("sys/class/thermal/thermal_zone1/temp", "47000\n");

        fixture
    }

    fn write(&self, relative: &str, contents: &str) {
        let path = self.root.join(relative);
        fs::create_dir_all(path.parent().expect("fixture paths have a parent"))
            .expect("fixture directory");
        fs::write(path, contents).expect("fixture file");
    }

    /// A `df` stand-in, so the disk row is asserted without depending on the
    /// filesystem the tests happen to run on.
    fn stub_df(&self, stdout: &str) -> OsString {
        let path = self.root.join("df");
        fs::write(
            &path,
            format!("#!/bin/sh\ncat <<'OUTPUT'\n{stdout}OUTPUT\n"),
        )
        .expect("stub df");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("stub df is runnable");
        path.into_os_string()
    }

    fn config(&self) -> Config {
        Config {
            proc_root: self.root.join("proc"),
            sys_root: self.root.join("sys"),
            df: self.stub_df(
                "Filesystem      1B-blocks         Used   Available Capacity Mounted on\n\
                 /dev/nvme0n1p2 1000000000    400000000   600000000      40% /\n",
            ),
            disk_path: OsString::from("/"),
            thermal_zone: None,
            interface: None,
            state_path: self.root.join("state"),
        }
    }

    /// Pre-load a previous sample so `collect` differences against known
    /// counters instead of seeding itself with a sleep.
    fn seed_state(&self, seconds_ago: f64) {
        let previous = State {
            timestamp: SAMPLE_TIME - seconds_ago,
            cpu_busy: 1_400,
            cpu_total: 9_100,
            net_rx: 5_000_000,
            net_tx: 2_000_000,
            cpu_percent: 99.0,
            rx_rate: 1.0,
            tx_rate: 2.0,
        };
        state::store(&self.state_path(), &previous).expect("seed state");
    }

    fn state_path(&self) -> PathBuf {
        self.root.join("state")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).ok();
    }
}

fn plain_rows(config: &Config) -> Vec<String> {
    collect_with_clock(config, || SAMPLE_TIME)
        .iter()
        .map(Row::plain)
        .collect()
}

/// The rows a config produces, named by the glyph that leads each one.
fn icons(config: &Config) -> Vec<&'static str> {
    collect_with_clock(config, || SAMPLE_TIME)
        .iter()
        .map(|row| row.icon)
        .collect()
}

#[test]
fn every_reading_becomes_a_row() {
    let fixture = Fixture::new();
    fixture.seed_state(2.0);
    let rows = plain_rows(&fixture.config());

    assert_eq!(
        icons(&fixture.config()),
        [
            icon::CPU,
            icon::MEMORY,
            icon::TEMPERATURE,
            icon::DISK,
            icon::WIRELESS
        ],
        "swap is absent because none is in use: {rows:#?}"
    );
}

#[test]
fn cpu_load_is_the_difference_against_the_stored_sample() {
    let fixture = Fixture::new();
    fixture.seed_state(2.0);
    let rows = plain_rows(&fixture.config());
    // busy 1400 -> 1600 while total 9100 -> 10100.
    assert!(rows[0].contains("20%"), "{}", rows[0]);
    assert!(rows[0].ends_with("2.41GHz"), "{}", rows[0]);
}

#[test]
fn network_throughput_is_the_byte_delta_over_the_interval() {
    let fixture = Fixture::new();
    fixture.seed_state(2.0);
    let rows = plain_rows(&fixture.config());
    let network = rows.last().expect("a network row");
    // 2048 received and 1024 sent over two seconds.
    assert!(network.contains("↓1.0K/s"), "{network}");
    assert!(network.ends_with("↑512B/s"), "{network}");
}

#[test]
fn a_run_that_lands_inside_the_previous_one_repeats_its_rates() {
    let fixture = Fixture::new();
    // A control center opened a moment after a timer tick must not divide the
    // counter delta by a near-zero interval.
    fixture.seed_state(0.01);
    let rows = plain_rows(&fixture.config());
    assert!(
        rows[0].contains("99%"),
        "the stored CPU figure: {}",
        rows[0]
    );
}

#[test]
fn the_first_run_stores_a_sample_for_the_next_one() {
    let fixture = Fixture::new();
    let config = fixture.config();
    assert!(state::load(&fixture.state_path()).is_empty());

    collect(&config);

    let stored = state::load(&fixture.state_path());
    assert_eq!(stored.cpu_total, 10_100);
    assert_eq!(stored.net_rx, 5_002_048);
}

#[test]
fn memory_reports_what_is_unavailable_rather_than_what_is_unfree() {
    let fixture = Fixture::new();
    fixture.seed_state(2.0);
    let rows = plain_rows(&fixture.config());
    // 32.0G total, 24.0G available.
    assert!(rows[1].contains("8.0G/32.0G"), "{}", rows[1]);
    assert!(rows[1].ends_with("25%"), "{}", rows[1]);
}

#[test]
fn swap_appears_only_once_something_is_in_it() {
    let fixture = Fixture::new();
    fixture.write(
        "proc/meminfo",
        "MemTotal:       33554432 kB\nMemAvailable:   25165824 kB\n\
         SwapTotal:      26214400 kB\nSwapFree:       20971520 kB\n",
    );
    fixture.seed_state(2.0);
    let rows = plain_rows(&fixture.config());
    assert!(rows[2].starts_with(icon::SWAP), "{rows:#?}");
    assert!(rows[2].contains("5.0G/25.0G"), "{}", rows[2]);
}

#[test]
fn the_package_sensor_outranks_the_chassis_one() {
    let fixture = Fixture::new();
    fixture.seed_state(2.0);
    let rows = plain_rows(&fixture.config());
    assert!(
        rows[2].contains("47°C"),
        "x86_pkg_temp, not acpitz's 39°C: {}",
        rows[2]
    );
}

#[test]
fn an_explicit_thermal_zone_wins() {
    let fixture = Fixture::new();
    fixture.seed_state(2.0);
    let mut config = fixture.config();
    config.thermal_zone = Some("0".to_owned());
    let rows = plain_rows(&config);
    assert!(rows[2].contains("39°C"), "{}", rows[2]);
}

#[test]
fn a_hot_package_is_marked_critical() {
    let fixture = Fixture::new();
    fixture.write("sys/class/thermal/thermal_zone1/temp", "91000\n");
    fixture.seed_state(2.0);
    let row = collect_with_clock(&fixture.config(), || SAMPLE_TIME)
        .into_iter()
        .find(|row| row.icon == icon::TEMPERATURE_HOT)
        .expect("a temperature row");
    assert_eq!(row.level, Level::Critical);
}

#[test]
fn a_machine_without_sensors_simply_omits_the_row() {
    let fixture = Fixture::new();
    fs::remove_dir_all(fixture.root.join("sys/class/thermal")).expect("remove sensors");
    fixture.seed_state(2.0);
    assert!(
        !icons(&fixture.config()).contains(&icon::TEMPERATURE),
        "a machine with no sensors gets no temperature row"
    );
}

#[test]
fn a_disconnected_machine_says_so() {
    let fixture = Fixture::new();
    fixture.write(
        "proc/net/route",
        "Iface\tDestination\tGateway\tFlags\tRefCnt\tUse\tMetric\tMask\tMTU\tWindow\tIRTT\n",
    );
    fixture.seed_state(2.0);
    let rows = plain_rows(&fixture.config());
    let network = rows.last().expect("a network row");
    assert!(network.contains("Disconnected"), "{network}");
}

#[test]
fn an_unreadable_proc_produces_the_placeholder_block() {
    let fixture = Fixture::new();
    let mut config = fixture.config();
    config.proc_root = fixture.root.join("nowhere");
    config.sys_root = fixture.root.join("nowhere");
    config.df = OsString::from(fixture.root.join("nowhere/df"));

    assert_eq!(
        format::block(&collect(&config), false),
        "No system readings available"
    );
}

#[test]
fn the_interface_decides_the_network_icon() {
    let wired = network_row(&State::default(), Some("enp0s31f6")).expect("a row");
    let wireless = network_row(&State::default(), Some("wlp0s20f3")).expect("a row");
    let other = network_row(&State::default(), Some("tun0")).expect("a row");
    assert_eq!(wired.icon, icon::WIRED);
    assert_eq!(wireless.icon, icon::WIRELESS);
    assert_eq!(other.icon, icon::TUNNEL);
}
