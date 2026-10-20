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

    let text = format!("<span size='large'>{}</span>", glyph(output.as_ref()));

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

/// The module's single glyph tracks the default output's volume and mute state.
/// Bluetooth power and connection state remain in the tooltip and CSS class.
pub(crate) fn glyph(output: Option<&AudioEntry>) -> &'static str {
    output.map(AudioEntry::volume_icon).unwrap_or("󰝟")
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

/// The Waybar middle-click action returns the state it settled on.
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
