//! Real-binary and opt-in desktop integration tests.

use media_control::AppResult;

mod cli {
    use std::fs;
    use std::process::{Command, Output, Stdio};

    fn run(args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_media-control"))
            .args(args)
            .env_clear()
            .env("PATH", "")
            .env("DBUS_SESSION_BUS_ADDRESS", "disabled:")
            .env("DBUS_SYSTEM_BUS_ADDRESS", "disabled:")
            .env("MEDIA_CONTROL_CURL", "/nonexistent/curl")
            .env("MEDIA_CONTROL_PLAYERCTL", "/nonexistent/playerctl")
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    #[test]
    fn help_succeeds_without_arguments_and_lists_all_commands() {
        for args in [vec![], vec!["--help"], vec!["-h"], vec!["help"]] {
            let output = run(&args);
            assert!(output.status.success(), "{args:?}: {output:?}");
            assert!(output.stderr.is_empty(), "{args:?}: {output:?}");
            let help = String::from_utf8(output.stdout).unwrap();
            assert!(help.contains("Usage: media-control"));
            for name in ["service", "model", "credit", "playerctl", "artwork", "JSON"] {
                assert!(help.contains(name), "{name}: {help}");
            }
        }
    }

    #[test]
    fn subcommand_help_never_reads_stdin_or_accesses_services() {
        for name in ["service", "model", "credit", "playerctl", "artwork"] {
            for args in [vec![name, "--help"], vec!["help", name]] {
                let output = run(&args);
                assert!(output.status.success(), "{args:?}: {output:?}");
                assert!(output.stderr.is_empty(), "{args:?}: {output:?}");
                let help = String::from_utf8(output.stdout).unwrap();
                assert!(
                    help.contains(&format!("Usage: media-control {name}")),
                    "{help}"
                );
                if name == "playerctl" {
                    assert!(help.contains("<command>"), "{help}");
                } else if name == "artwork" {
                    assert!(help.contains("<xesam-url|path>"), "{help}");
                }
            }
        }
    }

    #[test]
    fn invalid_arguments_fail_before_protocol_io_or_native_access() {
        for args in [
            vec!["retired"],
            vec!["--unknown"],
            vec!["help", "retired"],
            vec!["service", "extra"],
            vec!["model", "extra"],
            vec!["model", "--unknown"],
            vec!["credit", "extra"],
            vec!["playerctl"],
            vec!["playerctl", ""],
            vec!["playerctl", "play-pause", "extra"],
            vec!["artwork"],
            vec!["artwork", "/music/track.mp3", "extra"],
        ] {
            let output = run(&args);
            assert_eq!(output.status.code(), Some(2), "{args:?}: {output:?}");
            assert!(output.stdout.is_empty(), "{args:?}: {output:?}");
            let error = String::from_utf8(output.stderr).unwrap();
            assert!(error.starts_with("error:"), "{args:?}: {error}");
        }
    }

    #[test]
    fn artwork_cli_preserves_paths_and_file_urls_without_extra_stdout() {
        let directory = tempfile::tempdir().unwrap();
        let track = directory.path().join("Track +%#? 曲.mp3");
        let cover = track.with_extension("PNG");
        fs::write(&track, b"artwork beside the track wins over embedded tags").unwrap();
        fs::write(&cover, b"per-track cover").unwrap();
        fs::write(directory.path().join("cover.jpg"), b"album fallback").unwrap();
        let path = track.to_str().unwrap();
        const PATH: &percent_encoding::AsciiSet = &percent_encoding::NON_ALPHANUMERIC.remove(b'/');
        let url = format!(
            "file://{}",
            percent_encoding::utf8_percent_encode(path, PATH)
        );
        for args in [
            vec!["artwork", path],
            vec!["artwork", "--", path],
            vec!["artwork", &url],
        ] {
            let output = run(&args);
            assert!(output.status.success(), "{args:?}: {output:?}");
            assert!(output.stderr.is_empty(), "{args:?}: {output:?}");
            assert_eq!(output.stdout, format!("{}\n", cover.display()).as_bytes());
        }
    }
}

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
