use super::*;

const PROC_STAT: &str = "\
cpu  1000 200 300 8000 500 0 100 0 0 0
cpu0 500 100 150 4000 250 0 50 0 0 0
cpu1 500 100 150 4000 250 0 50 0 0 0
intr 12345
";

#[test]
fn cpu_times_reads_the_aggregate_line_only() {
    // 1000 + 200 + 300 + 8000 + 500 + 0 + 100 + 0
    let times = cpu_times(PROC_STAT).expect("aggregate cpu line");
    assert_eq!(times.total, 10_100);
    // idle (8000) and iowait (500) are not busy time.
    assert_eq!(times.busy, 1_600);
}

#[test]
fn cpu_times_ignores_guest_columns() {
    let with_guests = "cpu  10 0 0 0 0 0 0 0 99999 99999\n";
    let times = cpu_times(with_guests).expect("aggregate cpu line");
    assert_eq!(times.total, 10);
}

#[test]
fn cpu_times_rejects_a_file_without_an_aggregate_line() {
    assert_eq!(cpu_times("cpu0 1 2 3 4\nintr 5\n"), None);
}

#[test]
fn cpu_mhz_averages_every_core() {
    let cpuinfo = "\
processor\t: 0
cpu MHz\t\t: 2000.000
processor\t: 1
cpu MHz\t\t: 3000.000
";
    assert_eq!(cpu_mhz(cpuinfo), Some(2500.0));
}

#[test]
fn cpu_mhz_is_absent_when_the_architecture_omits_it() {
    assert_eq!(cpu_mhz("processor\t: 0\nmodel name\t: Something\n"), None);
}

#[test]
fn memory_kib_reads_totals_and_swap() {
    let meminfo = "\
MemTotal:       32000000 kB
MemFree:         1000000 kB
MemAvailable:   24000000 kB
SwapTotal:      26000000 kB
SwapFree:       25000000 kB
";
    assert_eq!(
        memory_kib(meminfo),
        Some(MemoryKib {
            total: 32_000_000,
            available: 24_000_000,
            swap_total: 26_000_000,
            swap_free: 25_000_000,
        })
    );
}

#[test]
fn memory_kib_falls_back_to_free_without_an_available_field() {
    let meminfo = "MemTotal:       1000 kB\nMemFree:         400 kB\n";
    let memory = memory_kib(meminfo).expect("MemTotal present");
    assert_eq!(memory.available, 400);
    assert_eq!(memory.swap_total, 0);
}

#[test]
fn memory_kib_needs_a_total() {
    assert_eq!(memory_kib("MemFree: 400 kB\n"), None);
}

const PROC_NET_ROUTE: &str = "\
Iface\tDestination\tGateway\tFlags\tRefCnt\tUse\tMetric\tMask\tMTU\tWindow\tIRTT
wlan0\t00000000\t0101A8C0\t0003\t0\t0\t600\t00000000\t0\t0\t0
tun0\t00000000\t00000000\t0001\t0\t0\t50\t00000000\t0\t0\t0
wlan0\t0001A8C0\t00000000\t0001\t0\t0\t600\t00FFFFFF\t0\t0\t0
";

#[test]
fn default_interface_prefers_the_lowest_metric() {
    assert_eq!(
        default_interface(PROC_NET_ROUTE).as_deref(),
        Some("tun0"),
        "the VPN's default route outranks the Wi-Fi one"
    );
}

#[test]
fn default_interface_ignores_non_default_routes() {
    let only_subnet =
        "Iface\tDestination\tGateway\tFlags\tRefCnt\tUse\tMetric\tMask\tMTU\tWindow\tIRTT
wlan0\t0001A8C0\t00000000\t0001\t0\t0\t600\t00FFFFFF\t0\t0\t0
";
    assert_eq!(default_interface(only_subnet), None);
}

const PROC_NET_DEV: &str = "\
Inter-|   Receive                                                |  Transmit
 face |bytes    packets errs drop fifo frame compressed multicast|bytes    packets errs drop fifo colls carrier compressed
    lo:  100000     100    0    0    0     0          0         0   100000     100    0    0    0     0       0          0
 wlan0: 5000000    5000    0    0    0     0          0         0  2000000    2000    0    0    0     0       0          0
docker0:  700000     700    0    0    0     0          0         0   300000     300    0    0    0     0       0          0
";

#[test]
fn net_counters_reads_one_interface() {
    assert_eq!(
        net_counters(PROC_NET_DEV, Some("wlan0")),
        Some(NetCounters {
            rx: 5_000_000,
            tx: 2_000_000
        })
    );
}

#[test]
fn net_counters_without_an_interface_skips_loopback_and_bridges() {
    assert_eq!(
        net_counters(PROC_NET_DEV, None),
        Some(NetCounters {
            rx: 5_000_000,
            tx: 2_000_000
        }),
        "lo and docker0 must not inflate the totals"
    );
}

#[test]
fn net_counters_reports_nothing_for_an_unknown_interface() {
    assert_eq!(net_counters(PROC_NET_DEV, Some("eth9")), None);
}

#[test]
fn millidegrees_converts_to_celsius() {
    assert_eq!(millidegrees("47000\n"), Some(47.0));
    assert_eq!(millidegrees("not a number"), None);
}

#[test]
fn disk_parses_a_df_row() {
    let output = "\
Filesystem      1B-blocks         Used   Available Capacity Mounted on
/dev/nvme0n1p2 1000000000    400000000   600000000      40% /
";
    assert_eq!(
        disk(output),
        Some(Disk {
            total: 1_000_000_000,
            used: 400_000_000,
            available: 600_000_000,
        })
    );
}

#[test]
fn disk_tolerates_a_device_name_containing_spaces() {
    let output = "\
Filesystem      1B-blocks         Used   Available Capacity Mounted on
my nas share   1000000000    400000000   600000000      40% /
";
    assert_eq!(
        disk(output).map(|disk| disk.available),
        Some(600_000_000),
        "columns are counted from the right"
    );
}

#[test]
fn disk_rejects_output_without_a_data_row() {
    assert_eq!(
        disk("Filesystem 1B-blocks Used Available Capacity Mounted on\n"),
        None
    );
}
