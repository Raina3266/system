//! Headless credit lines for the media panel. Only recognized video IDs can
//! trigger a public metadata lookup; neither page URLs nor response data select
//! an endpoint. Successful authors are cached, never metadata fallbacks.

use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufReader, Read, Seek, SeekFrom, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

use lofty::config::ParseOptions;
use lofty::file::TaggedFileExt;
use lofty::prelude::Accessor;
use lofty::probe::Probe;
use serde::{Deserialize, Serialize};

const MAX_INPUT_BYTES: usize = 64 * 1024;
const MAX_RESPONSE_BYTES: usize = 256 * 1024;
const MAX_AUTHOR_BYTES: usize = 1024;
const MAX_CACHE_BYTES: usize = 4096;
const MAX_CACHE_ENTRIES: usize = 128;
const MAX_TAG_READ_BYTES: usize = 8 * 1024 * 1024;
const CACHE_TTL: Duration = Duration::from_secs(24 * 60 * 60);
const CACHE_DIR: &str = "media-control/authors";
static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Deserialize)]
struct CreditRequest {
    url: String,
    artist: String,
    album: String,
}

#[derive(Serialize)]
struct CreditReply<'a> {
    url: &'a str,
    artist: &'a str,
    album: &'a str,
    credit: &'a str,
}

/// Reads one JSON object with string `url`, `artist`, and `album` fields from
/// stdin (at most 64 KiB, followed by EOF), then prints one JSON reply line.
/// The original fields are echoed unchanged so callers can reject stale replies.
/// Only invalid input or local stdin/stdout errors fail this operation; metadata
/// lookup and cache failures produce the ordinary credit fallback instead.
/// For local tracks, embedded tags fill empty artist/album fields in the credit
/// only; the echoed request fields remain unchanged.
///
/// `MEDIA_CONTROL_CURL` may name a curl-compatible executable (not a shell command).
/// The successful-author cache lives under XDG_CACHE_HOME, or HOME/.cache.
pub fn print_credit() -> crate::AppResult<()> {
    let request = read_request(io::stdin().lock())?;
    let cache = cache_directory(std::env::var_os("XDG_CACHE_HOME"), std::env::var_os("HOME"));
    let credit = resolve_credit(&request, cache.as_deref(), fetch_author);
    write_reply(io::stdout().lock(), &request, &credit)
}

fn read_request(reader: impl Read) -> crate::AppResult<CreditRequest> {
    let bytes = read_bounded(reader, MAX_INPUT_BYTES)?;
    serde_json::from_slice(&bytes).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid media credit JSON (expected string url, artist and album)",
        )
        .into()
    })
}

fn write_reply(
    mut writer: impl Write,
    request: &CreditRequest,
    credit: &str,
) -> crate::AppResult<()> {
    serde_json::to_writer(
        &mut writer,
        &CreditReply {
            url: &request.url,
            artist: &request.artist,
            album: &request.album,
            credit,
        },
    )?;
    writer.write_all(b"\n")?;
    Ok(())
}

fn read_bounded(reader: impl Read, limit: usize) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader.take(limit as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "media credit data exceeds size limit",
        ));
    }
    Ok(bytes)
}

#[derive(Debug, PartialEq, Eq)]
enum Video {
    Youtube(String),
    BilibiliBv(String),
    BilibiliAv(u64),
}

impl Video {
    fn parse(url: &str) -> Option<Self> {
        let (host, path, query) = http_url_parts(url)?;
        let host = host.to_ascii_lowercase();
        if host_matches(&host, "youtu.be") {
            let id = path.strip_prefix('/')?.split('/').next()?;
            return valid_youtube_id(id).then(|| Self::Youtube(id.to_owned()));
        }
        if host_matches(&host, "youtube.com") {
            let id = if path == "/watch" || path == "/watch/" {
                query
                    .split('&')
                    .find_map(|field| field.strip_prefix("v=").filter(|id| valid_youtube_id(id)))?
            } else {
                let mut segments = path.strip_prefix('/')?.split('/');
                match segments.next()? {
                    "shorts" | "live" | "embed" => segments.next()?,
                    _ => return None,
                }
            };
            return valid_youtube_id(id).then(|| Self::Youtube(id.to_owned()));
        }
        if host_matches(&host, "bilibili.com") {
            let id = path.strip_prefix("/video/")?.split('/').next()?;
            return Self::bilibili(id);
        }
        None
    }

    fn bilibili(id: &str) -> Option<Self> {
        if let Some(suffix) = id.strip_prefix("BV") {
            return (!suffix.is_empty()
                && id.len() <= 64
                && suffix.bytes().all(|byte| byte.is_ascii_alphanumeric()))
            .then(|| Self::BilibiliBv(id.to_owned()));
        }
        let digits = id.strip_prefix("av")?;
        if digits.is_empty() || digits.len() > 20 || !digits.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let aid = digits.parse::<u64>().ok()?;
        (aid != 0).then_some(Self::BilibiliAv(aid))
    }

    fn endpoint(&self) -> String {
        match self {
            Self::Youtube(id) => format!(
                "https://www.youtube.com/oembed?format=json&url=https%3A%2F%2Fwww.youtube.com%2Fwatch%3Fv%3D{id}"
            ),
            Self::BilibiliBv(id) => {
                format!("https://api.bilibili.com/x/web-interface/view?bvid={id}")
            }
            Self::BilibiliAv(aid) => {
                format!("https://api.bilibili.com/x/web-interface/view?aid={aid}")
            }
        }
    }

    fn cache_name(&self) -> String {
        match self {
            Self::Youtube(id) => format!("youtube-{id}.json"),
            Self::BilibiliBv(id) => format!("bilibili-{id}.json"),
            Self::BilibiliAv(aid) => format!("bilibili-av{aid}.json"),
        }
    }
}

// Share only the HTTP(S) envelope; credit and browser filtering deliberately
// retain different authority validation rules.
pub(crate) fn http_authority(url: &str) -> Option<(&str, &str)> {
    if url
        .bytes()
        .any(|b| b.is_ascii_control() || b.is_ascii_whitespace() || b == b'\\')
    {
        return None;
    }
    let (scheme, rest) = url.split_once("://")?;
    if !scheme.eq_ignore_ascii_case("https") && !scheme.eq_ignore_ascii_case("http") {
        return None;
    }
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    Some(rest.split_at(authority_end))
}

// Deliberately a narrow HTTP URL parser, not a general URL normalizer. Reject
// credentials, escaped hosts and browser-style backslashes rather than guessing
// which authority a different parser would see. IDs are never percent-decoded.
fn http_url_parts(url: &str) -> Option<(&str, &str, &str)> {
    let (authority, path_query) = http_authority(url)?;
    if authority.contains('@') {
        return None;
    }
    let host = match authority.rsplit_once(':') {
        Some((host, port)) => {
            if port.is_empty()
                || port.len() > 5
                || !port.bytes().all(|b| b.is_ascii_digit())
                || port.parse::<u16>().is_err()
            {
                return None;
            }
            host
        }
        None => authority,
    };
    if host.is_empty()
        || host.len() > 253
        || !host.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
    {
        return None;
    }
    let path_query = path_query.split('#').next()?;
    let (path, query) = path_query.split_once('?').unwrap_or((path_query, ""));
    Some((host, path, query))
}

fn host_matches(host: &str, domain: &str) -> bool {
    host == domain
        || host
            .strip_suffix(domain)
            .is_some_and(|prefix| prefix.ends_with('.'))
}

fn valid_youtube_id(id: &str) -> bool {
    id.len() == 11
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

fn resolve_credit(
    request: &CreditRequest,
    cache: Option<&Path>,
    mut fetch: impl FnMut(&Video) -> Option<String>,
) -> String {
    let Some(video) = Video::parse(&request.url) else {
        let (tag_artist, tag_album) = if request.artist.is_empty() || request.album.is_empty() {
            local_tags(&request.url).unwrap_or_default()
        } else {
            (None, None)
        };
        return [
            (request.artist.as_str(), tag_artist.as_deref()),
            (request.album.as_str(), tag_album.as_deref()),
        ]
        .into_iter()
        .map(|(supplied, tagged)| match supplied {
            "" => tagged.unwrap_or_default(),
            _ => supplied,
        })
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" · ");
    };
    let cache = cache.filter(|directory| prepare_cache_dir(directory).is_ok());
    if let Some(author) = cache.and_then(|directory| read_cached_author(directory, &video)) {
        return author;
    }
    if let Some(author) = fetch(&video).and_then(|author| validated_author(&author)) {
        if let Some(directory) = cache {
            write_cached_author(directory, &video, &author);
        }
        return author;
    }
    // A browser's album is usually the platform, not the author. Never include
    // it for supported videos, even when the author request fails.
    if is_site_name(&request.artist) {
        String::new()
    } else {
        request.artist.clone()
    }
}

// Accept only empty-authority file URLs or literal absolute filesystem paths.
// Validate before decoding: file://%2Fmusic/song.mp3 is not an absolute file URL.
fn local_credit_path(url: &str) -> Option<PathBuf> {
    if let Some(encoded) = url.strip_prefix("file://") {
        if !encoded.starts_with('/')
            || encoded.starts_with("//")
            || encoded.bytes().any(|byte| {
                byte.is_ascii_control()
                    || byte.is_ascii_whitespace()
                    || matches!(byte, b'?' | b'#' | b'\\')
            })
        {
            return None;
        }
    } else if !url.starts_with('/') {
        return None;
    }
    let path = crate::artwork::local_path(url)?;
    let text = path.to_str()?;
    (!text.starts_with("//") && !text.chars().any(char::is_control)).then_some(path)
}

// Budget actual reads rather than file length, allowing large audio files and
// seeks to trailing tags without scanning audio or loading it all into memory.
struct TagReader<R> {
    inner: R,
    remaining: usize,
}

impl<R: Read> Read for TagReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        if self.remaining == 0 {
            return Err(io::Error::other("media tag read limit exceeded"));
        }
        let limit = buffer.len().min(self.remaining);
        let count = self.inner.read(&mut buffer[..limit])?;
        self.remaining -= count;
        Ok(count)
    }
}

impl<R: Seek> Seek for TagReader<R> {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.inner.seek(position)
    }
}

fn local_tags(url: &str) -> Option<(Option<String>, Option<String>)> {
    let path = local_credit_path(url)?;
    if !fs::metadata(&path).ok()?.is_file() {
        return None;
    }
    // Linux O_NONBLOCK also protects against a regular file being replaced by
    // a FIFO between metadata and open. Check the opened file again before I/O.
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(0o4000)
        .open(path)
        .ok()?;
    if !file.metadata().ok()?.is_file() {
        return None;
    }
    let mut reader = TagReader {
        inner: BufReader::new(file),
        remaining: MAX_TAG_READ_BYTES,
    };
    let tagged = Probe::new(&mut reader)
        .options(
            ParseOptions::new()
                .read_properties(false)
                .read_cover_art(false),
        )
        .guess_file_type()
        .ok()?
        .read()
        .ok()?;
    // Some formats tolerate read errors; never use a partially budgeted parse.
    if reader.remaining == 0 {
        return None;
    }
    let tag = tagged.primary_tag().or_else(|| tagged.first_tag())?;
    Some((
        tag.artist().and_then(|value| validated_author(&value)),
        tag.album().and_then(|value| validated_author(&value)),
    ))
}

fn is_site_name(artist: &str) -> bool {
    let normalized = artist.trim().to_ascii_lowercase();
    normalized == "youtube"
        || normalized == "youtube music"
        || normalized.contains("bilibili")
        || normalized.contains("哔哩哔哩")
        || normalized.contains("嗶哩嗶哩")
}

fn fetch_author(video: &Video) -> Option<String> {
    let program = std::env::var_os("MEDIA_CONTROL_CURL")
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| OsString::from("curl"));
    let mut child = Command::new(program)
        // Ignore curlrc: it must not introduce extra URLs or change the output.
        .args([
            "--disable",
            "--fail",
            "--silent",
            "--show-error",
            "--location",
            "--max-time",
            "10",
            "--max-redirs",
            "3",
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "--max-filesize",
        ])
        .arg(MAX_RESPONSE_BYTES.to_string())
        .arg("--url")
        .arg(video.endpoint())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    // Also cap the pipe: Content-Length may be missing, and a wrapper or older
    // curl may not enforce --max-filesize while streaming. Always reap curl.
    let body = child
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("missing curl output"))
        .and_then(|stdout| read_bounded(stdout, MAX_RESPONSE_BYTES));
    if body.is_err() {
        let _ = child.kill();
    }
    let status = child.wait().ok()?;
    if !status.success() {
        return None;
    }
    author_from_response(video, &body.ok()?)
}

fn author_from_response(video: &Video, bytes: &[u8]) -> Option<String> {
    if bytes.len() > MAX_RESPONSE_BYTES {
        return None;
    }
    let response: serde_json::Value = serde_json::from_slice(bytes).ok()?;
    let name = match video {
        Video::Youtube(_) => response.get("author_name")?.as_str()?,
        Video::BilibiliBv(_) | Video::BilibiliAv(_) => {
            if response.get("code")?.as_i64()? != 0 {
                return None;
            }
            response.get("data")?.get("owner")?.get("name")?.as_str()?
        }
    };
    validated_author(name)
}

fn validated_author(name: &str) -> Option<String> {
    let name = name.trim();
    (!name.is_empty() && name.len() <= MAX_AUTHOR_BYTES && !name.chars().any(char::is_control))
        .then(|| name.to_owned())
}

fn cache_directory(xdg: Option<OsString>, home: Option<OsString>) -> Option<PathBuf> {
    let base = xdg
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| {
            home.map(PathBuf::from)
                .filter(|path| path.is_absolute())
                .map(|path| path.join(".cache"))
        })?;
    Some(base.join(CACHE_DIR))
}

fn prepare_cache_dir(directory: &Path) -> io::Result<()> {
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(directory)?;
    if !fs::symlink_metadata(directory)?.file_type().is_dir() {
        return Err(io::Error::other("media author cache is not a directory"));
    }
    fs::set_permissions(directory, fs::Permissions::from_mode(0o700))
}

fn read_cached_author(directory: &Path, video: &Video) -> Option<String> {
    let path = directory.join(video.cache_name());
    let metadata = fs::symlink_metadata(&path).ok()?;
    if !metadata.is_file()
        || metadata.len() > MAX_CACHE_BYTES as u64
        || metadata.permissions().mode() & 0o077 != 0
        || SystemTime::now()
            .duration_since(metadata.modified().ok()?)
            .ok()?
            >= CACHE_TTL
    {
        return None;
    }
    let bytes = read_bounded(File::open(path).ok()?, MAX_CACHE_BYTES).ok()?;
    let author: String = serde_json::from_slice(&bytes).ok()?;
    validated_author(&author)
}

fn write_cached_author(directory: &Path, video: &Video, author: &str) {
    let Ok(bytes) = serde_json::to_vec(author) else {
        return;
    };
    if bytes.len() > MAX_CACHE_BYTES {
        return;
    }
    for _ in 0..4 {
        let sequence = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let temporary = directory.join(format!(
            ".media-author-{}-{sequence}.tmp",
            std::process::id()
        ));
        let mut file = match OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)
        {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(_) => return,
        };
        let written = file.write_all(&bytes);
        drop(file);
        if written
            .and_then(|()| fs::rename(&temporary, directory.join(video.cache_name())))
            .is_err()
        {
            let _ = fs::remove_file(&temporary);
        } else {
            trim_cache(directory);
        }
        return;
    }
}

fn is_cache_file_name(name: &str) -> bool {
    let Some(key) = name.strip_suffix(".json") else {
        return false;
    };
    if let Some(id) = key.strip_prefix("youtube-") {
        valid_youtube_id(id)
    } else {
        key.strip_prefix("bilibili-")
            .and_then(Video::bilibili)
            .is_some()
    }
}

fn trim_cache(directory: &Path) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    let mut files: Vec<_> = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_str().is_some_and(is_cache_file_name))
        .filter_map(|entry| {
            let metadata = fs::symlink_metadata(entry.path()).ok()?;
            metadata.is_file().then(|| {
                (
                    metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                    entry.path(),
                )
            })
        })
        .collect();
    let excess = files.len().saturating_sub(MAX_CACHE_ENTRIES);
    files.sort_unstable_by_key(|(modified, _)| *modified);
    for (_, path) in files.into_iter().take(excess) {
        let _ = fs::remove_file(path);
    }
}

#[cfg(test)]
media_unit_tests!(credit);

#[cfg(test)]
mod local_file_tests {
    use super::*;
    use lofty::config::WriteOptions;
    use lofty::prelude::TagExt;
    use lofty::tag::{Tag, TagType};
    use std::io::Cursor;
    use std::os::unix::fs::symlink;

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            loop {
                let sequence = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
                let path = std::env::temp_dir().join(format!(
                    "media-control-local-credit-test-{}-{sequence}",
                    std::process::id()
                ));
                match fs::DirBuilder::new().mode(0o700).create(&path) {
                    Ok(()) => return Self(path),
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                    Err(error) => panic!("create test directory: {error}"),
                }
            }
        }

        fn mp3(&self, artist: Option<&str>, album: Option<&str>) -> String {
            let path = self.0.join("Not the artist || Not the album % #? 曲.mp3");
            let mut tag = Tag::new(TagType::Id3v2);
            if let Some(artist) = artist {
                tag.set_artist(artist.to_owned());
            }
            if let Some(album) = album {
                tag.set_album(album.to_owned());
            }
            let mut bytes = Vec::new();
            tag.dump_to(&mut bytes, WriteOptions::default()).unwrap();
            // Three MPEG-1 Layer III frames, 128 kbps at 44.1 kHz.
            for _ in 0..3 {
                bytes.extend_from_slice(&[0xff, 0xfb, 0x90, 0x64]);
                bytes.resize(bytes.len() + 413, 0);
            }
            fs::write(&path, bytes).unwrap();
            path.to_str().unwrap().to_owned()
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn credit(url: &str, artist: &str, album: &str) -> String {
        resolve_credit(
            &CreditRequest {
                url: url.to_owned(),
                artist: artist.to_owned(),
                album: album.to_owned(),
            },
            None,
            |_| panic!("local or unsupported URL must never fetch"),
        )
    }

    fn file_url(path: &str) -> String {
        let encoded: String = path
            .bytes()
            .map(|byte| match byte {
                b'/' | b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' => {
                    char::from(byte).to_string()
                }
                _ => format!("%{byte:02X}"),
            })
            .collect();
        format!("file://{encoded}")
    }

    #[test]
    fn embedded_mp3_tags_fill_only_missing_mpris_fields() {
        let fixture = Fixture::new();
        let path = fixture.mp3(Some("Tagged 作者"), Some("Tagged Album"));
        let before = fs::read(&path).unwrap();
        for url in [&path, &file_url(&path)] {
            for (artist, album, expected) in [
                ("", "", "Tagged 作者 · Tagged Album"),
                (" MPRIS Artist ", "", " MPRIS Artist  · Tagged Album"),
                ("", "MPRIS Album", "Tagged 作者 · MPRIS Album"),
                ("MPRIS Artist", "MPRIS Album", "MPRIS Artist · MPRIS Album"),
                (" ", "MPRIS Album", "  · MPRIS Album"),
            ] {
                assert_eq!(credit(url, artist, album), expected);
            }
        }
        assert_eq!(fs::read(&path).unwrap(), before, "tag lookup is read-only");
        let request = CreditRequest {
            url: file_url(&path),
            artist: String::new(),
            album: String::new(),
        };
        let resolved = resolve_credit(&request, None, |_| panic!("unexpected fetch"));
        let mut output = Vec::new();
        write_reply(&mut output, &request, &resolved).unwrap();
        let reply: serde_json::Value = serde_json::from_slice(&output).unwrap();
        assert_eq!(reply["artist"], "");
        assert_eq!(reply["album"], "");
        assert_eq!(reply["url"], request.url);
        assert_eq!(reply["credit"], "Tagged 作者 · Tagged Album");
    }

    #[test]
    fn absent_or_invalid_tags_never_guess_from_the_filename() {
        let fixture = Fixture::new();
        for (artist, album, expected) in [
            (None, None, ""),
            (Some("Only Artist"), None, "Only Artist"),
            (None, Some("Only Album"), "Only Album"),
            (Some(" \t "), Some("Bad\nAlbum"), ""),
        ] {
            let path = fixture.mp3(artist, album);
            assert_eq!(credit(&path, "", ""), expected);
        }
    }

    #[test]
    fn missing_corrupt_and_nonregular_files_preserve_supplied_metadata() {
        let fixture = Fixture::new();
        let missing = fixture.0.join("missing.mp3");
        let bad = fixture.0.join("bad.mp3");
        fs::write(&bad, b"not an audio file").unwrap();
        let fifo = fixture.0.join("fifo.mp3");
        assert!(
            Command::new("mkfifo")
                .arg(&fifo)
                .status()
                .unwrap()
                .success()
        );
        let link = fixture.0.join("fifo-link.mp3");
        symlink(&fifo, &link).unwrap();
        for path in [&missing, &bad, &fixture.0, &fifo, &link] {
            for (artist, album, expected) in [
                ("", "", ""),
                ("Artist", "", "Artist"),
                ("", "Album", "Album"),
                ("Artist", "Album", "Artist · Album"),
            ] {
                assert_eq!(credit(path.to_str().unwrap(), artist, album), expected);
            }
        }
    }

    #[test]
    fn only_strict_absolute_local_paths_and_file_urls_are_accepted() {
        let fixture = Fixture::new();
        let path = fixture.mp3(Some("Artist"), Some("Album"));
        let url = file_url(&path);
        assert_eq!(local_credit_path(&url), Some(PathBuf::from(&path)));
        assert_eq!(local_credit_path(&path), Some(PathBuf::from(&path)));
        for invalid in [
            String::new(),
            "relative/song.mp3".into(),
            "file:relative.mp3".into(),
            "file:/music/song.mp3".into(),
            "file://relative/song.mp3".into(),
            "file://%2Fmusic/song.mp3".into(),
            "file:///music/bad%2.mp3".into(),
            "file:///music/bad%GG.mp3".into(),
            "file:///music/bad%00.mp3".into(),
            "file:///music/bad%FF.mp3".into(),
            format!("file://remote.test{path}"),
            format!("file://localhost{path}"),
            format!("file://user@remote.test{path}"),
            format!("file:///{path}"),
            format!("file://%2F{path}"),
            format!("/{path}"),
            format!("{url}?query"),
            format!("{url}#fragment"),
            format!("https://example.test{path}"),
            format!("smb://example.test{path}"),
        ] {
            assert_eq!(local_credit_path(&invalid), None, "{invalid:?}");
            assert_eq!(credit(&invalid, "", ""), "", "{invalid:?}");
            assert_eq!(credit(&invalid, "Provided", ""), "Provided");
        }
    }

    #[test]
    fn tag_read_budget_is_not_reset_by_seeking() {
        let mut reader = TagReader {
            inner: Cursor::new(b"12345678"),
            remaining: 4,
        };
        let mut buffer = [0; 3];
        assert_eq!(reader.read(&mut buffer).unwrap(), 3);
        reader.seek(SeekFrom::Start(0)).unwrap();
        assert_eq!(reader.read(&mut buffer).unwrap(), 1);
        assert!(reader.read(&mut buffer).is_err());
        assert_eq!(reader.read(&mut []).unwrap(), 0);
    }

    #[test]
    fn excessive_tag_input_fails_closed() {
        let fixture = Fixture::new();
        let oversized_artist = "x".repeat(MAX_TAG_READ_BYTES + 1);
        let path = fixture.mp3(Some(&oversized_artist), Some("Album"));
        assert_eq!(credit(&path, "", ""), "");
        assert_eq!(credit(&path, "Provided", ""), "Provided");
    }
}
