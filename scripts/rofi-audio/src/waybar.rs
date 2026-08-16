use crate::bluetooth::Backend;
use crate::model::{AudioEntry, json_escape};
use crate::{AppResult, audio};

/// Waybar JSON for the merged module: the default output's volume glyph and a
/// Bluetooth glyph, with everything else in the tooltip. Both halves degrade
/// on their own, so a stopped bluetoothd still leaves the volume readable.
pub async fn print_status() {
    let (output, input) = audio::defaults();
    let bluetooth = match Backend::new().await {
        Ok(backend) => backend.status().await.ok(),
        Err(_) => None,
    };
    let (powered, connected) = bluetooth.unwrap_or((false, Vec::new()));

    // One glyph: the default output's volume state. Bluetooth lives in the
    // tooltip rather than taking a second slot on the bar.
    let audio_glyph = output.as_ref().map(AudioEntry::volume_icon).unwrap_or("󰝟");
    let text = format!("<span size='large'>{audio_glyph}</span>");

    let mut tooltip = vec![device_line("Output", output.as_ref())];
    tooltip.push(device_line("Input", input.as_ref()));
    tooltip.push(match (powered, connected.is_empty()) {
        (false, _) => "Bluetooth: off".to_owned(),
        (true, true) => "Bluetooth: on, nothing connected".to_owned(),
        (true, false) => format!("Bluetooth: {}", connected.join(", ")),
    });

    let class = match output.as_ref() {
        Some(entry) if entry.muted => "muted",
        Some(_) => "active",
        None => "unavailable",
    };
    println!(
        "{{\"text\":\"{}\",\"tooltip\":\"{}\",\"class\":\"{class}\"}}",
        json_escape(&text),
        json_escape(&tooltip.join("\n")),
    );
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
