use std::collections::HashMap;

use nmrs::{DeviceState, NetworkManager, SettingsSummary};

use crate::AppResult;
use crate::model::{EthernetEntry, SavedWifi, Snapshot, WifiEntry, signal_bars};

pub async fn snapshot(manager: &NetworkManager) -> AppResult<Snapshot> {
    let networks = manager.list_networks(None).await?;
    let wifi_devices = manager.list_wifi_devices().await?;
    let saved_connections = manager.list_saved_connections().await?;
    let wired_devices = manager.list_wired_device_details().await?;

    let device_states: HashMap<_, _> = wifi_devices
        .iter()
        .map(|device| {
            (
                device.interface.as_str(),
                (&device.state, device.active_ssid.as_deref()),
            )
        })
        .collect();

    let mut saved_wifi = HashMap::<String, SavedWifi>::new();
    for profile in saved_connections {
        if let SettingsSummary::Wifi { ssid, .. } = profile.summary {
            saved_wifi
                .entry(ssid)
                .or_insert(SavedWifi { uuid: profile.uuid });
        }
    }

    let mut wifi = Vec::with_capacity(networks.len());
    for network in networks {
        let (connected, connecting) = device_states
            .get(network.device.as_str())
            .map(|&(state, active_ssid)| {
                (
                    network.is_active && matches!(state, DeviceState::Activated),
                    state.is_transitional()
                        && (network.is_active || active_ssid == Some(network.ssid.as_str())),
                )
            })
            .unwrap_or((network.is_active, false));
        let saved = saved_wifi.get(&network.ssid).cloned();
        wifi.push(WifiEntry {
            network,
            saved,
            connected,
            connecting,
        });
    }
    wifi.sort_by(|left, right| {
        wifi_rank(left)
            .cmp(&wifi_rank(right))
            .then_with(|| signal_bars(right.strength()).cmp(&signal_bars(left.strength())))
            .then_with(|| left.ssid().to_lowercase().cmp(&right.ssid().to_lowercase()))
    });

    let mut ethernet: Vec<_> = wired_devices
        .into_iter()
        .map(|device| EthernetEntry { device })
        .collect();
    ethernet.sort_by(|left, right| {
        ethernet_rank(left)
            .cmp(&ethernet_rank(right))
            .then_with(|| left.device.interface.cmp(&right.device.interface))
    });

    Ok(Snapshot { wifi, ethernet })
}

fn wifi_rank(entry: &WifiEntry) -> u8 {
    if entry.connected || entry.connecting {
        0
    } else if entry.is_saved() {
        1
    } else {
        2
    }
}

fn ethernet_rank(entry: &EthernetEntry) -> u8 {
    if entry.connected() || entry.connecting() {
        0
    } else {
        1
    }
}

pub async fn print_waybar_status(manager: &NetworkManager) {
    match snapshot(manager).await {
        Ok(snapshot) => {
            if let Some(entry) = snapshot.ethernet.iter().find(|entry| entry.connected()) {
                let address = entry.device.ip4_address.as_deref().unwrap_or("No IPv4");
                println!(
                    "{{\"text\":\"<span size='large'>󰈀</span>\",\"tooltip\":\"{}\",\"class\":\"ethernet\"}}",
                    json_escape(&format!(
                        "Ethernet: {}\nIPv4: {address}",
                        entry.device.interface
                    ))
                );
            } else if let Some(entry) = snapshot.wifi.iter().find(|entry| entry.connected) {
                let address = entry.network.ip4_address.as_deref().unwrap_or("No IPv4");
                println!(
                    "{{\"text\":\"<span size='large'>{}</span>\",\"tooltip\":\"{}\",\"class\":\"wifi\"}}",
                    crate::model::signal_icon(entry.strength()),
                    json_escape(&format!(
                        "Wi-Fi: {}\nSignal: {}%\nSecurity: {}\nIPv4: {address}",
                        entry.ssid(),
                        entry.strength(),
                        entry.security_label()
                    ))
                );
            } else if snapshot.wifi.iter().any(|entry| entry.connecting)
                || snapshot.ethernet.iter().any(|entry| entry.connecting())
            {
                println!(
                    "{{\"text\":\"<span size='large'>󰔟</span>\",\"tooltip\":\"Connecting…\",\"class\":\"connecting\"}}"
                );
            } else {
                println!(
                    "{{\"text\":\"<span size='large'>󰤭</span>\",\"tooltip\":\"Network disconnected\",\"class\":\"offline\"}}"
                );
            }
        }
        Err(error) => println!(
            "{{\"text\":\"󰤭\",\"tooltip\":\"{}\",\"class\":\"offline\"}}",
            json_escape(&format!("Cannot read network state: {error}"))
        ),
    }
}

pub fn json_escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            character if character.is_control() => {
                use std::fmt::Write as _;
                let _ = write!(escaped, "\\u{:04x}", u32::from(character));
            }
            character => escaped.push(character),
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waybar_json_escapes_markup_tooltips_safely() {
        assert_eq!(
            json_escape("A \"network\"\nline"),
            "A \\\"network\\\"\\nline"
        );
    }

}
