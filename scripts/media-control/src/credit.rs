//! Headless credit lines for the media panel. Only recognized video IDs can
//! trigger a public metadata lookup; neither page URLs nor response data select
//! an endpoint. Successful authors are cached, never metadata fallbacks.

use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

const MAX_INPUT_BYTES: usize = 64 * 1024;
const MAX_RESPONSE_BYTES: usize = 256 * 1024;
const MAX_AUTHOR_BYTES: usize = 1024;
const MAX_CACHE_BYTES: usize = 4096;
const MAX_CACHE_ENTRIES: usize = 128;
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

// Deliberately a narrow HTTP URL parser, not a general URL normalizer. Reject
// credentials, escaped hosts and browser-style backslashes rather than guessing
// which authority a different parser would see. IDs are never percent-decoded.
fn http_url_parts(url: &str) -> Option<(&str, &str, &str)> {
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
    let authority = &rest[..authority_end];
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
    let path_query = rest[authority_end..].split('#').next()?;
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
        return [request.artist.as_str(), request.album.as_str()]
            .into_iter()
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
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    const YOUTUBE_ID: &str = "dQw4w9WgXcQ";
    const YOUTUBE_URL: &str = "https://www.youtube.com/watch?v=dQw4w9WgXcQ";
    const BILIBILI_URL: &str = "https://www.bilibili.com/video/BV1xx411c7mD";
    static NEXT_TEST_DIR: AtomicU64 = AtomicU64::new(0);

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            loop {
                let sequence = NEXT_TEST_DIR.fetch_add(1, Ordering::Relaxed);
                let path = std::env::temp_dir().join(format!(
                    "media-control-credit-test-{}-{sequence}",
                    std::process::id()
                ));
                match fs::DirBuilder::new().mode(0o700).create(&path) {
                    Ok(()) => return Self(path),
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                    Err(error) => panic!("create test directory: {error}"),
                }
            }
        }

        fn cache(&self) -> PathBuf {
            self.0.join("cache")
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn request(url: &str, artist: &str, album: &str) -> CreditRequest {
        CreditRequest {
            url: url.to_owned(),
            artist: artist.to_owned(),
            album: album.to_owned(),
        }
    }

    #[test]
    fn youtube_forms_share_a_video_key_and_canonical_endpoint() {
        let expected = Video::Youtube(YOUTUBE_ID.to_owned());
        for url in [
            YOUTUBE_URL,
            "HTTP://M.YouTube.COM:80/watch?list=abc&v=dQw4w9WgXcQ&t=20#anything",
            "https://music.youtube.com/watch/?v=dQw4w9WgXcQ",
            "https://youtube.com/shorts/dQw4w9WgXcQ?feature=share",
            "https://youtube.com/live/dQw4w9WgXcQ#t=10",
            "https://www.youtube.com/embed/dQw4w9WgXcQ/",
            "https://youtu.be/dQw4w9WgXcQ?si=abc",
            "https://www.youtu.be/dQw4w9WgXcQ",
        ] {
            assert_eq!(
                Video::parse(url),
                Some(Video::Youtube(YOUTUBE_ID.into())),
                "{url}"
            );
            assert_eq!(
                Video::parse(url).unwrap().cache_name(),
                expected.cache_name()
            );
        }
        assert_eq!(
            expected.endpoint(),
            "https://www.youtube.com/oembed?format=json&url=https%3A%2F%2Fwww.youtube.com%2Fwatch%3Fv%3DdQw4w9WgXcQ"
        );
        assert_eq!(expected.cache_name(), "youtube-dQw4w9WgXcQ.json");
        assert!(Video::parse("https://youtu.be/a_B-0123456").is_some());
    }

    #[test]
    fn bilibili_ids_select_only_fixed_api_parameters() {
        let bv = Video::BilibiliBv("BV1xx411c7mD".into());
        for url in [
            BILIBILI_URL,
            "https://m.bilibili.com:443/video/BV1xx411c7mD/?p=2#reply",
        ] {
            assert_eq!(
                Video::parse(url),
                Some(Video::BilibiliBv("BV1xx411c7mD".into()))
            );
        }
        assert_eq!(
            bv.endpoint(),
            "https://api.bilibili.com/x/web-interface/view?bvid=BV1xx411c7mD"
        );
        assert_eq!(bv.cache_name(), "bilibili-BV1xx411c7mD.json");
        let av = Video::parse("http://bilibili.com/video/av000170001?redirect=https://evil.test")
            .unwrap();
        assert_eq!(av, Video::BilibiliAv(170001));
        assert_eq!(
            av.endpoint(),
            "https://api.bilibili.com/x/web-interface/view?aid=170001"
        );
        assert_eq!(av.cache_name(), "bilibili-av170001.json");
    }

    #[test]
    fn malicious_or_ambiguous_authorities_are_not_video_urls() {
        for url in [
            "https://youtube.com.evil.test/watch?v=dQw4w9WgXcQ",
            "https://notyoutube.com/watch?v=dQw4w9WgXcQ",
            "https://youtu.be.evil.test/dQw4w9WgXcQ",
            "https://notyoutu.be/dQw4w9WgXcQ",
            "https://bilibili.com.evil.test/video/BV1xx411c7mD",
            "https://notbilibili.com/video/BV1xx411c7mD",
            "https://youtube.com@evil.test/watch?v=dQw4w9WgXcQ",
            "https://evil.test@youtube.com/watch?v=dQw4w9WgXcQ",
            "https://user:password@bilibili.com/video/av170001",
            "https://evil.test/youtube.com/watch?v=dQw4w9WgXcQ",
            "https://evil.test/?url=https://youtu.be/dQw4w9WgXcQ",
            "https://youtube.com\\@evil.test/watch?v=dQw4w9WgXcQ",
            "https://youtube%2ecom/watch?v=dQw4w9WgXcQ",
            "https://yоutube.com/watch?v=dQw4w9WgXcQ",
            "https://.youtube.com/watch?v=dQw4w9WgXcQ",
            "https://x..youtube.com/watch?v=dQw4w9WgXcQ",
            "https://-x.youtube.com/watch?v=dQw4w9WgXcQ",
            "https://youtube.com./watch?v=dQw4w9WgXcQ",
            "https://youtube.com:/watch?v=dQw4w9WgXcQ",
            "https://youtube.com:65536/watch?v=dQw4w9WgXcQ",
            "https://youtube.com:abc/watch?v=dQw4w9WgXcQ",
            "https://youtube.com:443:80/watch?v=dQw4w9WgXcQ",
            "https:///youtube.com/watch?v=dQw4w9WgXcQ",
            "https://youtube.com\n/watch?v=dQw4w9WgXcQ",
            " https://youtu.be/dQw4w9WgXcQ",
            "//youtube.com/watch?v=dQw4w9WgXcQ",
            "ftp://youtube.com/watch?v=dQw4w9WgXcQ",
            "file://youtube.com/watch?v=dQw4w9WgXcQ",
        ] {
            assert_eq!(Video::parse(url), None, "{url:?}");
        }
    }

    #[test]
    fn ids_must_be_ascii_bounded_and_in_the_expected_path_or_query() {
        for url in [
            "https://youtube.com/?v=dQw4w9WgXcQ",
            "https://youtube.com/watch#v=dQw4w9WgXcQ",
            "https://youtube.com/watch?notv=dQw4w9WgXcQ",
            "https://youtube.com/watch?v=dQw4w9WgXc",
            "https://youtube.com/watch?v=dQw4w9WgXcQQ",
            "https://youtube.com/watch?v=dQw4w9WgXc%51",
            "https://youtube.com/shorts/dQw4w9WgXcQQ",
            "https://youtu.be/é123456789",
            "https://youtu.be/../dQw4w9WgXcQ",
            "https://bilibili.com/?path=/video/av170001",
            "https://bilibili.com/video/BV",
            "https://bilibili.com/video/bv1xx411c7mD",
            "https://bilibili.com/video/BV123_abc",
            "https://bilibili.com/video/BV123%26aid=5",
            "https://bilibili.com/video/av",
            "https://bilibili.com/video/av0",
            "https://bilibili.com/video/av123x",
            "https://bilibili.com/video/av１２３",
            "https://bilibili.com/video/av18446744073709551616",
        ] {
            assert_eq!(Video::parse(url), None, "{url}");
        }
        assert!(
            Video::parse(&format!("https://bilibili.com/video/BV{}", "a".repeat(63))).is_none()
        );
        let video = Video::parse(&format!(
            "{YOUTUBE_URL}&url=https://evil.test/&v=../../secret"
        ))
        .unwrap();
        assert_eq!(video, Video::Youtube(YOUTUBE_ID.into()));
    }

    #[test]
    fn unsupported_and_local_music_never_fetch_and_join_nonempty_metadata() {
        for url in [
            "",
            "file:///music/song.flac",
            "/music/song.flac",
            "https://example.test/song",
        ] {
            for (artist, album, expected) in [
                ("Artist", "Album", "Artist · Album"),
                ("Artist", "", "Artist"),
                ("", "Album", "Album"),
                ("", "", ""),
            ] {
                assert_eq!(
                    resolve_credit(&request(url, artist, album), None, |_| panic!(
                        "unexpected fetch"
                    )),
                    expected
                );
            }
        }
    }

    #[test]
    fn supported_video_uses_author_or_artist_but_never_album() {
        for url in [YOUTUBE_URL, BILIBILI_URL] {
            let metadata = request(url, "Actual artist", "Platform album");
            assert_eq!(
                resolve_credit(&metadata, None, |_| Some(" Channel 作者 ".into())),
                "Channel 作者"
            );
            assert_eq!(resolve_credit(&metadata, None, |_| None), "Actual artist");
            for artist in [
                "",
                "YouTube",
                " YOUTUBE MUSIC ",
                "BILIBILI",
                "哔哩哔哩 (゜-゜)つロ 干杯~-bilibili",
                "嗶哩嗶哩",
            ] {
                assert_eq!(
                    resolve_credit(&request(url, artist, "Fake album"), None, |_| None),
                    ""
                );
            }
            assert_eq!(
                resolve_credit(&request(url, "YouTube Orchestra", "Album"), None, |_| None),
                "YouTube Orchestra"
            );
        }
    }

    #[test]
    fn author_response_shapes_are_checked() {
        let youtube = Video::parse(YOUTUBE_URL).unwrap();
        let bilibili = Video::parse(BILIBILI_URL).unwrap();
        assert_eq!(
            author_from_response(&youtube, br#"{"author_name":" Channel "}"#),
            Some("Channel".into())
        );
        assert_eq!(
            author_from_response(
                &bilibili,
                r#"{"code":0,"data":{"owner":{"name":"作者"}}}"#.as_bytes()
            ),
            Some("作者".into())
        );
        for bytes in [
            b"not json".as_slice(),
            br#"[]"#,
            br#"null"#,
            br#"{}"#,
            br#"{"author_name":42}"#,
            br#"{"author_name":null}"#,
            br#"{"author_name":"   "}"#,
            br#"{"author_name":"Bad\nName"}"#,
        ] {
            assert_eq!(author_from_response(&youtube, bytes), None);
        }
        for bytes in [
            br#"{"code":-404,"data":{"owner":{"name":"Wrong"}}}"#.as_slice(),
            br#"{"code":"0","data":{"owner":{"name":"Wrong"}}}"#,
            br#"{"data":{"owner":{"name":"Wrong"}}}"#,
            br#"{"code":0,"data":null}"#,
            br#"{"code":0,"data":{"owner":{"name":12}}}"#,
        ] {
            assert_eq!(author_from_response(&bilibili, bytes), None);
        }
        let long_name = serde_json::to_vec(
            &serde_json::json!({"author_name": "x".repeat(MAX_AUTHOR_BYTES + 1)}),
        )
        .unwrap();
        assert_eq!(author_from_response(&youtube, &long_name), None);
        assert_eq!(
            author_from_response(&youtube, &vec![b' '; MAX_RESPONSE_BYTES + 1]),
            None
        );
    }

    #[test]
    fn input_is_one_bounded_object_and_errors_do_not_echo_it() {
        for bytes in [
            b"private invalid input".as_slice(),
            b"{}",
            br#"{"url":null,"artist":"a","album":"b"}"#,
            br#"{"url":"u","artist":[],"album":"b"}"#,
            br#"{"url":"u","artist":"a","album":"b"} {}"#,
        ] {
            let error = read_request(bytes).unwrap_err().to_string();
            assert_eq!(
                error,
                "invalid media credit JSON (expected string url, artist and album)"
            );
        }
        let oversized = vec![b'x'; MAX_INPUT_BYTES + 1];
        assert_eq!(
            read_request(oversized.as_slice()).unwrap_err().to_string(),
            "media credit data exceeds size limit"
        );
        assert!(read_bounded(&b"1234"[..], 4).is_ok());
        assert!(read_bounded(&b"12345"[..], 4).is_err());
    }

    #[test]
    fn replies_echo_identity_exactly_and_escape_json() {
        let metadata = request("file:///a%20b?x=\"#y", " 作者\n\"Name\" ", "Album\\Disc");
        let input = serde_json::to_vec(&serde_json::json!({
            "url": metadata.url,
            "artist": metadata.artist,
            "album": metadata.album,
        }))
        .unwrap();
        let parsed = read_request(input.as_slice()).unwrap();
        let mut output = Vec::new();
        write_reply(&mut output, &parsed, "credit \"作者\"").unwrap();
        let reply: serde_json::Value = serde_json::from_slice(&output).unwrap();
        assert_eq!(
            reply,
            serde_json::json!({
                "url": metadata.url,
                "artist": metadata.artist,
                "album": metadata.album,
                "credit": "credit \"作者\"",
            })
        );
        assert_eq!(output.iter().filter(|&&b| b == b'\n').count(), 1);
        assert_eq!(output.last(), Some(&b'\n'));
    }

    #[test]
    fn cache_paths_follow_xdg_without_changing_process_environment() {
        assert_eq!(
            cache_directory(Some("/xdg/cache".into()), Some("/home/test".into())),
            Some(PathBuf::from("/xdg/cache/media-control/authors"))
        );
        for xdg in [None, Some("".into()), Some("relative".into())] {
            assert_eq!(
                cache_directory(xdg, Some("/home/test".into())),
                Some(PathBuf::from("/home/test/.cache/media-control/authors"))
            );
        }
        assert_eq!(cache_directory(None, None), None);
        assert_eq!(cache_directory(None, Some("relative".into())), None);
    }

    #[test]
    fn successful_authors_persist_across_url_variants_in_private_files() {
        let temporary = TempDir::new();
        let cache = temporary.cache();
        let first = request(YOUTUBE_URL, "YouTube", "YouTube");
        assert_eq!(
            resolve_credit(&first, Some(&cache), |_| Some("Channel".into())),
            "Channel"
        );
        let second = request("https://youtu.be/dQw4w9WgXcQ?t=45", "Other", "Album");
        assert_eq!(
            resolve_credit(&second, Some(&cache), |_| panic!("cache miss")),
            "Channel"
        );
        assert_eq!(
            fs::metadata(&cache).unwrap().permissions().mode() & 0o777,
            0o700
        );
        let file = cache.join(Video::parse(YOUTUBE_URL).unwrap().cache_name());
        assert_eq!(
            fs::metadata(file).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(fs::read_dir(cache).unwrap().count(), 1);
    }

    #[test]
    fn failures_and_invalid_authors_are_not_cached() {
        let temporary = TempDir::new();
        let cache = temporary.cache();
        let metadata = request(YOUTUBE_URL, "Artist", "Album");
        let mut calls = 0;
        for author in [None, Some(" ".into()), Some("Bad\nName".into()), None] {
            assert_eq!(
                resolve_credit(&metadata, Some(&cache), |_| {
                    calls += 1;
                    author.clone()
                }),
                "Artist"
            );
        }
        assert_eq!(calls, 4);
        assert_eq!(fs::read_dir(cache).unwrap().count(), 0);
    }

    #[test]
    fn stale_corrupt_oversized_or_public_cache_entries_are_misses() {
        let temporary = TempDir::new();
        let cache = temporary.cache();
        let video = Video::parse(YOUTUBE_URL).unwrap();
        let metadata = request(YOUTUBE_URL, "Artist", "Album");
        prepare_cache_dir(&cache).unwrap();
        write_cached_author(&cache, &video, "Old");
        let path = cache.join(video.cache_name());
        File::open(&path)
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(SystemTime::UNIX_EPOCH))
            .unwrap();
        assert_eq!(read_cached_author(&cache, &video), None);
        fs::write(&path, b"not JSON").unwrap();
        assert_eq!(read_cached_author(&cache, &video), None);
        fs::write(&path, vec![b'x'; MAX_CACHE_BYTES + 1]).unwrap();
        assert_eq!(read_cached_author(&cache, &video), None);
        fs::write(&path, b"\"Public\"").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(read_cached_author(&cache, &video), None);
        assert_eq!(
            resolve_credit(&metadata, Some(&cache), |_| Some("Fresh".into())),
            "Fresh"
        );
        assert_eq!(read_cached_author(&cache, &video), Some("Fresh".into()));
        assert_eq!(
            fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[test]
    fn cache_symlinks_are_not_followed_and_unusable_caches_do_not_break_credit() {
        let temporary = TempDir::new();
        let outside = temporary.0.join("outside");
        fs::create_dir(&outside).unwrap();
        let cache = temporary.cache();
        symlink(&outside, &cache).unwrap();
        let metadata = request(YOUTUBE_URL, "Artist", "Album");
        assert_eq!(
            resolve_credit(&metadata, Some(&cache), |_| Some("Channel".into())),
            "Channel"
        );
        assert_eq!(fs::read_dir(&outside).unwrap().count(), 0);
        fs::remove_file(&cache).unwrap();
        fs::write(&cache, b"not a directory").unwrap();
        assert_eq!(resolve_credit(&metadata, Some(&cache), |_| None), "Artist");
        fs::remove_file(&cache).unwrap();
        prepare_cache_dir(&cache).unwrap();
        let external = outside.join("author");
        fs::write(&external, b"\"Untrusted\"").unwrap();
        let video = Video::parse(YOUTUBE_URL).unwrap();
        symlink(&external, cache.join(video.cache_name())).unwrap();
        assert_eq!(read_cached_author(&cache, &video), None);
        assert_eq!(
            resolve_credit(&metadata, Some(&cache), |_| Some("Trusted".into())),
            "Trusted"
        );
        assert_eq!(fs::read(&external).unwrap(), b"\"Untrusted\"");
        assert_eq!(read_cached_author(&cache, &video), Some("Trusted".into()));
    }

    #[test]
    fn cache_entry_count_is_bounded_and_unrelated_files_are_left_alone() {
        let temporary = TempDir::new();
        let cache = temporary.cache();
        prepare_cache_dir(&cache).unwrap();
        fs::write(cache.join("unrelated"), b"keep").unwrap();
        for index in 0..MAX_CACHE_ENTRIES + 4 {
            write_cached_author(&cache, &Video::Youtube(format!("{index:011x}")), "Author");
        }
        let names: Vec<_> = fs::read_dir(&cache)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(
            names
                .iter()
                .filter(|name| name.to_str().is_some_and(is_cache_file_name))
                .count(),
            MAX_CACHE_ENTRIES
        );
        assert_eq!(names.len(), MAX_CACHE_ENTRIES + 1);
        assert_eq!(fs::read(cache.join("unrelated")).unwrap(), b"keep");
    }
}
