mod model {
    use crate::model::*;

    fn strings(arguments: &[&str]) -> Vec<String> {
        arguments
            .iter()
            .map(|argument| (*argument).to_owned())
            .collect()
    }

    fn parsed(arguments: &[&str]) -> Config {
        match Config::parse(strings(arguments)).expect("arguments should parse") {
            Action::Run(config) => config,
            Action::Help => panic!("expected a runnable configuration"),
        }
    }

    #[test]
    fn defaults_match_the_nixos_service() {
        assert_eq!(parsed(&[]), Config::default());
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
    fn rejects_zero_poll_interval() {
        let error = match Config::parse(strings(&["--poll-seconds", "0"])) {
            Ok(_) => panic!("zero poll interval should fail"),
            Err(error) => error,
        };
        assert!(error.contains("greater than zero"));
    }
}

mod supervisor {
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
    fn placeholder_is_generated_without_a_build_time_image() {
        let arguments = placeholder_ffmpeg_arguments(&Config::default());
        assert!(arguments.contains(&"lavfi".to_owned()));
        assert!(arguments.contains(&"color=c=0x101418:s=720x720:r=1".to_owned()));
        assert_eq!(arguments.last().map(String::as_str), Some("/dev/video10"));
    }

    #[test]
    fn active_filter_centre_crops_and_scales() {
        let arguments = active_ffmpeg_arguments(&Config::default());
        assert!(arguments.contains(&"1280x720".to_owned()));
        assert!(arguments.contains(&"crop=ih:ih,scale=720:720,format=yuv420p".to_owned()));
        assert_eq!(arguments.last().map(String::as_str), Some("/dev/video10"));
    }
}
