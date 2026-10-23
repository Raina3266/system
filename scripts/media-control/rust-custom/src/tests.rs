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
                        title: &player.title,
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
                    assert!(response.error.is_some());
                }
                snapshot.action = Some(action("unknown", ""));
                assert_eq!(
                    analyze(&snapshot).error.as_deref(),
                    Some("Unknown media action.")
                );
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
                assert!(!same_optional(None, None));
                assert!(!same_optional(Some("cover"), None));
                assert!(!same_optional(Some(" \t"), Some("\n")));
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
                assert!(!echoes_bus_name("VLC media player"));
                assert!(echoes_bus_name("mprisence_web.spotify.p2911"));

                let mut player = elisa();
                player.identity = String::from("VLC media player");
                assert_eq!(source_name(&player), "VLC media player");
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
