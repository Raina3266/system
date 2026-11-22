//! All media policy and real-binary tests in one integration target.

// Expand unit tests inside their source modules to retain private access.
macro_rules! media_unit_tests {
    (artwork) => {
        mod tests {
            use super::*;

            #[test]
            fn file_urls_are_percent_decoded_into_paths() {
                assert_eq!(
                    percent_decode("MAMAMOO%20%7C%7C%20MEMORY"),
                    Some("MAMAMOO || MEMORY".to_owned())
                );
                assert_eq!(
                    local_path("file:///home/raina/Documents/MAMAMOO%20%7C%7C%20MEMORY/track%20one.mp3"),
                    Some(PathBuf::from(
                        "/home/raina/Documents/MAMAMOO || MEMORY/track one.mp3"
                    ))
                );
                assert_eq!(
                    local_path("/home/raina/track.mp3"),
                    Some(PathBuf::from("/home/raina/track.mp3"))
                );
                assert_eq!(local_path("relative/track.mp3"), None);
                assert_eq!(local_path("file:///home/raina/bad%2track.mp3"), None);
            }
        }
    };
    (credit) => {
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
                    "https://youtube.com:000443/watch?v=dQw4w9WgXcQ",
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
    };
    (model) => {
        mod tests {
            use super::*;
            use std::io::{BufReader, Cursor};

            use serde_json::json;

            fn player(bus: &str, app: &str, title: &str) -> Player {
                Player {
                    bus: bus.to_owned(),
                    identity: app.to_owned(),
                    title: title.to_owned(),
                    length: Some(203.0),
                    playing: true,
                    can_seek: true,
                    can_next: true,
                    can_previous: true,
                    ..Player::default()
                }
            }

            fn spotify_players() -> [Player; 2] {
                let mut real = player("real", "Chromium", "Get Up");
                real.artist = "Far East Movement".to_owned();
                real.album = "Album".to_owned();
                real.url = "https://open.spotify.com/search/far%20east%20movement%20get%20up/tracks"
                    .to_owned();
                real.length = Some(166.968);
                real.can_play = true;
                real.can_pause = true;
                let mut duplicate = real.clone();
                duplicate.bus = "duplicate".to_owned();
                duplicate.url = "https://open.spotify.com/album/id".to_owned();
                duplicate.length = None;
                duplicate.can_play = false;
                duplicate.can_pause = false;
                duplicate.can_seek = false;
                [real, duplicate]
            }

            fn browser_companions(url: &str) -> [Player; 2] {
                let mut native = player("native", "Native Publisher", "Track");
                native.browser_hint = json!(true);
                native.artist = "Artist".to_owned();
                native.album = "Album".to_owned();
                native.length = Some(222.353378);
                native.position = 121.125;
                native.can_play = true;
                native.can_pause = true;
                let mut companion = native.clone();
                companion.bus = "companion".to_owned();
                companion.identity = "Companion Publisher".to_owned();
                companion.url = url.to_owned();
                companion.length = Some(222.353);
                companion.position = 0.0;
                companion.can_play = false;
                companion.can_pause = false;
                companion.can_seek = false;
                [native, companion]
            }

            fn node(id: u32, app: &str, title: &str) -> Stream {
                Stream {
                    id,
                    name: app.to_owned(),
                    ready: true,
                    properties: Map::from_iter([
                        ("application.name".to_owned(), json!(app)),
                        ("media.name".to_owned(), json!(title)),
                    ]),
                    ..Stream::default()
                }
            }

            fn request(players: Vec<Player>, streams: Vec<Stream>) -> Request {
                Request {
                    request_id: 7,
                    players,
                    streams,
                    ..Request::default()
                }
            }

            fn action(kind: &str, player: &str) -> Action {
                Action {
                    kind: kind.to_owned(),
                    player: player.to_owned(),
                    ..Action::default()
                }
            }

            // Isolate stream ownership from player filtering and duplicate matching.
            fn volume_matches<'a>(players: &[Player], streams: &'a [Stream]) -> Vec<StreamMatch<'a>> {
                let groups: Vec<_> = players
                    .iter()
                    .map(|player| Members {
                        players: vec![player],
                        primary: player,
                    })
                    .collect();
                match_groups(&groups, streams)
            }

            fn matched_ids(players: &[Player], streams: &[Stream]) -> Vec<Option<u32>> {
                volume_matches(players, streams)
                    .iter()
                    .map(|matched| matched.node.map(|node| node.id))
                    .collect()
            }

            #[test]
            fn volume_distinct_apps_and_order() {
                let mut players = vec![
                    player("spotify", "Spotify", "Same"),
                    player("firefox", "Firefox", "Same"),
                ];
                let mut nodes = vec![node(1, "Spotify", "Same"), node(2, "Firefox", "Same")];
                for _ in 0..2 {
                    for _ in 0..2 {
                        let matches = volume_matches(&players, &nodes);
                        for (source, matched) in players.iter().zip(matches) {
                            assert_eq!(matched.node.unwrap().name, source.identity);
                        }
                        nodes.reverse();
                    }
                    players.reverse();
                }
                assert_eq!(
                    matched_ids(&[player("other", "Other", "Same")], &nodes),
                    [None]
                );
            }

            #[test]
            fn volume_titles_and_urls_are_trimmed_application_scoped_evidence() {
                let mut players = vec![player("a", "Chromium", " A "), player("b", "Chromium", "B")];
                let mut nodes = vec![node(1, "Chromium", "A"), node(2, "Chromium", "B")];
                assert_eq!(matched_ids(&players, &nodes), [Some(1), Some(2)]);
                nodes.reverse();
                assert_eq!(matched_ids(&players, &nodes), [Some(1), Some(2)]);
                players[0].url = " https://example/media ".to_owned();
                nodes[1]
                    .properties
                    .insert("media.name".to_owned(), json!("Unknown"));
                for key in ["media.url", "media.filename"] {
                    nodes[1]
                        .properties
                        .insert(key.to_owned(), json!(" https://example/media "));
                    assert_eq!(matched_ids(&players, &nodes), [Some(1), Some(2)]);
                    nodes[1].properties.remove(key);
                }
                nodes[1]
                    .properties
                    .insert("media.title".to_owned(), json!(" A "));
                assert_eq!(matched_ids(&players, &nodes), [Some(1), Some(2)]);
                assert_eq!(
                    matched_ids(&[player("other", "Other", "A")], &nodes),
                    [None]
                );
            }

            #[test]
            fn volume_shared_and_multiple_streams_are_never_consumed_in_order() {
                let players = vec![player("a", "Chromium", ""), player("b", "Chromium", "")];
                let mut nodes = vec![node(1, "Chromium", "")];
                let matches = volume_matches(&players, &nodes);
                assert!(matches.iter().all(|matched| matched.node.is_none()));
                assert!(matches[0].reason.to_lowercase().contains("share"));
                nodes.push(node(2, "Chromium", ""));
                assert_eq!(volume_matches(&players[..1], &nodes)[0].reason, MULTIPLE);
                let native = [player("a", "Spotify", ""), player("b", "Spotify", "")];
                let native_nodes = [node(3, "Spotify", "")];
                let matches = volume_matches(&native, &native_nodes);
                assert!(matches.iter().all(|matched| matched.reason == SHARED));
            }

            #[test]
            fn volume_multiple_candidates_retain_claims_on_otherwise_unique_matches() {
                let mut players = vec![
                    player("a", "Native", "A"),
                    player("b", "Native", "Unknown"),
                    player("c", "Other", "C"),
                ];
                let mut nodes = vec![
                    node(1, "Native", "A"),
                    node(2, "Native", "Another track"),
                    node(3, "Other", "C"),
                ];
                for _ in 0..2 {
                    for (source, matched) in players.iter().zip(volume_matches(&players, &nodes)) {
                        match source.bus.as_str() {
                            "a" => {
                                assert!(matched.node.is_none());
                                assert_eq!(matched.reason, SHARED);
                            }
                            "b" => {
                                assert!(matched.node.is_none());
                                assert_eq!(matched.reason, MULTIPLE);
                            }
                            "c" => {
                                assert_eq!(matched.node.unwrap().id, 3);
                                assert_eq!(matched.reason, UNIQUE);
                            }
                            _ => unreachable!(),
                        }
                    }
                    players.reverse();
                    nodes.reverse();
                }
            }

            #[test]
            fn volume_active_paused_and_aliases() {
                let mut players = vec![
                    player("paused", "Google Chrome", ""),
                    player("playing", "Chromium", ""),
                ];
                players[0].playing = false;
                let nodes = [node(1, "chrome", "")];
                assert_eq!(matched_ids(&players, &nodes), [None, None]);
                players.reverse();
                assert_eq!(matched_ids(&players, &nodes), [None, None]);
                assert_eq!(
                    matched_ids(&players, &[node(2, "Spotify", "")]),
                    [None, None]
                );
                assert_eq!(
                    matched_ids(
                        &[player("a", "Chrome", "")],
                        &[node(1, "Chrome", ""), node(2, "Chromium", "")]
                    ),
                    [None]
                );
                for alias in [
                    "Google Chrome",
                    "google-chrome",
                    "google-chrome-stable",
                    "chrome",
                    "org.chromium.chromium",
                    "org.google.chrome",
                    "Chromium.desktop",
                ] {
                    assert_eq!(canonical(alias), "chromium");
                }
                for alias in [
                    "Mozilla Firefox",
                    "org.mozilla.firefox",
                    " Firefox.desktop ",
                ] {
                    assert_eq!(canonical(alias), "firefox");
                }
            }

            #[test]
            fn volume_missing_unready_and_unbound_streams() {
                let players = [player("a", "Spotify", "")];
                assert_eq!(volume_matches(&players, &[])[0].reason, NO_MATCH);
                let mut stream = node(1, "Spotify", "");
                stream.ready = false;
                let matches = volume_matches(&players, std::slice::from_ref(&stream));
                assert_eq!(matches[0].reason, READING);
                assert!(matches[0].node.is_none());
                assert_eq!(
                    matched_ids(&[player("a", "Spot", "")], &[stream.clone()]),
                    [None]
                );
                stream.ready = true;
                assert_eq!(matched_ids(&players, &[stream.clone()]), [Some(1)]);
                stream.id = 0;
                let matches = volume_matches(&players, std::slice::from_ref(&stream));
                assert_eq!(matches[0].reason, READING);
                assert!(matches[0].node.is_none());
            }

            #[test]
            fn volume_native_keys_and_explicit_browser_hint_do_not_mutate_inputs() {
                let mut source = player("a", "Chromium", "");
                source.browser_hint = json!("Firefox");
                let mut firefox = node(1, "unrelated-node", "");
                firefox
                    .properties
                    .insert("application.process.binary".to_owned(), json!("firefox"));
                let chrome = node(2, "Chromium", "");
                let before = (source.clone(), firefox.clone(), chrome.clone());
                assert_eq!(
                    matched_ids(&[source.clone()], &[chrome.clone(), firefox.clone()]),
                    [Some(1)]
                );
                assert_eq!((source.clone(), firefox.clone(), chrome), before);
                for hint in [
                    json!(true),
                    json!(false),
                    Value::Null,
                    json!("unknown"),
                    json!(["Firefox"]),
                ] {
                    source.browser_hint = hint;
                    assert_eq!(matched_ids(&[source.clone()], &[firefox.clone()]), [None]);
                }
                source = player("a", "Unknown", "");
                source.desktop_entry = "org.example.Player.desktop".to_owned();
                firefox
                    .properties
                    .insert("application.id".to_owned(), json!("org.example.Player"));
                assert_eq!(matched_ids(&[source], &[firefox]), [Some(1)]);
            }

            #[test]
            fn volume_each_native_application_key_and_nickname_is_used() {
                let players = [player("a", "org.example.Player", "")];
                for key in [
                    "application.name",
                    "application.id",
                    "application.process.binary",
                ] {
                    let mut stream = node(1, "unrelated", "");
                    stream
                        .properties
                        .insert(key.to_owned(), json!(" Org.Example.Player.desktop "));
                    assert_eq!(matched_ids(&players, &[stream]), [Some(1)]);
                }
                let mut stream = node(1, "unrelated", "");
                stream.nickname = "org.example.Player".to_owned();
                assert_eq!(matched_ids(&players, &[stream]), [Some(1)]);
            }

            #[test]
            fn volume_paused_with_explicit_track_evidence() {
                let mut players = vec![
                    player("a", "Chromium", "Paused track"),
                    player("b", "Chromium", "Playing track"),
                ];
                players[0].playing = false;
                let mut nodes = vec![
                    node(1, "chrome", "Paused track"),
                    node(2, "chrome", "Playing track"),
                ];
                assert_eq!(matched_ids(&players, &nodes), [Some(1), Some(2)]);
                players.reverse();
                nodes.reverse();
                assert_eq!(matched_ids(&players, &nodes), [Some(2), Some(1)]);
            }

            #[test]
            fn volume_shared_stream_even_with_one_matching_title() {
                let mut players = vec![player("a", "Chromium", "A"), player("b", "Chromium", "B")];
                let nodes = [node(1, "chrome", "A")];
                for _ in 0..2 {
                    let matches = volume_matches(&players, &nodes);
                    assert!(matches.iter().all(|matched| matched.node.is_none()));
                    assert!(matches.iter().any(|matched| matched.reason == SHARED));
                    assert!(
                        matches
                            .iter()
                            .any(|matched| matched.reason == BROWSER_SHARED)
                    );
                    players.reverse();
                }
            }

            #[test]
            fn volume_paused_stream_cannot_be_claimed_by_new_playing_source() {
                let mut players = vec![
                    player("a", "Chromium", "Old track"),
                    player("b", "Chromium", "New track"),
                ];
                players[0].playing = false;
                let mut nodes = vec![node(1, "chrome", "Old track")];
                assert_eq!(matched_ids(&players, &nodes), [None, None]);
                let mut fresh = node(2, "chrome", "New track");
                fresh.ready = false;
                nodes.push(fresh);
                assert_eq!(matched_ids(&players, &nodes), [None, None]);
                nodes[1].ready = true;
                assert_eq!(matched_ids(&players, &nodes), [Some(1), Some(2)]);
            }

            #[test]
            fn volume_deduplicates_service_wrappers_but_not_distinct_nodes() {
                let players = [player("a", "Spotify", "Track")];
                let stream = node(1, "Spotify", "Track");
                assert_eq!(matched_ids(&players, &[stream.clone(), stream]), [Some(1)]);
                let stream = node(0, "Spotify", "Track");
                assert_eq!(matched_ids(&players, &[stream.clone(), stream]), [None]);
                assert_eq!(
                    matched_ids(
                        &players,
                        &[node(1, "Spotify", "Track"), node(2, "Spotify", "Track")]
                    ),
                    [None]
                );
            }

            #[test]
            fn volume_browser_requires_exactly_one_detailed_match_when_shared() {
                let players = [player("a", "Chromium", "A"), player("b", "Chromium", "B")];
                let nodes = [
                    node(1, "Chromium", "A"),
                    node(2, "Chromium", "A"),
                    node(3, "Chromium", "B"),
                ];
                let matches = volume_matches(&players, &nodes);
                assert_eq!(matches[0].reason, BROWSER_SHARED);
                assert_eq!(matches[1].node.unwrap().id, 3);
            }

            #[test]
            fn browser_allowlist_checks_media_paths_and_most_specific_host() {
                for url in [
                    "https://youtu.be/id",
                    "https://www.youtube.com/watch?v=id",
                    "https://youtube.com/shorts/id",
                    "https://youtube.com/live/id",
                    "https://youtube.com/embed/id",
                    "https://youtube.com/playlist?list=id",
                    "https://youtube.com/v/id",
                    "https://youtube.com/e/id",
                    "https://music.youtube.com/watch?v=id",
                    "https://music.youtube.com/playlist?list=id",
                    "https://www.bilibili.com/video/BV1",
                    "https://bilibili.com/bangumi/play/id",
                    "https://bilibili.com/cheese/id",
                    "https://bilibili.com/medialist/id",
                    "https://bilibili.com/list/id",
                    "https://bilibili.com/festival/id",
                    "https://bilibili.com/audio/id",
                    "https://b23.tv/id",
                    "https://live.bilibili.com/123",
                    "https://open.spotify.com/track/id",
                    "https://spotify.com/album/id",
                    "https://spotify.com/playlist/id",
                    "https://spotify.com/artist/id",
                    "https://spotify.com/show/id",
                    "https://spotify.com/episode/id",
                    "https://spotify.com/collection/tracks",
                    "https://open.spotify.com/intl-zh/track/id",
                    "https://open.spotify.com/search/far%20east%20movement%20get%20up/tracks",
                    "https://open.spotify.com/intl-zh/search/far%20east%20movement%20get%20up/tracks",
                    "https://open.spotify.com/intl-en/search/far%20east%20movement%20get%20up/tracks?type=tracks#results",
                    "https://spotify.com/search/query",
                    "HTTPS://WWW.YOUTUBE.COM/WATCH?v=id",
                    "http://youtube.com:8080/watch?v=id",
                    "https://youtube.com:000443/watch?v=id",
                    "https://-x.youtube.com/watch?v=id",
                ] {
                    assert!(browser_media_allowed(url), "should allow {url}");
                }
                for url in [
                    "",
                    "https://youtube.com",
                    "https://youtube.com/?v=id",
                    "https://youtu.be/",
                    "https://youtube.com/feed/subscriptions",
                    "https://youtube.com/watchlater",
                    "https://music.youtube.com/shorts/id",
                    "https://music.youtube.com/",
                    "https://bilibili.com/",
                    "https://live.bilibili.com/",
                    "https://b23.tv/",
                    "https://spotify.com/intl-zh/",
                    "https://open.spotify.com",
                    "https://open.spotify.com/",
                    "https://open.spotify.com/intl-en/",
                    "https://open.spotify.com/home",
                    "https://open.spotify.com/?next=/search/query",
                    "https://open.spotify.com/#search/query",
                    "https://open.spotify.com/searching/query",
                    "https://github.com/search/query",
                    "https://github.com/watch/id",
                    "https://docs.example.com/video/id",
                    "file:///watch/id",
                    "ftp://youtube.com/watch",
                    "https://youtube.com/#watch/id",
                    "https://youtube.com/?next=/watch/id",
                ] {
                    assert!(!browser_media_allowed(url), "should reject {url}");
                }
            }

            #[test]
            fn browser_allowlist_and_display_names_respect_host_boundaries() {
                for url in [
                    "https://notyoutube.com/watch",
                    "https://youtube.com.evil.test/watch",
                    "https://evil.test/youtube.com/watch",
                    "https://youtube.com@evil.test/watch",
                    "https://evil.test@youtube.com/watch",
                    "https://youtube.com\\@evil.test/watch",
                    "https://youtube.com%2eevil.test/watch",
                    "https://youtube.com:bad/watch",
                    "https://youtube.com:65536/watch",
                    "https://youtube.com./watch",
                    "https://youtube..com/watch",
                    "https:// youtube.com/watch",
                    "https://youtube.com/watch\n",
                    "https://notspotify.com/search/query",
                    "https://open.spotify.com.evil.test/search/query",
                    "https://evil.test/open.spotify.com/search/query",
                    "https://open.spotify.com@evil.test/search/query",
                    "https://evil.test@open.spotify.com/search/query",
                    "https://open.spotify.com\\@evil.test/search/query",
                    "https://open.spotify.com%2eevil.test/search/query",
                    "https://open.spotify.com:bad/search/query",
                    "https://open.spotify.com:65536/search/query",
                    "https://open.spotify.com./search/query",
                    "https://open..spotify.com/search/query",
                    "https:// open.spotify.com/search/query",
                    "https://open.spotify.com/search/far east movement/tracks",
                    "https://open.spotify.com/search/query\n",
                    "file://open.spotify.com/search/query",
                    "ftp://open.spotify.com/search/query",
                ] {
                    assert!(!browser_media_allowed(url), "should reject {url:?}");
                    let mut p = player("a", "Chromium", "");
                    p.url = url.to_owned();
                    assert_eq!(display_name(&p), "Chromium");
                }
                for (url, name) in [
                    ("https://music.youtube.com/watch", "YouTube Music"),
                    ("https://www.youtube.com/watch", "YouTube"),
                    ("https://-x.youtube.com:000443/watch", "YouTube"),
                    ("https://youtu.be/id", "YouTube"),
                    ("https://live.bilibili.com/123", "Bilibili Live"),
                    ("https://www.bilibili.com/video/id", "Bilibili"),
                    ("https://b23.tv/id", "Bilibili"),
                    ("https://open.spotify.com/track/id", "Spotify"),
                ] {
                    let mut p = player("a", "Chromium", "");
                    p.url = url.to_owned();
                    assert_eq!(display_name(&p), name);
                }
            }

            #[test]
            fn display_names_fall_back_to_friendly_sources_after_browser_overrides() {
                for identity in ["", " \t ", "mprisence_web.youtube_music.pa808"] {
                    let mut p = player(
                        "org.mpris.MediaPlayer2.mprisence_web.youtube_music.pa808",
                        identity,
                        "Track",
                    );
                    assert_eq!(display_name(&p), "Youtube Music");
                    p.browser_hint = json!(true);
                    p.url = "https://music.youtube.com/watch?v=id".to_owned();
                    assert_eq!(display_name(&p), "YouTube Music");
                }
                let p = player("org.mpris.MediaPlayer2.elisa", "", "Track");
                let response = analyze(&request(vec![p], vec![]));
                assert_eq!(response.groups[0].display_name, "Elisa");
                assert_eq!(
                    display_name(&player("bus", " Custom Player ", "")),
                    " Custom Player "
                );
            }

            #[test]
            fn browser_hint_presence_filters_but_boolean_is_not_browser_identity() {
                for hint in [json!(true), json!(false), json!("Firefox"), json!("")] {
                    let mut p = player("a", "Bridge", "Track");
                    p.browser_hint = hint;
                    assert!(browser_player(&p));
                    assert!(analyze(&request(vec![p.clone()], vec![])).groups.is_empty());
                    p.url = "https://youtube.com/watch?v=id".to_owned();
                    assert_eq!(
                        analyze(&request(vec![p], vec![])).groups[0].display_name,
                        "YouTube"
                    );
                }
                let mut p = player("a", "Native", "Track");
                p.url = "https://youtube.com/watch?v=id".to_owned();
                assert!(!browser_player(&p));
                assert_eq!(display_name(&p), "Native");
            }

            #[test]
            fn group_response_omits_artist_and_artwork_without_changing_player_metadata() {
                let mut native = player("a", "Native", "Track");
                native.artist = "Artist".to_owned();
                native.art_url = "https://i.ytimg.com/vi/id/hq.jpg?format=avif".to_owned();
                let snapshot = request(vec![native], vec![]);
                let before = snapshot.clone();
                let response = serde_json::to_value(analyze(&snapshot)).unwrap();
                assert_eq!(
                    response["groups"],
                    json!([{
                        "title": "Track", "primary": "a", "players": ["a"],
                        "displayName": "Native", "streamId": null, "streamReason": NO_MATCH
                    }])
                );
                assert_eq!(snapshot, before);
                let input = serde_json::to_value(&snapshot).unwrap();
                assert_eq!(input["players"][0]["artist"], "Artist");
                assert_eq!(
                    input["players"][0]["artUrl"],
                    "https://i.ytimg.com/vi/id/hq.jpg?format=avif"
                );
            }

            #[test]
            fn grouping_filters_stopped_proxies_and_nonmedia_browsers() {
                let mut stopped = player("stopped", "Native", "Stopped");
                stopped.stopped = true;
                let mut tab = player("tab", "Chromium", "Tab");
                tab.url = "https://github.com/project".to_owned();
                let response = analyze(&request(
                    vec![
                        stopped,
                        player("org.mpris.MediaPlayer2.PLAYERCTLD", "Proxy", "Proxy"),
                        player(
                            "org.mpris.MediaPlayer2.kdeconnect.instance",
                            "Proxy",
                            "Proxy2",
                        ),
                        tab,
                        player("native", "Native", "Native"),
                    ],
                    vec![],
                ));
                assert_eq!(response.groups.len(), 1);
                assert_eq!(response.groups[0].players, ["native"]);
            }

            #[test]
            fn url_less_browser_uses_allowed_companion_without_losing_native_primary() {
                for (url, label) in [
                    ("https://open.spotify.com/album/id", "Spotify"),
                    ("https://music.youtube.com/watch?v=id", "YouTube Music"),
                    ("https://www.youtube.com/watch?v=id", "YouTube"),
                    ("https://youtu.be/id", "YouTube"),
                    ("https://www.bilibili.com/video/id", "Bilibili"),
                    ("https://live.bilibili.com/123", "Bilibili Live"),
                    ("https://b23.tv/id", "Bilibili"),
                ] {
                    for hint in [json!(true), json!(false), json!(""), json!("Arbitrary Browser")] {
                        for playing in [false, true] {
                            for reversed in [false, true] {
                                let mut players = browser_companions(url);
                                players[0].browser_hint = hint.clone();
                                for player in &mut players {
                                    player.playing = playing;
                                }
                                if reversed {
                                    players.reverse();
                                }
                                let members = players.each_ref().map(|p| p.bus.clone());
                                let [first, second] = players;
                                let mut snapshot = request(
                                    vec![first, player("other", "Other", "Other"), second],
                                    vec![],
                                );
                                for selected in ["native", "companion"] {
                                    snapshot.selected_player = selected.to_owned();
                                    let before = snapshot.clone();
                                    let response = analyze(&snapshot);
                                    assert!(response.error.is_none());
                                    assert_eq!(response.groups.len(), 2);
                                    assert_eq!(response.groups[0].players, members);
                                    assert_eq!(response.groups[0].primary, "native");
                                    assert_eq!(response.groups[0].display_name, label);
                                    assert_eq!(response.groups[1].players, ["other"]);
                                    assert_eq!(response.active_player, "native");
                                    assert_eq!(snapshot, before);
                                }
                            }
                        }
                    }
                }
            }

            #[test]
            fn url_less_browser_needs_direct_live_allowed_browser_corroboration() {
                let [native, companion] =
                    browser_companions("https://www.youtube.com/watch?v=id");
                assert!(analyze(&request(vec![native.clone()], vec![])).groups.is_empty());
                for case in ["stopped", "playerctld", "kdeconnect", "nonbrowser", "empty", "blocked"] {
                    let mut peer = companion.clone();
                    match case {
                        "stopped" => peer.stopped = true,
                        "playerctld" => peer.bus = "org.mpris.MediaPlayer2.PLAYERCTLD".to_owned(),
                        "kdeconnect" => peer.bus = "org.mpris.MediaPlayer2.kdeconnect.instance".to_owned(),
                        "nonbrowser" => peer.browser_hint = Value::Null,
                        "empty" => peer.url.clear(),
                        "blocked" => peer.url = "https://youtube.com/channel/id".to_owned(),
                        _ => unreachable!(),
                    }
                    for reversed in [false, true] {
                        let mut players = vec![native.clone(), peer.clone()];
                        if reversed {
                            players.reverse();
                        }
                        let response = analyze(&request(players, vec![]));
                        assert!(response.error.is_none());
                        assert!(
                            response.groups.iter().all(|group| !group.players.contains(&native.bus)),
                            "{case}"
                        );
                    }
                }
            }

            #[test]
            fn browser_corroboration_does_not_rescue_explicit_disallowed_urls() {
                for url in [
                    " ",
                    "https://example.test/watch",
                    "https://open.spotify.com/login",
                    "https://youtube.com/channel/id",
                    "https://youtube.com.evil.test/watch?v=id",
                ] {
                    let [mut native, companion] =
                        browser_companions("https://www.youtube.com/watch?v=id");
                    native.url = url.to_owned();
                    assert!(same_playback(&native, &companion));
                    for reversed in [false, true] {
                        let mut players = vec![native.clone(), companion.clone()];
                        if reversed {
                            players.reverse();
                        }
                        let response = analyze(&request(players, vec![]));
                        assert_eq!(response.groups.len(), 1);
                        assert_eq!(response.groups[0].players, ["companion"], "{url:?}");
                    }
                }
            }

            #[test]
            fn browser_corroboration_retains_metadata_and_known_position_checks() {
                for case in ["title", "metadata", "sparse", "position"] {
                    let [native, mut companion] =
                        browser_companions("https://www.youtube.com/watch?v=id");
                    match case {
                        "title" => companion.title = "Different Track".to_owned(),
                        "metadata" => {
                            companion.artist = "Different Artist".to_owned();
                            companion.album = "Different Album".to_owned();
                        }
                        "sparse" => {
                            companion.album.clear();
                            companion.length = None;
                        }
                        "position" => companion.position = native.position - 6.0,
                        _ => unreachable!(),
                    }
                    assert!(!same_playback(&native, &companion), "{case}");
                    for reversed in [false, true] {
                        let mut players = vec![native.clone(), companion.clone()];
                        if reversed {
                            players.reverse();
                        }
                        let response = analyze(&request(players, vec![]));
                        assert_eq!(response.groups.len(), 1);
                        assert_eq!(response.groups[0].players, ["companion"], "{case}");
                    }
                }
            }

            #[test]
            fn spotify_search_and_album_duplicates_keep_the_rich_primary_in_either_order() {
                for position in [0.0, 63.25] {
                    for reversed in [false, true] {
                        let mut players = spotify_players();
                        players[0].position = position;
                        if reversed {
                            players.reverse();
                        }
                        let members = players.each_ref().map(|player| player.bus.clone());
                        let [first, second] = players;
                        let mut snapshot = request(
                            vec![first, player("other", "Native", "Other"), second],
                            vec![node(11, "Chromium", "Get Up")],
                        );
                        let groups = analyze(&snapshot).groups;
                        for selected in ["real", "duplicate"] {
                            snapshot.selected_player = selected.to_owned();
                            let response = analyze(&snapshot);
                            assert!(response.error.is_none());
                            assert_eq!(response.groups, groups);
                            assert_eq!(response.groups.len(), 2);
                            assert_eq!(response.groups[0].players, members);
                            assert_eq!(response.groups[0].primary, "real");
                            assert_eq!(response.groups[0].title, "Get Up");
                            assert_eq!(response.groups[0].display_name, "Spotify");
                            assert_eq!(response.groups[0].stream_id, Some(11));
                            assert_eq!(response.groups[1].players, ["other"]);
                            assert_eq!(response.active_player, "real");
                        }
                    }
                }
            }

            #[test]
            fn representative_quality_is_playing_then_transport_then_length_then_seek() {
                let mut paused = player("paused", "Native", "Track");
                paused.artist = "Artist".to_owned();
                paused.playing = false;
                paused.can_play = true;
                paused.can_pause = true;
                let mut bare = paused.clone();
                bare.bus = "bare".to_owned();
                bare.playing = true;
                bare.can_play = false;
                bare.can_pause = false;
                bare.length = None;
                bare.can_seek = false;
                let mut seeker = bare.clone();
                seeker.bus = "seeker".to_owned();
                seeker.can_seek = true;
                let mut length = bare.clone();
                length.bus = "length".to_owned();
                length.length = Some(203.0);
                let mut transport = bare.clone();
                transport.bus = "transport".to_owned();
                transport.title = " TRACK ".to_owned();
                transport.artist = " ARTIST ".to_owned();
                let mut players = vec![paused, bare, seeker, length, transport];
                for (can_play, can_pause) in [(true, false), (false, true), (true, true)] {
                    players[4].can_play = can_play;
                    players[4].can_pause = can_pause;
                    for count in 2..=players.len() {
                        for reversed in [false, true] {
                            let mut members = players[..count].to_vec();
                            if reversed {
                                members.reverse();
                            }
                            let snapshot = request(members, vec![]);
                            let response = analyze(&snapshot);
                            let expected = &players[count - 1];
                            assert_eq!(response.groups.len(), 1);
                            assert_eq!(response.groups[0].primary, expected.bus);
                            assert_eq!(response.groups[0].title, expected.title);
                            assert_eq!(
                                response.groups[0].players,
                                snapshot.players.iter().map(|p| p.bus.clone()).collect::<Vec<_>>()
                            );
                        }
                    }
                }
            }

            #[test]
            fn representative_prefers_usable_length_with_absent_or_false_transport_caps() {
                for absent_caps in [false, true] {
                    for length in [
                        None,
                        Some(0.0),
                        Some(-1.0),
                        Some(f64::NAN),
                        Some(f64::INFINITY),
                        Some(f64::NEG_INFINITY),
                    ] {
                        for reversed in [false, true] {
                            let [mut real, mut duplicate] = spotify_players();
                            real.can_play = false;
                            real.can_pause = false;
                            real.can_seek = false;
                            duplicate.can_seek = true;
                            duplicate.length = length;
                            let mut players = vec![real, duplicate];
                            if reversed {
                                players.reverse();
                            }
                            if absent_caps {
                                let mut wire = serde_json::to_value(&players).unwrap();
                                for player in wire.as_array_mut().unwrap() {
                                    let player = player.as_object_mut().unwrap();
                                    player.remove("canPlay");
                                    player.remove("canPause");
                                }
                                players = serde_json::from_value(wire).unwrap();
                            }
                            assert!(players.iter().all(|p| !p.can_play && !p.can_pause));
                            let response = analyze(&request(players, vec![]));
                            assert!(response.error.is_none());
                            assert_eq!(response.groups.len(), 1);
                            assert_eq!(response.groups[0].primary, "real");
                        }
                    }
                }
            }

            #[test]
            fn representative_quality_ties_keep_original_order_and_matching_metadata() {
                let mut first = player("first", "Native", "Track");
                first.artist = "Artist".to_owned();
                first.can_play = true;
                let mut second = first.clone();
                second.bus = "second".to_owned();
                second.title = " TRACK ".to_owned();
                second.artist = " ARTIST ".to_owned();
                second.length = Some(204.0);
                second.can_play = false;
                second.can_pause = true;
                second.art_url = "https://example.com/cover.jpg".to_owned();
                for playing in [false, true] {
                    for reversed in [false, true] {
                        let mut players = vec![first.clone(), second.clone()];
                        for player in &mut players {
                            player.playing = playing;
                        }
                        if reversed {
                            players.reverse();
                        }
                        let snapshot = request(players, vec![]);
                        let response = analyze(&snapshot);
                        let expected = &snapshot.players[0];
                        assert_eq!(response.groups.len(), 1);
                        assert_eq!(response.groups[0].primary, expected.bus);
                        assert_eq!(response.groups[0].title, expected.title);
                    }
                }
            }

            #[test]
            fn spotify_representative_does_not_reorder_or_gate_native_action_fanout() {
                for playing in [false, true] {
                    for reversed in [false, true] {
                        let mut players = spotify_players();
                        for player in &mut players {
                            player.playing = playing;
                        }
                        let real = players[0].clone();
                        if reversed {
                            players.reverse();
                        }
                        let mut snapshot = request(players.to_vec(), vec![]);
                        for target in ["real", "duplicate"] {
                            snapshot.selected_player = target.to_owned();
                            for kind in ["toggle", "pauseAll", "togglePauseAll", "seek", "next", "previous"] {
                                snapshot.action = Some(Action {
                                    position: 12.5,
                                    ..action(kind, target)
                                });
                                let response = analyze(&snapshot);
                                assert!(response.error.is_none());
                                assert_eq!(response.groups[0].primary, "real");
                                let expected = match kind {
                                    "pauseAll" if !playing => vec![],
                                    "toggle" | "pauseAll" | "togglePauseAll" => snapshot
                                        .players
                                        .iter()
                                        .map(|p| native_action(p, if playing { "pause" } else { "play" }, None))
                                        .collect(),
                                    "seek" => vec![native_action(&real, kind, Some(12.5))],
                                    _ => vec![native_action(&real, kind, None)],
                                };
                                assert_eq!(response.actions, expected);
                            }
                        }
                    }
                }
            }

            #[test]
            fn grouping_preserves_keeper_metadata_and_five_second_window() {
                let mut a = player("a", "Native", "Track");
                a.artist = "Artist".to_owned();
                a.length = None;
                a.playing = false;
                a.position = 10.0;
                let mut b = a.clone();
                b.bus = "b".to_owned();
                b.playing = true;
                b.position = 15.0;
                b.title = " TRACK ".to_owned();
                b.artist = " ARTIST ".to_owned();
                b.album = "A different album does not veto artist agreement".to_owned();
                b.art_url = "https://i.ytimg.com/vi/id/hq.jpg?format=avif".to_owned();
                let mut c = a.clone();
                c.bus = "c".to_owned();
                c.position = 20.0;
                let snapshot = request(vec![a, b, c], vec![]);
                let before = snapshot.clone();
                let response = analyze(&snapshot);
                assert_eq!(snapshot, before);
                assert_eq!(response.groups.len(), 1);
                assert_eq!(response.groups[0].players, ["a", "b", "c"]);
                assert_eq!(response.groups[0].primary, "b");
                assert_eq!(response.groups[0].title, " TRACK ");
                let mut snapshot = snapshot;
                snapshot.players[1].position = 15.001;
                snapshot.players[2].title = "Other".to_owned();
                assert_eq!(analyze(&snapshot).groups.len(), 3);
                snapshot.players[1].position = 10.0;
                snapshot.players[1].artist = "Different".to_owned();
                assert_eq!(analyze(&snapshot).groups.len(), 3);
                snapshot.players[1].artist = "Artist".to_owned();
                assert_eq!(analyze(&snapshot).groups.len(), 2);
            }

            #[test]
            fn grouping_compares_only_the_keeper_not_any_matching_member() {
                let mut players = vec![
                    player("a", "Native", "Track"),
                    player("b", "Native", "Track"),
                    player("c", "Native", "Track"),
                ];
                players[0].position = 10.0;
                players[1].position = 15.0;
                players[1].can_play = true;
                players[1].can_pause = true;
                players[2].position = 20.0;
                let response = analyze(&request(players, vec![]));
                assert_eq!(response.groups.len(), 2);
                assert_eq!(response.groups[0].players, ["a", "b"]);
                assert_eq!(response.groups[0].primary, "b");
                assert_eq!(response.groups[1].players, ["c"]);
            }

            #[test]
            fn grouping_keeps_snapshot_card_and_member_order_with_playing_first_keepers() {
                let mut a = player("a", "Native", "A");
                a.playing = false;
                let mut last = player("last", "Native", "Last");
                last.playing = false;
                let snapshot = request(
                    vec![
                        a,
                        player("b", "Native", "B"),
                        player("alias", "Native", "A"),
                        last,
                    ],
                    vec![],
                );
                let response = analyze(&snapshot);
                assert_eq!(response.groups.len(), 3);
                assert_eq!(response.groups[0].players, ["a", "alias"]);
                assert_eq!(response.groups[0].primary, "alias");
                assert_eq!(response.groups[1].players, ["b"]);
                assert_eq!(response.groups[2].players, ["last"]);
                assert_eq!(response.active_player, "alias");
            }

            #[test]
            fn grouping_requires_support_beyond_title_and_more_for_cross_source_mirrors() {
                let mut first = player("a", "Native", "Track");
                first.length = None;
                let mut arriving = first.clone();
                arriving.bus = "b".to_owned();
                let mut snapshot = request(vec![first, arriving], vec![]);
                assert_eq!(analyze(&snapshot).groups.len(), 2);

                for player in &mut snapshot.players {
                    player.artist = "Artist".to_owned();
                }
                assert_eq!(analyze(&snapshot).groups.len(), 1);
                snapshot.players[1].identity = "Different source".to_owned();
                assert_eq!(analyze(&snapshot).groups.len(), 2);
                for player in &mut snapshot.players {
                    player.album = "Album".to_owned();
                }
                assert_eq!(analyze(&snapshot).groups.len(), 1);
            }

            #[test]
            fn grouping_uses_only_known_lengths_with_a_two_second_tolerance() {
                let first = player("a", "Native", "Track");
                let mut second = first.clone();
                second.bus = "b".to_owned();
                second.length = Some(205.0);
                let mut snapshot = request(vec![first, second], vec![]);
                assert_eq!(analyze(&snapshot).groups.len(), 1);
                for length in [
                    None,
                    Some(0.0),
                    Some(-1.0),
                    Some(205.001),
                    Some(f64::NAN),
                    Some(f64::INFINITY),
                ] {
                    snapshot.players[1].length = length;
                    let response = analyze(&snapshot);
                    assert!(response.error.is_none());
                    assert_eq!(response.groups.len(), 2);
                }
            }

            #[test]
            fn grouping_treats_zero_positions_as_unknown() {
                let mut watched = player("watched", "Native", "Track");
                watched.position = 92.0;
                let unread = player("unread", "Native", "Track");
                let response = analyze(&request(vec![watched, unread], vec![]));
                assert_eq!(response.groups.len(), 1);
                assert_eq!(response.groups[0].players, ["watched", "unread"]);
            }

            #[test]
            fn grouping_does_not_merge_empty_tracks_or_duplicate_bus_names() {
                for title in ["", "  "] {
                    let response = analyze(&request(
                        vec![player("a", "Native", title), player("b", "Native", title)],
                        vec![],
                    ));
                    assert_eq!(response.groups.len(), 2);
                }
                let p = player("a", "Native", "Track");
                let mut snapshot = request(vec![p.clone(), p], vec![]);
                snapshot.action = Some(action("toggle", "a"));
                let response = analyze(&snapshot);
                assert_eq!(response.groups[0].players, ["a"]);
                assert_eq!(response.actions.len(), 1);
            }

            #[test]
            fn invalid_positions_are_rejected_not_merged_as_unknown() {
                for position in [-1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
                    let mut snapshot = request(
                        vec![
                            player("a", "Native", "Track"),
                            player("b", "Native", "Track"),
                        ],
                        vec![],
                    );
                    snapshot.players[1].position = position;
                    snapshot.action = Some(action("toggle", "a"));
                    let response = analyze(&snapshot);
                    assert!(response.error.is_some());
                    assert!(response.groups.is_empty());
                    assert!(response.actions.is_empty());
                    assert_eq!(response.request_id, 7);
                }
            }

            #[test]
            fn selection_retains_any_member_else_playing_else_first() {
                let mut first = player("a", "Native", "A");
                first.playing = false;
                let mut alias = first.clone();
                alias.bus = "alias".to_owned();
                let second = player("b", "Native", "B");
                let mut snapshot = request(vec![first, alias, second], vec![]);
                snapshot.selected_player = "alias".to_owned();
                assert_eq!(analyze(&snapshot).active_player, "a");
                snapshot.selected_player = "gone".to_owned();
                assert_eq!(analyze(&snapshot).active_player, "b");
                snapshot.players[2].playing = false;
                assert_eq!(analyze(&snapshot).active_player, "a");
                snapshot.players.clear();
                assert_eq!(analyze(&snapshot).active_player, "");
            }

            #[test]
            fn toggle_fans_out_absolute_operations_not_double_toggles() {
                let mut snapshot = request(
                    vec![
                        player("a", "Native", "Track"),
                        player("b", "Native", "Track"),
                    ],
                    vec![],
                );
                snapshot.players[0].playing = false;
                snapshot.action = Some(action("toggle", "a"));
                let response = analyze(&snapshot);
                assert_eq!(response.actions.len(), 2);
                assert!(
                    response
                        .actions
                        .iter()
                        .all(|a| a.operation == "pause" && a.value.is_none())
                );
                assert_eq!(
                    response
                        .actions
                        .iter()
                        .map(|a| a.player.as_str())
                        .collect::<Vec<_>>(),
                    ["a", "b"]
                );
                snapshot.players[1].playing = false;
                let response = analyze(&snapshot);
                assert!(response.actions.iter().all(|a| a.operation == "play"));
            }

            #[test]
            fn next_and_previous_only_address_capable_primary() {
                let mut snapshot = request(
                    vec![
                        player("a", "Native", "Track"),
                        player("b", "Native", "Track"),
                    ],
                    vec![],
                );
                snapshot.players[0].playing = false;
                for kind in ["next", "previous"] {
                    snapshot.action = Some(action(kind, "a"));
                    assert_eq!(
                        analyze(&snapshot).actions,
                        [native_action(&snapshot.players[1], kind, None)]
                    );
                }
                snapshot.players[1].can_next = false;
                snapshot.players[1].can_previous = false;
                for kind in ["next", "previous"] {
                    snapshot.action = Some(action(kind, "a"));
                    assert!(analyze(&snapshot).actions.is_empty());
                }
            }

            #[test]
            fn fractional_seek_json_roundtrips_without_changing_the_requested_value() {
                let action: Action = serde_json::from_str(
                    r#"{"kind":"seek","player":"a","position":237.60851756361546}"#,
                )
                .unwrap();
                let requested = 237.60851756361546_f64;
                assert_eq!(action.position.to_bits(), requested.to_bits());

                let native = native_action(&player("a", "Native", "Track"), "seek", Some(action.position));
                let encoded = serde_json::to_string(&native).unwrap();
                let decoded: NativeAction = serde_json::from_str(&encoded).unwrap();
                assert_eq!(decoded.value.unwrap().to_bits(), requested.to_bits());
            }

            #[test]
            fn seek_only_addresses_capable_members_and_valid_positions() {
                let mut snapshot = request(
                    vec![
                        player("a", "Native", "Track"),
                        player("b", "Native", "Track"),
                    ],
                    vec![],
                );
                snapshot.players[0].can_seek = false;
                snapshot.action = Some(Action {
                    position: 12.5,
                    ..action("seek", "a")
                });
                assert_eq!(
                    analyze(&snapshot).actions,
                    [native_action(&snapshot.players[1], "seek", Some(12.5))]
                );
                snapshot.action.as_mut().unwrap().position = 0.0;
                assert_eq!(analyze(&snapshot).actions[0].value, Some(0.0));
                for position in [-1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
                    snapshot.action.as_mut().unwrap().position = position;
                    let response = analyze(&snapshot);
                    assert!(response.actions.is_empty());
                    assert!(response.error.is_some());
                }
            }

            #[test]
            fn pause_all_includes_stopped_and_browser_filtered_players_not_proxies() {
                let mut stopped = player("stopped", "Native", "A");
                stopped.stopped = true;
                let mut paused = player("paused", "Native", "B");
                paused.playing = false;
                let tab = player("tab", "Chromium", "Docs");
                let mut snapshot = request(
                    vec![
                        stopped,
                        tab,
                        paused,
                        player("playerctld", "Proxy", "X"),
                        player("kdeconnect", "Proxy", "Y"),
                    ],
                    vec![],
                );
                snapshot.players.push(snapshot.players[1].clone());
                for kind in ["pauseAll", "togglePauseAll"] {
                    snapshot.action = Some(action(kind, "irrelevant"));
                    let response = analyze(&snapshot);
                    assert_eq!(
                        response
                            .actions
                            .iter()
                            .map(|a| a.player.as_str())
                            .collect::<Vec<_>>(),
                        ["stopped", "tab"]
                    );
                    assert!(response.actions.iter().all(|a| a.operation == "pause"));
                }
            }

            #[test]
            fn toggle_pause_all_when_idle_resumes_selected_group_only() {
                let mut players = vec![
                    player("a", "Native", "A"),
                    player("alias", "Native", "A"),
                    player("b", "Native", "B"),
                    player("playerctld", "Proxy", "Proxy"),
                ];
                for player in &mut players[..3] {
                    player.playing = false;
                }
                let mut snapshot = request(players, vec![]);
                snapshot.selected_player = "alias".to_owned();
                snapshot.action = Some(action("togglePauseAll", "b"));
                let response = analyze(&snapshot);
                assert_eq!(
                    response
                        .actions
                        .iter()
                        .map(|a| a.player.as_str())
                        .collect::<Vec<_>>(),
                    ["a", "alias"]
                );
                assert!(response.actions.iter().all(|a| a.operation == "play"));
                snapshot.players.clear();
                assert!(analyze(&snapshot).actions.is_empty());
            }

            #[test]
            fn actions_default_to_selection_but_explicit_missing_player_never_falls_back() {
                let mut snapshot = request(
                    vec![player("a", "Native", "A"), player("b", "Native", "B")],
                    vec![],
                );
                snapshot.selected_player = "b".to_owned();
                snapshot.action = Some(action("toggle", ""));
                assert_eq!(analyze(&snapshot).actions[0].player, "b");
                for kind in ["toggle", "next", "previous", "seek"] {
                    snapshot.action = Some(action(kind, "gone"));
                    let response = analyze(&snapshot);
                    assert!(response.actions.is_empty());
                    assert_eq!(
                        response.error.as_deref(),
                        Some("The requested media source is no longer available.")
                    );
                }
                snapshot.action = Some(action("unknown", ""));
                assert_eq!(
                    analyze(&snapshot).error.as_deref(),
                    Some("Unknown media action.")
                );
                for (kind, position, expected) in [
                    ("seek", -1.0, "Seek position must be finite and nonnegative."),
                    ("unknown", -1.0, "Unknown media action."),
                    ("unknown", f64::NAN, "Action position must be finite."),
                ] {
                    snapshot.action = Some(Action { position, ..action(kind, "gone") });
                    let response = analyze(&snapshot);
                    assert!(response.actions.is_empty());
                    assert_eq!(response.error.as_deref(), Some(expected));
                }
            }

            #[test]
            fn protocol_camel_case_defaults_and_optional_output_fields() {
                let wire = json!({
                    "requestId": 123,
                    "players": [{"bus":"a", "identity":"Native", "desktopEntry":"native",
                        "title":"Track", "artist":"Artist", "album":"Album", "url":"", "artUrl":"",
                        "playing":true, "stopped":false, "position":0.0,
                        "canSeek":true, "canNext":true, "canPrevious":true}],
                    "streams": [], "action": {"kind":"toggle"}
                });
                let snapshot: Request = serde_json::from_value(wire.clone()).unwrap();
                assert_eq!(snapshot.selected_player, "");
                assert!(snapshot.players[0].browser_hint.is_null());
                assert_eq!(snapshot.players[0].length, None);
                assert!(!snapshot.players[0].can_play);
                assert!(!snapshot.players[0].can_pause);
                assert_eq!(snapshot.action.as_ref().unwrap(), &action("toggle", ""));
                let value = serde_json::to_value(analyze(&snapshot)).unwrap();
                assert_eq!(value["requestId"], 123);
                assert_eq!(value["activePlayer"], "a");
                assert_eq!(value["groups"][0]["displayName"], "Native");
                assert_eq!(value["groups"][0]["streamId"], Value::Null);
                assert_eq!(value["groups"][0]["streamReason"], NO_MATCH);
                assert!(value["groups"][0].get("artUrl").is_none());
                assert!(value["groups"][0].get("artist").is_none());
                assert!(value["groups"][0].get("selector").is_none());
                assert!(value["groups"][0].get("routingReason").is_none());
                assert!(value.get("request_id").is_none());
                assert!(value.get("route").is_none());
                assert!(value.get("error").is_none());
                assert!(value["actions"][0].get("value").is_none());
                assert!(
                    serde_json::to_value(snapshot.action.as_ref().unwrap())
                        .unwrap()
                        .get("destination")
                        .is_none()
                );
                for (can_play, can_pause) in [(true, false), (false, true), (true, true)] {
                    let mut with_caps = wire.clone();
                    with_caps["players"][0]["canPlay"] = json!(can_play);
                    with_caps["players"][0]["canPause"] = json!(can_pause);
                    let snapshot: Request = serde_json::from_value(with_caps).unwrap();
                    assert_eq!(snapshot.players[0].can_play, can_play);
                    assert_eq!(snapshot.players[0].can_pause, can_pause);
                    let serialized = serde_json::to_value(&snapshot).unwrap();
                    assert_eq!(serialized["players"][0]["canPlay"], can_play);
                    assert_eq!(serialized["players"][0]["canPause"], can_pause);
                    assert!(serialized["players"][0].get("can_play").is_none());
                    assert!(serialized["players"][0].get("can_pause").is_none());
                }
                let mut with_length = wire.clone();
                with_length["players"][0]["length"] = json!(203.5);
                let snapshot: Request = serde_json::from_value(with_length.clone()).unwrap();
                assert_eq!(snapshot.players[0].length, Some(203.5));
                assert_eq!(
                    serde_json::to_value(&snapshot).unwrap()["players"][0]["length"],
                    203.5
                );
                with_length["players"][0]["length"] = Value::Null;
                assert_eq!(
                    serde_json::from_value::<Request>(with_length)
                        .unwrap()
                        .players[0]
                        .length,
                    None
                );
                let mut missing_position = wire;
                missing_position["players"][0]
                    .as_object_mut()
                    .unwrap()
                    .remove("position");
                assert!(serde_json::from_value::<Request>(missing_position).is_err());
                assert!(serde_json::from_slice::<Request>(b"{\"requestId\":1,\"players\":[],\"streams\":[],\"action\":{\"kind\":\"seek\",\"position\":1e999}}").is_err());
            }

            #[derive(Default)]
            struct RecordingWriter {
                bytes: Vec<u8>,
                flushes: usize,
            }

            impl Write for RecordingWriter {
                fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                    self.bytes.extend_from_slice(bytes);
                    Ok(bytes.len())
                }

                fn flush(&mut self) -> io::Result<()> {
                    self.flushes += 1;
                    Ok(())
                }
            }

            fn responses(writer: &RecordingWriter) -> Vec<Response> {
                writer
                    .bytes
                    .split(|byte| *byte == b'\n')
                    .filter(|line| !line.is_empty())
                    .map(|line| serde_json::from_slice(line).unwrap())
                    .collect()
            }

            #[test]
            fn stdio_keeps_serving_invalid_lines_with_safe_diagnostics_and_flushes() {
                let valid = serde_json::to_vec(&request(vec![], vec![])).unwrap();
                let mut input = b"{\"requestId\":\"private-token-\\u001b\"}\n\xff\n\n".to_vec();
                input.extend_from_slice(&valid);
                input.extend_from_slice(b"\r\n{\n");
                input.extend_from_slice(&valid); // A final request need not end in LF.
                let mut writer = RecordingWriter::default();
                serve_io(
                    BufReader::with_capacity(13, Cursor::new(input)),
                    &mut writer,
                )
                .unwrap();
                let output = responses(&writer);
                assert_eq!(output.len(), 6);
                assert_eq!(writer.flushes, 6);
                for index in [0, 1, 2, 4] {
                    assert_eq!(output[index].request_id, 0);
                    assert!(
                        output[index]
                            .error
                            .as_ref()
                            .unwrap()
                            .contains("Invalid JSON request")
                    );
                    assert!(
                        !output[index]
                            .error
                            .as_ref()
                            .unwrap()
                            .contains("private-token")
                    );
                    assert!(output[index].actions.is_empty());
                }
                for index in [3, 5] {
                    assert_eq!(output[index].request_id, 7);
                    assert!(output[index].error.is_none());
                }
                assert!(!writer.bytes.contains(&0x1b));
            }

            #[test]
            fn stdio_bounds_lines_drains_oversized_payload_and_resumes() {
                let valid = serde_json::to_vec(&request(vec![], vec![])).unwrap();
                let mut input = valid.clone();
                input.resize(MAX_REQUEST_BYTES, b' ');
                input.push(b'\n');
                input.extend(std::iter::repeat_n(b'x', MAX_REQUEST_BYTES + 1));
                input.push(b'\n');
                input.extend_from_slice(&valid);
                input.push(b'\n');
                let mut writer = RecordingWriter::default();
                serve_io(
                    BufReader::with_capacity(4096, Cursor::new(input)),
                    &mut writer,
                )
                .unwrap();
                let output = responses(&writer);
                assert_eq!(writer.flushes, 3);
                assert!(output[0].error.is_none());
                assert_eq!(output[1].request_id, 0);
                assert!(output[1].error.as_ref().unwrap().contains("1 MiB"));
                assert_eq!(output[2].request_id, 7);
                assert!(output[2].error.is_none());
            }

            #[test]
            fn stdio_handles_empty_input_and_oversized_final_line() {
                let mut writer = RecordingWriter::default();
                serve_io(Cursor::new(b""), &mut writer).unwrap();
                assert!(writer.bytes.is_empty());
                serve_io(Cursor::new(vec![b'x'; MAX_REQUEST_BYTES + 1]), &mut writer).unwrap();
                assert_eq!(responses(&writer).len(), 1);
                assert_eq!(writer.flushes, 1);
            }
        }
    };
    (resident) => {
        mod resident_tests {
            use super::*;
            use serde_json::json;

            fn native(bus: &str) -> Value {
                json!({
                    "bus": bus, "identity": format!("App {bus}"), "desktopEntry": format!("app-{bus}"),
                    "title": format!("Track {bus}"), "artist": "Artist 雪", "album": "Album",
                    "url": "file:///music/track.flac", "artUrl": "", "browserHint": null,
                    "playing": true, "stopped": false, "position": 10.0, "length": 200.0,
                    "lengthMicros": null, "rate": 1.0, "canPlay": true, "canPause": true,
                    "canSeek": true, "canNext": true, "canPrevious": true,
                    "instance": 1, "trackId": format!("/track/{bus}"), "seekRevision": 0,
                    "positionRevision": 0, "positionSupported": true,
                    "cover": "", "available": true, "volume": 0.5
                })
            }

            fn audio(id: u32) -> Value {
                json!({"id": id, "instance": id, "name": "App a", "nickname": "",
                    "ready": true, "volume": 0.5, "muted": false,
                    "properties": {"application.name": "App a", "media.name": "Track a"}})
            }

            struct Harness {
                runtime: Runtime,
                raw: Value,
                now: f64,
            }

            impl Harness {
                fn new(players: Vec<Value>, streams: Vec<Value>) -> Self {
                    let raw = json!({"players": players, "streams": streams});
                    let mut runtime = Runtime::new(100.0);
                    assert!(runtime.snapshot(raw.clone(), 100.0));
                    Self { runtime, raw, now: 100.0 }
                }

                fn one() -> Self { Self::new(vec![native("a")], vec![]) }

                fn message(&mut self, value: Value) {
                    self.runtime.handle(serde_json::from_value(value).unwrap(), self.now);
                }

                fn capture(&mut self) {
                    assert!(self.runtime.snapshot(self.raw.clone(), self.now));
                }

                fn action(&mut self, kind: &str, bus: &str, position: f64) {
                    self.message(json!({"type": "action", "action": {
                        "kind": kind, "player": bus, "position": position
                    }, "createdAt": self.now, "snapshot": self.raw}));
                }

                fn volume(&mut self, bus: &str, value: f64) {
                    self.message(json!({"type": "volume", "player": bus, "value": value,
                        "createdAt": self.now, "snapshot": self.raw}));
                }

                fn mute(&mut self, bus: &str, muted: bool) {
                    self.message(json!({"type": "mute", "player": bus, "muted": muted,
                        "createdAt": self.now, "snapshot": self.raw}));
                }

                fn apply(&mut self) -> ApplyMessage {
                    self.runtime.advance(self.now).expect("one guarded dispatch")
                }

                fn ack(&mut self, apply: &ApplyMessage) {
                    if apply.operation.operation == "seek" {
                        let player = self.raw["players"].as_array_mut().unwrap().iter_mut()
                            .find(|p| p["bus"] == apply.operation.player).unwrap();
                        player["seekRevision"] = Value::from(player["seekRevision"].as_u64().unwrap() + 1);
                    }
                    if matches!(apply.operation.operation.as_str(), "volume" | "mute") {
                        let target = if let Some(id) = apply.operation.stream_id {
                            self.raw["streams"].as_array_mut().unwrap().iter_mut().find(|s| s["id"] == id).unwrap()
                        } else {
                            self.raw["players"].as_array_mut().unwrap().iter_mut().find(|p| p["bus"] == apply.operation.player).unwrap()
                        };
                        if let Some(value) = apply.operation.value { target["volume"] = json!(value); }
                        if let Some(muted) = apply.operation.muted { target["muted"] = json!(muted); }
                    }
                    self.result(apply, true, None);
                }

                fn result(&mut self, apply: &ApplyMessage, ok: bool, error: Option<&str>) {
                    self.message(json!({"type": "result", "id": apply.id,
                        "ok": ok, "error": error, "snapshot": self.raw}));
                }

                fn state(&self) -> Value {
                    serde_json::to_value(self.runtime.state(self.now)).unwrap()
                }

                fn progress(&self) -> Value { self.state()["progress"][0].clone() }

                fn model_position(&self) -> f64 {
                    self.runtime.request(Action::default(), self.now).players[0].position
                }
            }

            fn grouped() -> Harness {
                let a = native("a");
                let mut alias = a.clone();
                alias["bus"] = json!("alias");
                alias["trackId"] = json!("/track/alias");
                alias["instance"] = json!(2);
                alias["playing"] = json!(false);
                Harness::new(vec![a, alias], vec![])
            }

            #[test]
            fn state_is_complete_ready_and_guarded_with_native_metadata() {
                let mut h = Harness::new(vec![native("a")], vec![audio(42)]);
                let state = h.state();
                assert_eq!(state["type"], "state");
                assert_eq!(state["ready"], true);
                assert_eq!(state["activePlayer"], "a");
                assert_eq!(state["groups"][0]["primary"], "a");
                assert_eq!(state["groups"][0]["streamId"], 42);
                assert_eq!(state["players"][0]["artist"], "Artist 雪");
                assert_eq!(state["players"][0]["trackKey"], "/track/a");
                assert_eq!(state["progress"][0]["samplePosition"], 10.0);
                assert_eq!(state["transports"]["a"]["reason"], "Pause");
                for key in ["position", "positionRevision", "trackKey", "samplePosition", "cover", "volume"] {
                    assert!(state["guard"]["players"][0].get(key).is_none(), "{key}");
                }
                for key in ["volume", "muted"] {
                    assert!(state["guard"]["streams"][0].get(key).is_none());
                }
                h.raw["players"] = json!([]);
                h.capture();
                assert_eq!(h.state()["groups"], json!([]));
                assert_eq!(h.state()["ready"], true);
                assert_eq!(h.state()["guard"]["players"], json!([]));
            }

            #[test]
            fn native_normalization_preserves_raw_samples_and_numeric_length_fallback() {
                for position in [json!(-1), Value::Null, json!(0), json!(250.25)] {
                    let mut player = native("a");
                    player["position"] = position.clone();
                    player["rate"] = Value::Null;
                    player["length"] = json!(-1);
                    player["lengthMicros"] = json!(361929437.5);
                    let mut h = Harness::new(vec![player], vec![]);
                    let supported = position.as_f64().is_some_and(|p| p >= 0.0);
                    assert_eq!(h.state()["players"][0]["positionSupported"], supported);
                    assert_eq!(h.state()["players"][0]["position"], position.as_f64().filter(|p| *p >= 0.0).unwrap_or(0.0));
                    assert_eq!(h.state()["players"][0]["rate"], 1.0);
                    assert_eq!(h.state()["players"][0]["length"], 361.9294375);
                    assert_eq!(h.state()["players"][0]["canSeek"], true);
                    assert_eq!(h.raw["players"][0]["position"], position);
                    h.raw["players"][0]["length"] = json!(120.0);
                    h.capture();
                    assert_eq!(h.state()["players"][0]["length"], 120.0);
                }
                for metadata in [json!("200000000"), json!(true), json!([]), json!({}), json!(0), json!(-1)] {
                    let mut raw = native("a");
                    raw["length"] = Value::Null;
                    raw["lengthMicros"] = metadata.clone();
                    let h = Harness::new(vec![raw], vec![]);
                    assert_eq!(h.state()["players"][0]["length"], Value::Null);
                    assert_eq!(h.state()["guard"]["players"][0]["lengthMicros"], metadata);
                }
            }

            #[test]
            fn native_track_keys_use_dbus_path_syntax_and_metadata_only_as_freshness() {
                for id in ["", "not/a/path", "/invalid-dash", "/trailing/", NO_TRACK] {
                    let mut raw = native("a");
                    raw["trackId"] = json!(id);
                    let mut h = Harness::new(vec![raw], vec![]);
                    let before = h.state()["players"][0]["trackKey"].clone();
                    assert_ne!(before, id);
                    h.raw["players"][0]["title"] = json!("Different metadata");
                    h.capture();
                    assert_ne!(before, h.state()["players"][0]["trackKey"]);
                    h.action("seek", "a", 80.0);
                    assert!(h.runtime.advance(h.now).is_none());
                    assert!(h.runtime.failure.contains("unavailable"));
                    assert_eq!(h.raw["players"][0]["seekRevision"], 0);
                }
                for id in ["/", "/valid_123/path", "/track/a"] {
                    let mut raw = native("a");
                    raw["trackId"] = json!(id);
                    let h = Harness::new(vec![raw], vec![]);
                    assert_eq!(h.state()["players"][0]["trackKey"], id);
                }
            }

            #[test]
            fn display_estimates_never_overwrite_confirmed_or_raw_model_samples() {
                let mut h = Harness::one();
                h.now += 1.5;
                assert_eq!(h.progress()["position"], 11.5);
                assert_eq!(h.progress()["samplePosition"], 10.0);
                assert_eq!(h.state()["players"][0]["position"], 10.0);
                assert_eq!(h.model_position(), 10.0);
                let guard = h.runtime.guard();
                h.raw["players"][0]["position"] = json!(20);
                h.capture();
                h.now += 0.5;
                assert_eq!(h.progress()["position"], 20.5);
                h.raw["players"][0]["position"] = json!(5);
                h.capture();
                h.now += 1.0;
                assert_eq!(h.progress()["position"], 6.0);
                h.raw["players"][0]["length"] = json!(5.5);
                h.capture();
                assert_eq!(h.progress()["position"], 5.5);
                assert_eq!(h.state()["players"][0]["position"], 5.0);
                assert_eq!(guard.native.players[0]["length"], 200.0);
                assert_eq!(h.raw["players"][0]["position"], 5);
            }

            #[test]
            fn pause_resume_and_rate_reanchor_without_counting_paused_time() {
                let mut h = Harness::one();
                h.now += 1.0;
                h.raw["players"][0]["rate"] = json!(2);
                h.capture();
                h.now += 1.0;
                assert_eq!(h.progress()["position"], 13.0);
                h.raw["players"][0]["playing"] = json!(false);
                h.capture();
                h.now += 38.0;
                assert_eq!(h.progress()["position"], 13.0);
                assert_eq!(h.progress()["samplePosition"], 10.0);
                h.raw["players"][0]["playing"] = json!(true);
                h.capture();
                h.now += 1.0;
                assert_eq!(h.progress()["position"], 15.0);
                h.raw["players"][0]["stopped"] = json!(true);
                h.capture();
                assert_eq!(h.progress()["positionSupported"], false);
            }

            #[test]
            fn frozen_nonzero_samples_expire_after_five_seconds_for_matching_only() {
                let mut h = Harness::one();
                let guard = h.runtime.guard();
                h.now += 5.0;
                assert_eq!(h.progress()["positionSupported"], true);
                h.now += 0.01;
                h.raw["players"][0]["cover"] = json!("/cover.png");
                h.raw["players"][0]["volume"] = json!(0.9);
                h.capture();
                assert_eq!(h.progress()["positionSupported"], false);
                assert_eq!(h.model_position(), 0.0);
                assert_eq!(h.state()["players"][0]["position"], 10.0);
                assert_eq!(h.state()["players"][0]["positionSupported"], true);
                assert_eq!(h.runtime.guard(), guard);
                h.raw["players"][0]["playing"] = json!(false);
                h.capture();
                assert_eq!(h.progress()["positionSupported"], false, "pause cannot revive stale timing");
                h.raw["players"][0]["position"] = json!(15);
                h.capture();
                h.now += 100.0;
                assert_eq!(h.progress()["position"], 15.0);
                assert_eq!(h.model_position(), 15.0);
            }

            #[test]
            fn equal_value_position_revisions_confirm_samples_but_capture_ticks_do_not() {
                let mut h = Harness::one();
                h.now += 4.0;
                h.capture();
                h.now += 2.0;
                assert_eq!(h.progress()["positionSupported"], false);
                h.raw["players"][0]["positionRevision"] = json!(1);
                h.capture();
                assert_eq!(h.progress()["position"], 10.0);
                h.now += 4.0;
                assert_eq!(h.progress()["position"], 14.0);
            }

            #[test]
            fn playing_zero_is_unknown_even_with_seek_support_but_paused_zero_is_real() {
                for can_seek in [false, true] {
                    for position in [json!(0), json!(-1), Value::Null] {
                        let mut raw = native("a");
                        raw["position"] = position;
                        raw["canSeek"] = json!(can_seek);
                        let mut h = Harness::new(vec![raw], vec![]);
                        h.now += 2.0;
                        h.raw["players"][0]["positionRevision"] = json!(1);
                        h.capture();
                        assert_eq!(h.progress()["positionSupported"], false);
                        assert_eq!(h.model_position(), 0.0);
                        assert_eq!(h.state()["players"][0]["canSeek"], can_seek);
                    }
                }
                let mut raw = native("a");
                raw["position"] = json!(0);
                raw["playing"] = json!(false);
                let mut h = Harness::new(vec![raw], vec![]);
                assert_eq!(h.progress()["positionSupported"], true);
                assert_eq!(h.progress()["position"], 0.0);
                h.raw["players"][0]["playing"] = json!(true);
                h.capture();
                assert_eq!(h.progress()["positionSupported"], false);
            }

            #[test]
            fn confirmed_position_reads_do_not_fabricate_native_support_or_replace_getters() {
                let mut raw = native("a");
                raw["position"] = json!(-1);
                let mut h = Harness::new(vec![raw], vec![]);
                let request = h.runtime.reads.pop_front().unwrap();
                h.runtime.position_result(PositionResult { request, position: Some(25.5) }, h.now);
                h.now += 1.0;
                assert_eq!(h.progress()["position"], 26.5);
                assert_eq!(h.progress()["samplePosition"], 25.5);
                assert_eq!(h.state()["players"][0]["positionSupported"], false);
                assert_eq!(h.state()["players"][0]["position"], 0.0);
                assert_eq!(h.model_position(), 25.5);
                assert_eq!(h.raw["players"][0]["position"], -1);
                h.raw["players"][0]["playing"] = json!(false);
                h.capture();
                h.now += 100.0;
                assert_eq!(h.progress()["position"], 26.5);
            }

            #[test]
            fn track_changes_invalidate_timing_and_guard_against_late_or_replaced_reads() {
                let mut h = Harness::one();
                let old = h.runtime.reads.pop_front().unwrap();
                h.raw["players"][0]["title"] = json!("Another track");
                h.raw["players"][0]["url"] = json!("file:///music/another.flac");
                h.capture();
                assert_eq!(h.progress()["positionSupported"], false);
                let fresh = h.runtime.reads.pop_front().unwrap();
                h.runtime.position_result(PositionResult { request: old, position: Some(99.0) }, h.now);
                assert_eq!(h.progress()["positionSupported"], false);
                h.runtime.position_result(PositionResult { request: fresh.clone(), position: Some(20.0) }, h.now);
                assert_eq!(h.model_position(), 20.0);
                h.raw["players"][0]["instance"] = json!(2);
                h.raw["players"][0]["position"] = json!(40);
                h.capture();
                h.runtime.position_result(PositionResult { request: fresh, position: Some(99.0) }, h.now);
                assert_eq!(h.progress()["position"], 40.0);
                assert_eq!(h.state()["players"][0]["trackKey"], "/track/a");
            }

            #[test]
            fn paused_track_and_rate_changes_renew_recovery_without_accepting_cancelled_reads() {
                let mut h = Harness::one();
                h.raw["players"][0]["trackId"] = json!("/track/two");
                h.capture();
                let playing = h.runtime.reads.pop_front().unwrap();
                h.raw["players"][0]["playing"] = json!(false);
                h.capture();
                let paused = h.runtime.reads.pop_front().unwrap();
                h.raw["players"][0]["rate"] = json!(2);
                h.capture();
                let rate = h.runtime.reads.pop_front().unwrap();
                for request in [playing, paused] {
                    h.runtime.position_result(PositionResult { request, position: Some(99.0) }, h.now);
                }
                assert_eq!(h.progress()["positionSupported"], false);
                h.runtime.position_result(PositionResult { request: rate, position: Some(20.0) }, h.now);
                h.now += 100.0;
                assert_eq!(h.progress()["position"], 20.0);
                assert_eq!(h.model_position(), 20.0);
                assert_eq!(h.state()["players"][0]["position"], 10.0);
            }

            #[test]
            fn queue_is_bounded_at_eight_and_coalesces_only_adjacent_same_target_seeks() {
                let mut h = Harness::one();
                h.action("next", "a", 0.0);
                let _first = h.apply();
                for _ in 0..8 { h.action("next", "a", 0.0); }
                assert_eq!(h.runtime.queue.len(), 8);
                h.action("next", "a", 0.0);
                assert_eq!(h.runtime.queue.len(), 8);
                assert!(h.runtime.failure.contains("queue is full"));
                let mut h = Harness::one();
                for value in 0..100 { h.action("seek", "", value as f64); }
                assert_eq!(h.runtime.queue.len(), 1);
                assert_eq!(h.apply().operation.value, Some(99.0));
                h.action("seek", "a", 2.0);
                h.action("next", "a", 0.0);
                h.action("seek", "a", 3.0);
                h.action("seek", "b", 4.0);
                assert_eq!(h.runtime.queue.len(), 4);
            }

            #[test]
            fn apply_fences_raw_sampling_and_the_next_freshness_boundary_not_display_projection() {
                let mut h = Harness::one();
                h.now += 4.9;
                h.action("next", "a", 0.0);
                let apply = h.apply();
                assert_eq!(apply.expires_at, 105.0, "source freshness expires before this young intent");
                assert_eq!(apply.guard.players[0]["position"], 10.0);
                assert_eq!(apply.guard.players[0]["positionRevision"], 0);
                assert!(h.state()["guard"]["players"][0].get("position").is_none());
                assert!(h.progress()["position"].as_f64().unwrap() > 14.8);

                let mut h = Harness::new(vec![], vec![]);
                h.runtime.update_recovered(vec![recovered(1, 1)], h.now);
                h.now += 4.9;
                h.action("jump", ":1.220", 15.0);
                let apply = h.apply();
                assert_eq!(apply.expires_at, 105.0);
                h.now += 0.2;
                let result = h.runtime.validate_recovered_apply(apply.id, vec![recovered(1, 1)], h.now);
                assert!(result.unwrap_err().contains("expired"));
            }

            #[test]
            fn exact_binary64_seek_values_survive_input_plan_and_output_without_clamping() {
                for position in [0.0, 2.0022300000000002e-7, 1.2345678901234567, 361.9294375, 9999.000000000002] {
                    let mut h = Harness::one();
                    h.action("seek", "a", position);
                    let apply = h.apply();
                    assert_eq!(apply.operation.value.unwrap().to_bits(), position.to_bits());
                    let encoded = service_bytes(&apply).unwrap();
                    let decoded: Value = serde_json::from_slice(&encoded).unwrap();
                    assert_eq!(decoded["operation"]["value"].as_f64().unwrap().to_bits(), position.to_bits());
                    assert_eq!(decoded["type"], "apply");
                    assert_eq!(decoded["expiresAt"], 105.0);
                    assert_eq!(decoded["guard"]["players"][0]["seekRevision"], 0);
                }
            }

            #[test]
            fn native_seek_ack_requires_exactly_own_revision_and_does_not_confirm_position() {
                for supported in [false, true] {
                    let mut h = Harness::one();
                    h.raw["players"][0]["playing"] = json!(false);
                    h.raw["players"][0]["position"] = if supported { json!(10) } else { json!(-1) };
                    h.capture();
                    h.action("seek", "a", 80.0);
                    let apply = h.apply();
                    let before = h.state()["players"][0].clone();
                    let old_read = h.runtime.reads.pop_front().unwrap();
                    h.ack(&apply);
                    assert!(h.runtime.failure.is_empty());
                    assert_eq!(h.state()["players"][0]["seekRevision"], 1);
                    assert_eq!(h.state()["players"][0]["position"], before["position"]);
                    assert_eq!(h.state()["players"][0]["positionSupported"], supported);
                    assert_eq!(h.progress()["positionSupported"], false);
                    h.runtime.position_result(PositionResult { request: old_read, position: Some(100.0) }, h.now);
                    assert_eq!(h.progress()["positionSupported"], false);
                    h.raw["players"][0]["position"] = json!(79.5);
                    h.raw["players"][0]["positionRevision"] = json!(1);
                    h.capture();
                    assert_eq!(h.progress()["position"], 79.5);
                    assert!(h.runtime.advance(h.now).is_none());
                }
                for increment in [0, 2] {
                    let mut h = Harness::one();
                    h.action("seek", "a", 20.0);
                    let apply = h.apply();
                    h.action("seek", "a", 50.0);
                    h.raw["players"][0]["seekRevision"] = json!(increment);
                    h.result(&apply, true, None);
                    assert!(h.runtime.failure.contains("advance only its own"));
                    assert!(h.runtime.queue.is_empty());
                    assert!(h.runtime.advance(h.now).is_none());
                }
            }

            #[test]
            fn grouped_seek_rebases_the_sole_final_seek_by_each_own_exact_revision() {
                let mut h = grouped();
                assert_eq!(h.runtime.groups.len(), 1);
                h.action("seek", "alias", 20.0);
                let first = h.apply();
                for value in [30.0, 40.0, 50.0] { h.action("seek", "alias", value); }
                h.ack(&first);
                let second = h.apply();
                assert_eq!(second.operation.player, "alias");
                assert_eq!(second.operation.value, Some(20.0));
                h.ack(&second);
                let last_a = h.apply();
                assert_eq!(last_a.operation.value, Some(50.0));
                assert_eq!(last_a.guard.players[0]["seekRevision"], 1);
                assert_eq!(last_a.guard.players[1]["seekRevision"], 1);
                h.ack(&last_a);
                let last_alias = h.apply();
                h.ack(&last_alias);
                assert_eq!(h.raw["players"][0]["seekRevision"], 2);
                assert_eq!(h.raw["players"][1]["seekRevision"], 2);
                assert_eq!(h.raw["players"][0]["position"], 10.0);
                assert!(h.runtime.failure.is_empty());
                assert!(h.runtime.advance(h.now).is_none());
            }

            #[test]
            fn seek_rebase_cannot_cross_a_target_barrier_or_external_change() {
                for barrier in ["other", "next", "external"] {
                    let mut h = Harness::new(vec![native("a"), native("b")], vec![]);
                    h.action("seek", "a", 20.0);
                    let first = h.apply();
                    if barrier == "other" { h.action("seek", "b", 30.0); }
                    else {
                        if barrier == "next" { h.action("next", "a", 0.0); }
                        h.action("seek", "a", 30.0);
                    }
                    h.ack(&first);
                    if barrier == "external" {
                        h.raw["players"][0]["trackId"] = json!("/track/other");
                        h.capture();
                    }
                    assert!(h.runtime.advance(h.now).is_none());
                    assert!(h.runtime.failure.contains("stale queued"));
                    assert!(h.runtime.failure.contains("not retried"));
                }
            }

            #[test]
            fn actions_expire_before_dispatch_and_at_ack_even_with_no_incoming_events() {
                for in_flight in [false, true] {
                    let mut h = Harness::one();
                    h.action("next", "a", 0.0);
                    let apply = in_flight.then(|| h.apply());
                    h.now += 5.01;
                    if let Some(apply) = &apply { h.result(apply, true, None); }
                    assert!(h.runtime.advance(h.now).is_none());
                    assert!(h.runtime.failure.contains("expired"));
                    assert!(h.runtime.failure.contains("not retried"));
                }
                let mut h = Harness::one();
                h.action("seek", "a", 10.0);
                let apply = h.apply();
                h.action("seek", "a", 20.0);
                h.now += 5.01;
                assert!(h.runtime.advance(h.now).is_none());
                assert!(h.runtime.queue.is_empty());
                h.result(&apply, true, None);
                assert!(h.runtime.advance(h.now).is_none(), "late ack cannot revive an expired transaction");
                h.runtime.enqueue(Action { kind: "next".into(), player: "a".into(), position: 0.0 }, h.now + 1.0, h.now);
                assert!(h.runtime.queue.is_empty(), "future timestamps are not accepted");
            }

            #[test]
            fn guards_ignore_position_volume_cover_mute_and_property_insertion_order() {
                let mut h = Harness::new(vec![native("a")], vec![audio(42)]);
                h.action("next", "a", 0.0);
                let first_guard = h.runtime.guard();
                h.raw["players"][0]["position"] = json!(11);
                h.raw["players"][0]["positionRevision"] = json!(10);
                h.raw["players"][0]["volume"] = json!(0.9);
                h.raw["players"][0]["cover"] = json!("/cover.png");
                h.raw["streams"][0]["volume"] = json!(0.1);
                h.raw["streams"][0]["muted"] = json!(true);
                h.raw["streams"][0]["properties"] = json!({"media.name": "Track a", "application.name": "App a"});
                h.capture();
                assert_eq!(h.runtime.guard(), first_guard);
                let apply = h.apply();
                h.raw["players"][0]["position"] = json!(12);
                h.ack(&apply);
                assert!(h.runtime.failure.is_empty());
                assert_eq!(h.state()["players"][0]["position"], 12.0);
            }

            #[test]
            fn position_driven_regrouping_invalidates_intents_and_latches_pending_changes() {
                for from_read in [false, true] {
                    for pending in [false, true] {
                        let mut h = grouped();
                        h.action("seek", "alias", 30.0);
                        let apply = pending.then(|| h.apply());
                        if from_read {
                            let request = h.runtime.reads.iter().find(|r| r.bus == "alias").unwrap().clone();
                            h.runtime.position_result(PositionResult { request, position: Some(80.0) }, h.now);
                        } else {
                            h.raw["players"][1]["position"] = json!(80.0);
                            h.capture();
                        }
                        assert_eq!(h.runtime.groups.len(), 2);
                        if let Some(apply) = &apply {
                            h.action("next", "a", 0.0);
                            h.raw["players"][1]["position"] = json!(80.0);
                            h.ack(apply);
                            assert!(h.runtime.queue.is_empty());
                        }
                        assert!(h.runtime.advance(h.now).is_none(), "from_read={from_read}, pending={pending}");
                        assert!(h.runtime.failure.contains("not retried"));
                    }
                }
                let mut h = grouped();
                h.action("seek", "alias", 30.0);
                let apply = h.apply();
                let request = h.runtime.reads.iter().find(|r| r.bus == "alias").unwrap().clone();
                h.runtime.position_result(PositionResult { request, position: Some(80.0) }, h.now);
                h.raw["players"][1]["position"] = json!(11.0);
                h.capture();
                assert_eq!(h.runtime.groups.len(), 1);
                h.ack(&apply);
                assert!(h.runtime.advance(h.now).is_none(), "regrouping away and back cannot revive a pending transaction");
                assert!(h.runtime.failure.contains("not retried"));
            }

            #[test]
            fn timing_expiry_invalidates_intents_even_before_the_action_deadline() {
                for pending in [false, true] {
                    let mut h = grouped();
                    h.raw["players"][1]["position"] = json!(80.0);
                    h.capture();
                    assert_eq!(h.runtime.groups.len(), 2);
                    h.now += 4.9;
                    h.action("seek", "alias", 30.0);
                    let apply = pending.then(|| h.apply());
                    h.now += 0.2;
                    h.message(json!({"type": "tick"}));
                    assert_eq!(h.runtime.groups.len(), 1, "the playing sample became unknown for matching");
                    if let Some(apply) = &apply { h.ack(apply); }
                    assert!(h.runtime.advance(h.now).is_none());
                    assert!(h.runtime.failure.contains("not retried"));
                    assert!(!h.runtime.failure.contains("expired"), "the intent itself is only 0.2 seconds old");
                }
            }

            #[test]
            fn every_raw_ownership_and_action_field_invalidates_queued_and_pending_work() {
                let changes = [
                    ("bus", json!("replacement")), ("identity", json!("Replacement")),
                    ("desktopEntry", json!("other.desktop")), ("title", json!("Other track")),
                    ("artist", json!("Other artist")), ("album", json!("Other album")),
                    ("url", json!("file:///other")), ("artUrl", json!("file:///other-art")),
                    ("browserHint", json!("firefox")), ("playing", json!(false)),
                    ("stopped", json!(true)), ("length", json!(201)), ("lengthMicros", json!(200000000)),
                    ("canPlay", json!(false)), ("canPause", json!(false)), ("canSeek", json!(false)),
                    ("canNext", json!(false)), ("canPrevious", json!(false)), ("trackId", json!("/track/other")),
                    ("instance", json!(2)), ("seekRevision", json!(1)), ("rate", json!(2)),
                    ("positionSupported", json!(false)), ("available", json!(false)),
                ];
                for (field, value) in changes {
                    for pending in [false, true] {
                        let mut h = Harness::one();
                        h.action("next", "a", 0.0);
                        let apply = pending.then(|| h.apply());
                        h.raw["players"][0][field] = value.clone();
                        if let Some(apply) = &apply { h.result(apply, true, None); }
                        else { h.capture(); }
                        assert!(h.runtime.advance(h.now).is_none(), "{field}, pending={pending}");
                        assert!(h.runtime.failure.contains("not retried"), "{field}: {}", h.runtime.failure);
                    }
                }
                for (field, value) in [
                    ("id", json!(43)), ("name", json!("Other")), ("nickname", json!("Other")),
                    ("ready", json!(false)), ("properties", json!({"application.process.id": "5678"})),
                    ("instance", json!(100)),
                ] {
                    for pending in [false, true] {
                        let mut h = Harness::new(vec![native("a")], vec![audio(42)]);
                        h.action("next", "a", 0.0);
                        let apply = pending.then(|| h.apply());
                        h.raw["streams"][0][field] = value.clone();
                        if let Some(apply) = &apply { h.result(apply, true, None); }
                        else { h.capture(); }
                        assert!(h.runtime.advance(h.now).is_none(), "stream {field}");
                        assert!(h.runtime.failure.contains("ownership changed"));
                    }
                }
            }

            #[test]
            fn list_addition_removal_and_order_changes_interrupt_transactions() {
                for change in 0..6 {
                    let mut h = Harness::new(vec![native("a"), native("b")], vec![audio(42), audio(43)]);
                    h.action("next", "a", 0.0);
                    let apply = h.apply();
                    match change {
                        0 => h.raw["players"].as_array_mut().unwrap().push(native("c")),
                        1 => { h.raw["players"] = json!([]); }
                        2 => h.raw["players"].as_array_mut().unwrap().reverse(),
                        3 => h.raw["streams"].as_array_mut().unwrap().push(audio(44)),
                        4 => { h.raw["streams"] = json!([]); }
                        _ => h.raw["streams"].as_array_mut().unwrap().reverse(),
                    }
                    h.result(&apply, true, None);
                    assert!(h.runtime.advance(h.now).is_none());
                    assert!(h.runtime.failure.contains("not retried"));
                }
            }

            #[test]
            fn selection_is_retained_cycles_groups_and_detects_a_b_a_races() {
                for pending in [false, true] {
                    let mut h = Harness::new(vec![native("a"), native("b"), native("c")], vec![]);
                    h.action("next", "", 0.0);
                    let apply = pending.then(|| h.apply());
                    h.runtime.select("b".into(), h.now);
                    h.runtime.select("a".into(), h.now);
                    if let Some(apply) = &apply { h.result(apply, true, None); }
                    assert!(h.runtime.advance(h.now).is_none());
                    assert!(h.runtime.failure.contains("selection changed"));
                    h.runtime.select("b".into(), h.now);
                    h.message(json!({"type": "cycle", "snapshot": h.raw}));
                    assert_eq!(h.runtime.selected, "c");
                    h.message(json!({"type": "cycle", "snapshot": h.raw}));
                    assert_eq!(h.runtime.selected, "a");
                }
                let mut h = grouped();
                h.runtime.select("alias".into(), h.now);
                assert_eq!(h.runtime.selected, "alias");
                assert_eq!(h.runtime.active, "a");
                h.capture();
                assert_eq!(h.runtime.selected, "alias");
                h.runtime.select("gone".into(), h.now);
                assert!(h.runtime.failure.contains("no longer available"));
            }

            #[test]
            fn native_events_while_awaiting_ack_latch_external_changes_even_if_reverted() {
                let mut h = grouped();
                h.action("toggle", "a", 0.0);
                let apply = h.apply();
                h.action("next", "a", 0.0);
                h.raw["players"][1]["instance"] = json!(3);
                h.capture();
                assert!(h.runtime.queue.is_empty());
                h.raw["players"][1]["instance"] = json!(2);
                h.result(&apply, true, None);
                assert!(h.runtime.advance(h.now).is_none());
                assert!(h.runtime.failure.contains("remaining actions"));
                assert!(h.runtime.failure.contains("not retried"));
            }

            #[test]
            fn seek_revision_can_arrive_in_a_snapshot_before_the_matching_ack() {
                let mut h = Harness::one();
                h.action("seek", "a", 20.0);
                let apply = h.apply();
                h.action("seek", "a", 50.0);
                h.raw["players"][0]["seekRevision"] = json!(1);
                h.capture();
                assert!(h.runtime.transaction.as_ref().unwrap().interrupted.is_none());
                h.result(&apply, true, None);
                assert_eq!(h.apply().operation.value, Some(50.0));
            }

            #[test]
            fn pause_all_fans_out_even_to_filtered_players_with_bad_can_pause() {
                let mut browser = native("browser");
                browser["identity"] = json!("Chromium");
                browser["url"] = json!("https://example.test/not-allowed");
                browser["canPause"] = json!(false);
                let mut a = native("a");
                a["canPause"] = json!(false);
                let mut h = Harness::new(vec![a, browser], vec![]);
                assert_eq!(h.runtime.groups.len(), 1);
                h.action("pauseAll", "", 0.0);
                let first = h.apply();
                assert_eq!(first.operation.operation, "pause");
                h.ack(&first);
                let second = h.apply();
                assert_eq!(second.operation.player, "browser");
                assert_eq!(second.operation.operation, "pause");
                h.ack(&second);
                assert!(h.runtime.advance(h.now).is_none());
            }

            #[test]
            fn next_and_previous_only_address_the_ranked_primary_of_a_selected_alias() {
                for kind in ["next", "previous"] {
                    let mut h = grouped();
                    h.action(kind, "alias", 0.0);
                    let apply = h.apply();
                    assert_eq!(apply.operation.player, "a");
                    assert_eq!(apply.operation.operation, kind);
                    h.ack(&apply);
                    assert!(h.runtime.advance(h.now).is_none());
                }
                let mut h = grouped();
                h.raw["players"][0]["canNext"] = json!(false);
                h.capture();
                h.action("next", "alias", 0.0);
                assert!(h.runtime.advance(h.now).is_none(), "alias capability cannot override primary");
            }

            #[test]
            fn toggle_pause_all_resumes_only_the_current_selected_group_when_idle() {
                let mut h = Harness::new(vec![native("a"), native("b")], vec![]);
                for player in h.raw["players"].as_array_mut().unwrap() { player["playing"] = json!(false); }
                h.capture();
                h.runtime.select("b".into(), h.now);
                h.action("togglePauseAll", "a", 0.0);
                let apply = h.apply();
                assert_eq!(apply.operation.player, "b");
                assert_eq!(apply.operation.operation, "play");
                h.ack(&apply);
                assert!(h.runtime.advance(h.now).is_none());
            }

            #[test]
            fn preflight_validates_every_target_before_the_first_native_dispatch() {
                let mut h = grouped();
                h.raw["players"][1]["trackId"] = json!("not/a/path");
                h.capture();
                h.action("seek", "a", 20.0);
                assert!(h.runtime.advance(h.now).is_none());
                assert!(h.runtime.failure.contains("Seeking is unavailable"));
                assert_eq!(h.raw["players"][0]["seekRevision"], 0);
                let mut h = grouped();
                h.raw["players"][1]["available"] = json!(false);
                h.capture();
                h.action("toggle", "a", 0.0);
                assert!(h.runtime.advance(h.now).is_none());
                assert!(h.runtime.failure.contains("disappeared"));
            }

            #[test]
            fn failure_discards_the_entire_queue_and_never_retries_native_calls() {
                for error in [None, Some("native seek failed")] {
                    let mut h = Harness::one();
                    h.action("seek", "a", 20.0);
                    let apply = h.apply();
                    h.action("seek", "a", 50.0);
                    h.result(&apply, false, error);
                    assert!(h.runtime.queue.is_empty());
                    assert!(h.runtime.transaction.is_none());
                    assert!(h.runtime.failure.contains("not retried"));
                    assert!(h.runtime.advance(h.now).is_none());
                    h.capture();
                    assert!(h.runtime.advance(h.now).is_none());
                }
            }

            #[test]
            fn orphan_duplicate_and_restart_results_never_mutate_current_snapshots() {
                let mut h = Harness::one();
                h.action("next", "a", 0.0);
                let apply = h.apply();
                let mut stale = h.raw.clone();
                stale["players"][0]["title"] = json!("Old snapshot");
                h.message(json!({"type": "result", "id": apply.id - 1, "ok": true, "snapshot": stale}));
                assert_eq!(h.state()["players"][0]["title"], "Track a");
                assert!(h.runtime.transaction.is_some());
                h.ack(&apply);
                h.message(json!({"type": "result", "id": apply.id, "ok": true, "snapshot": stale}));
                assert_eq!(h.state()["players"][0]["title"], "Track a");
                h.runtime = Runtime::new(h.now + 1.0);
                h.message(json!({"type": "result", "id": apply.id, "ok": true, "snapshot": stale}));
                assert!(h.runtime.queue.is_empty());
                assert!(h.runtime.native.players.is_empty());
                assert!(h.runtime.advance(h.now + 1.0).is_none());
            }

            #[test]
            fn ui_reported_errors_survive_snapshots_ticks_and_selection_until_success() {
                let mut h = Harness::one();
                h.message(json!({"type": "report", "error": "Source changed. Try again."}));
                h.capture();
                h.message(json!({"type": "tick", "snapshot": h.raw}));
                h.runtime.select("a".into(), h.now);
                assert_eq!(h.state()["failure"], "Source changed. Try again.");
                h.action("next", "a", 0.0);
                let apply = h.apply();
                assert_eq!(h.state()["failure"], "Source changed. Try again.");
                h.ack(&apply);
                assert_eq!(h.state()["failure"], "");
            }

            #[test]
            fn invalid_actions_and_routing_never_reach_native_dispatch() {
                for (kind, bus, position) in [("move", "a", 0.0), ("seek", "a", -1.0), ("jump", "a", 20.0)] {
                    let mut h = Harness::one();
                    h.action(kind, bus, position);
                    assert!(h.runtime.advance(h.now).is_none());
                    assert!(h.runtime.failure.contains("Invalid media action"));
                }
                let mut h = Harness::one();
                h.action("next", "gone", 0.0);
                assert!(h.runtime.advance(h.now).is_none());
                assert!(h.runtime.failure.contains("no longer available"));
                assert!(serde_json::from_value::<ServiceMessage>(json!({"type": "action",
                    "action": {"kind": "next", "target": "speaker"},
                    "createdAt": 100, "snapshot": h.raw})).is_err());
            }

            #[test]
            fn jumps_use_latest_confirmed_samples_not_display_estimates_and_clamp() {
                for (sample, delta, target) in [(10.0_f64, 15.0, 25.0), (3.0, -15.0, 0.0), (195.0, 15.0, 200.0)] {
                    let mut h = Harness::one();
                    h.now += 2.0;
                    h.raw["players"][0]["position"] = json!(sample);
                    h.raw["players"][0]["positionRevision"] = json!(1);
                    h.capture();
                    h.now += 1.0;
                    assert_eq!(h.progress()["position"], (sample + 1.0).min(200.0));
                    h.action("jump", "a", delta);
                    assert_eq!(h.apply().operation.value, Some(target));
                }
                let mut h = Harness::one();
                h.now += 5.01;
                h.action("jump", "a", 15.0);
                assert!(h.runtime.advance(h.now).is_none());
                assert!(h.runtime.failure.contains("fresh native position"));
                let mut h = Harness::one();
                h.raw["players"][0]["length"] = Value::Null;
                h.capture();
                h.action("jump", "a", 15.0);
                assert!(h.runtime.advance(h.now).is_none());
                assert!(h.runtime.failure.contains("duration"));
            }

            #[test]
            fn transport_preserves_browser_bridge_and_all_member_capability_reasons() {
                let mut h = grouped();
                let bus = "org.mpris.MediaPlayer2.mprisence_web.generic.pmetadata";
                h.raw["players"][0]["bus"] = json!(bus);
                for player in h.raw["players"].as_array_mut().unwrap() {
                    player["canPlay"] = json!(false);
                    player["canPause"] = json!(false);
                }
                h.capture();
                let state = h.state();
                assert_eq!(state["transports"][bus]["enabled"], false);
                assert!(state["transports"][bus]["reason"].as_str().unwrap().contains("browser bridge does not expose pause"));
                h.raw["players"][1]["canPause"] = json!(true);
                h.capture();
                assert_eq!(h.state()["transports"][bus]["reason"], "Pause");
                h.raw["players"][1]["available"] = json!(false);
                h.capture();
                assert!(h.state()["transports"][bus]["reason"].as_str().unwrap().contains("member is unavailable"));
                h.raw["players"][1]["available"] = json!(true);
                for player in h.raw["players"].as_array_mut().unwrap() { player["playing"] = json!(false); }
                h.capture();
                let primary = h.runtime.active.clone();
                assert_eq!(primary, "alias", "paused representative ranking still favors transport support");
                assert!(h.state()["transports"][&primary]["reason"].as_str().unwrap().contains("does not advertise play"));
            }

            fn recovered(instance: u64, position_revision: u64) -> crate::playback::RecoveredPlayer {
                crate::playback::RecoveredPlayer {
                    player: Player {
                        bus: ":1.220".into(), identity: "Haruna".into(), title: "Recovered track".into(),
                        artist: "Artist".into(), album: "Album".into(), playing: true, position: 10.0,
                        length: Some(200.0), can_play: true, can_pause: false, can_seek: true,
                        can_next: true, can_previous: true, ..Player::default()
                    }, instance, track_id: "/track/one".into(), rate: 1.0,
                    position_supported: true, position_revision, cover: String::new(),
                }
            }

            #[test]
            fn recovered_effects_stay_in_rust_and_preserve_own_seek_revisions() {
                let mut h = Harness::new(vec![], vec![]);
                h.runtime.update_recovered(vec![recovered(1, 1)], h.now);
                assert_eq!(h.state()["players"][0]["bus"], ":1.220");
                assert!(h.runtime.reads.is_empty(), "Recovery exclusively owns private position reads");
                assert_eq!(h.state()["guard"]["players"], json!([]), "private owners never enter the frontend native guard");
                h.action("seek", ":1.220", 80.0);
                let apply = h.apply();
                assert!(apply.recovered);
                h.runtime.finish_recovered(apply.id, Ok(()), vec![recovered(1, 1)], h.now);
                assert!(h.runtime.failure.is_empty());
                assert_eq!(h.state()["players"][0]["seekRevision"], 1);
                assert_eq!(h.state()["players"][0]["position"], 10.0);
                assert_eq!(h.progress()["positionSupported"], false);
                assert!(h.runtime.reads.is_empty(), "private seek refreshes must use Recovery's reader");
                h.runtime.update_recovered(vec![recovered(1, 2)], h.now);
                assert_eq!(h.progress()["position"], 10.0);
                h.action("pauseAll", "", 0.0);
                let pause = h.apply();
                assert_eq!(pause.operation.operation, "pause");
                assert!(pause.recovered, "bad CanPause does not gate private-owner pause-all");
            }

            #[test]
            fn recovered_dispatch_revalidates_latest_policy_and_deadline_before_sending() {
                for change in ["track", "instance", "playback", "capability", "rate", "disappear", "deadline"] {
                    let mut h = Harness::new(vec![], vec![]);
                    h.runtime.update_recovered(vec![recovered(1, 1)], h.now);
                    h.action("seek", ":1.220", 80.0);
                    let apply = h.apply();
                    h.action("next", ":1.220", 0.0);
                    let mut latest = recovered(1, 1);
                    match change {
                        "track" => latest.track_id = "/track/two".into(),
                        "instance" => latest.instance = 2,
                        "playback" => latest.player.playing = false,
                        "capability" => latest.player.can_seek = false,
                        "rate" => latest.rate = 1.5,
                        "deadline" => h.now += 5.01,
                        _ => {}
                    }
                    let players = if change == "disappear" { vec![] } else { vec![latest] };
                    let result = h.runtime.validate_recovered_apply(apply.id, players.clone(), h.now);
                    assert!(result.is_err(), "{change}");
                    h.runtime.finish_recovered(apply.id, result, players, h.now);
                    assert!(h.runtime.queue.is_empty());
                    assert!(h.runtime.advance(h.now).is_none());
                    assert!(h.runtime.failure.contains("not retried"));
                }
                let mut h = Harness::new(vec![], vec![]);
                h.runtime.update_recovered(vec![recovered(1, 1)], h.now);
                h.action("seek", ":1.220", 80.0);
                let apply = h.apply();
                let mut latest = recovered(1, 2);
                latest.player.position = 11.0;
                assert!(h.runtime.validate_recovered_apply(apply.id, vec![latest.clone()], h.now).is_ok(), "ordinary samples do not invalidate unchanged grouping");
                h.runtime.finish_recovered(apply.id, Ok(()), vec![latest], h.now);
                assert!(h.runtime.failure.is_empty());
                assert_eq!(h.state()["players"][0]["seekRevision"], 1);
            }

            #[test]
            fn recovered_replacement_or_disappearance_interrupts_without_retry() {
                for replacement in [true, false] {
                    let mut h = Harness::new(vec![], vec![]);
                    h.runtime.update_recovered(vec![recovered(1, 1)], h.now);
                    h.action("next", ":1.220", 0.0);
                    let apply = h.apply();
                    let players = if replacement { vec![recovered(2, 1)] } else { vec![] };
                    h.runtime.finish_recovered(apply.id, Ok(()), players, h.now);
                    assert!(h.runtime.failure.contains("not retried"));
                    assert!(h.runtime.advance(h.now).is_none());
                }
            }

            #[test]
            fn capture_requests_are_bounded_and_native_updates_reset_the_silence_clock() {
                let mut h = Harness::one();
                assert!(!h.runtime.capture_due(104.99));
                assert!(h.runtime.capture_due(105.0));
                assert!(!h.runtime.capture_due(105.5));
                assert!(!h.runtime.capture_due(109.99));
                assert!(h.runtime.capture_due(110.0));
                h.now = 111.0;
                h.capture();
                assert!(!h.runtime.capture_due(115.0));
                assert!(h.runtime.capture_due(116.0));
            }

            #[test]
            fn wire_snapshot_needs_no_available_extension_and_guards_compare_raw_fields() {
                let mut raw = native("a");
                raw.as_object_mut().unwrap().remove("available");
                raw["length"] = json!(-1);
                raw["lengthMicros"] = json!(361929437.5);
                raw["positionRevision"] = json!(24);
                let mut h = Harness::new(vec![raw], vec![audio(42)]);
                let state = h.state();
                assert_eq!(state["players"][0]["length"], 361.9294375);
                assert_eq!(state["players"][0]["lengthMicros"], 361929437.5);
                assert_eq!(state["players"][0]["positionRevision"], 24);
                assert_eq!(state["players"][0]["volume"], 0.5);
                assert_eq!(state["transports"]["a"]["enabled"], true);
                assert!(state["guard"]["players"][0].get("available").is_none());
                for collection in ["players", "streams"] {
                    let expected = state["guard"][collection].as_array().unwrap();
                    let current = h.raw[collection].as_array().unwrap();
                    assert_eq!(expected.len(), current.len());
                    for (expected, current) in expected.iter().zip(current) {
                        for (field, value) in expected.as_object().unwrap() {
                            assert_eq!(current.get(field), Some(value), "opaque guard field {field}");
                        }
                    }
                }
                assert_eq!(state["guard"]["players"][0]["length"], -1);
                h.action("next", "a", 0.0);
                let apply = h.apply();
                h.ack(&apply);
                assert!(h.runtime.failure.is_empty());
            }

            #[test]
            fn explicit_unsupported_positions_cannot_become_confirmed_positive_samples() {
                let mut raw = native("a");
                raw["positionSupported"] = json!(false);
                let mut h = Harness::new(vec![raw], vec![]);
                assert_eq!(h.state()["players"][0]["positionSupported"], false);
                assert_eq!(h.progress()["positionSupported"], false);
                assert_eq!(h.model_position(), 0.0);
                h.now += 1.0;
                h.raw["players"][0]["positionRevision"] = json!(1);
                h.capture();
                assert_eq!(h.model_position(), 0.0);
                h.raw["players"][0]["positionSupported"] = json!(true);
                h.capture();
                assert_eq!(h.model_position(), 10.0);
                assert_eq!(h.raw["players"][0]["position"], 10.0);
            }

            #[test]
            fn frozen_bridge_position_cannot_keep_an_advancing_url_less_chrome_hidden() {
                let mut bridge = native("bridge");
                bridge["identity"] = json!("YouTube Music");
                bridge["browserHint"] = json!("chromium");
                bridge["url"] = json!("https://music.youtube.com/watch?v=track");
                bridge["position"] = json!(0.208);
                let mut chrome = bridge.clone();
                chrome["bus"] = json!("chrome");
                chrome["identity"] = json!("Chromium");
                chrome["browserHint"] = Value::Null;
                chrome["url"] = json!("");
                chrome["instance"] = json!(2);
                chrome["position"] = json!(80.0);
                let mut h = Harness::new(vec![bridge, chrome], vec![]);
                assert_eq!(h.runtime.groups[0].players, ["bridge"]);
                h.now += 5.01;
                h.raw["players"][1]["position"] = json!(85.0);
                h.raw["players"][1]["positionRevision"] = json!(1);
                h.capture();
                assert_eq!(h.runtime.groups.len(), 1);
                assert_eq!(h.runtime.groups[0].players, ["bridge", "chrome"]);
                let model = h.runtime.request(Action::default(), h.now);
                assert_eq!(model.players[0].position, 0.0);
                assert_eq!(model.players[1].position, 85.0);
                assert_eq!(h.raw["players"][0]["position"], 0.208);
                assert_eq!(h.state()["progress"][0]["samplePosition"], Value::Null);
            }

            #[test]
            fn snapshot_free_ticks_emit_only_progress_until_structural_state_changes() {
                let mut h = Harness::new(vec![native("a"), native("b")], vec![]);
                let mut output = ServiceOutput::default();
                let first: Value = serde_json::from_slice(&output.update(&h.runtime, h.now, false).unwrap().unwrap()).unwrap();
                assert_eq!(first["type"], "state");
                assert!(output.update(&h.runtime, h.now, false).unwrap().is_none());
                h.now += 1.25;
                h.message(json!({"type": "tick"}));
                let progress: Value = serde_json::from_slice(&output.update(&h.runtime, h.now, true).unwrap().unwrap()).unwrap();
                assert_eq!(progress["type"], "progress");
                assert!(progress.get("groups").is_none());
                assert_eq!(progress["progress"][0]["position"], 11.25);
                assert_eq!(progress["progress"][0]["samplePosition"], 10.0);
                assert_eq!(h.state()["players"][0]["position"], 10.0);
                h.now += 4.0;
                h.message(json!({"type": "tick"}));
                let progress: Value = serde_json::from_slice(&output.update(&h.runtime, h.now, true).unwrap().unwrap()).unwrap();
                assert_eq!(progress["type"], "progress");
                assert_eq!(progress["progress"][0]["positionSupported"], false);
                h.runtime.select("b".into(), h.now);
                let state: Value = serde_json::from_slice(&output.update(&h.runtime, h.now, true).unwrap().unwrap()).unwrap();
                assert_eq!(state["type"], "state");
                assert_eq!(state["activePlayer"], "b");
                for _ in 0..10 {
                    assert!(output.update(&h.runtime, h.now, false).unwrap().is_none(), "map order must not generate false refreshes");
                }
            }

            #[test]
            fn hidden_timeout_polls_are_silent_until_sample_quality_changes_once() {
                let mut h = Harness::one();
                let mut output = ServiceOutput::default();
                output.update(&h.runtime, h.now, false).unwrap().unwrap();
                for _ in 0..10 {
                    h.now += 0.5;
                    assert!(h.runtime.advance(h.now).is_none());
                    assert!(output.update(&h.runtime, h.now, false).unwrap().is_none(), "projection alone is not an observation");
                }
                h.now += 0.01;
                h.runtime.advance(h.now);
                let expired: Value = serde_json::from_slice(&output.update(&h.runtime, h.now, false).unwrap().unwrap()).unwrap();
                assert_eq!(expired["type"], "progress");
                assert_eq!(expired["progress"][0]["positionSupported"], false);
                for _ in 0..10 {
                    h.now += 0.5;
                    h.runtime.advance(h.now);
                    assert!(output.update(&h.runtime, h.now, false).unwrap().is_none());
                }
                let tick: Value = serde_json::from_slice(&output.update(&h.runtime, h.now, true).unwrap().unwrap()).unwrap();
                assert_eq!(tick["type"], "progress");
            }

            #[test]
            fn confirmed_reads_publish_corrections_even_at_equal_values_without_ui_ticks() {
                let mut h = Harness::one();
                let mut output = ServiceOutput::default();
                output.update(&h.runtime, h.now, false).unwrap();
                let request = h.runtime.reads.pop_front().unwrap();
                h.now += 1.0;
                assert!(output.update(&h.runtime, h.now, false).unwrap().is_none());
                h.runtime.position_result(PositionResult { request, position: Some(10.0) }, h.now);
                let progress: Value = serde_json::from_slice(&output.update(&h.runtime, h.now, false).unwrap().unwrap()).unwrap();
                assert_eq!(progress["type"], "progress");
                assert_eq!(progress["progress"][0]["position"], 10.0);
                assert_eq!(progress["progress"][0]["samplePosition"], 10.0);
                h.now += 0.5;
                assert!(output.update(&h.runtime, h.now, false).unwrap().is_none());
                h.raw["players"][0]["positionRevision"] = json!(1);
                h.capture();
                let state: Value = serde_json::from_slice(&output.update(&h.runtime, h.now, false).unwrap().unwrap()).unwrap();
                assert_eq!(state["type"], "state");
                assert_eq!(state["progress"][0]["samplePosition"], 10.0);
            }

            #[test]
            fn recovery_observations_publish_while_hidden_but_projection_does_not() {
                let mut h = Harness::new(vec![], vec![]);
                h.runtime.update_recovered(vec![recovered(1, 1)], h.now);
                let mut output = ServiceOutput::default();
                output.update(&h.runtime, h.now, false).unwrap();
                h.now += 0.5;
                assert!(output.update(&h.runtime, h.now, false).unwrap().is_none());
                h.runtime.update_recovered(vec![recovered(1, 2)], h.now);
                let state: Value = serde_json::from_slice(&output.update(&h.runtime, h.now, false).unwrap().unwrap()).unwrap();
                assert_eq!(state["type"], "state");
                assert_eq!(state["players"][0]["positionRevision"], 2);
            }

            fn stream_volume_harness() -> Harness {
                let mut player = native("a");
                player["volume"] = json!(-1);
                let mut stream = audio(42);
                stream["volume"] = json!(0.73);
                stream["muted"] = json!(true);
                Harness::new(vec![player], vec![stream])
            }

            #[test]
            fn native_volume_descriptor_and_stream_mute_are_independent() {
                let mut h = Harness::new(vec![native("a")], vec![audio(42)]);
                h.raw["streams"][0]["muted"] = json!(true);
                h.raw["streams"][0]["volume"] = json!(0.8);
                h.capture();
                assert_eq!(h.state()["volumes"]["a"], json!({
                    "enabled": true, "value": 0.5, "key": "player:1", "reason": "Media volume (0–100%)",
                    "muteEnabled": true, "muted": true, "muteReason": "Application stream mute"
                }));
                h.volume("a", 0.38);
                let volume = h.apply();
                assert_eq!(serde_json::to_value(&volume.operation).unwrap(), json!({"player": "a", "operation": "volume", "value": 0.38}));
                assert_eq!(volume.guard.players[0]["volume"], h.raw["players"][0]["volume"]);
                assert!(h.state()["guard"]["players"][0].get("volume").is_none());
                assert_eq!(h.state()["volumes"]["a"]["value"], 0.5, "dispatch is not an optimistic update");
                h.ack(&volume);
                assert_eq!(h.state()["volumes"]["a"]["value"], 0.38);
                assert_eq!(h.raw["streams"][0]["muted"], true);
                h.mute("a", false);
                let mute = h.apply();
                assert_eq!(serde_json::to_value(&mute.operation).unwrap(), json!({"player": "a", "operation": "mute", "streamId": 42, "muted": false}));
                h.ack(&mute);
                assert!(h.runtime.failure.is_empty());
                assert_eq!(h.raw["players"][0]["volume"], 0.38);
                assert_eq!(h.raw["streams"][0]["volume"], 0.8);
                assert_eq!(h.state()["volumes"]["a"]["muted"], false);
            }

            #[test]
            fn native_volume_without_an_owned_stream_never_gains_mute() {
                let mut h = Harness::one();
                let descriptor = h.state()["volumes"]["a"].clone();
                assert_eq!(descriptor["enabled"], true);
                assert_eq!(descriptor["key"], "player:1");
                assert_eq!(descriptor["muteEnabled"], false);
                assert_eq!(descriptor["muted"], false);
                assert!(descriptor["muteReason"].as_str().unwrap().contains("ready, uniquely owned"));
                h.mute("a", true);
                assert!(h.runtime.advance(h.now).is_none());
                assert_eq!(h.raw["players"][0]["volume"], 0.5);
                h.volume("a", 1.7);
                let apply = h.apply();
                assert_eq!(apply.operation.value, Some(1.0));
                h.ack(&apply);
                assert_eq!(h.state()["volumes"]["a"]["value"], 1.0);
            }

            #[test]
            fn stream_volume_clamps_and_unmutes_only_on_positive_requests() {
                for (value, expected, unmute) in [(-0.5, 0.0, false), (0.0, 0.0, false), (0.3845678901234567, 0.3845678901234567, true), (1.5, 1.0, true)] {
                    let mut h = stream_volume_harness();
                    assert_eq!(h.state()["volumes"]["a"]["key"], "stream:42");
                    assert_eq!(h.state()["volumes"]["a"]["value"], 0.73);
                    assert_eq!(h.state()["volumes"]["a"]["reason"], "Application stream volume (0–100%)");
                    h.volume("a", value);
                    let apply = h.apply();
                    assert!(!apply.recovered);
                    assert_eq!(apply.operation.stream_id, Some(42));
                    assert_eq!(apply.operation.value, Some(expected));
                    assert_eq!(apply.operation.muted, unmute.then_some(false));
                    assert_eq!(apply.guard.streams[0]["volume"], h.raw["streams"][0]["volume"]);
                    assert_eq!(apply.guard.players[0]["volume"], -1);
                    assert!(h.state()["guard"]["streams"][0].get("volume").is_none());
                    h.ack(&apply);
                    assert!(h.runtime.failure.is_empty());
                    assert_eq!(h.raw["streams"][0]["volume"], expected);
                    assert_eq!(h.raw["streams"][0]["muted"], !unmute);
                    assert_eq!(h.raw["players"][0]["volume"], -1);
                    assert_eq!(h.raw["players"][0]["seekRevision"], 0);
                }
            }

            #[test]
            fn web_bridge_constant_volume_never_masquerades_as_a_tab_slider() {
                let bus = "org.mpris.MediaPlayer2.mprisence_web.generic.ptest";
                for owned_stream in [false, true] {
                    let mut player = native("a");
                    player["bus"] = json!(bus);
                    player["volume"] = json!(1);
                    let mut h = Harness::new(vec![player], if owned_stream { vec![audio(42)] } else { vec![] });
                    let descriptor = h.state()["volumes"][bus].clone();
                    assert_eq!(descriptor["enabled"], owned_stream);
                    assert_eq!(descriptor["muteEnabled"], owned_stream);
                    if owned_stream {
                        assert_eq!(descriptor["key"], "stream:42");
                        h.volume(bus, 0.4);
                        let apply = h.apply();
                        assert_eq!(apply.operation.stream_id, Some(42));
                        h.ack(&apply);
                        assert_eq!(h.raw["players"][0]["volume"], 1);
                    } else {
                        assert_eq!(descriptor["key"], "");
                        assert!(descriptor["reason"].as_str().unwrap().starts_with("This browser bridge does not support tab volume. "));
                        h.volume(bus, 0.4);
                        assert!(h.runtime.advance(h.now).is_none());
                    }
                }
            }

            #[test]
            fn private_players_use_owned_native_streams_without_exposing_private_controls_to_js() {
                let mut stream = audio(42);
                stream["name"] = json!("Haruna");
                stream["properties"] = json!({"application.name": "Haruna", "media.name": "Recovered track"});
                let mut h = Harness::new(vec![], vec![stream]);
                h.runtime.update_recovered(vec![recovered(1, 1)], h.now);
                assert_eq!(h.state()["volumes"][":1.220"]["key"], "stream:42");
                h.volume(":1.220", 0.4);
                let apply = h.apply();
                assert!(!apply.recovered, "this is a PipeWire setter, not a private MPRIS call");
                assert_eq!(apply.operation.player, ":1.220");
                assert_eq!(apply.operation.stream_id, Some(42));
                assert_eq!(apply.guard.players, Vec::<Value>::new());
                h.ack(&apply);
                assert!(h.runtime.failure.is_empty());
                h.mute(":1.220", true);
                let mute = h.apply();
                h.ack(&mute);
                assert_eq!(h.state()["volumes"][":1.220"]["muted"], true);
                assert_eq!(h.raw["streams"][0]["volume"], 0.4);
                h.raw["streams"] = json!([]);
                h.capture();
                assert_eq!(h.state()["players"][0]["bus"], ":1.220", "stream removal cannot hide a live recovered player");
                assert_eq!(h.state()["volumes"][":1.220"]["enabled"], false);
            }

            #[test]
            fn shared_or_unready_streams_are_never_inferred_as_mute_or_fallback_targets() {
                let mut a = native("a");
                a["volume"] = json!(-1);
                let mut b = native("b");
                b["identity"] = a["identity"].clone();
                b["desktopEntry"] = a["desktopEntry"].clone();
                b["volume"] = json!(-1);
                let mut stream = audio(42);
                stream["properties"] = json!({"application.name": "App a"});
                let mut h = Harness::new(vec![a, b], vec![stream]);
                for bus in ["a", "b"] {
                    assert_eq!(h.state()["volumes"][bus]["enabled"], false);
                    assert_eq!(h.state()["volumes"][bus]["muteEnabled"], false);
                    h.mute(bus, true);
                    assert!(h.runtime.advance(h.now).is_none());
                }
                for (field, value) in [("ready", json!(false)), ("volume", json!(-1)), ("volume", Value::Null)] {
                    let mut h = stream_volume_harness();
                    h.raw["streams"][0][field] = value;
                    h.capture();
                    assert_eq!(h.state()["volumes"]["a"]["enabled"], false);
                    assert_eq!(h.state()["volumes"]["a"]["muteEnabled"], false);
                }
            }

            #[test]
            fn unavailable_sources_disable_both_volume_and_mute_with_liveness_feedback() {
                let mut h = stream_volume_harness();
                h.raw["players"][0]["available"] = json!(false);
                h.capture();
                let descriptor = h.state()["volumes"]["a"].clone();
                assert_eq!(descriptor["enabled"], false);
                assert_eq!(descriptor["muteEnabled"], false);
                assert_eq!(descriptor["muted"], false);
                assert_eq!(descriptor["muteReason"], "Media source or audio stream ownership is no longer current.");
                h.volume("a", 0.3);
                assert!(h.runtime.advance(h.now).is_none());
                h.mute("a", true);
                assert!(h.runtime.advance(h.now).is_none());
            }

            #[test]
            fn fallback_volume_never_reroutes_to_new_mpris_support_but_mute_still_owns_the_stream() {
                for mute in [false, true] {
                    let mut h = stream_volume_harness();
                    if mute { h.mute("a", false); } else { h.volume("a", 0.4); }
                    h.raw["players"][0]["volume"] = json!(0.8);
                    h.capture();
                    if mute {
                        let apply = h.apply();
                        assert_eq!(apply.operation.operation, "mute");
                        assert_eq!(apply.operation.stream_id, Some(42));
                        h.ack(&apply);
                        assert!(h.runtime.failure.is_empty());
                    } else {
                        assert!(h.runtime.advance(h.now).is_none());
                        assert!(h.runtime.failure.contains("not retried"));
                        assert_eq!(h.raw["streams"][0]["volume"], 0.73);
                    }
                }
                let mut h = Harness::new(vec![native("a")], vec![audio(42)]);
                h.volume("a", 0.2);
                h.raw["players"][0]["volume"] = json!(-1);
                h.capture();
                assert!(h.runtime.advance(h.now).is_none(), "MPRIS loss must not redirect an old setter to the stream");
            }

            #[test]
            fn volume_and_mute_share_queue_bounds_deadlines_and_failure_cancellation() {
                let mut h = stream_volume_harness();
                h.action("next", "a", 0.0);
                let first = h.apply();
                for _ in 0..8 { h.volume("a", 0.4); }
                h.mute("a", true);
                assert_eq!(h.runtime.queue.len(), 8);
                assert!(h.runtime.failure.contains("queue is full"));
                h.ack(&first);
                let volume = h.apply();
                h.result(&volume, false, Some("native stream setter failed"));
                assert!(h.runtime.queue.is_empty());
                assert!(h.runtime.advance(h.now).is_none());
                assert!(h.runtime.failure.contains("not retried"));
                for pending in [false, true] {
                    let mut h = stream_volume_harness();
                    h.mute("a", false);
                    let apply = pending.then(|| h.apply());
                    h.volume("a", 0.4);
                    h.now += 5.01;
                    if let Some(apply) = apply { h.result(&apply, true, None); }
                    assert!(h.runtime.advance(h.now).is_none());
                    assert!(h.runtime.failure.contains("expired"));
                    assert!(h.runtime.queue.is_empty());
                }
            }

            #[test]
            fn volume_mutations_reject_replacement_and_ownership_changes_before_and_after_dispatch() {
                for mute in [false, true] {
                    for pending in [false, true] {
                        for change in 0..5 {
                            let mut h = stream_volume_harness();
                            if mute { h.mute("a", false); } else { h.volume("a", 0.4); }
                            let apply = pending.then(|| h.apply());
                            match change {
                                0 => h.raw["players"][0]["instance"] = json!(99),
                                1 => h.raw["streams"][0]["instance"] = json!(99),
                                2 => h.raw["streams"][0]["properties"] = json!({"application.name": "Other owner"}),
                                3 => h.raw["streams"][0]["ready"] = json!(false),
                                _ => h.raw["streams"][0]["volume"] = json!(-1),
                            }
                            if let Some(apply) = apply { h.result(&apply, true, None); } else { h.capture(); }
                            assert!(h.runtime.advance(h.now).is_none(), "mute={mute}, pending={pending}, change={change}");
                            assert!(h.runtime.failure.contains("not retried"));
                            assert!(h.runtime.queue.is_empty());
                        }
                    }
                }
            }

            #[test]
            fn pending_volume_readiness_aba_cannot_revive_a_setter_or_its_queue() {
                let mut h = stream_volume_harness();
                h.volume("a", 0.4);
                let apply = h.apply();
                h.mute("a", false);
                h.raw["players"][0]["volume"] = json!(0.8);
                h.capture();
                h.raw["players"][0]["volume"] = json!(-1);
                h.capture();
                h.result(&apply, true, None);
                assert!(h.runtime.failure.contains("not retried"));
                assert!(h.runtime.queue.is_empty());
                assert!(h.runtime.advance(h.now).is_none());
            }

            #[test]
            fn invalid_volume_input_never_becomes_a_native_operation() {
                let mut h = stream_volume_harness();
                for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
                    h.runtime.enqueue_volume("a".into(), VolumeMutation::Volume(value), h.now, h.now);
                    assert!(h.runtime.queue.is_empty());
                }
                for value in [json!("0.5"), json!(true), Value::Null] {
                    assert!(serde_json::from_value::<ServiceMessage>(json!({"type": "volume", "player": "a", "value": value, "snapshot": h.raw, "createdAt": h.now})).is_err());
                }
                for muted in [json!("true"), json!(0), Value::Null] {
                    assert!(serde_json::from_value::<ServiceMessage>(json!({"type": "mute", "player": "a", "muted": muted, "snapshot": h.raw, "createdAt": h.now})).is_err());
                }
                h.volume("gone", 0.4);
                assert!(h.runtime.advance(h.now).is_none());
                assert!(h.runtime.failure.contains("no longer available"));
                h.message(json!({"type": "volume", "player": "a", "value": 0.4, "createdAt": h.now - 5.01, "snapshot": h.raw}));
                assert!(h.runtime.advance(h.now).is_none());
                assert!(h.runtime.failure.contains("expired"));
            }

            #[test]
            fn expiry_that_changes_browser_grouping_emits_full_state_not_just_progress() {
                let mut browser = native("chrome");
                browser["identity"] = json!("Chromium");
                browser["url"] = json!("");
                browser["position"] = json!(40.0);
                let mut peer = browser.clone();
                peer["bus"] = json!("bridge");
                peer["url"] = json!("https://music.youtube.com/watch?v=track");
                peer["position"] = json!(0.208);
                let mut h = Harness::new(vec![browser, peer], vec![]);
                let mut output = ServiceOutput::default();
                output.update(&h.runtime, h.now, false).unwrap();
                assert_eq!(h.runtime.groups[0].players, ["bridge"]);
                h.now += 5.01;
                h.message(json!({"type": "tick"}));
                let state: Value = serde_json::from_slice(&output.update(&h.runtime, h.now, true).unwrap().unwrap()).unwrap();
                assert_eq!(state["type"], "state");
                assert_eq!(state["groups"][0]["players"], json!(["chrome", "bridge"]));
            }

            #[test]
            fn seek_revision_cannot_revert_after_its_expected_post_dispatch_snapshot() {
                let mut h = Harness::one();
                h.action("seek", "a", 80.0);
                let apply = h.apply();
                h.action("seek", "a", 90.0);
                h.raw["players"][0]["seekRevision"] = json!(1);
                h.capture();
                h.raw["players"][0]["seekRevision"] = json!(0);
                h.capture();
                h.raw["players"][0]["seekRevision"] = json!(1);
                h.result(&apply, true, None);
                assert!(h.runtime.queue.is_empty());
                assert!(h.runtime.failure.contains("not retried"));
                assert!(h.runtime.advance(h.now).is_none());
            }

            #[test]
            fn grouped_partial_apply_failure_discards_remaining_members_and_trailing_intents() {
                let mut h = grouped();
                h.action("seek", "alias", 80.0);
                let first = h.apply();
                h.action("seek", "alias", 90.0);
                h.ack(&first);
                let second = h.apply();
                h.result(&second, false, Some("native setter refused"));
                assert!(h.runtime.queue.is_empty());
                assert!(h.runtime.advance(h.now).is_none());
                assert_eq!(h.raw["players"][0]["seekRevision"], 1);
                assert_eq!(h.raw["players"][1]["seekRevision"], 0);
                h.result(&second, true, None);
                assert!(h.runtime.advance(h.now).is_none(), "a duplicate result is never a retry");
            }

            #[test]
            fn capture_ids_share_the_non_reused_apply_id_sequence() {
                let mut h = Harness::one();
                let first = h.runtime.capture_id(105.0).unwrap();
                assert!(h.runtime.capture_id(105.1).is_none());
                h.action("next", "a", 0.0);
                let apply = h.apply();
                assert!(apply.id > first);
                let next = h.runtime.capture_id(110.0).unwrap();
                assert!(next > apply.id);
            }

            struct ServiceDriver {
                input: std::os::unix::net::UnixStream,
                output: std::io::BufReader<std::os::unix::net::UnixStream>,
                done: std::sync::mpsc::Receiver<crate::AppResult<()>>,
                worker: Option<std::thread::JoinHandle<()>>,
            }

            impl ServiceDriver {
                fn start() -> Self {
                    use std::os::unix::net::UnixStream;
                    use std::time::Duration;
                    let (input, service_input) = UnixStream::pair().unwrap();
                    let (service_output, output) = UnixStream::pair().unwrap();
                    input.set_write_timeout(Some(Duration::from_secs(3))).unwrap();
                    output.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
                    let (sender, done) = std::sync::mpsc::channel();
                    let worker = std::thread::spawn(move || {
                        let result = serve_service_io(std::io::BufReader::new(service_input), service_output, None, None);
                        let _ = sender.send(result);
                    });
                    let mut service = Self { input, output: std::io::BufReader::new(output), done, worker: Some(worker) };
                    assert_eq!(service.line()["type"], "ready");
                    service
                }

                fn send(&mut self, value: Value) {
                    serde_json::to_writer(&mut self.input, &value).unwrap();
                    self.input.write_all(b"\n").unwrap();
                    self.input.flush().unwrap();
                }

                fn line(&mut self) -> Value {
                    let mut line = String::new();
                    assert!(self.output.read_line(&mut line).unwrap() > 0, "unexpected service EOF");
                    assert!(line.len() <= MAX_REQUEST_BYTES);
                    serde_json::from_str(&line).unwrap()
                }

                fn until(&mut self, predicate: impl Fn(&Value) -> bool) -> Value {
                    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
                    loop {
                        let value = self.line();
                        if predicate(&value) { return value; }
                        assert!(std::time::Instant::now() < deadline, "service response deadline expired");
                    }
                }

                fn stop(&mut self) {
                    self.input.shutdown(std::net::Shutdown::Write).unwrap();
                    self.done.recv_timeout(std::time::Duration::from_secs(3)).expect("EOF must close without awaiting apply results").unwrap();
                    self.worker.take().unwrap().join().unwrap();
                }
            }

            impl Drop for ServiceDriver {
                fn drop(&mut self) {
                    let _ = self.input.shutdown(std::net::Shutdown::Write);
                    if let Some(worker) = self.worker.take() {
                        // Closing the read end also releases a writer after a failed assertion.
                        let _ = self.output.get_ref().shutdown(std::net::Shutdown::Read);
                        let _ = worker.join();
                    }
                }
            }

            #[test]
            fn unchanged_snapshots_receive_liveness_acknowledgements_without_ui_updates() {
                let mut service = ServiceDriver::start();
                let mut player = native("a");
                player["playing"] = json!(false);
                let raw = json!({"players": [player], "streams": []});
                service.send(json!({"type": "snapshot", "snapshot": raw}));
                assert_eq!(service.line()["type"], "state");
                for _ in 0..3 {
                    service.send(json!({"type": "snapshot", "snapshot": raw}));
                    assert_eq!(service.line(), json!({"type": "ack"}));
                }
                service.send(json!({"type": "report", "error": ""}));
                assert_eq!(service.line(), json!({"type": "ack"}));
                service.send(json!({"type": "tick"}));
                assert_eq!(service.line()["type"], "progress");
                service.stop();
            }

            #[test]
            fn jsonl_service_roundtrips_state_seek_ack_and_snapshot_free_progress() {
                let mut service = ServiceDriver::start();
                let mut raw = json!({"players": [native("a")], "streams": [audio(42)]});
                raw["players"][0].as_object_mut().unwrap().remove("available");
                service.send(json!({"type": "snapshot", "snapshot": raw}));
                let state = service.until(|v| v["type"] == "state");
                assert_eq!(state["players"][0]["trackKey"], "/track/a");
                assert_eq!(state["groups"][0]["streamId"], 42);
                let created = crate::playback::monotonic_seconds();
                let target = 237.60851756361546_f64;
                service.send(json!({"type": "action", "action": {"kind": "seek", "player": "a", "position": target}, "snapshot": raw, "createdAt": created}));
                let apply = service.until(|v| v["type"] == "apply");
                assert_eq!(apply["operation"]["value"].as_f64().unwrap().to_bits(), target.to_bits());
                let expires_at = apply["expiresAt"].as_f64().unwrap();
                assert!(expires_at > created && expires_at <= created + 5.0, "source freshness can impose an earlier deadline");
                assert_eq!(apply["guard"]["players"][0]["seekRevision"], 0);
                raw["players"][0]["seekRevision"] = json!(1);
                service.send(json!({"type": "result", "id": apply["id"], "ok": true, "snapshot": raw}));
                let state = service.until(|v| v["type"] == "state" && v["players"][0]["seekRevision"] == 1);
                assert_eq!(state["players"][0]["position"], 10.0, "dispatch cannot optimistically modify a sample");
                assert_eq!(state["progress"][0]["positionSupported"], false);
                service.send(json!({"type": "tick"}));
                let progress = service.until(|v| v["type"] == "progress");
                assert_eq!(progress["progress"][0]["samplePosition"], Value::Null);
                raw["players"][0]["position"] = json!(target);
                raw["players"][0]["positionRevision"] = json!(1);
                service.send(json!({"type": "snapshot", "snapshot": raw}));
                let state = service.until(|v| v["type"] == "state" && v["players"][0]["positionRevision"] == 1);
                assert_eq!(state["progress"][0]["samplePosition"].as_f64().unwrap().to_bits(), target.to_bits());
                service.stop();
            }

            #[test]
            fn jsonl_service_roundtrips_owned_stream_volume_and_mute_with_helper_alive() {
                let mut service = ServiceDriver::start();
                let h = stream_volume_harness();
                let mut raw = h.raw;
                service.send(json!({"type": "snapshot", "snapshot": raw}));
                let state = service.until(|v| v["type"] == "state");
                assert_eq!(state["volumes"]["a"]["key"], "stream:42");
                service.send(json!({"type": "volume", "player": "a", "value": 0.47,
                    "createdAt": crate::playback::monotonic_seconds(), "snapshot": raw}));
                let apply = service.until(|v| v["type"] == "apply");
                assert_eq!(apply["operation"], json!({"player": "a", "operation": "volume", "streamId": 42, "value": 0.47, "muted": false}));
                raw["streams"][0]["volume"] = json!(0.47);
                raw["streams"][0]["muted"] = json!(false);
                service.send(json!({"type": "result", "id": apply["id"], "ok": true, "snapshot": raw}));
                let state = service.until(|v| v["type"] == "state" && v["volumes"]["a"]["value"] == 0.47);
                assert_eq!(state["volumes"]["a"]["muted"], false);
                assert_eq!(state["players"][0]["seekRevision"], 0);
                service.send(json!({"type": "mute", "player": "a", "muted": true,
                    "createdAt": crate::playback::monotonic_seconds(), "snapshot": raw}));
                let apply = service.until(|v| v["type"] == "apply");
                assert_eq!(apply["operation"], json!({"player": "a", "operation": "mute", "streamId": 42, "muted": true}));
                raw["streams"][0]["muted"] = json!(true);
                service.send(json!({"type": "result", "id": apply["id"], "ok": true, "snapshot": raw}));
                let state = service.until(|v| v["type"] == "state" && v["volumes"]["a"]["muted"] == true);
                assert_eq!(state["volumes"]["a"]["value"], 0.47);
                assert_eq!(state["players"][0]["volume"].as_f64(), Some(-1.0));
                assert_eq!(state["failure"], "");
                service.stop();
            }

            #[test]
            fn jsonl_service_keeps_serving_invalid_utf8_json_and_oversized_lines_without_leaking_input() {
                let mut service = ServiceDriver::start();
                for invalid in [b"{\"private-secret-\\u001b\":\n".as_slice(), b"\xff\n".as_slice()] {
                    service.input.write_all(invalid).unwrap();
                    let state = service.until(|v| v["type"] == "state" && v["failure"].as_str().is_some_and(|s| s.contains("Invalid media service JSON")));
                    assert!(!state.to_string().contains("private-secret"));
                    assert_eq!(state["groups"], json!([]));
                }
                service.input.write_all(&vec![b'x'; MAX_REQUEST_BYTES + 1]).unwrap();
                service.input.write_all(b"\n").unwrap();
                let state = service.until(|v| v["type"] == "state" && v["failure"].as_str().is_some_and(|s| s.contains("1 MiB")));
                assert_eq!(state["ready"], true);
                service.send(json!({"type": "snapshot", "snapshot": {"players": [native("a")], "streams": []}}));
                let state = service.until(|v| v["type"] == "state" && v["activePlayer"] == "a");
                assert_eq!(state["players"][0]["title"], "Track a");
                service.stop();
            }

            #[test]
            fn jsonl_service_eof_closes_with_an_outstanding_apply_and_queued_mutations() {
                let mut service = ServiceDriver::start();
                let raw = json!({"players": [native("a")], "streams": []});
                let action = |position| json!({"type": "action", "action": {"kind": "seek", "player": "a", "position": position}, "createdAt": crate::playback::monotonic_seconds(), "snapshot": raw});
                service.send(action(20.0));
                let first = service.until(|v| v["type"] == "apply");
                assert_eq!(first["operation"]["value"], 20.0);
                service.send(action(30.0));
                service.send(action(40.0));
                service.stop();
                let mut line = String::new();
                while service.output.read_line(&mut line).unwrap() != 0 {
                    let value: Value = serde_json::from_str(&line).unwrap();
                    assert_ne!(value["type"], "apply", "no pending intent may dispatch after EOF");
                    line.clear();
                }
            }

            #[test]
            fn bounded_serialization_never_emits_oversized_partial_states_or_private_errors() {
                let mut h = Harness::one();
                let bytes = service_bytes(&h.runtime.state(h.now)).unwrap();
                assert!(bytes.len() <= MAX_REQUEST_BYTES);
                assert_eq!(bytes.last(), Some(&b'\n'));
                h.raw["players"][0]["title"] = json!("x".repeat(MAX_REQUEST_BYTES / 3));
                h.capture();
                assert!(service_bytes(&h.runtime.state(h.now)).is_err());
                h.runtime.protocol_failure("Media service state exceeds the 1 MiB limit.");
                let safe: Value = serde_json::from_slice(&service_bytes(&h.runtime.state(h.now)).unwrap()).unwrap();
                assert_eq!(safe["groups"], json!([]));
                assert_eq!(safe["guard"]["players"], json!([]));
                assert!(safe["failure"].as_str().unwrap().contains("1 MiB"));
                assert_eq!(failure_text("Bad\n\u{001b}native error"), "Badnative error");
            }
        }
    };
    (playback) => {
        mod tests {
            use super::*;

            fn player(bus: &str, source: &str, title: &str, artist: &str, album: &str) -> Player {
                Player {
                    bus: String::from(bus),
                    identity: String::from(source),
                    title: String::from(title),
                    artist: String::from(artist),
                    album: String::from(album),
                    length: Some(203.0),
                    playing: true,
                    ..Player::default()
                }
            }

            fn elisa() -> Player {
                player(
                    "org.mpris.MediaPlayer2.elisa",
                    "Elisa",
                    "Tomboy",
                    "Destiny Rogers",
                    "Tomboy",
                )
            }

            #[test]
            fn one_app_publishing_itself_twice_needs_one_further_field() {
                let first = player(
                    "org.mpris.MediaPlayer2.mprisence_web.youtube_music.pa808",
                    "YouTube Music",
                    "Back To Sleep",
                    "Chris Brown",
                    "Royalty (Deluxe Version)",
                );
                let arriving = Player {
                    bus: String::from("org.mpris.MediaPlayer2.mprisence_web.youtube_music.p2911"),
                    album: String::new(),
                    length: None,
                    ..first.clone()
                };

                assert!(same_playback(&first, &arriving));
            }

            #[test]
            fn a_mirror_naming_itself_differently_is_asked_for_more() {
                let elisa = elisa();
                let thin_mirror = Player {
                    bus: String::from("org.mpris.MediaPlayer2.other"),
                    identity: String::from("YouTube Music"),
                    album: String::new(),
                    length: None,
                    ..elisa.clone()
                };
                assert!(!same_playback(&elisa, &thin_mirror));

                let full_mirror = Player {
                    album: elisa.album.clone(),
                    length: elisa.length,
                    ..thin_mirror
                };
                assert!(same_playback(&elisa, &full_mirror));
            }

            #[test]
            fn different_tracks_stay_apart() {
                assert!(!same_playback(
                    &elisa(),
                    &player(
                        "org.mpris.MediaPlayer2.mprisence_web.youtube_music.pa808",
                        "YouTube Music",
                        "ANGEL (Visualizer)",
                        "keshi",
                        "",
                    ),
                ));
            }

            #[test]
            fn a_shared_title_alone_is_not_enough() {
                let left = player("a", "Elisa", "Everlong", "Foo Fighters", "The Colour");
                let right = Player {
                    bus: String::from("b"),
                    identity: String::from("Chromium"),
                    artist: String::new(),
                    album: String::new(),
                    length: Some(600.0),
                    ..left.clone()
                };

                assert!(!same_playback(&left, &right));
            }

            #[test]
            fn two_players_at_different_points_are_separate() {
                let early = Player {
                    position: 6.0,
                    ..elisa()
                };
                let late = Player {
                    bus: String::from("b"),
                    position: 120.0,
                    ..elisa()
                };

                assert!(!same_playback(&early, &late));
            }

            #[test]
            fn an_unread_position_of_zero_is_not_disagreement() {
                let watched = Player {
                    position: 92.0,
                    ..elisa()
                };
                let unread = Player {
                    bus: String::from("b"),
                    ..elisa()
                };

                assert!(same_playback(&watched, &unread));
                assert!(same_playback(&unread, &watched));
            }

            #[test]
            fn position_tolerance_is_five_seconds_inclusive() {
                let left = Player {
                    position: 10.0,
                    ..elisa()
                };
                let mut right = left.clone();
                right.position = 15.0;
                assert!(same_playback(&left, &right));
                right.position = 15.001;
                assert!(!same_playback(&left, &right));
            }

            #[test]
            fn an_untitled_player_never_merges() {
                for title in ["", " \t\n "] {
                    let blank = Player {
                        title: String::from(title),
                        ..elisa()
                    };
                    assert!(!same_playback(&blank, &blank));
                }
            }

            #[test]
            fn metadata_and_source_matching_normalise_whitespace_and_case() {
                let left = elisa();
                let right = Player {
                    identity: String::from(" ELISA\t"),
                    title: String::from(" \tTOMBOY\n "),
                    artist: String::from(" destiny   ROGERS "),
                    album: String::new(),
                    length: None,
                    ..left.clone()
                };
                assert!(same_playback(&left, &right));
            }

            #[test]
            fn each_nonempty_supporting_field_can_supply_the_agreement() {
                let mut left = player("a", "App", "Track", "", "");
                left.length = None;
                let mut right = left.clone();
                right.bus = String::from("b");
                assert!(!same_playback(&left, &right));

                for field in ["artist", "album", "art", "url", "length"] {
                    let mut left = left.clone();
                    let mut right = right.clone();
                    match field {
                        "artist" => {
                            left.artist = String::from(" Artist ");
                            right.artist = String::from("ARTIST");
                        }
                        "album" => {
                            left.album = String::from(" Album ");
                            right.album = String::from("ALBUM");
                        }
                        "art" => {
                            left.art_url = String::from(" https://example/cover.jpg ");
                            right.art_url = String::from("https://example/cover.jpg");
                        }
                        "url" => {
                            left.url = String::from(" https://example/track ");
                            right.url = String::from("https://example/track");
                        }
                        "length" => {
                            left.length = Some(203.0);
                            right.length = Some(205.0);
                        }
                        _ => unreachable!(),
                    }
                    assert!(same_playback(&left, &right), "same-source {field}");
                    right.identity = String::from("Other app");
                    assert!(!same_playback(&left, &right), "cross-source {field}");
                }
            }

            #[test]
            fn matching_support_does_not_require_artist_equality() {
                let left = elisa();
                let right = Player {
                    identity: String::from("Other app"),
                    artist: String::from("Different artist"),
                    ..left.clone()
                };
                // Album and length agree even though artist does not.
                assert!(same_playback(&left, &right));
            }

            #[test]
            fn unknown_text_or_length_never_counts_as_agreement() {
                assert!(!same_text("", ""));
                assert!(!same_text("cover", ""));
                assert!(!same_text(" \t", "\n"));
                for length in [
                    None,
                    Some(0.0),
                    Some(-1.0),
                    Some(f64::NAN),
                    Some(f64::INFINITY),
                ] {
                    assert!(!near_length(length, length));
                    assert!(!near_length(Some(203.0), length));
                }
                assert!(near_length(Some(203.0), Some(205.0)));
                assert!(!near_length(Some(203.0), Some(205.001)));
            }

            #[test]
            fn friendly_source_drops_a_chromium_instance_suffix() {
                assert_eq!(
                    friendly_source("org.mpris.MediaPlayer2.chromium.instance1234"),
                    "Chromium"
                );
                assert_eq!(friendly_source("org.mpris.MediaPlayer2.elisa"), "Elisa");
            }

            #[test]
            fn friendly_source_names_the_app_behind_an_mprisence_tab() {
                assert_eq!(
                    friendly_source("org.mpris.MediaPlayer2.mprisence_web.spotify.p2911"),
                    "Spotify"
                );
                assert_eq!(
                    friendly_source("org.mpris.MediaPlayer2.mprisence_web.youtube_music.pa8082085c72e81fe"),
                    "Youtube Music"
                );
                // An app segment that merely starts with `p` is not a tab id.
                assert_eq!(
                    friendly_source("org.mpris.MediaPlayer2.mprisence.pandora"),
                    "Pandora"
                );
            }

            #[test]
            fn a_published_identity_wins_unless_it_is_the_bus_name() {
                assert!(!echoes_bus_name("YouTube Music"));
                assert!(!echoes_bus_name("Elisa"));
                assert!(!echoes_bus_name("Haruna"));
                assert!(echoes_bus_name("mprisence_web.spotify.p2911"));

                let mut player = elisa();
                player.identity = String::from("Haruna");
                assert_eq!(source_name(&player), "Haruna");
                for identity in ["", " \t ", "org.mpris.MediaPlayer2.elisa"] {
                    player.identity = String::from(identity);
                    assert_eq!(source_name(&player), "Elisa");
                }
            }

            #[test]
            fn unusable_identities_use_friendly_names_for_agreement_too() {
                let left = Player {
                    bus: String::from("org.mpris.MediaPlayer2.mprisence_web.youtube_music.pa808"),
                    identity: String::new(),
                    ..elisa()
                };
                let right = Player {
                    bus: String::from("org.mpris.MediaPlayer2.mprisence_web.youtube_music.p2911"),
                    identity: String::from("mprisence_web.youtube_music.p2911"),
                    album: String::new(),
                    length: None,
                    ..left.clone()
                };
                assert!(same_playback(&left, &right));
            }

            #[test]
            fn only_actual_instance_suffixes_are_removed() {
                for suffix in ["instance1234", "pa8082085c72e81fe", "1234"] {
                    assert!(is_instance(suffix), "{suffix}");
                }
                for suffix in ["instance", "instanceabc", "p", "pandora", "p12g", ""] {
                    assert!(!is_instance(suffix), "{suffix}");
                }
                assert_eq!(
                    friendly_source("org.mpris.MediaPlayer2.player.1234"),
                    "Player"
                );
                assert_eq!(
                    friendly_source("org.mpris.MediaPlayer2.my-player"),
                    "My Player"
                );
            }
        }
    };
}

use media_control::AppResult;

#[path = "artwork.rs"]
pub mod artwork;
#[path = "credit.rs"]
pub mod credit;
#[path = "model.rs"]
pub mod model;
#[path = "playback.rs"]
pub mod playback;

mod media_bridge {
    //! Exercise the frontend-independent helper processes without a GUI or desktop services.

    use std::io::{self, BufRead, BufReader, Read, Write};
    use std::process::{Child, ChildStdin, Command, Output, Stdio};
    use std::sync::mpsc::{self, Receiver};
    use std::thread::{self, JoinHandle};
    use std::time::{Duration, Instant};

    use media_control::model::MAX_REQUEST_BYTES;
    use serde_json::{Value, json};

    const PROCESS_TIMEOUT: Duration = Duration::from_secs(5);

    struct Helper {
        child: Child,
        input: Option<ChildStdin>,
        lines: Receiver<io::Result<Vec<u8>>>,
        stdout: Option<JoinHandle<()>>,
        stderr: Option<JoinHandle<Vec<u8>>>,
    }

    impl Helper {
        fn start(command: &str) -> Self {
            let mut child = Command::new(env!("CARGO_BIN_EXE_media-control"))
                .arg(command)
                .env_clear()
                .env("PATH", "")
                .env("DBUS_SESSION_BUS_ADDRESS", "disabled:")
                .env("DBUS_SYSTEM_BUS_ADDRESS", "disabled:")
                .env("PULSE_SERVER", "disabled:")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .expect("start the real media-control binary");
            let input = child.stdin.take();
            let mut output = BufReader::new(child.stdout.take().unwrap());
            let (sender, lines) = mpsc::channel();
            let stdout = thread::spawn(move || {
                loop {
                    let mut line = Vec::new();
                    match output.read_until(b'\n', &mut line) {
                        Ok(0) => break,
                        Ok(_) => {
                            if sender.send(Ok(line)).is_err() {
                                break;
                            }
                        }
                        Err(error) => {
                            let _ = sender.send(Err(error));
                            break;
                        }
                    }
                }
            });
            let mut errors = child.stderr.take().unwrap();
            let stderr = thread::spawn(move || {
                let mut bytes = Vec::new();
                errors.read_to_end(&mut bytes).unwrap();
                bytes
            });
            Self {
                child,
                input,
                lines,
                stdout: Some(stdout),
                stderr: Some(stderr),
            }
        }

        fn write(&mut self, bytes: &[u8]) {
            let input = self.input.as_mut().unwrap();
            input.write_all(bytes).unwrap();
            input.flush().unwrap();
        }

        fn reply(&self) -> Value {
            let line = self
                .lines
                .recv_timeout(PROCESS_TIMEOUT)
                .expect("helper must flush one reply without waiting for stdin EOF")
                .expect("read helper stdout");
            assert_eq!(line.last(), Some(&b'\n'), "reply must be JSONL");
            serde_json::from_slice(&line).expect("reply must be one JSON object")
        }

        fn request(&mut self, request: &Value) -> Value {
            self.write(format!("{request}\n").as_bytes());
            self.reply()
        }

        fn close_input(&mut self) {
            self.input.take();
        }

        fn finish(mut self) -> Output {
            self.close_input();
            let deadline = Instant::now() + PROCESS_TIMEOUT;
            let status = loop {
                if let Some(status) = self.child.try_wait().unwrap() {
                    break status;
                }
                assert!(
                    Instant::now() < deadline,
                    "helper did not exit after stdin EOF"
                );
                thread::sleep(Duration::from_millis(5));
            };
            self.stdout.take().unwrap().join().unwrap();
            Output {
                status,
                stdout: self.lines.try_iter().flat_map(Result::unwrap).collect(),
                stderr: self.stderr.take().unwrap().join().unwrap(),
            }
        }

        fn finish_success(self) {
            let output = self.finish();
            assert!(output.status.success(), "{output:?}");
            assert!(
                output.stdout.is_empty(),
                "unexpected extra replies: {output:?}"
            );
            assert!(
                output.stderr.is_empty(),
                "unexpected diagnostics: {output:?}"
            );
        }
    }

    impl Drop for Helper {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }

    fn player(bus: &str, application: &str, title: &str) -> Value {
        json!({
            "bus": bus, "identity": application, "desktopEntry": application,
            "title": title, "artist": "Artist 雪", "album": "Album",
            "url": "", "artUrl": "", "playing": true, "stopped": false,
            "position": 12.0, "length": 203.0,
            "canSeek": true, "canNext": true, "canPrevious": true
        })
    }

    fn stream(id: u32, application: &str, title: &str) -> Value {
        json!({
            "id": id, "name": application, "nickname": "", "ready": true,
            "properties": {"application.name": application, "media.name": title}
        })
    }

    fn snapshot(players: Vec<Value>, streams: Vec<Value>) -> Value {
        json!({"requestId": 1, "selectedPlayer": "", "players": players, "streams": streams})
    }

    fn stream_ids(reply: &Value) -> Vec<Value> {
        reply["groups"]
            .as_array()
            .unwrap()
            .iter()
            .map(|group| group["streamId"].clone())
            .collect()
    }

    #[test]
    fn resident_model_roundtrips_groups_selection_volume_and_native_actions() {
        let mut helper = Helper::start("model");
        let mut request = snapshot(
            vec![
                player("native", "Native", "Track \"A\""),
                player("alias", "Native", "Track \"A\""),
                player("other", "Other", "Track B"),
            ],
            vec![stream(42, "Native", "Track \"A\"")],
        );
        request["selectedPlayer"] = json!("alias");
        request["players"][1]["playing"] = json!(false);
        let reply = helper.request(&request);
        assert_eq!(
            reply,
            json!({
                "requestId": 1, "activePlayer": "native", "actions": [],
                "groups": [
                    {"title": "Track \"A\"", "primary": "native",
                     "players": ["native", "alias"], "displayName": "Native",
                     "streamId": 42, "streamReason": "Unique application playback stream"},
                    {"title": "Track B", "primary": "other",
                     "players": ["other"], "displayName": "Other",
                     "streamId": null,
                     "streamReason": "No matching audio stream. Start playback in this source."}
                ]
            })
        );

        for (index, (action, expected)) in [
            (
                json!({"kind": "toggle", "player": "alias"}),
                json!([{"player": "native", "operation": "pause"},
                   {"player": "alias", "operation": "pause"}]),
            ),
            (
                json!({"kind": "next", "player": "alias"}),
                json!([{"player": "native", "operation": "next"}]),
            ),
            (
                json!({"kind": "previous", "player": "alias"}),
                json!([{"player": "native", "operation": "previous"}]),
            ),
            (
                json!({"kind": "seek", "player": "alias", "position": 42.25}),
                json!([{"player": "native", "operation": "seek", "value": 42.25},
                   {"player": "alias", "operation": "seek", "value": 42.25}]),
            ),
            (
                json!({"kind": "pauseAll"}),
                json!([{"player": "native", "operation": "pause"},
                   {"player": "other", "operation": "pause"}]),
            ),
        ]
        .into_iter()
        .enumerate()
        {
            request["requestId"] = json!(index + 2);
            request["action"] = action;
            let reply = helper.request(&request);
            assert_eq!(reply["requestId"], index + 2);
            assert_eq!(reply["activePlayer"], "native");
            assert_eq!(reply["actions"], expected);
            assert!(reply.get("error").is_none());
        }
        for player in request["players"].as_array_mut().unwrap() {
            player["playing"] = json!(false);
        }
        request["action"] = json!({"kind": "togglePauseAll"});
        assert_eq!(
            helper.request(&request)["actions"],
            json!([{"player": "native", "operation": "play"},
               {"player": "alias", "operation": "play"}])
        );
        request["action"] = json!({"kind": "next", "player": "gone"});
        let reply = helper.request(&request);
        assert_eq!(reply["actions"], json!([]));
        assert!(
            reply["error"]
                .as_str()
                .unwrap()
                .contains("no longer available")
        );
        helper.finish_success();
    }

    #[test]
    fn volume_matching_uses_only_the_current_ready_streams_without_serial_selectors() {
        let mut helper = Helper::start("model");
        let mut request = snapshot(vec![player("native", "Native", "Track")], vec![]);
        let mut unready = stream(42, "Native", "Track");
        unready["ready"] = json!(false);
        for (streams, expected, reason) in [
            (json!([]), Value::Null, "No matching"),
            (json!([stream(42, "Native", "Track")]), json!(42), "Unique"),
            (json!([unready]), Value::Null, "Reading"),
            (
                json!([stream(42, "Native", "Track"), stream(43, "Native", "Track")]),
                Value::Null,
                "Multiple",
            ),
            (json!([stream(43, "Native", "Track")]), json!(43), "Unique"),
            (
                json!([stream(0, "Native", "Track")]),
                Value::Null,
                "Reading",
            ),
            (
                json!([stream(43, "Other", "Track")]),
                Value::Null,
                "No matching",
            ),
            (json!([]), Value::Null, "No matching"),
        ] {
            request["streams"] = streams;
            let reply = helper.request(&request);
            assert_eq!(stream_ids(&reply), [expected]);
            assert!(
                reply["groups"][0]["streamReason"]
                    .as_str()
                    .unwrap()
                    .contains(reason)
            );
            assert!(reply.get("error").is_none());
        }
        helper.finish_success();
    }

    #[test]
    fn shared_browser_volume_requires_application_scoped_title_or_url_evidence() {
        let mut helper = Helper::start("model");
        let mut a = player("tab-a", "Chromium", "Track A");
        let mut b = player("tab-b", "Chromium", "Track B");
        a["url"] = json!("https://www.youtube.com/watch?v=aaaaaaaaaaa");
        b["url"] = json!("https://www.youtube.com/watch?v=bbbbbbbbbbb");
        b["playing"] = json!(false);
        let mut request = snapshot(vec![a, b], vec![stream(1, "Chrome", "Track A")]);
        assert_eq!(
            stream_ids(&helper.request(&request)),
            [Value::Null, Value::Null]
        );

        let mut second = stream(2, "Chrome", "Unknown");
        second["properties"]["media.url"] = json!(" https://www.youtube.com/watch?v=bbbbbbbbbbb ");
        request["streams"].as_array_mut().unwrap().push(second);
        assert_eq!(stream_ids(&helper.request(&request)), [json!(1), json!(2)]);
        request["streams"].as_array_mut().unwrap().reverse();
        assert_eq!(stream_ids(&helper.request(&request)), [json!(1), json!(2)]);
        request["streams"][0]["properties"]["media.url"] = json!("");
        assert_eq!(
            stream_ids(&helper.request(&request)),
            [Value::Null, Value::Null]
        );
        helper.finish_success();
    }

    #[test]
    fn url_less_native_browser_and_generic_companion_keep_live_snapshot_shape() {
        let native_bus = "org.mpris.MediaPlayer2.chromium.instance171244";
        let companion_bus = "org.mpris.MediaPlayer2.mprisence_web.generic.pa808";
        let mut native = player(native_bus, "Chrome", "Nothing Compares - Bonus Track");
        native["artist"] = json!("The Weeknd");
        native["album"] = json!("After Hours (Deluxe)");
        native["position"] = json!(121.125);
        native["length"] = json!(222.353378);
        native["canPlay"] = json!(true);
        native["canPause"] = json!(true);
        let mut companion = native.clone();
        companion["bus"] = json!(companion_bus);
        companion["identity"] = json!("Generic Companion");
        companion["desktopEntry"] = json!("");
        companion["browserHint"] = json!(true);
        companion["url"] = json!("https://open.spotify.com/album/4ZyeHIgdujClXSjC2cSqSb");
        companion["position"] = json!(0.0);
        companion["length"] = json!(222.353);
        companion["canPlay"] = json!(false);
        companion["canPause"] = json!(false);
        companion["canSeek"] = json!(false);
        let mut helper = Helper::start("model");
        for reversed in [false, true] {
            let mut players = vec![native.clone(), companion.clone()];
            if reversed {
                players.reverse();
            }
            let members: Vec<_> = players.iter().map(|p| p["bus"].clone()).collect();
            let mut request = snapshot(players, vec![]);
            for selected in [native_bus, companion_bus] {
                request["selectedPlayer"] = json!(selected);
                let reply = helper.request(&request);
                assert!(reply.get("error").is_none());
                assert_eq!(reply["activePlayer"], native_bus);
                assert_eq!(
                    reply["groups"],
                    json!([{
                        "title": "Nothing Compares - Bonus Track",
                        "primary": native_bus, "players": members,
                        "displayName": "Spotify", "streamId": null,
                        "streamReason": "No matching audio stream. Start playback in this source."
                    }])
                );
            }
        }
        helper.finish_success();
    }

    #[test]
    fn grouping_accepts_native_positions_and_zero_for_unsupported_positions() {
        let mut helper = Helper::start("model");
        let mut request = snapshot(
            vec![
                player("a", "Native", "Track"),
                player("b", "Native", "Track"),
            ],
            vec![],
        );
        for (a, b, groups) in [(0.0, 120.0, 1), (10.0, 16.0, 2), (10.0, 15.0, 1)] {
            request["players"][0]["position"] = json!(a);
            request["players"][1]["position"] = json!(b);
            let reply = helper.request(&request);
            assert_eq!(reply["groups"].as_array().unwrap().len(), groups);
            assert!(reply.get("error").is_none());
        }
        request["players"][0]["position"] = json!(-1.0);
        request["action"] = json!({"kind": "toggle"});
        let reply = helper.request(&request);
        assert_eq!(reply["requestId"], 1);
        assert_eq!(reply["groups"], json!([]));
        assert_eq!(reply["actions"], json!([]));
        assert!(reply["error"].as_str().unwrap().contains("nonnegative"));
        request["players"][0]["position"] = json!(0.0);
        assert!(helper.request(&request).get("error").is_none());
        helper.finish_success();
    }

    #[test]
    fn model_recovers_from_invalid_and_oversized_lines_and_serves_final_unterminated_json() {
        let mut helper = Helper::start("model");
        for invalid in [
        b"{\"requestId\":\"private-token-\\u001b\"}\n".as_slice(),
        b"\xff\n",
        b"\n",
        b"{\"requestId\":1,\"players\":[],\"streams\":[],\"action\":{\"kind\":\"seek\",\"position\":1e999}}\n",
    ] {
        helper.write(invalid);
        let reply = helper.reply();
        assert_eq!(reply["requestId"], 0);
        assert_eq!(reply["actions"], json!([]));
        assert!(reply["error"].as_str().unwrap().starts_with("Invalid JSON request"));
        assert!(!reply.to_string().contains("private-token"));
    }
        let mut oversized = vec![b'x'; MAX_REQUEST_BYTES + 1];
        oversized.push(b'\n');
        helper.write(&oversized);
        let reply = helper.reply();
        assert_eq!(reply["requestId"], 0);
        assert!(reply["error"].as_str().unwrap().contains("1 MiB"));

        let mut request = snapshot(vec![], vec![]);
        request["requestId"] = json!(71);
        helper.write(format!("{request}\r\n").as_bytes());
        assert_eq!(helper.reply()["requestId"], 71);
        request["requestId"] = json!(72);
        helper.write(request.to_string().as_bytes());
        helper.close_input();
        assert_eq!(helper.reply()["requestId"], 72);
        helper.finish_success();
    }

    #[test]
    fn removed_media_move_action_is_rejected_without_a_routing_protocol() {
        let mut helper = Helper::start("model");
        let mut request = snapshot(
            vec![player("native", "Native", "Track")],
            vec![stream(42, "Native", "Track")],
        );
        request["action"] =
            json!({"kind": "move", "player": "native", "destination": "old-output"});
        let reply = helper.request(&request);
        assert_eq!(reply["error"], "Unknown media action.");
        assert_eq!(reply["actions"], json!([]));
        assert!(reply.get("route").is_none());
        for field in ["selector", "routingReason", "destination"] {
            assert!(reply["groups"][0].get(field).is_none());
        }
        assert_eq!(reply["groups"][0]["streamId"], 42);
        helper.finish_success();
    }

    #[test]
    fn credit_process_echoes_track_identity_and_local_metadata_without_desktop_or_network() {
        for (request, credit) in [
            (
                json!({"url": "file:///music/track-a.ogg", "artist": "Artist \"A\" 雪", "album": "Album A"}),
                "Artist \"A\" 雪 · Album A",
            ),
            (
                json!({"url": "file:///music/track-b.ogg", "artist": "", "album": "Album B"}),
                "Album B",
            ),
            (json!({"url": "", "artist": "", "album": ""}), ""),
        ] {
            let mut helper = Helper::start("credit");
            helper.write(request.to_string().as_bytes());
            helper.close_input();
            let mut expected = request;
            expected["credit"] = json!(credit);
            assert_eq!(helper.reply(), expected);
            helper.finish_success();
        }
    }

    #[test]
    fn invalid_credit_input_fails_without_echoing_private_metadata() {
        let mut helper = Helper::start("credit");
        helper.write(b"{\"url\":\"private-token\",\"artist\":7,\"album\":\"secret\"}");
        let output = helper.finish();
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        let error = String::from_utf8(output.stderr).unwrap();
        assert!(error.contains("invalid media credit JSON"));
        assert!(!error.contains("private-token") && !error.contains("secret"));
    }
}

mod live_panel_ipc {
    //! Opt-in, read-only lifecycle check against an already running Linux desktop panel.
    //!
    //! Run from the repository root:
    //! ```text
    //! cargo test --manifest-path scripts/Cargo.toml --package media-control --locked --test tests live_panel_ipc_lifecycle -- --ignored --nocapture
    //! ```
    //! Requires an active `ags-media.service`, readable cgroup v2/procfs data, and
    //! `media-panel` on PATH. `MEDIA_PANEL_LAUNCHER` can select another launcher
    //! executable (not a shell command); `MEDIA_PANEL_IPC_REQUESTS` must be a positive
    //! integer and defaults to 1200. Only `status` is requested, through the launcher's
    //! native AGS IPC path; no playback/UI commands or service mutations are performed.
    //! Each subprocess/output read has a shared five-second deadline. Progress is
    //! printed at least every 100 requests or five seconds between requests.
    //! PID/start-time changes fail the check. After a one-second settle, connected
    //! panel socket growth must be at most two and total FD growth at most eight.

    use std::collections::HashSet;
    use std::env;
    use std::ffi::OsStr;
    use std::fs;
    use std::io::{self, Read};
    use std::os::unix::fs::MetadataExt;
    use std::path::{Path, PathBuf};
    use std::process::{Child, Command, Stdio};
    use std::sync::mpsc::{self, Receiver};
    use std::thread;
    use std::time::{Duration, Instant};

    use super::AppResult;

    const SERVICE: &str = "ags-media.service";
    const COMMAND_TIMEOUT: Duration = Duration::from_secs(5);

    struct CommandChild(Child);

    impl Drop for CommandChild {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    fn read_pipe(mut pipe: impl Read + Send + 'static) -> Receiver<io::Result<Vec<u8>>> {
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let mut bytes = Vec::new();
            let result = pipe.read_to_end(&mut bytes).map(|_| bytes);
            let _ = sender.send(result);
        });
        receiver
    }

    fn command(executable: &OsStr, args: &[&str]) -> AppResult<Vec<u8>> {
        let mut command = Command::new(executable);
        command.args(args);
        let label = format!("{command:?}");
        let deadline = Instant::now() + COMMAND_TIMEOUT;
        let mut child = CommandChild(
            command
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .map_err(|error| format!("start {label}: {error}"))?,
        );
        let stdout = read_pipe(child.0.stdout.take().unwrap());
        let stderr = read_pipe(child.0.stderr.take().unwrap());
        let status = loop {
            if let Some(status) = child.0.try_wait()? {
                break status;
            }
            if Instant::now() >= deadline {
                return Err(format!("{label} timed out after {COMMAND_TIMEOUT:?}").into());
            }
            thread::sleep(Duration::from_millis(5));
        };
        let stdout = stdout
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .map_err(|error| format!("{label} stdout deadline: {error}"))??;
        let stderr = stderr
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .map_err(|error| format!("{label} stderr deadline: {error}"))??;
        if !status.success() {
            return Err(format!(
                "{label} exited with {status}: {}",
                String::from_utf8_lossy(&stderr)
            )
            .into());
        }
        Ok(stdout)
    }

    fn service_cgroup() -> AppResult<PathBuf> {
        let output = String::from_utf8(command(
            OsStr::new("systemctl"),
            &[
                "--user",
                "show",
                SERVICE,
                "--property=ActiveState",
                "--property=ControlGroup",
            ],
        )?)?;
        let active = output.lines().any(|line| line == "ActiveState=active");
        let group = output
            .lines()
            .find_map(|line| line.strip_prefix("ControlGroup="))
            .filter(|group| group.starts_with('/') && *group != "/");
        match group {
            Some(group) if active => Ok(PathBuf::from(format!("/sys/fs/cgroup{group}"))),
            _ => Err(format!("{SERVICE} must be active with a non-root ControlGroup").into()),
        }
    }

    fn gjs_pid(cgroup: &Path) -> AppResult<u32> {
        let mut candidates = Vec::new();
        for pid in fs::read_to_string(cgroup.join("cgroup.procs"))?.split_whitespace() {
            let pid: u32 = pid.parse()?;
            match fs::read_to_string(format!("/proc/{pid}/comm")) {
                Ok(comm) if matches!(comm.trim(), "gjs" | "gjs-console") => candidates.push(pid),
                Ok(_) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        if candidates.len() != 1 {
            return Err(format!(
                "expected one GJS process in {}, found {candidates:?}",
                cgroup.display()
            )
            .into());
        }
        Ok(candidates[0])
    }

    /// Field 22 is starttime; comm may itself contain spaces and parentheses.
    fn process_start(pid: u32) -> AppResult<u64> {
        let path = format!("/proc/{pid}/stat");
        let stat = fs::read_to_string(&path).map_err(|error| format!("read {path}: {error}"))?;
        let (_, fields) = stat.rsplit_once(')').ok_or("invalid /proc stat comm")?;
        let mut fields = fields.split_whitespace();
        let state = fields.next().ok_or("missing /proc process state")?;
        if matches!(state, "Z" | "X" | "x") {
            return Err(format!("PID {pid} is no longer alive ({state})").into());
        }
        Ok(fields.nth(18).ok_or("missing /proc starttime")?.parse()?)
    }

    fn same_process(cgroup: &Path, pid: u32, start: u64) -> AppResult<()> {
        if gjs_pid(cgroup)? != pid || process_start(pid)? != start {
            return Err(
                format!("{SERVICE} GJS process (PID {pid}) was replaced during the test").into(),
            );
        }
        Ok(())
    }

    struct Descriptors {
        connected: usize,
        total: usize,
    }

    fn descriptors(pid: u32, socket_path: &str) -> AppResult<Descriptors> {
        let mut owned = HashSet::new();
        let mut total = 0;
        for entry in fs::read_dir(format!("/proc/{pid}/fd"))? {
            let entry = entry?;
            total += 1;
            match fs::read_link(entry.path()) {
                Ok(target) => {
                    if let Some(inode) = target
                        .to_str()
                        .and_then(|target| target.strip_prefix("socket:["))
                        .and_then(|target| target.strip_suffix(']'))
                    {
                        owned.insert(inode.parse::<u64>()?);
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        let mut connected = HashSet::new();
        for line in fs::read_to_string("/proc/net/unix")?.lines().skip(1) {
            let fields: Vec<_> = line.split_whitespace().collect();
            if fields.len() == 8 && fields[5] == "03" && fields[7] == socket_path {
                let inode = fields[6].parse::<u64>()?;
                if owned.contains(&inode) {
                    connected.insert(inode);
                }
            }
        }
        Ok(Descriptors {
            connected: connected.len(),
            total,
        })
    }

    #[test]
    #[ignore = "requires a running desktop and ags-media.service; sends live status requests"]
    fn live_panel_ipc_lifecycle() -> AppResult<()> {
        let launcher = env::var_os("MEDIA_PANEL_LAUNCHER").unwrap_or_else(|| "media-panel".into());
        let requests = match env::var("MEDIA_PANEL_IPC_REQUESTS") {
            Ok(value) => value
                .parse::<usize>()
                .ok()
                .filter(|count| *count > 0 && value.bytes().all(|byte| byte.is_ascii_digit()))
                .ok_or("MEDIA_PANEL_IPC_REQUESTS must be a positive integer")?,
            Err(env::VarError::NotPresent) => 1200,
            Err(error) => return Err(error.into()),
        };
        eprintln!("Checking {SERVICE}: launcher={launcher:?}, status requests={requests}");
        let cgroup = service_cgroup()?;
        let pid = gjs_pid(&cgroup)?;
        let start = process_start(pid)?;
        let uid = fs::metadata("/proc/self")?.uid();
        let socket_path = format!("/run/user/{uid}/astal/media-panel.sock");
        same_process(&cgroup, pid, start)?;
        let before = descriptors(pid, &socket_path)?;
        same_process(&cgroup, pid, start)?;
        eprintln!(
            "pid={pid} starttime={start} socket={socket_path} connected_before={} fds_before={}",
            before.connected, before.total
        );

        let mut completed = 0;
        let began = Instant::now();
        let mut progress = began;
        let result = (|| -> AppResult<()> {
            for request in 1..=requests {
                same_process(&cgroup, pid, start)?;
                let output = command(&launcher, &["status"]).map_err(|error| {
                    format!("status request {request}/{requests} failed: {error}")
                })?;
                let status: serde_json::Value = serde_json::from_slice(&output)
                    .map_err(|error| format!("status request {request}: invalid JSON: {error}"))?;
                if !status.is_object()
                    || ["text", "class", "alt", "tooltip"]
                        .iter()
                        .any(|key| !status.get(key).is_some_and(serde_json::Value::is_string))
                    || status.get("error").is_some()
                {
                    return Err(format!(
                        "status request {request} must return a Waybar badge object, got {status}"
                    )
                    .into());
                }
                same_process(&cgroup, pid, start)?;
                completed = request;
                if completed % 100 == 0
                    || progress.elapsed() >= Duration::from_secs(5)
                    || completed == requests
                {
                    eprintln!(
                        "pid={pid} requests={completed}/{requests} elapsed={:.1?}",
                        began.elapsed()
                    );
                    progress = Instant::now();
                }
            }
            Ok(())
        })();

        eprintln!("Settling for one second after {completed}/{requests} status requests");
        thread::sleep(Duration::from_secs(1));
        same_process(&cgroup, pid, start)?;
        if service_cgroup()? != cgroup {
            return Err(format!("{SERVICE} ControlGroup changed during the test").into());
        }
        let after = descriptors(pid, &socket_path)?;
        same_process(&cgroup, pid, start)?;
        let socket_growth = after.connected as i64 - before.connected as i64;
        let fd_growth = after.total as i64 - before.total as i64;
        eprintln!(
            "pid={pid} requests={completed}/{requests} connected_before={} connected_after={} growth={socket_growth:+} fds_before={} fds_after={} growth={fd_growth:+}",
            before.connected, after.connected, before.total, after.total
        );
        result?;
        if socket_growth > 2 {
            return Err(
                format!("connected panel sockets grew by {socket_growth} (maximum 2)").into(),
            );
        }
        if fd_growth > 8 {
            return Err(format!("open descriptors grew by {fd_growth} (maximum 8)").into());
        }
        eprintln!("PASS: IPC connections and file descriptors stayed bounded");
        Ok(())
    }
}
