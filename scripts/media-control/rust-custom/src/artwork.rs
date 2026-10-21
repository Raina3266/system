//! Cover art for the media panel's local tracks.
//!
//! The frontend loads a published `mpris:artUrl` itself, but local players such
//! as Elisa publish none: the cover sits inside the file or beside it. Look for
//! nearby cover files first, then the embedded picture, written to a cache so
//! repeat lookups cost nothing.

use std::hash::{DefaultHasher, Hash, Hasher};
use std::io;
use std::path::{Path, PathBuf};

use lofty::file::TaggedFileExt;
use lofty::probe::Probe;

use crate::AppResult;

/// Names a ripper or a tagger gives the folder's cover, best first.
const COVER_STEMS: &[&str] = &["cover", "folder", "front", "album", "albumart", "thumb"];
const IMAGE_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "webp", "avif", "bmp", "gif"];

/// Where extracted covers are kept between runs.
const CACHE_DIR: &str = "media-control/artwork";

/// Prints the cover's path for a `xesam:url` (or a plain path), or fails with
/// a message when neither the folder nor the file's tags carry one.
pub fn print_path(track_url: &str) -> AppResult<()> {
    let track = local_path(track_url).ok_or_else(|| {
        io::Error::other(format!(
            "artwork only handles local file:// URLs: {track_url:?}"
        ))
    })?;
    let path = beside(&track)
        .or_else(|| embedded(&track))
        .ok_or_else(|| io::Error::other(format!("no cover found beside or inside {track:?}")))?;
    println!("{}", path.display());
    Ok(())
}

/// Turns a `file://` URL into a path, undoing the percent-encoding a player
/// applies to spaces and to anything non-ASCII in a filename. Plain absolute
/// paths pass through for terminal use.
pub(crate) fn local_path(url: &str) -> Option<PathBuf> {
    let decoded = match url.strip_prefix("file://") {
        Some(encoded) => percent_decode(encoded)?,
        None => url.to_owned(),
    };
    let path = PathBuf::from(decoded);
    path.is_absolute().then_some(path)
}

pub(crate) fn percent_decode(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            out.push(u8::from_str_radix(value.get(index + 1..index + 3)?, 16).ok()?);
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// A cover stored as its own file in the track's folder.
///
/// A picture named after the track beats the folder cover: one album per
/// folder still gets per-track pictures when they differ.
pub(crate) fn beside(track: &Path) -> Option<PathBuf> {
    let folder = track.parent()?;
    let entries: Vec<String> = std::fs::read_dir(folder)
        .ok()?
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .filter_map(|entry| entry.file_name().to_str().map(String::from))
        .collect();

    let own = track.file_stem().and_then(|stem| stem.to_str());
    own.and_then(|stem| matching(&entries, folder, &[stem]))
        .or_else(|| matching(&entries, folder, COVER_STEMS))
}

fn matching(entries: &[String], folder: &Path, stems: &[&str]) -> Option<PathBuf> {
    // Ordered by the caller's preference, not by what the folder lists first,
    // so `cover.jpg` beats `thumb.png` wherever they sit.
    stems.iter().find_map(|stem| {
        IMAGE_EXTENSIONS.iter().find_map(|extension| {
            let wanted = format!("{stem}.{extension}");
            entries
                .iter()
                .find(|entry| entry.eq_ignore_ascii_case(&wanted))
                .map(|entry| folder.join(entry))
        })
    })
}

/// The cover carried inside the file, written out so the frontend can load it.
fn embedded(track: &Path) -> Option<PathBuf> {
    let tagged = Probe::open(track).ok()?.read().ok()?;
    let tag = tagged.primary_tag().or_else(|| tagged.first_tag())?;
    let picture = tag.pictures().first()?;

    let extension = match picture.mime_type().map(lofty::picture::MimeType::as_str) {
        Some("image/png") => "png",
        Some("image/webp") => "webp",
        Some("image/gif") => "gif",
        Some("image/bmp") => "bmp",
        _ => "jpg",
    };

    let mut hasher = DefaultHasher::new();
    track.hash(&mut hasher);
    picture.data().len().hash(&mut hasher);
    write_cache(
        &format!("{:016x}.{extension}", hasher.finish()),
        picture.data(),
    )
}

fn write_cache(name: &str, bytes: &[u8]) -> Option<PathBuf> {
    let directory = cache_dir()?;
    std::fs::create_dir_all(&directory).ok()?;
    let path = directory.join(name);
    if !path.is_file() {
        std::fs::write(&path, bytes).ok()?;
    }
    Some(path)
}

fn cache_dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))?;
    Some(base.join(CACHE_DIR))
}

#[cfg(test)]
media_unit_tests!(artwork);
