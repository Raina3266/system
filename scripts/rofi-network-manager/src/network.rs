use std::collections::{HashMap, HashSet};
use std::env;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use nmrs::{ConnectionError, DeviceState, NetworkManager, SettingsSummary, WifiSecurity};

use crate::AppResult;
use crate::model::{
    EthernetEntry, InfoContent, SavedWifi, SecurityKind, Snapshot, WifiEntry, hex_encode, stable_id,
};

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
        let key = format!(
            "wifi:{}:{}",
            hex_encode(&network.device),
            hex_encode(&network.ssid)
        );
        let saved = saved_wifi.get(&network.ssid).cloned();
        wifi.push(WifiEntry {
            key,
            network,
            saved,
            connected,
            connecting,
        });
    }
    wifi.sort_by(|left, right| {
        wifi_rank(left)
            .cmp(&wifi_rank(right))
            .then_with(|| right.strength().cmp(&left.strength()))
            .then_with(|| left.ssid().to_lowercase().cmp(&right.ssid().to_lowercase()))
    });

    let mut ethernet: Vec<_> = wired_devices
        .into_iter()
        .map(|device| EthernetEntry {
            key: format!("ethernet:{}", hex_encode(&device.interface)),
            device,
        })
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

pub async fn scan(manager: &NetworkManager) -> Result<(), ConnectionError> {
    manager.scan_networks(None).await
}

pub async fn connect_wifi(
    manager: &NetworkManager,
    entry: &WifiEntry,
    password: Option<String>,
) -> AppResult<()> {
    if entry.connected || entry.connecting {
        manager.wifi(entry.interface()).disconnect().await?;
        return Ok(());
    }

    let credentials = if entry.is_saved() {
        if entry.security_kind() == SecurityKind::Open {
            WifiSecurity::Open
        } else {
            // nmrs treats an empty PSK as "activate this saved profile and let
            // NetworkManager retrieve its stored secret".
            WifiSecurity::WpaPsk { psk: String::new() }
        }
    } else {
        match entry.security_kind() {
            SecurityKind::Open => WifiSecurity::Open,
            SecurityKind::Personal => WifiSecurity::WpaPsk {
                psk: password.ok_or_else(|| io::Error::other("a password is required"))?,
            },
            SecurityKind::Enterprise => {
                return Err(io::Error::other(
                    "Enterprise Wi-Fi needs identity and certificate settings; save its NetworkManager profile first",
                )
                .into());
            }
            SecurityKind::EnhancedOpen => {
                return nmcli_connect_without_password(entry.interface(), entry.ssid());
            }
            SecurityKind::Legacy => {
                return Err(io::Error::other(
                    "New WEP profiles are intentionally unsupported; create a NetworkManager profile first",
                )
                .into());
            }
        }
    };

    manager
        .wifi(entry.interface())
        .connect(entry.ssid(), credentials)
        .await?;
    Ok(())
}

pub async fn forget_wifi(manager: &NetworkManager, entry: &WifiEntry) -> AppResult<()> {
    manager.forget(entry.ssid()).await?;
    Ok(())
}

pub fn connect_ethernet(entry: &EthernetEntry) -> AppResult<()> {
    let operation = if entry.connected() || entry.connecting() {
        "disconnect"
    } else {
        "connect"
    };
    let output = Command::new(nmcli_binary())
        .args(["device", operation, &entry.device.interface])
        .output()?;
    command_succeeded("change Ethernet connection", output)
}

pub async fn wifi_info(manager: &NetworkManager, entry: &WifiEntry) -> AppResult<InfoContent> {
    let info = manager.show_details(&entry.network).await?;
    let password = entry
        .saved
        .as_ref()
        .and_then(|profile| saved_wifi_password(&profile.uuid).ok().flatten());
    let dns = device_dns(entry.interface()).unwrap_or_default();
    let frequency = info
        .freq
        .map(|frequency| format!("{frequency} MHz ({})", frequency_band(frequency)))
        .unwrap_or_else(|| "Unknown".to_owned());
    let channel = info
        .channel
        .map(|channel| channel.to_string())
        .unwrap_or_else(|| "Unknown".to_owned());
    let rate = info
        .rate_mbps
        .map(|rate| format!("{rate} Mbps"))
        .unwrap_or_else(|| "Unknown".to_owned());
    let ipv4 = info.ip4_address.as_deref().unwrap_or("Not assigned");
    let ipv6 = info.ip6_address.as_deref().unwrap_or("Not assigned");
    let dns = if dns.is_empty() {
        "Not assigned".to_owned()
    } else {
        dns.join(", ")
    };
    let profile = entry
        .saved
        .as_ref()
        .map(|saved| format!("Saved ({})", saved.uuid))
        .unwrap_or_else(|| "Not saved".to_owned());
    let password_line = if entry.security_kind() == SecurityKind::Open {
        "None (open network)".to_owned()
    } else if entry.security_kind() == SecurityKind::Enterprise {
        "Managed by the enterprise profile".to_owned()
    } else if entry.saved.is_none() {
        "Unavailable (network is not saved)".to_owned()
    } else {
        password
            .as_deref()
            .filter(|password| !password.is_empty())
            .unwrap_or("Unavailable from NetworkManager")
            .to_owned()
    };

    let details = format!(
        "Network (SSID: {})\n\n\
         Signal:     {}% ({})\n\
         Profile:    {profile}\n\
         Security:   {}\n\
         Interface:  {}\n\
         Password:   {password_line}\n\n\
         Addresses\n\n\
         IPv4:       {ipv4}\n\
         IPv6:       {ipv6}\n\
         DNS:        {dns}\n\n\
         Radio\n\n\
         BSSID:      {}\n\
         Frequency:  {frequency}\n\
         Channel:    {channel}\n\
         Mode:       {}\n\
         Link rate:  {rate}",
        info.ssid,
        info.strength,
        info.bars,
        entry.security_label(),
        entry.interface(),
        info.bssid,
        info.mode,
    );

    let qr_payload = match entry.security_kind() {
        SecurityKind::Open => Some(wifi_qr_payload(entry.ssid(), "nopass", None)),
        SecurityKind::Personal => password
            .as_deref()
            .filter(|password| !password.is_empty())
            .map(|password| wifi_qr_payload(entry.ssid(), "WPA", Some(password))),
        SecurityKind::Enterprise | SecurityKind::EnhancedOpen | SecurityKind::Legacy => None,
    };

    Ok(InfoContent {
        id: stable_id(&entry.key),
        details,
        qr_payload,
    })
}

pub fn ethernet_info(entry: &EthernetEntry) -> InfoContent {
    let ipv4 = entry
        .device
        .ip4_address
        .as_deref()
        .unwrap_or("Not assigned");
    let ipv6 = entry
        .device
        .ip6_address
        .as_deref()
        .unwrap_or("Not assigned");
    let speed = entry
        .device
        .speed_mbps
        .map(|speed| format!("{speed} Mbps"))
        .unwrap_or_else(|| "Unknown".to_owned());
    let profile = entry
        .device
        .active_connection_id
        .as_deref()
        .unwrap_or("None");
    let dns = device_dns(&entry.device.interface)
        .map(|addresses| addresses.join(", "))
        .ok()
        .filter(|addresses| !addresses.is_empty())
        .unwrap_or_else(|| "Not assigned".to_owned());
    let details = format!(
        "Ethernet\n\n\
         Interface:  {}\n\
         Profile:    {profile}\n\
         MAC:        {}\n\
         Permanent:  {}\n\
         Link speed: {speed}\n\n\
         Addresses\n\n\
         IPv4:       {ipv4}\n\
         IPv6:       {ipv6}\n\
         DNS:        {dns}\n\n\
         QR codes are only available for Wi-Fi connections.",
        entry.device.interface,
        entry.device.hw_address,
        entry
            .device
            .permanent_hw_address
            .as_deref()
            .unwrap_or("Unknown"),
    );
    InfoContent {
        id: stable_id(&entry.key),
        details,
        qr_payload: None,
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

pub fn connection_error_message(ssid: &str, error: &(dyn std::error::Error + 'static)) -> String {
    if let Some(error) = error.downcast_ref::<ConnectionError>() {
        return match error {
            ConnectionError::AuthFailed
            | ConnectionError::SupplicantConfigFailed
            | ConnectionError::SupplicantTimeout => {
                format!("Wrong password for {ssid}.")
            }
            ConnectionError::NotFound => format!("{ssid} is no longer available."),
            ConnectionError::DhcpFailed => format!("No IP address for {ssid}."),
            ConnectionError::Timeout => format!("{ssid} timed out."),
            ConnectionError::MissingPassword => format!("{ssid} requires a password."),
            _ => format!("Cannot connect to {ssid}: {error}"),
        };
    }
    format!("Cannot connect to {ssid}: {error}")
}

fn saved_wifi_password(uuid: &str) -> AppResult<Option<String>> {
    let output = Command::new(nmcli_binary())
        .args([
            "--show-secrets",
            "--get-values",
            "802-11-wireless-security.psk",
            "connection",
            "show",
            "uuid",
            uuid,
        ])
        .output()?;
    if !output.status.success() {
        return Ok(None);
    }
    let password = String::from_utf8(output.stdout)?;
    let password = password
        .trim_end_matches(|character| character == '\r' || character == '\n')
        .to_owned();
    Ok((!password.is_empty()).then_some(password))
}

fn device_dns(interface: &str) -> AppResult<Vec<String>> {
    let output = Command::new(nmcli_binary())
        .args([
            "--get-values",
            "IP4.DNS,IP6.DNS",
            "device",
            "show",
            interface,
        ])
        .output()?;
    if !output.status.success() {
        return Ok(Vec::new());
    }
    let values = String::from_utf8(output.stdout)?;
    let mut seen = HashSet::new();
    Ok(values
        .lines()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .filter(|value| seen.insert((*value).to_owned()))
        .map(str::to_owned)
        .collect())
}

fn nmcli_connect_without_password(interface: &str, ssid: &str) -> AppResult<()> {
    let output = Command::new(nmcli_binary())
        .args(["device", "wifi", "connect", ssid, "ifname", interface])
        .output()?;
    command_succeeded("connect to enhanced-open Wi-Fi", output)
}

fn command_succeeded(context: &str, output: Output) -> AppResult<()> {
    if output.status.success() {
        return Ok(());
    }
    let detail = String::from_utf8_lossy(&output.stderr);
    Err(io::Error::other(format!("{context}: {}", detail.trim())).into())
}

fn nmcli_binary() -> PathBuf {
    env::var_os("ROFI_NETWORK_NMCLI")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new("nmcli").to_path_buf())
}

fn frequency_band(frequency: u32) -> &'static str {
    if frequency >= 59_400 {
        "60 GHz"
    } else if frequency >= 5_925 {
        "6 GHz"
    } else if frequency >= 4_900 {
        "5 GHz"
    } else {
        "2.4 GHz"
    }
}

fn wifi_qr_payload(ssid: &str, authentication: &str, password: Option<&str>) -> String {
    let mut payload = format!(
        "WIFI:T:{};S:{};",
        wifi_qr_escape(authentication),
        wifi_qr_escape(ssid)
    );
    if let Some(password) = password {
        payload.push_str("P:");
        payload.push_str(&wifi_qr_escape(password));
        payload.push(';');
    }
    payload.push_str("H:false;;");
    payload
}

fn wifi_qr_escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        if matches!(character, '\\' | ';' | ',' | ':' | '"') {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped
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
    fn wifi_qr_escapes_delimiters_and_backslashes() {
        assert_eq!(
            wifi_qr_payload("Cafe;Guest", "WPA", Some("a:b,c\\d")),
            "WIFI:T:WPA;S:Cafe\\;Guest;P:a\\:b\\,c\\\\d;H:false;;"
        );
    }

    #[test]
    fn waybar_json_escapes_markup_tooltips_safely() {
        assert_eq!(
            json_escape("A \"network\"\nline"),
            "A \\\"network\\\"\\nline"
        );
    }

    #[test]
    fn frequency_bands_include_wifi_6e() {
        assert_eq!(frequency_band(2_437), "2.4 GHz");
        assert_eq!(frequency_band(5_180), "5 GHz");
        assert_eq!(frequency_band(6_115), "6 GHz");
    }
}
