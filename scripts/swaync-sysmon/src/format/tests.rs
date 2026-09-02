use super::*;

#[test]
fn escape_covers_every_markup_character() {
    assert_eq!(
        escape(r#"a & b < c > d " e ' f"#),
        "a &amp; b &lt; c &gt; d &quot; e &apos; f"
    );
}

#[test]
fn a_span_escapes_its_content() {
    assert_eq!(
        span("#ff7edb", "a<b"),
        "<span foreground=\"#ff7edb\">a&lt;b</span>"
    );
}

#[test]
fn bytes_uses_binary_units() {
    assert_eq!(bytes(0), "0B");
    assert_eq!(bytes(512), "512B");
    assert_eq!(bytes(1024), "1.0K");
    assert_eq!(bytes(1536), "1.5K");
    assert_eq!(bytes(5 * 1024 * 1024), "5.0M");
}

#[test]
fn bytes_drops_the_decimal_once_the_mantissa_is_three_digits() {
    assert_eq!(bytes(412 * 1024 * 1024 * 1024), "412G");
    assert_eq!(bytes(99 * 1024 * 1024 * 1024), "99.0G");
}

#[test]
fn a_rate_is_a_byte_count_per_second() {
    assert_eq!(rate(1536.0), "1.5K/s");
    assert_eq!(rate(0.0), "0B/s");
}

#[test]
fn a_negative_rate_cannot_be_shown() {
    // Counter resets are filtered upstream; clamping keeps a stray value sane.
    assert_eq!(rate(-5.0), "0B/s");
}

#[test]
fn gibibytes_keeps_one_decimal() {
    assert_eq!(gibibytes(32 * 1024 * 1024), "32.0G");
    assert_eq!(gibibytes(7 * 1024 * 1024 + 512 * 1024), "7.5G");
}

#[test]
fn levels_are_classified_against_ascending_thresholds() {
    assert_eq!(Level::from_thresholds(40.0, 55.0, 80.0), Level::Normal);
    assert_eq!(Level::from_thresholds(55.0, 55.0, 80.0), Level::Warning);
    assert_eq!(Level::from_thresholds(85.0, 55.0, 80.0), Level::Critical);
}

#[test]
fn a_row_pads_its_name_so_values_line_up() {
    let cpu = Row::new("󰻠", "CPU", "12%".to_owned()).plain();
    let network = Row::new("󰖩", "Network", "↓ 0B/s".to_owned()).plain();
    let column = |row: &str| row.char_indices().find(|(_, c)| *c == '1' || *c == '↓');
    assert_eq!(
        cpu.chars().position(|c| c == '1'),
        network.chars().position(|c| c == '↓'),
        "values start in the same column: {cpu:?} vs {network:?}"
    );
    assert!(column(&cpu).is_some());
}

#[test]
fn a_row_colours_its_value_by_level() {
    let row = Row::new("󰄏", "Temp", "91°C".to_owned()).level(Level::Critical);
    assert!(row.markup().contains(colour::CRITICAL));
    assert!(row.markup().contains(colour::ICON));
}

#[test]
fn a_detail_is_appended_in_the_dim_colour() {
    let row = Row::new("󰻠", "CPU", "12%".to_owned()).detail("2.41 GHz");
    assert!(row.plain().ends_with("2.41 GHz"));
    assert_eq!(row.markup().matches(colour::NAME).count(), 2);
}

#[test]
fn a_block_joins_rows_with_newlines() {
    let rows = [
        Row::new("󰻠", "CPU", "12%".to_owned()),
        Row::new("󰍛", "Memory", "7.5G / 32.0G".to_owned()),
    ];
    assert_eq!(block(&rows, false).lines().count(), 2);
    assert_eq!(block(&rows, true).lines().count(), 2);
}

#[test]
fn an_empty_block_says_so_rather_than_rendering_nothing() {
    assert_eq!(block(&[], false), "No system readings available");
    assert!(block(&[], true).contains("No system readings available"));
}
