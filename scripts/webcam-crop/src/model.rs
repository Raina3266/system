use std::env;
use std::ffi::OsString;

use clap::{Parser, Subcommand, builder::NonEmptyStringValueParser};

/// Keep a selectable v4l2loopback webcam idle on a placeholder and switch
/// to a centre-cropped real camera feed while applications use it.
#[derive(Clone, Debug, Eq, PartialEq, Parser)]
#[command(
    name = "webcam-crop",
    args_override_self = true,
    disable_help_subcommand = true
)]
pub(crate) struct Config {
    /// Real camera device.
    #[arg(long, default_value = "/dev/cam-raw", value_parser = NonEmptyStringValueParser::new())]
    pub(crate) source: String,
    /// v4l2loopback device.
    #[arg(long, default_value = "/dev/video10", value_parser = NonEmptyStringValueParser::new())]
    pub(crate) output: String,
    /// Input stream format.
    #[arg(long, default_value = "mjpeg", value_parser = NonEmptyStringValueParser::new())]
    pub(crate) input_format: String,
    /// Input width in pixels.
    #[arg(long, default_value_t = 1280, value_parser = clap::value_parser!(u32).range(1..))]
    pub(crate) input_width: u32,
    /// Input height in pixels.
    #[arg(long, default_value_t = 720, value_parser = clap::value_parser!(u32).range(1..))]
    pub(crate) input_height: u32,
    /// Capture/output framerate.
    #[arg(long, default_value_t = 30, value_parser = clap::value_parser!(u32).range(1..))]
    pub(crate) framerate: u32,
    /// Square output size in pixels.
    #[arg(long, default_value_t = 720, value_parser = clap::value_parser!(u32).range(1..))]
    pub(crate) output_size: u32,
    /// Release the real camera after this many idle seconds.
    #[arg(long, default_value_t = 3)]
    pub(crate) idle_seconds: u64,
    /// Let a consumer settle for this many seconds before switching.
    #[arg(long, default_value_t = 2)]
    pub(crate) warmup_seconds: u64,
    /// Active consumer polling interval in seconds.
    #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u64).range(1..))]
    pub(crate) poll_seconds: u64,
    #[command(subcommand)]
    pub(crate) action: Option<Action>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Subcommand)]
pub(crate) enum Action {
    /// Show this help.
    Help,
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
