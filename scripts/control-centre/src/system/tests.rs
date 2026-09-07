use super::{percentage, ratio};

#[test]
fn a_ratio_of_nothing_is_zero_rather_than_a_division_by_zero() {
    assert_eq!(ratio(0, 0), 0.0);
    assert_eq!(ratio(5, 0), 0.0);
}

#[test]
fn readings_are_clamped_so_a_ring_never_overdraws() {
    assert_eq!(percentage(1.4).fraction, 1.0);
    assert_eq!(percentage(-0.2).fraction, 0.0);
}

#[test]
fn a_reading_spells_out_the_percentage_it_draws() {
    assert_eq!(percentage(0.5).text, "50%");
    assert_eq!(percentage(0.0).text, "0%");
    assert_eq!(percentage(1.0).text, "100%");
}
