mod model {
    use clap::{CommandFactory, Parser, error::ErrorKind};

    use crate::model::{Action, Config};

    const OPTIONS: [&str; 10] = [
        "--source",
        "--output",
        "--input-format",
        "--input-width",
        "--input-height",
        "--framerate",
        "--output-size",
        "--idle-seconds",
        "--warmup-seconds",
        "--poll-seconds",
    ];
    const POSITIVE_U32_OPTIONS: [&str; 4] = [
        "--input-width",
        "--input-height",
        "--framerate",
        "--output-size",
    ];
    const SECONDS_OPTIONS: [&str; 3] = ["--idle-seconds", "--warmup-seconds", "--poll-seconds"];

    fn parse(arguments: &[&str]) -> Result<Config, clap::Error> {
        Config::try_parse_from(std::iter::once("webcam-crop").chain(arguments.iter().copied()))
    }

    fn parsed(arguments: &[&str]) -> Config {
        parse(arguments).expect("arguments should parse")
    }

    #[test]
    fn defaults_match_the_nixos_service() {
        assert_eq!(
            parsed(&[]),
            Config {
                source: "/dev/cam-raw".to_owned(),
                output: "/dev/video10".to_owned(),
                input_format: "mjpeg".to_owned(),
                input_width: 1280,
                input_height: 720,
                framerate: 30,
                output_size: 720,
                idle_seconds: 3,
                warmup_seconds: 2,
                poll_seconds: 1,
                action: None,
            }
        );
    }

    #[test]
    fn parses_every_runtime_setting() {
        let config = parsed(&[
            "--source",
            "/dev/source",
            "--output",
            "/dev/output",
            "--input-format",
            "yuyv422",
            "--input-width",
            "1920",
            "--input-height",
            "1080",
            "--framerate",
            "60",
            "--output-size",
            "1080",
            "--idle-seconds",
            "5",
            "--warmup-seconds",
            "1",
            "--poll-seconds",
            "2",
        ]);

        assert_eq!(config.source, "/dev/source");
        assert_eq!(config.output, "/dev/output");
        assert_eq!(config.input_format, "yuyv422");
        assert_eq!(config.input_width, 1920);
        assert_eq!(config.input_height, 1080);
        assert_eq!(config.framerate, 60);
        assert_eq!(config.output_size, 1080);
        assert_eq!(config.idle_seconds, 5);
        assert_eq!(config.warmup_seconds, 1);
        assert_eq!(config.poll_seconds, 2);
    }

    #[test]
    fn help_flags_display_options_and_defaults() {
        for argument in ["-h", "--help"] {
            let error = parse(&[argument]).unwrap_err();
            assert_eq!(error.kind(), ErrorKind::DisplayHelp);
            assert_eq!(error.exit_code(), 0);
            let help = error.to_string();
            assert!(help.contains("Usage: webcam-crop"));
            for option in OPTIONS {
                assert!(help.contains(option), "missing option: {option}");
            }
            assert!(help.contains("/dev/cam-raw"));
            assert!(help.contains("/dev/video10"));
            assert!(help.contains("[default: 30]"));
        }
    }

    #[test]
    fn legacy_bare_help_is_an_explicit_subcommand() {
        assert_eq!(parsed(&["help"]).action, Some(Action::Help));
        let help = Config::command().render_help().to_string();
        assert!(help.contains("Usage: webcam-crop"));
        assert_eq!(help, parse(&["-h"]).unwrap_err().to_string());
        for option in OPTIONS {
            assert!(help.contains(option), "missing option: {option}");
        }
        assert!(help.contains("/dev/cam-raw"));
        assert!(help.contains("/dev/video10"));
    }

    #[test]
    fn rejects_unknown_options_and_extra_arguments() {
        for arguments in [
            &["--unknown"][..],
            &["unexpected"],
            &["--idle-seconds", "1", "extra"],
            &["help", "extra"],
            &["help", "--unknown"],
        ] {
            assert_eq!(parse(arguments).unwrap_err().exit_code(), 2);
        }
    }

    #[test]
    fn rejects_missing_and_empty_values() {
        for option in OPTIONS {
            assert_eq!(parse(&[option]).unwrap_err().exit_code(), 2);
        }
        for option in ["--source", "--output", "--input-format"] {
            let error = parse(&[option, ""]).unwrap_err();
            assert_eq!(error.kind(), ErrorKind::InvalidValue);
            assert_eq!(error.exit_code(), 2);
        }
    }

    #[test]
    fn positive_u32_settings_validate_types_and_ranges() {
        for option in POSITIVE_U32_OPTIONS {
            for invalid in ["0", "text", "-1", "1.5", "4294967296"] {
                assert_eq!(parse(&[option, invalid]).unwrap_err().exit_code(), 2);
            }
            for valid in ["1", "4294967295"] {
                assert!(parse(&[option, valid]).is_ok(), "{option} {valid}");
            }
        }
    }

    #[test]
    fn second_settings_validate_types_and_ranges_without_rejecting_zero_delays() {
        for option in SECONDS_OPTIONS {
            for invalid in ["text", "-1", "1.5", "18446744073709551616"] {
                assert_eq!(parse(&[option, invalid]).unwrap_err().exit_code(), 2);
            }
            assert!(parse(&[option, "18446744073709551615"]).is_ok());
        }
        assert_eq!(parsed(&["--idle-seconds", "0"]).idle_seconds, 0);
        assert_eq!(parsed(&["--warmup-seconds", "0"]).warmup_seconds, 0);
        assert_eq!(parse(&["--poll-seconds", "0"]).unwrap_err().exit_code(), 2);
        assert_eq!(parsed(&["--poll-seconds", "1"]).poll_seconds, 1);
    }

    #[test]
    fn accepts_equals_syntax_and_repeated_options_use_the_last_value() {
        let config = parsed(&[
            "--framerate=10",
            "--framerate",
            "60",
            "--source=/dev/source",
        ]);
        assert_eq!(config.framerate, 60);
        assert_eq!(config.source, "/dev/source");
    }

    #[test]
    fn help_remains_a_valid_option_value_and_program_name() {
        let config =
            Config::try_parse_from(["help", "--source", "help", "--input-format=help"]).unwrap();
        assert_eq!(config.source, "help");
        assert_eq!(config.input_format, "help");
        assert_eq!(config.action, None);
    }
}

mod supervisor {
    use clap::Parser;

    use crate::model::Config;
    use crate::supervisor::*;

    #[test]
    fn parses_fuser_output_and_ignores_non_pid_fields() {
        assert_eq!(
            parse_fuser_pids(b"/dev/video10:  1204 99m nonsense 1204\n"),
            vec![1204, 99, 1204]
        );
    }

    #[test]
    fn parses_pid_prefixes_without_accepting_overflow_or_signs() {
        assert_eq!(
            parse_fuser_pids(b"\t0 4294967295m 4294967296 +12 -34 m99 77\xff \r\n"),
            vec![0, u32::MAX, 77]
        );
        assert!(parse_fuser_pids(b" \t\r\n").is_empty());
    }

    #[test]
    fn placeholder_is_generated_without_a_build_time_image() {
        let arguments = placeholder_ffmpeg_arguments(&Config::parse_from(["webcam-crop"]));
        assert!(arguments.contains(&"lavfi".to_owned()));
        assert!(arguments.contains(&"color=c=0x101418:s=720x720:r=1".to_owned()));
        assert_eq!(arguments.last().map(String::as_str), Some("/dev/video10"));
    }

    #[test]
    fn active_filter_centre_crops_and_scales() {
        let arguments = active_ffmpeg_arguments(&Config::parse_from(["webcam-crop"]));
        assert!(arguments.contains(&"1280x720".to_owned()));
        assert!(arguments.contains(&"crop=ih:ih,scale=720:720,format=yuv420p".to_owned()));
        assert_eq!(arguments.last().map(String::as_str), Some("/dev/video10"));
    }
}
