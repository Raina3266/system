use super::*;

fn sample() -> State {
    State {
        timestamp: 1_700_000_000.5,
        cpu_busy: 1_000,
        cpu_total: 10_000,
        net_rx: 500_000,
        net_tx: 200_000,
        cpu_percent: 12.5,
        rx_rate: 1024.0,
        tx_rate: 512.0,
    }
}

#[test]
fn a_state_survives_a_round_trip() {
    assert_eq!(State::parse(&sample().serialise()), sample());
}

#[test]
fn parsing_a_truncated_file_costs_one_stale_tick_rather_than_an_error() {
    let truncated = "timestamp=1700000000.5\ncpu_busy=1000\ncpu_to";
    let state = State::parse(truncated);
    assert_eq!(state.cpu_busy, 1_000);
    assert_eq!(state.cpu_total, 0);
    assert!(!state.is_empty());
}

#[test]
fn a_missing_file_reads_as_empty() {
    let state = State::parse("");
    assert!(state.is_empty());
}

#[test]
fn cpu_percent_is_the_busy_share_of_the_interval() {
    let previous = sample();
    // 200 more busy jiffies out of 1000 more total.
    assert_eq!(cpu_percent(&previous, 1_200, 11_000), Some(20.0));
}

#[test]
fn cpu_percent_is_absent_when_the_counters_did_not_advance() {
    let previous = sample();
    assert_eq!(cpu_percent(&previous, 1_000, 10_000), None);
}

#[test]
fn cpu_percent_is_absent_when_the_counters_went_backwards() {
    let previous = sample();
    assert_eq!(cpu_percent(&previous, 900, 9_000), None);
}

#[test]
fn rate_divides_the_byte_delta_by_the_interval() {
    assert_eq!(rate(1_000, 3_048, 2.0), Some(1_024.0));
}

#[test]
fn rate_is_absent_for_a_zero_interval_or_a_reset_counter() {
    assert_eq!(rate(1_000, 2_000, 0.0), None);
    assert_eq!(rate(2_000, 1_000, 1.0), None);
}

#[test]
fn storing_replaces_the_previous_state_atomically() {
    let directory = std::env::temp_dir().join(format!(
        "swaync-sysmon-state-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let path = directory.join("state");

    assert!(load(&path).is_empty(), "nothing has been stored yet");
    store(&path, &sample()).expect("the runtime directory is created on demand");
    assert_eq!(load(&path), sample());

    let mut second = sample();
    second.cpu_percent = 90.0;
    store(&path, &second).expect("an existing state is replaced");
    assert_eq!(load(&path).cpu_percent, 90.0);

    std::fs::remove_dir_all(&directory).ok();
}
