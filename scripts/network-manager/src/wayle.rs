use std::env;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use nmrs::NetworkManager;

use crate::model::SecurityKind;
use crate::{AppResult, network};

pub async fn print_wifi_info(manager: &NetworkManager, expected_ssid: &str) -> AppResult<()> {
    let snapshot = network::snapshot(manager).await?;
    let entry = snapshot
        .wifi
        .iter()
        .find(|entry| entry.connected && entry.ssid() == expected_ssid)
        .ok_or_else(|| io::Error::other("the Wi-Fi connection changed while reading its details"))?;
    let info = manager.show_details(&entry.network).await?;

    let profile = entry
        .saved
        .as_ref()
        .map(|saved| {
            let name = nmcli_value(&[
                "--get-values",
                "connection.id",
                "connection",
                "show",
                "uuid",
                &saved.uuid,
            ])
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "Saved".to_owned());
            format!("{name} ({})", saved.uuid)
        })
        .unwrap_or_else(|| "Not saved".to_owned());

    let password = match entry.security_kind() {
        SecurityKind::Open => "None (open network)".to_owned(),
        SecurityKind::Enterprise => "Managed by the enterprise profile".to_owned(),
        _ if entry.saved.is_none() => "Unavailable (network is not saved)".to_owned(),
        _ => entry
            .saved
            .as_ref()
            .and_then(|saved| saved_secret(&saved.uuid, "802-11-wireless-security.psk"))
            .or_else(|| {
                entry.saved.as_ref().and_then(|saved| {
                    saved_secret(&saved.uuid, "802-11-wireless-security.wep-key0")
                })
            })
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "Unavailable from NetworkManager".to_owned()),
    };

    let dns = device_dns(entry.interface());
    let dns = if dns.is_empty() {
        "Not assigned".to_owned()
    } else {
        dns.join(", ")
    };
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
        .map(|rate| format!("{:.1} Mbps", rate as f64))
        .unwrap_or_else(|| "Unknown".to_owned());
    let ipv4 = info.ip4_address.as_deref().unwrap_or("Not assigned");
    let ipv6 = info.ip6_address.as_deref().unwrap_or("Not assigned");

    print!(
        "Network (SSID: {})\n\n\
         Signal:     {}%\n\
         Profile:    {profile}\n\
         Security:   {}\n\
         Interface:  {}\n\
         Password:   {password}\n\n\
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
        entry.security_label(),
        entry.interface(),
        info.bssid,
        info.mode,
    );
    Ok(())
}

pub async fn write_wifi_qr(manager: &NetworkManager, expected_ssid: &str) -> AppResult<()> {
    let snapshot = network::snapshot(manager).await?;
    let entry = snapshot
        .wifi
        .iter()
        .find(|entry| entry.connected && entry.ssid() == expected_ssid)
        .ok_or_else(|| io::Error::other("the Wi-Fi connection changed while creating its QR code"))?;
    let saved = entry
        .saved
        .as_ref()
        .ok_or_else(|| io::Error::other("no saved NetworkManager profile was found for this connection"))?;

    if entry.security_kind() == SecurityKind::Enterprise {
        return Err(io::Error::other(
            "enterprise Wi-Fi credentials cannot be represented by this QR format",
        )
        .into());
    }
    if entry.security_kind() == SecurityKind::EnhancedOpen {
        return Err(io::Error::other(
            "Enhanced Open (OWE) networks are not supported by the Wi-Fi QR format",
        )
        .into());
    }

    let hidden = nmcli_value(&[
        "--get-values",
        "802-11-wireless.hidden",
        "connection",
        "show",
        "uuid",
        &saved.uuid,
    ])
    .is_some_and(|value| matches!(value.trim().to_ascii_lowercase().as_str(), "yes" | "true"));

    let key_mgmt = nmcli_value(&[
        "--get-values",
        "802-11-wireless-security.key-mgmt",
        "connection",
        "show",
        "uuid",
        &saved.uuid,
    ])
    .unwrap_or_default();

    let (authentication, password) = match entry.security_kind() {
        SecurityKind::Open => ("nopass", None),
        SecurityKind::Personal => {
            let password = saved_secret(&saved.uuid, "802-11-wireless-security.psk")
                .filter(|value| !value.is_empty())
                .ok_or_else(|| io::Error::other("NetworkManager did not return the saved WPA password"))?;
            ("WPA", Some(password))
        }
        SecurityKind::Legacy => {
            let password = saved_secret(&saved.uuid, "802-11-wireless-security.wep-key0")
                .filter(|value| !value.is_empty())
                .ok_or_else(|| io::Error::other("NetworkManager did not return the saved WEP key"))?;
            ("WEP", Some(password))
        }
        SecurityKind::Enterprise | SecurityKind::EnhancedOpen => unreachable!(),
    };

    if matches!(key_mgmt.as_str(), "wpa-eap" | "wpa-eap-suite-b-192" | "ieee8021x") {
        return Err(io::Error::other(
            "enterprise Wi-Fi credentials cannot be represented by this QR format",
        )
        .into());
    }
    if key_mgmt == "owe" {
        return Err(io::Error::other(
            "Enhanced Open (OWE) networks are not supported by the Wi-Fi QR format",
        )
        .into());
    }

    let payload = wifi_qr_payload(expected_ssid, authentication, password.as_deref(), hidden);
    let png = qr_png(&payload)?;
    io::stdout().lock().write_all(&png)?;
    Ok(())
}

fn saved_secret(uuid: &str, field: &str) -> Option<String> {
    nmcli_value(&[
        "--show-secrets",
        "--get-values",
        field,
        "connection",
        "show",
        "uuid",
        uuid,
    ])
}

fn device_dns(interface: &str) -> Vec<String> {
    let Some(values) = nmcli_value(&[
        "--get-values",
        "IP4.DNS,IP6.DNS",
        "device",
        "show",
        interface,
    ]) else {
        return Vec::new();
    };
    let mut servers = Vec::new();
    for value in values.lines().map(str::trim).filter(|value| !value.is_empty()) {
        if !servers.iter().any(|server| server == value) {
            servers.push(value.to_owned());
        }
    }
    servers
}

fn nmcli_value(args: &[&str]) -> Option<String> {
    let output = Command::new(nmcli_binary()).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout)
        .ok()
        .map(|value| value.trim_end_matches(['\r', '\n']).to_owned())
}

fn qr_png(payload: &str) -> AppResult<Vec<u8>> {
    let mut child = Command::new(qrencode_binary())
        .args(["-t", "PNG", "-s", "10", "-m", "4", "-o", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut input = child
        .stdin
        .take()
        .ok_or_else(|| io::Error::other("qrencode standard input is unavailable"))?;
    input.write_all(payload.as_bytes())?;
    drop(input);
    let output = child.wait_with_output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "qrencode failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
        .into());
    }
    if output.stdout.is_empty() {
        return Err(io::Error::other("qrencode returned an empty image").into());
    }
    Ok(output.stdout)
}

fn nmcli_binary() -> PathBuf {
    env::var_os("NETWORK_MANAGER_NMCLI")
        .or_else(|| env::var_os("ROFI_NETWORK_NMCLI"))
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new("nmcli").to_path_buf())
}

fn qrencode_binary() -> PathBuf {
    env::var_os("NETWORK_MANAGER_QRENCODE")
        .or_else(|| env::var_os("ROFI_NETWORK_QRENCODE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new("qrencode").to_path_buf())
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

fn wifi_qr_payload(
    ssid: &str,
    authentication: &str,
    password: Option<&str>,
    hidden: bool,
) -> String {
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
    payload.push_str(if hidden { "H:true;;" } else { "H:false;;" });
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payload_escapes_wifi_qr_delimiters() {
        assert_eq!(
            wifi_qr_payload("Cafe;Guest", "WPA", Some("a:b,c\\d"), false),
            "WIFI:T:WPA;S:Cafe\\;Guest;P:a\\:b\\,c\\\\d;H:false;;"
        );
    }

    #[test]
    fn frequency_bands_include_wifi_6e() {
        assert_eq!(frequency_band(2_437), "2.4 GHz");
        assert_eq!(frequency_band(5_180), "5 GHz");
        assert_eq!(frequency_band(6_115), "6 GHz");
    }

    #[test]
    fn payload_marks_hidden_open_networks() {
        assert_eq!(
            wifi_qr_payload("Hidden", "nopass", None, true),
            "WIFI:T:nopass;S:Hidden;H:true;;"
        );
    }
}
