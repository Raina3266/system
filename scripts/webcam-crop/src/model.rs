use std::env;
use std::ffi::OsString;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Config {
    pub(crate) source: String,
    pub(crate) output: String,
    pub(crate) input_format: String,
    pub(crate) input_width: u32,
    pub(crate) input_height: u32,
    pub(crate) framerate: u32,
    pub(crate) output_size: u32,
    pub(crate) idle_seconds: u64,
    pub(crate) warmup_seconds: u64,
    pub(crate) poll_seconds: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
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
        }
    }
}

pub(crate) enum Action {
    Run(Config),
    Help,
}

impl Config {
    pub(crate) fn parse(arguments: impl IntoIterator<Item = String>) -> Result<Action, String> {
        let arguments: Vec<String> = arguments.into_iter().collect();
        let mut config = Self::default();
        let mut index = 0;

        while index < arguments.len() {
            let option = arguments[index].as_str();
            if matches!(option, "-h" | "--help" | "help") {
                return Ok(Action::Help);
            }

            let value = arguments
                .get(index + 1)
                .ok_or_else(|| format!("missing value for {option}"))?;

            match option {
                "--source" => config.source.clone_from(value),
                "--output" => config.output.clone_from(value),
                "--input-format" => config.input_format.clone_from(value),
                "--input-width" => {
                    config.input_width = parse_positive_u32(option, value)?;
                }
                "--input-height" => {
                    config.input_height = parse_positive_u32(option, value)?;
                }
                "--framerate" => {
                    config.framerate = parse_positive_u32(option, value)?;
                }
                "--output-size" => {
                    config.output_size = parse_positive_u32(option, value)?;
                }
                "--idle-seconds" => {
                    config.idle_seconds = parse_u64(option, value)?;
                }
                "--warmup-seconds" => {
                    config.warmup_seconds = parse_u64(option, value)?;
                }
                "--poll-seconds" => {
                    config.poll_seconds = parse_positive_u64(option, value)?;
                }
                _ => return Err(format!("unknown option: {option}")),
            }

            index += 2;
        }

        if config.source.is_empty() {
            return Err("--source cannot be empty".to_owned());
        }
        if config.output.is_empty() {
            return Err("--output cannot be empty".to_owned());
        }
        if config.input_format.is_empty() {
            return Err("--input-format cannot be empty".to_owned());
        }

        Ok(Action::Run(config))
    }
}

fn parse_u64(option: &str, value: &str) -> Result<u64, String> {
    value
        .parse::<u64>()
        .map_err(|_| format!("invalid value for {option}: {value}"))
}

fn parse_positive_u64(option: &str, value: &str) -> Result<u64, String> {
    let number = parse_u64(option, value)?;
    if number == 0 {
        Err(format!("{option} must be greater than zero"))
    } else {
        Ok(number)
    }
}

fn parse_positive_u32(option: &str, value: &str) -> Result<u32, String> {
    let number = value
        .parse::<u32>()
        .map_err(|_| format!("invalid value for {option}: {value}"))?;
    if number == 0 {
        Err(format!("{option} must be greater than zero"))
    } else {
        Ok(number)
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Tools {
    pub(crate) ffmpeg: OsString,
    pub(crate) fuser: OsString,
    pub(crate) inotifywait: OsString,
    pub(crate) v4l2_ctl: OsString,
    pub(crate) v4l2loopback_ctl: OsString,
}

impl Tools {
    pub(crate) fn from_environment() -> Self {
        Self {
            ffmpeg: executable("WEBCAM_CROP_FFMPEG", "ffmpeg"),
            fuser: executable("WEBCAM_CROP_FUSER", "fuser"),
            inotifywait: executable("WEBCAM_CROP_INOTIFYWAIT", "inotifywait"),
            v4l2_ctl: executable("WEBCAM_CROP_V4L2_CTL", "v4l2-ctl"),
            v4l2loopback_ctl: executable("WEBCAM_CROP_V4L2LOOPBACK_CTL", "v4l2loopback-ctl"),
        }
    }
}

fn executable(variable: &str, fallback: &str) -> OsString {
    env::var_os(variable).unwrap_or_else(|| OsString::from(fallback))
}
