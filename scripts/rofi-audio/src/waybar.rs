use crate::bluetooth::Backend;
use crate::model::{AudioEntry, json_escape};
use crate::{AppResult, audio};

/// Waybar JSON for the merged module: one glyph, with everything else in the
/// tooltip. Both halves degrade on their own, so a stopped bluetoothd still
/// leaves the audio state readable.
pub async fn print_status() {
    let (output, input) = audio::defaults();
    let bluetooth = match Backend::new().await {
        Ok(backend) => backend.status().await.ok(),
        Err(_) => None,
    };
    let (powered, connected) = bluetooth.unwrap_or((false, Vec::new()));

    let text = format!(
        "<span size='large'>{}</span>",
        glyph(output.as_ref(), powered, !connected.is_empty())
    );

    let mut tooltip = vec![device_line("Output", output.as_ref())];
    tooltip.push(device_line("Input", input.as_ref()));
    tooltip.push(match (powered, connected.is_empty()) {
        (false, _) => "Bluetooth: off".to_owned(),
        (true, true) => "Bluetooth: on, nothing connected".to_owned(),
        (true, false) => format!("Bluetooth: {}", connected.join(", ")),
    });

    // Styling hook for waybar.css, so the glyph can also be recoloured per
    // state without changing this program.
    let class = match (powered, connected.is_empty(), output.as_ref()) {
        (true, false, _) => "bluetooth-connected",
        (true, true, _) => "bluetooth-on",
        (false, _, Some(entry)) if entry.muted => "muted",
        (false, _, Some(_)) => "active",
        (false, _, None) => "unavailable",
    };
    println!(
        "{{\"text\":\"{}\",\"tooltip\":\"{}\",\"class\":\"{class}\"}}",
        json_escape(&text),
        json_escape(&tooltip.join("\n")),
    );
}

/// The module's single glyph.
///
/// Bluetooth off is the plain sound icon, tracking the default output's level
/// and mute state. Bluetooth on swaps it for a Bluetooth glyph, distinct at a
/// glance and distinguishing an idle adapter from a connected one. Nothing is
/// lost by giving the slot over to Bluetooth while it is on: the separate
/// `pulseaudio` module in the top-left group already shows the volume and its
/// own mute glyph.
fn glyph(output: Option<&AudioEntry>, powered: bool, connected: bool) -> &'static str {
    match (powered, connected) {
        (true, true) => "󰂱",
        (true, false) => "󰂯",
        (false, _) => output.map(AudioEntry::volume_icon).unwrap_or("󰝟"),
    }
}

fn device_line(label: &str, entry: Option<&AudioEntry>) -> String {
    match entry {
        Some(entry) if entry.muted => {
            format!("{label}: {} ({}%, muted)", entry.label, entry.volume)
        }
        Some(entry) => format!("{label}: {} ({}%)", entry.label, entry.volume),
        None => format!("{label}: none"),
    }
}

/// `on-click-right` on the Waybar module. Returns the state it settled on.
pub async fn set_bluetooth_power(argument: &str) -> AppResult<bool> {
    let backend = Backend::new().await?;
    let powered = match argument {
        "on" => true,
        "off" => false,
        _ => !backend.is_powered().await?,
    };
    backend.set_powered(powered).await?;
    Ok(powered)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::AudioKind;

    fn output(volume: u8, muted: bool) -> AudioEntry {
        AudioEntry {
            key: "sink:00".to_owned(),
            kind: AudioKind::Output,
            name: "alsa_output.pci".to_owned(),
            description: "Built-in Audio".to_owned(),
            label: "Built-in Audio".to_owned(),
            port: None,
            volume,
            muted,
            default: true,
        }
    }

    #[test]
    fn bluetooth_on_and_off_are_different_glyphs() {
        let off = glyph(Some(&output(60, false)), false, false);
        let on = glyph(Some(&output(60, false)), true, false);
        let on_connected = glyph(Some(&output(60, false)), true, true);
        assert_eq!(off, "󰕾");
        assert_eq!(on, "󰂯");
        assert_eq!(on_connected, "󰂱");
        assert_ne!(off, on);
        assert_ne!(on, on_connected);
    }

    #[test]
    fn with_bluetooth_off_the_glyph_tracks_volume_and_mute() {
        assert_eq!(glyph(Some(&output(0, false)), false, false), "󰕿");
        assert_eq!(glyph(Some(&output(30, false)), false, false), "󰖀");
        assert_eq!(glyph(Some(&output(90, false)), false, false), "󰕾");
        assert_eq!(glyph(Some(&output(90, true)), false, false), "󰝟");
        // No default output at all, and Bluetooth off: nothing to report.
        assert_eq!(glyph(None, false, false), "󰝟");
    }
}
