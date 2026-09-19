//! The countdown state machine: the timer, the commands Waybar sends it,
//! and the state Waybar's CSS distinguishes.

use std::time::{Duration, Instant};

const FIVE_MINUTES: Duration = Duration::from_secs(5 * 60);
const MAX_TIMER: Duration = Duration::from_secs(2 * 60 * 60);

pub(crate) struct Timer {
    pub(crate) remaining: Duration,
    pub(crate) running: bool,
    pub(crate) last_tick: Instant,
}

impl Timer {
    pub(crate) fn new() -> Self {
        Self {
            remaining: Duration::from_secs(0),
            running: false,
            last_tick: Instant::now(),
        }
    }

    /// Update the remaining time. Returns true when the timer ends.
    pub(crate) fn tick(&mut self) -> bool {
        let now = Instant::now();

        // That prevents paused time from being counted.
        if !self.running {
            self.last_tick = now;
            return false;
        }

        let elapsed = now.duration_since(self.last_tick);
        self.last_tick = now;

        if elapsed >= self.remaining {
            self.remaining = Duration::from_secs(0);
            self.running = false;
            true
        } else {
            self.remaining -= elapsed;
            false
        }
    }

    /// Apply a command.
    pub(crate) fn apply(&mut self, command: &str) -> bool {
        let ended = self.tick();

        match command {
            "add" => {
                let next = self.remaining + FIVE_MINUTES;
                if next > MAX_TIMER {
                    self.remaining = Duration::ZERO;
                    self.running = false;
                } else {
                    self.remaining = next;
                }
                self.last_tick = Instant::now();
            }
            "toggle" => {
                if !self.remaining.is_zero() {
                    self.running = !self.running;
                    self.last_tick = Instant::now();
                }
            }
            "clear" => {
                self.remaining = Duration::from_secs(0);
                self.running = false;
                self.last_tick = Instant::now();
            }
            _ => eprintln!("waybar-timer: unknown command: {command}"),
        }

        ended
    }

    /// Round upward so a freshly-added five minutes is displayed as 5:00.
    pub(crate) fn displayed_seconds(&self) -> u64 {
        let seconds = self.remaining.as_secs();
        if self.remaining.subsec_nanos() == 0 {
            seconds
        } else {
            seconds.saturating_add(1)
        }
    }

    pub(crate) fn state(&self) -> State {
        if self.running {
            State::Run
        } else if self.remaining.is_zero() {
            State::Stop
        } else {
            State::Pause
        }
    }
}

#[derive(Debug, PartialEq)]
pub(crate) enum State {
    Run,
    Pause,
    Stop,
}

impl State {
    pub(crate) fn output(&self) -> &'static str {
        match self {
            State::Run => "running",
            State::Pause => "paused",
            State::Stop => "stopped",
        }
    }
}
