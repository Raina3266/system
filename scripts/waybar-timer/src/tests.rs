//! Every test in the script, one mod per feature and one test per sub-feature.

mod model {
    use std::time::{Duration, Instant};

    use crate::model::*;

    fn timer(remaining: Duration, running: bool) -> Timer {
        Timer {
            remaining,
            running,
            last_tick: Instant::now(),
        }
    }

    #[test]
    fn adding_five_minutes_at_a_time_builds_up_the_countdown() {
        let mut timer = Timer::new();
        assert!(!timer.apply("add"));
        assert_eq!(timer.displayed_seconds(), 300);
        assert!(!timer.apply("add"));
        assert_eq!(timer.displayed_seconds(), 600);
        assert_eq!(timer.state(), State::Pause);
    }

    #[test]
    fn adding_past_the_two_hour_cap_clears_the_timer() {
        let mut timer = Timer::new();
        for _ in 0..24 {
            timer.apply("add");
        }
        assert_eq!(timer.displayed_seconds(), 2 * 60 * 60);

        assert!(!timer.apply("add"));
        assert_eq!(timer.displayed_seconds(), 0);
        assert_eq!(timer.state(), State::Stop);
    }

    #[test]
    fn toggle_only_runs_while_time_remains() {
        let mut timer = Timer::new();
        assert!(!timer.apply("toggle"));
        assert_eq!(timer.state(), State::Stop);

        timer.apply("add");
        assert!(!timer.apply("toggle"));
        assert_eq!(timer.state(), State::Run);
        assert!(!timer.apply("toggle"));
        assert_eq!(timer.state(), State::Pause);
    }

    #[test]
    fn clear_stops_and_empties_the_timer() {
        let mut timer = Timer::new();
        timer.apply("add");
        timer.apply("toggle");
        assert!(!timer.apply("clear"));
        assert_eq!(timer.state(), State::Stop);
        assert_eq!(timer.displayed_seconds(), 0);
    }

    #[test]
    fn displayed_seconds_round_up_to_the_next_second() {
        assert_eq!(
            timer(Duration::from_secs(300), true).displayed_seconds(),
            300
        );
        assert_eq!(
            timer(Duration::from_millis(299_500), true).displayed_seconds(),
            300
        );
        assert_eq!(
            timer(Duration::from_millis(299_001), false).displayed_seconds(),
            300
        );
    }

    #[test]
    fn unknown_commands_leave_the_timer_untouched() {
        let mut timer = Timer::new();
        timer.apply("add");
        assert!(!timer.apply("explode"));
        assert_eq!(timer.displayed_seconds(), 300);
        assert_eq!(timer.state(), State::Pause);
    }
}

mod waybar {
    use std::time::{Duration, Instant};

    use crate::model::Timer;
    use crate::waybar::{ICON, status_json};

    fn timer(remaining: Duration, running: bool) -> Timer {
        Timer {
            remaining,
            running,
            last_tick: Instant::now(),
        }
    }

    #[test]
    fn running_timer_shows_the_time_and_running_class() {
        let running = timer(Duration::from_secs(300), true);
        assert_eq!(
            status_json(&running),
            format!(r#"{{"text":"{ICON} 05:00","tooltip":"Timer","class":"running"}}"#)
        );
    }

    #[test]
    fn paused_timer_shows_the_rounded_time_and_paused_class() {
        let paused = timer(Duration::from_millis(599_500), false);
        assert_eq!(
            status_json(&paused),
            format!(r#"{{"text":"{ICON} 10:00","tooltip":"Timer","class":"paused"}}"#)
        );
    }

    #[test]
    fn stopped_timer_shows_only_the_icon() {
        let stopped = timer(Duration::ZERO, false);
        assert_eq!(
            status_json(&stopped),
            format!(r#"{{"text":"{ICON}","tooltip":"Timer","class":"stopped"}}"#)
        );
    }
}
