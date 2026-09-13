use super::centre_square;

#[test]
fn a_wide_thumbnail_is_cropped_from_its_middle() {
    // A 16:9 video thumbnail, which is what a browser tab publishes.
    assert_eq!(centre_square(1280, 720), Some((280, 0, 720)));
}

#[test]
fn a_tall_cover_is_cropped_from_its_middle() {
    assert_eq!(centre_square(600, 900), Some((0, 150, 600)));
}

#[test]
fn a_square_cover_is_left_alone() {
    assert_eq!(centre_square(300, 300), Some((0, 0, 300)));
}

#[test]
fn an_empty_image_has_no_square_to_take() {
    assert_eq!(centre_square(0, 500), None);
    assert_eq!(centre_square(-1, -1), None);
}
