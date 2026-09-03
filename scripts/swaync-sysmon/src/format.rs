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

/// How wide each cell's text is, counted from the icon's trailing space.
///
/// Two cells share a 380px panel, so this is the widest the longest reading
/// ("↓302B/s ↑220B/s") can be and still leave the second column room. Every
/// cell holds exactly one icon, so padding the text alone keeps the second
/// column aligned regardless of what the glyph's real advance width is.
const CELL_WIDTH: usize = 18;

/// What separates the two columns.
const COLUMN_GAP: &str = "  ";

/// How many readings share a line.
const COLUMNS: usize = 2;

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

    /// The cell's text: the value, then the dimmed detail.
    fn text(&self) -> String {
        match &self.detail {
            Some(detail) => format!("{} {detail}", self.value),
            None => self.value.clone(),
        }
    }

    /// The spaces that carry the next column out to its own start.
    fn padding(&self) -> String {
        " ".repeat(CELL_WIDTH.saturating_sub(self.text().chars().count()))
    }

    pub fn detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    pub fn level(mut self, level: Level) -> Self {
        self.level = level;
        self
    }

    /// The cell as Pango markup.
    pub fn markup(&self) -> String {
        let mut cell = format!(
            "{}  {}",
            span(colour::ICON, self.icon),
            span(self.level.colour(), &self.value),
        );
        if let Some(detail) = &self.detail {
            cell.push(' ');
            cell.push_str(&span(colour::NAME, detail));
        }
        cell
    }

    /// The cell as plain text, for `--plain` and for the tests.
    pub fn plain(&self) -> String {
        format!("{}  {}", self.icon, self.text())
    }
}

/// Lay the readings out two to a line.
///
/// A single column left the panel narrow and tall next to everything else in
/// the control center; paired cells fit the readings into half the height at
/// the width the panel already has.
pub fn block(rows: &[Row], markup: bool) -> String {
    if rows.is_empty() {
        let unavailable = "No system readings available";
        return if markup {
            span(colour::NAME, unavailable)
        } else {
            unavailable.to_owned()
        };
    }
    rows.chunks(COLUMNS)
        .map(|line| render_line(line, markup))
        .collect::<Vec<_>>()
        .join("\n")
}

/// One line of cells. The last cell on a line needs no padding after it.
fn render_line(cells: &[Row], markup: bool) -> String {
    let mut line = String::new();
    for (index, cell) in cells.iter().enumerate() {
        line.push_str(&if markup { cell.markup() } else { cell.plain() });
        if index + 1 < cells.len() {
            line.push_str(&cell.padding());
            line.push_str(COLUMN_GAP);
        }
    }
    line
}

#[cfg(test)]
mod tests;
