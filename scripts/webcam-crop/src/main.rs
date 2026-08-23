use std::collections::BTreeSet;
use std::env;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io;
use std::path::Path;
use std::process::{Child, Command, ExitCode, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const DEVICE_WAIT_TIMEOUT: Duration = Duration::from_secs(60);
const DEVICE_WAIT_INTERVAL: Duration = Duration::from_millis(500);
const PRODUCER_SWITCH_DELAY: Duration = Duration::from_millis(500);
const IDLE_INOTIFY_TIMEOUT_SECONDS: &str = "10";

#[derive(Clone, Debug, Eq, PartialEq)]
struct Config {
    source: String,
    output: String,
    input_format: String,
    input_width: u32,
    input_height: u32,
    framerate: u32,
    output_size: u32,
    idle_seconds: u64,
    warmup_seconds: u64,
    poll_seconds: u64,
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

enum Action {
    Run(Config),
    Help,
}

impl Config {
    fn parse(arguments: impl IntoIterator<Item = String>) -> Result<Action, String> {
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
struct Tools {
    ffmpeg: OsString,
    fuser: OsString,
    inotifywait: OsString,
    v4l2_ctl: OsString,
    v4l2loopback_ctl: OsString,
}

impl Tools {
    fn from_environment() -> Self {
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ProducerMode {
    Placeholder,
    Active,
}

impl ProducerMode {
    fn label(self) -> &'static str {
        match self {
            Self::Placeholder => "placeholder",
            Self::Active => "camera",
        }
    }
}

struct Producer {
    child: Child,
    mode: ProducerMode,
}

struct Supervisor {
    config: Config,
    tools: Tools,
    own_cgroup: String,
    producer: Option<Producer>,
}

impl Supervisor {
    fn new(config: Config, tools: Tools) -> Self {
        Self {
            config,
            tools,
            own_cgroup: fs::read_to_string("/proc/self/cgroup").unwrap_or_default(),
            producer: None,
        }
    }

    fn run(&mut self) -> io::Result<()> {
        self.wait_for_output_device()?;
        self.configure_loopback();
        self.start_producer(ProducerMode::Placeholder)?;

        loop {
            self.wait_for_consumer()?;

            thread::sleep(Duration::from_secs(self.config.warmup_seconds));
            let consumers = self.consumers()?;
            if consumers.is_empty() {
                continue;
            }

            println!(
                "loopback in use by: {} -- starting camera",
                consumers.join(" ")
            );
            self.restart_producer(ProducerMode::Active)?;
            self.wait_until_idle()?;

            println!(
                "idle for {}s -- releasing the camera",
                self.config.idle_seconds
            );
            self.restart_producer(ProducerMode::Placeholder)?;
        }
    }

    fn wait_for_output_device(&self) -> io::Result<()> {
        let deadline = Instant::now() + DEVICE_WAIT_TIMEOUT;
        while !Path::new(&self.config.output).exists() {
            if Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    format!(
                        "output device {} did not appear within {}s",
                        self.config.output,
                        DEVICE_WAIT_TIMEOUT.as_secs()
                    ),
                ));
            }
            thread::sleep(DEVICE_WAIT_INTERVAL);
        }
        Ok(())
    }

    fn configure_loopback(&self) {
        Self::run_best_effort(
            &self.tools.v4l2_ctl,
            &[
                "-d".to_owned(),
                self.config.output.clone(),
                "-c".to_owned(),
                "keep_format=0".to_owned(),
            ],
            "unlock the loopback format",
            true,
        );

        Self::run_best_effort(
            &self.tools.v4l2loopback_ctl,
            &[
                "set-caps".to_owned(),
                self.config.output.clone(),
                format!(
                    "YU12:{size}x{size}@{framerate}/1",
                    size = self.config.output_size,
                    framerate = self.config.framerate
                ),
            ],
            "set the loopback capabilities",
            false,
        );

        for control in ["sustain_framerate=1", "keep_format=1"] {
            Self::run_best_effort(
                &self.tools.v4l2_ctl,
                &[
                    "-d".to_owned(),
                    self.config.output.clone(),
                    "-c".to_owned(),
                    control.to_owned(),
                ],
                &format!("set {control}"),
                true,
            );
        }
    }

    fn run_best_effort(program: &OsStr, arguments: &[String], description: &str, quiet: bool) {
        let mut command = Command::new(program);
        command.args(arguments).stdin(Stdio::null());
        if quiet {
            command.stdout(Stdio::null()).stderr(Stdio::null());
        }

        match command.status() {
            Ok(status) if status.success() => {}
            Ok(status) => {
                eprintln!("webcam-crop: could not {description}: {status}");
            }
            Err(error) => {
                eprintln!("webcam-crop: could not {description}: {error}");
            }
        }
    }

    fn wait_for_consumer(&mut self) -> io::Result<()> {
        while self.consumers()?.is_empty() {
            self.ensure_producer(ProducerMode::Placeholder)?;
            self.wait_for_open_event()?;
        }
        Ok(())
    }

    fn wait_for_open_event(&self) -> io::Result<()> {
        Command::new(&self.tools.inotifywait)
            .args([
                "-q",
                "-e",
                "open",
                "-t",
                IDLE_INOTIFY_TIMEOUT_SECONDS,
                self.config.output.as_str(),
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|_| ())
    }

    fn wait_until_idle(&mut self) -> io::Result<()> {
        let idle_duration = Duration::from_secs(self.config.idle_seconds);
        let poll_duration = Duration::from_secs(self.config.poll_seconds);
        let mut idle_for = Duration::ZERO;

        loop {
            thread::sleep(poll_duration);
            self.ensure_producer(ProducerMode::Active)?;

            if self.consumers()?.is_empty() {
                idle_for += poll_duration;
                if idle_for >= idle_duration {
                    return Ok(());
                }
            } else {
                idle_for = Duration::ZERO;
            }
        }
    }

    fn consumers(&self) -> io::Result<Vec<String>> {
        let output = Command::new(&self.tools.fuser)
            .arg(&self.config.output)
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()?;

        let producer_pid = self.producer.as_ref().map(|producer| producer.child.id());
        let mut consumers = BTreeSet::new();

        for pid in parse_fuser_pids(&output.stdout) {
            if pid == std::process::id() || producer_pid == Some(pid) {
                continue;
            }

            let cgroup = fs::read_to_string(format!("/proc/{pid}/cgroup")).unwrap_or_default();
            if !self.own_cgroup.is_empty() && cgroup == self.own_cgroup {
                continue;
            }

            let name = fs::read_to_string(format!("/proc/{pid}/comm"))
                .unwrap_or_else(|_| "unknown".to_owned());
            consumers.insert(format!("{pid}:{}", name.trim()));
        }

        Ok(consumers.into_iter().collect())
    }

    fn ensure_producer(&mut self, expected_mode: ProducerMode) -> io::Result<()> {
        let needs_restart = match self.producer.as_mut() {
            Some(producer) if producer.mode != expected_mode => true,
            Some(producer) => match producer.child.try_wait()? {
                Some(status) => {
                    eprintln!(
                        "webcam-crop: {} producer exited with {status}; restarting",
                        producer.mode.label()
                    );
                    true
                }
                None => false,
            },
            None => true,
        };

        if needs_restart {
            self.restart_producer(expected_mode)?;
        }
        Ok(())
    }

    fn restart_producer(&mut self, mode: ProducerMode) -> io::Result<()> {
        self.stop_producer();
        self.start_producer(mode)
    }

    fn start_producer(&mut self, mode: ProducerMode) -> io::Result<()> {
        let arguments = match mode {
            ProducerMode::Placeholder => placeholder_ffmpeg_arguments(&self.config),
            ProducerMode::Active => active_ffmpeg_arguments(&self.config),
        };

        let child = Command::new(&self.tools.ffmpeg)
            .args(&arguments)
            .stdin(Stdio::null())
            .spawn()
            .map_err(|error| {
                io::Error::new(
                    error.kind(),
                    format!("could not start {} producer: {error}", mode.label()),
                )
            })?;

        self.producer = Some(Producer { child, mode });
        Ok(())
    }

    fn stop_producer(&mut self) {
        let Some(mut producer) = self.producer.take() else {
            return;
        };

        match producer.child.try_wait() {
            Ok(Some(_)) => {}
            Ok(None) | Err(_) => {
                let _ = producer.child.kill();
                let _ = producer.child.wait();
            }
        }

        // v4l2loopback permits only one producer in exclusive-caps mode. Give
        // the kernel time to release the output node before opening it again.
        thread::sleep(PRODUCER_SWITCH_DELAY);
    }
}

impl Drop for Supervisor {
    fn drop(&mut self) {
        self.stop_producer();
    }
}

fn parse_fuser_pids(output: &[u8]) -> Vec<u32> {
    output
        .split(u8::is_ascii_whitespace)
        .filter_map(|field| {
            let digits: Vec<u8> = field
                .iter()
                .copied()
                .take_while(u8::is_ascii_digit)
                .collect();
            (!digits.is_empty())
                .then(|| std::str::from_utf8(&digits).ok()?.parse::<u32>().ok())
                .flatten()
        })
        .collect()
}

fn placeholder_ffmpeg_arguments(config: &Config) -> Vec<String> {
    vec![
        "-nostdin".to_owned(),
        "-hide_banner".to_owned(),
        "-loglevel".to_owned(),
        "error".to_owned(),
        "-re".to_owned(),
        "-f".to_owned(),
        "lavfi".to_owned(),
        "-i".to_owned(),
        format!(
            "color=c=0x101418:s={size}x{size}:r=1",
            size = config.output_size
        ),
        "-vf".to_owned(),
        "format=yuv420p".to_owned(),
        "-f".to_owned(),
        "v4l2".to_owned(),
        "-pix_fmt".to_owned(),
        "yuv420p".to_owned(),
        config.output.clone(),
    ]
}

fn active_ffmpeg_arguments(config: &Config) -> Vec<String> {
    vec![
        "-nostdin".to_owned(),
        "-hide_banner".to_owned(),
        "-loglevel".to_owned(),
        "warning".to_owned(),
        "-fflags".to_owned(),
        "nobuffer".to_owned(),
        "-analyzeduration".to_owned(),
        "0".to_owned(),
        "-probesize".to_owned(),
        "32".to_owned(),
        "-f".to_owned(),
        "v4l2".to_owned(),
        "-input_format".to_owned(),
        config.input_format.clone(),
        "-video_size".to_owned(),
        format!("{}x{}", config.input_width, config.input_height),
        "-framerate".to_owned(),
        config.framerate.to_string(),
        "-i".to_owned(),
        config.source.clone(),
        "-vf".to_owned(),
        format!(
            "crop=ih:ih,scale={size}:{size},format=yuv420p",
            size = config.output_size
        ),
        "-f".to_owned(),
        "v4l2".to_owned(),
        "-pix_fmt".to_owned(),
        "yuv420p".to_owned(),
        config.output.clone(),
    ]
}

fn print_help() {
    println!(
        "webcam-crop\n\n\
         Keep a selectable v4l2loopback webcam idle on a placeholder and switch\n\
         to a centre-cropped real camera feed while applications use it.\n\n\
         Usage: webcam-crop [options]\n\n\
         Options:\n\
           --source PATH          Real camera device (default: /dev/cam-raw)\n\
           --output PATH          v4l2loopback device (default: /dev/video10)\n\
           --input-format FORMAT  Input stream format (default: mjpeg)\n\
           --input-width PIXELS   Input width (default: 1280)\n\
           --input-height PIXELS  Input height (default: 720)\n\
           --framerate FPS        Capture/output framerate (default: 30)\n\
           --output-size PIXELS   Square output size (default: 720)\n\
           --idle-seconds SECONDS Release the real camera after this idle time (default: 3)\n\
           --warmup-seconds SEC   Let a consumer settle before switching (default: 2)\n\
           --poll-seconds SEC     Active consumer polling interval (default: 1)\n\
           -h, --help             Show this help"
    );
}

fn main() -> ExitCode {
    let action = match Config::parse(env::args().skip(1)) {
        Ok(action) => action,
        Err(error) => {
            eprintln!("webcam-crop: {error}\n");
            print_help();
            return ExitCode::from(2);
        }
    };

    let Action::Run(config) = action else {
        print_help();
        return ExitCode::SUCCESS;
    };

    let mut supervisor = Supervisor::new(config, Tools::from_environment());
    match supervisor.run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("webcam-crop: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
