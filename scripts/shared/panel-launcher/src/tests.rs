use super::*;

#[test]
fn configuration_command_parsing_and_invalid_dispatch() {
    let mut config = Config {
        name: "fixture-panel".to_owned(),
        commands: ["run", "toggle", "close", "status", "toggleWifi"]
            .into_iter()
            .map(str::to_owned)
            .collect(),
        strict_help: true,
        check_response: true,
        astal: PathBuf::from("/fixture/astal"),
        bundle: PathBuf::from("/fixture/bundle"),
        environment: BTreeMap::new(),
        forced_environment: BTreeMap::new(),
    };
    let args = [
        OsString::from("toggle"),
        OsString::from("monitor with spaces"),
    ];
    let Action::Request(request) =
        parse_action(&config, &args).unwrap_or_else(|error| panic!("{error}"))
    else {
        panic!("expected IPC action");
    };
    assert_eq!(
        serde_json::to_value(request).unwrap(),
        serde_json::json!({"command":"toggle","monitor":"monitor with spaces"})
    );
    assert!(matches!(
        parse_action(&config, &[OsString::from("run")]),
        Ok(Action::Run)
    ));
    for args in [
        vec![],
        vec![OsString::from("backend")],
        vec![OsString::from("close"), OsString::from("extra")],
        vec![OsString::from("--help"), OsString::from("extra")],
    ] {
        assert!(parse_action(&config, &args).is_err());
    }
    config.strict_help = false;
    assert!(matches!(
        parse_action(
            &config,
            &[OsString::from("--help"), OsString::from("extra")]
        ),
        Ok(Action::Help)
    ));
    assert_eq!(
        main_status([OsString::from("backend"), OsString::from("network")].into_iter()),
        2
    );
    let help = config.cli().render_help().to_string();
    assert!(help.contains("Usage: fixture-panel"));
    assert!(help.contains("toggleWifi"));
    assert!(
        config
            .cli()
            .find_subcommand("toggle")
            .unwrap()
            .clone()
            .render_usage()
            .to_string()
            .contains("[monitor]")
    );
}

#[test]
fn clap_command_trees_are_valid_and_help_does_not_execute_runtime_actions() {
    Cli::command().debug_assert();
    assert_eq!(
        main_status(
            ["--help", "runtime", "input-toggle"]
                .map(OsString::from)
                .into_iter()
        ),
        0
    );
    assert_eq!(main_status(["--config"].map(OsString::from).into_iter()), 2);
    for args in [
        vec!["panel-launcher", "runtime", "--help"],
        vec!["panel-launcher", "runtime", "with-parent-death", "--help"],
    ] {
        assert_eq!(
            Cli::try_parse_from(args).unwrap_err().kind(),
            clap::error::ErrorKind::DisplayHelp
        );
    }
}

#[test]
fn runtime_command_arguments_preserve_native_flags_and_non_unicode_paths() {
    use std::os::unix::ffi::OsStringExt;
    let program = OsString::from_vec(b"/program-\xff".to_vec());
    for prefix in [
        vec!["panel-launcher", "runtime", "with-parent-death"],
        vec!["panel-launcher", "runtime", "bisync", "/state"],
        vec![
            "panel-launcher",
            "runtime",
            "watch",
            "3",
            "systemctl",
            "sync.service",
        ],
    ] {
        for suffix in [
            vec![],
            vec!["--help"],
            vec!["-h"],
            vec!["--", "-c", "data"],
            vec!["--native-flag", "--help", "a b"],
        ] {
            let expected: Vec<OsString> = std::iter::once(program.clone())
                .chain(suffix.iter().map(OsString::from))
                .collect();
            let cli =
                Cli::try_parse_from(prefix.iter().map(OsString::from).chain(expected.clone()))
                    .unwrap();
            let Some(LauncherCommand::Runtime { command }) = cli.command else {
                panic!("wrong runtime action")
            };
            let (desktop_runtime::Action::WithParentDeath { command }
            | desktop_runtime::Action::Bisync { command, .. }
            | desktop_runtime::Action::Watch { command, .. }) = command
            else {
                panic!("wrong runtime action")
            };
            assert_eq!(command, expected);
        }
    }
}

#[test]
fn config_mode_preserves_the_panel_help_policy_and_opaque_argument_tail() {
    use std::os::unix::ffi::OsStringExt;
    let file = tempfile::NamedTempFile::new().unwrap();
    let mut config = serde_json::json!({"name":"fixture-panel","commands":["run","toggle","status"],"strict_help":false,"check_response":false,"astal":"/never-execute","bundle":"/never-execute","environment":{},"forced_environment":{}});
    for strict in [false, true] {
        config["strict_help"] = strict.into();
        fs::write(file.path(), config.to_string()).unwrap();
        for suffix in [
            vec![OsString::from("--native-flag")],
            vec!["runtime".into(), "input-toggle".into()],
            vec![OsString::from_vec(vec![255])],
            vec!["--version".into()],
        ] {
            let arguments = [
                OsString::from("--config"),
                file.path().into(),
                "--help".into(),
            ]
            .into_iter()
            .chain(suffix);
            assert_eq!(main_status(arguments), if strict { 2 } else { 0 });
        }
    }
}

#[test]
fn shell_response_newline_semantics() {
    for (input, expected) in [
        (b"".as_slice(), b"".as_slice()),
        (b"\n\n", b""),
        (b"ok\n\n", b"ok"),
        (b"ok\r\n", b"ok\r"),
        (b"ok \n", b"ok "),
        (b"one\ntwo\n", b"one\ntwo"),
        (b"\xff\n", b"\xff"),
    ] {
        assert_eq!(response_body(input), expected);
    }
}

#[test]
fn only_top_level_error_keys_are_failures() {
    for response in [
        r#"{"error":null}"#,
        r#"{"error":false}"#,
        r#"{"error":0}"#,
        r#"{"error":""}"#,
        r#"{"error":{}}"#,
        r#"{"result":"ok","error":[]}"#,
    ] {
        assert!(response_has_error(response.as_bytes()), "{response}");
    }
    for response in [
        "ok",
        "",
        "not json",
        "null",
        "false",
        r#""error""#,
        r#"{"result":{"error":true}}"#,
        r#"[{"error":true}]"#,
        r#"{"error":true} trailing"#,
    ] {
        assert!(!response_has_error(response.as_bytes()), "{response}");
    }
}

// Expand inside desktop_runtime to retain private access and test paths.
macro_rules! desktop_runtime_tests {
    () => {
        mod tests {
            use super::*;
            use std::{io::Write, os::unix::net::UnixStream};
            #[test]
            fn parsing() {
                use clap::Parser;
                for args in [
                    vec!["panel-launcher", "runtime"],
                    vec!["panel-launcher", "runtime", "input-toggle", "extra"],
                    vec![
                        "panel-launcher",
                        "runtime",
                        "watch",
                        "0",
                        "ctl",
                        "unit",
                        "watcher",
                    ],
                ] {
                    assert!(super::super::Cli::try_parse_from(args).is_err());
                }
                assert_eq!(
                    name(br#"{"name":"DP-1","other":1}"#).unwrap().as_deref(),
                    Some("DP-1")
                );
                assert!(name(br#"{"name":123}"#).is_err());
                assert!(name(b"null").unwrap().is_none());
            }
            #[test]
            fn input_switches_only_after_valid_query() {
                for (current, next) in [
                    ("pinyin\n", "keyboard-gb"),
                    ("keyboard-gb\n", "pinyin"),
                    ("other", "pinyin"),
                ] {
                    let mut calls = Vec::new();
                    input_toggle(|args| {
                        calls.push(args.join(" "));
                        Ok(current.as_bytes().to_vec())
                    })
                    .unwrap();
                    assert_eq!(calls, ["-n".to_string(), format!("-s {next}")]);
                }
                for response in [Ok(vec![]), Ok(vec![255]), Err(bad())] {
                    let mut once = Some(response);
                    assert!(
                        input_toggle(|args| {
                            assert_eq!(args, ["-n"]);
                            once.take().unwrap()
                        })
                        .is_err()
                    );
                }
            }
            #[test]
            fn identity_checks() {
                for s in ["0", "-2", "1", "+42", "999999999999", "123x"] {
                    assert!(parse_pid(s).is_none())
                }
                assert_eq!(parse_pid("42\n"), Some(42));
                assert!(same_parent(4, 4));
                assert!(!same_parent(1, 1));
                assert!(!same_parent(42, 43));
            }
            #[test]
            fn state_listing() {
                let dir = env::temp_dir().join(format!("panel-state-{}", std::process::id()));
                fs::create_dir(&dir).unwrap();
                assert!(resync(&dir).unwrap());
                fs::write(dir.join("old.lst"), []).unwrap();
                assert!(!resync(&dir).unwrap());
                fs::remove_dir_all(&dir).unwrap();
                assert!(resync(&dir).is_err());
            }
            #[test]
            fn coalesces_and_cancels() {
                let (mut rx, mut tx) = UnixStream::pair().unwrap();
                tx.write_all(b"a\nb\n").unwrap();
                let mut calls = 0;
                debounce(&mut rx, Duration::from_millis(1), || {
                    calls += 1;
                    tx.shutdown(std::net::Shutdown::Write)?;
                    Ok(())
                })
                .unwrap();
                assert_eq!(calls, 1);
                let (mut rx, mut tx) = UnixStream::pair().unwrap();
                tx.write_all(b"pending\n").unwrap();
                drop(tx);
                debounce(&mut rx, Duration::from_secs(3), || panic!("cancelled")).unwrap();
            }
            #[test]
            fn bounded_failure() {
                assert!(
                    bounded(Command::new("sleep").arg("10"), Duration::from_millis(10)).is_err()
                );
                assert!(toggle(OsStr::new("false")).is_err());
            }
        }
    };
}
pub(crate) use desktop_runtime_tests;
