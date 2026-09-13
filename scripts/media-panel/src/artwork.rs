//! Cover art for a track, including the local files that publish none.
//!
//! A player that has art puts it in `mpris:artUrl`; an HTTP one is downloaded
//! once and kept. Local players routinely publish nothing, because the cover
//! is inside the file or beside it in the folder and the player never
//! extracted one — so `xesam:url` is followed to the file and both places are
//! looked in.

use std::collections::{HashMap, HashSet};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::thread;

use lofty::file::TaggedFileExt;
use lofty::probe::Probe;

/// Names a ripper or a tagger gives the folder's cover, best first.
const COVER_STEMS: &[&str] = &["cover", "folder", "front", "album", "albumart", "thumb"];
const IMAGE_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "webp", "avif", "bmp", "gif"];

/// Where downloaded and extracted covers are kept between runs.
const CACHE_DIR: &str = "media-panel/art";

/// Art already resolved this run, so a panel redrawing twice a second does not
/// go back to the disk for a track it has already found.
fn memo() -> &'static Mutex<HashMap<String, Option<PathBuf>>> {
    static MEMO: OnceLock<Mutex<HashMap<String, Option<PathBuf>>>> = OnceLock::new();
    MEMO.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Lookups already running, so a panel refreshing twice a second starts one
/// thread per track rather than one per tick.
fn pending() -> &'static Mutex<HashSet<String>> {
    static PENDING: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    PENDING.get_or_init(|| Mutex::new(HashSet::new()))
}

fn key(art_url: Option<&str>, track_url: Option<&str>) -> String {
    format!("{}\u{1}{}", art_url.unwrap_or(""), track_url.unwrap_or(""))
}

/// The cover if it is already known, and never a download.
///
/// `find` reads tags and fetches over HTTP, either of which can take seconds —
/// `curl` alone is allowed ten. Doing that on the thread drawing the panel
/// freezes every card until it returns, which is what a track change used to
/// cost. A miss starts the lookup out of the way and returns `None`; the
/// refresh after it finishes picks the answer up.
pub fn find_when_known(art_url: Option<&str>, track_url: Option<&str>) -> Option<PathBuf> {
    let key = key(art_url, track_url);
    if let Ok(memo) = memo().lock()
        && let Some(found) = memo.get(&key)
    {
        return found.clone();
    }

    let fresh = pending()
        .lock()
        .is_ok_and(|mut pending| pending.insert(key.clone()));
    if fresh {
        let (art, track) = (art_url.map(str::to_owned), track_url.map(str::to_owned));
        thread::spawn(move || {
            let found = resolve(art.as_deref(), track.as_deref());
            if let Ok(mut memo) = memo().lock() {
                memo.insert(key.clone(), found);
            }
            if let Ok(mut pending) = pending().lock() {
                pending.remove(&key);
            }
        });
    }
    None
}

fn resolve(art_url: Option<&str>, track_url: Option<&str>) -> Option<PathBuf> {
    if let Some(art) = art_url {
        if let Some(path) = local_path(art).filter(|path| path.is_file()) {
            return Some(path);
        }
        if art.starts_with("http://") || art.starts_with("https://") {
            return download(art);
        }
    }

    let track = local_path(track_url?)?;
    beside(&track).or_else(|| embedded(&track))
}

/// Turns a `file://` URL into a path, undoing the percent-encoding a player
/// applies to spaces and to anything non-ASCII in a filename.
fn local_path(url: &str) -> Option<PathBuf> {
    let encoded = url.strip_prefix("file://")?;
    let path = PathBuf::from(percent_decode(encoded)?);
    path.is_absolute().then_some(path)
}

fn percent_decode(value: &str) -> Option<String> {
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
/// A picture named after the track wins over the folder's cover, because a
/// folder holding one album still gets a per-track picture when they differ.
fn beside(track: &Path) -> Option<PathBuf> {
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

/// The cover carried inside the file, written out so GTK can load it.
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

fn download(url: &str) -> Option<PathBuf> {
    let mut hasher = DefaultHasher::new();
    url.hash(&mut hasher);
    let name = format!("{:016x}", hasher.finish());

    let cached = cache_dir()?.join(&name);
    if cached.is_file() {
        return Some(cached);
    }

    // GTK can load a remote picture itself, but only asynchronously and only
    // if it is handed a file; caching keeps the redraw loop off the network.
    let bytes = ureq_get(url)?;
    write_cache(&name, &bytes)
}

/// A tiny HTTP GET. Cover art is the only thing fetched, and a whole HTTP
/// client crate for one request is not worth the closure size.
fn ureq_get(url: &str) -> Option<Vec<u8>> {
    let output = std::process::Command::new(curl())
        .args([
            "--silent",
            "--show-error",
            "--fail",
            "--location",
            "--max-time",
            "10",
            url,
        ])
        .output()
        .ok()?;
    output.status.success().then_some(output.stdout)
}

fn curl() -> String {
    std::env::var("MEDIA_PANEL_CURL").unwrap_or_else(|_| String::from("curl"))
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
mod tests;
