//! Number and markup formatting for the rows the widget draws.

/// The cyberpunk palette shared with `themes/waybar.css` and the Rofi themes.
pub mod colour {
    pub const ICON: &str = "#ff7edb";
    pub const NAME: &str = "#5c6776";
    pub const VALUE: &str = "#7afcff";
    pub const WARNING: &str = "#f29e74";
    pub const CRITICAL: &str = "#ff5c57";
}

/// How alarming a reading is. Only the value's colour depends on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Normal,
    Warning,
    Critical,
}

impl Level {
    /// Classify `value` against an ascending pair of thresholds.
    pub fn from_thresholds(value: f64, warning: f64, critical: f64) -> Self {
        if value >= critical {
            Level::Critical
        } else if value >= warning {
            Level::Warning
        } else {
            Level::Normal
        }
    }

    pub fn colour(self) -> &'static str {
        match self {
            Level::Normal => colour::VALUE,
            Level::Warning => colour::WARNING,
            Level::Critical => colour::CRITICAL,
        }
    }
}

/// Escape the five characters Pango's markup parser treats as syntax.
pub fn escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&apos;"),
            _ => escaped.push(character),
        }
    }
    escaped
}

/// Wrap `text` in a Pango `<span>` of the given colour.
pub fn span(colour: &str, text: &str) -> String {
    format!("<span foreground=\"{colour}\">{}</span>", escape(text))
}

/// Render a byte count the way a status bar does: binary units, at most one
/// decimal, and no decimal at all once the mantissa reaches three digits.
pub fn bytes(count: u64) -> String {
    const UNITS: [&str; 5] = ["B", "K", "M", "G", "T"];

    let mut value = count as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }

    // Whole bytes never need a fraction, and neither does a three-digit
    // mantissa: "412G" is easier to read at a glance than "412.4G".
    if unit == 0 || value >= 100.0 {
        format!("{value:.0}{}", UNITS[unit])
    } else {
        format!("{value:.1}{}", UNITS[unit])
    }
}

/// Render a throughput as a byte count per second.
pub fn rate(bytes_per_second: f64) -> String {
    format!("{}/s", bytes(bytes_per_second.max(0.0).round() as u64))
}

/// Render gibibytes with one decimal, matching the memory figures the Waybar
/// `memory` module used to show.
pub fn gibibytes(kibibytes: u64) -> String {
    format!("{:.1}G", kibibytes as f64 / (1024.0 * 1024.0))
}

/// The value column's width, wide enough for the longest reading
/// ("↓302B/s ↑220B/s") so the dimmed detail after it still lines up.
const VALUE_WIDTH: usize = 16;

/// One line of the widget: an icon, a value, and optional dimmed detail.
///
/// There is no name column. The icons are distinct enough to carry the meaning
/// on their own, and dropping six words of dim text is most of what makes the
/// block read like a control-center panel rather than a table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub icon: &'static str,
    pub value: String,
    pub detail: Option<String>,
    pub level: Level,
}

impl Row {
    pub fn new(icon: &'static str, value: String) -> Self {
        Row {
            icon,
            value,
            detail: None,
            level: Level::Normal,
        }
    }

    /// The value, padded into its column only when something follows it.
    fn column(&self) -> String {
        match self.detail {
            Some(_) => format!("{:<VALUE_WIDTH$}", self.value),
            None => self.value.clone(),
        }
    }

    pub fn detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    pub fn level(mut self, level: Level) -> Self {
        self.level = level;
        self
    }

    /// The row as Pango markup.
    pub fn markup(&self) -> String {
        let mut line = format!(
            "{}  {}",
            span(colour::ICON, self.icon),
            span(self.level.colour(), &self.column()),
        );
        if let Some(detail) = &self.detail {
            line.push_str("  ");
            line.push_str(&span(colour::NAME, detail));
        }
        line
    }

    /// The row as plain text, for `--plain` and for the tests.
    pub fn plain(&self) -> String {
        let mut line = format!("{}  {}", self.icon, self.column());
        if let Some(detail) = &self.detail {
            line.push_str("  ");
            line.push_str(detail);
        }
        line
    }
}

/// Join rows into the block the widget shows.
pub fn block(rows: &[Row], markup: bool) -> String {
    if rows.is_empty() {
        let unavailable = "No system readings available";
        return if markup {
            span(colour::NAME, unavailable)
        } else {
            unavailable.to_owned()
        };
    }
    rows.iter()
        .map(|row| if markup { row.markup() } else { row.plain() })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests;
