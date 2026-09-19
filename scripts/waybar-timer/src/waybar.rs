//! The status line Waybar polls.

use crate::model::Timer;

pub(crate) const ICON: &str = "<span size='large'>󰔛</span>";

pub(crate) fn status_json(timer: &Timer) -> String {
    let total_seconds = timer.displayed_seconds();

    let minutes = total_seconds / 60;
    let seconds = total_seconds % 60;
    let time = format!("{minutes:02}:{seconds:02}");

    let text = if total_seconds == 0 {
        ICON.to_string()
    } else {
        format!("{ICON} {time}")
    };

    format!(
        r#"{{"text":"{text}","tooltip":"Timer","class":"{}"}}"#,
        timer.state().output()
    )
}
