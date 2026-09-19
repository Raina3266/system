use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use crate::model::{Config, Tools};

const DEVICE_WAIT_TIMEOUT: Duration = Duration::from_secs(60);
const DEVICE_WAIT_INTERVAL: Duration = Duration::from_millis(500);
const PRODUCER_SWITCH_DELAY: Duration = Duration::from_millis(500);
const IDLE_INOTIFY_TIMEOUT_SECONDS: &str = "10";

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

pub(crate) struct Supervisor {
    config: Config,
    tools: Tools,
    own_cgroup: String,
    producer: Option<Producer>,
}

impl Supervisor {
    pub(crate) fn new(config: Config, tools: Tools) -> Self {
        Self {
            config,
            tools,
            own_cgroup: fs::read_to_string("/proc/self/cgroup").unwrap_or_default(),
            producer: None,
        }
    }

    pub(crate) fn run(&mut self) -> io::Result<()> {
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

pub(crate) fn parse_fuser_pids(output: &[u8]) -> Vec<u32> {
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

pub(crate) fn placeholder_ffmpeg_arguments(config: &Config) -> Vec<String> {
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

pub(crate) fn active_ffmpeg_arguments(config: &Config) -> Vec<String> {
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
