use std::path::PathBuf;

use super::{beside, local_path, percent_decode};

/// A directory that removes itself, so the tests leave nothing behind.
struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        let base = std::env::temp_dir().join(format!("media-panel-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).expect("temp dir");
        Self(base)
    }

    fn file(&self, name: &str) -> PathBuf {
        let path = self.0.join(name);
        std::fs::write(&path, b"x").expect("write");
        path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn local_path_decodes_percent_escapes() {
    // Elisa publishes exactly this shape: spaces and pipes escaped.
    assert_eq!(
        local_path("file:///home/raina/Documents/Destiny%20Rogers%20%7C%7C%20Tomboy/track.mp3"),
        Some(PathBuf::from(
            "/home/raina/Documents/Destiny Rogers || Tomboy/track.mp3"
        ))
    );
}

#[test]
fn local_path_rejects_other_schemes() {
    assert_eq!(local_path("https://music.youtube.com/watch?v=abc"), None);
    assert_eq!(local_path("spotify:track:abc"), None);
}

#[test]
fn local_path_rejects_a_truncated_escape() {
    assert_eq!(local_path("file:///music/track%2.mp3"), None);
}

#[test]
fn percent_decode_leaves_ordinary_text_alone() {
    assert_eq!(
        percent_decode("plain/path.flac"),
        Some(String::from("plain/path.flac"))
    );
}

#[test]
fn a_folder_cover_is_found() {
    let dir = TempDir::new("folder");
    let track = dir.file("01 - song.flac");
    let cover = dir.file("cover.jpg");

    assert_eq!(beside(&track), Some(cover));
}

#[test]
fn the_tracks_own_picture_wins_over_the_folder_cover() {
    let dir = TempDir::new("own");
    let track = dir.file("song.mp3");
    let own = dir.file("song.png");
    dir.file("cover.jpg");

    assert_eq!(beside(&track), Some(own));
}

#[test]
fn the_name_is_matched_whatever_its_case() {
    let dir = TempDir::new("case");
    let track = dir.file("song.flac");
    let cover = dir.file("Folder.JPG");

    assert_eq!(beside(&track), Some(cover));
}

#[test]
fn cover_is_preferred_to_the_other_folder_names() {
    let dir = TempDir::new("order");
    let track = dir.file("song.flac");
    dir.file("thumb.png");
    let cover = dir.file("cover.png");

    assert_eq!(beside(&track), Some(cover));
}

#[test]
fn a_folder_without_a_picture_yields_nothing() {
    let dir = TempDir::new("bare");
    let track = dir.file("song.flac");
    dir.file("notes.txt");

    assert_eq!(beside(&track), None);
}

#[test]
fn a_directory_named_like_a_cover_is_not_one() {
    let dir = TempDir::new("dir");
    let track = dir.file("song.flac");
    std::fs::create_dir(dir.0.join("cover.jpg")).expect("mkdir");

    assert_eq!(beside(&track), None);
}
