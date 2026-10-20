use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use std::{
    env,
    io::{Read, Write},
    process::{Command, Stdio},
};

type Result<T> = std::result::Result<T, String>;

fn nm(args: &[&str]) -> Result<String> {
    let output =
        Command::new(env::var_os("NETWORK_MANAGER_NMCLI").unwrap_or_else(|| "nmcli".into()))
            .env("LC_ALL", "C")
            .args(["--terse", "--escape", "no"])
            .args(args)
            .output()
            .map_err(|_| "could not start nmcli")?;
    if !output.status.success() {
        // Never forward subprocess diagnostics: they can contain credentials.
        return Err(
            "NetworkManager read failed (device/profile unavailable or permission denied)".into(),
        );
    }
    String::from_utf8(output.stdout)
        .map(|s| s.strip_suffix('\n').unwrap_or(&s).to_owned())
        .map_err(|_| "NetworkManager returned invalid UTF-8".into())
}

fn field(kind: &str, id: &str, key: &str) -> Result<String> {
    if kind == "connection" {
        nm(&["--get-values", key, kind, "show", "uuid", id])
    } else {
        nm(&["--get-values", key, kind, "show", id])
    }
}

#[derive(Clone, PartialEq, Eq)]
struct Identity {
    interface: String,
    uuid: String,
    ssid: String,
    bssid: String,
    active_path: String,
}

fn split_row(row: &str) -> Vec<String> {
    let mut fields = vec![String::new()];
    let mut escaped = false;
    for c in row.chars() {
        if escaped {
            fields.last_mut().unwrap().push(c);
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else if c == ':' {
            fields.push(String::new());
        } else {
            fields.last_mut().unwrap().push(c);
        }
    }
    fields
}

fn active_ap(interface: &str) -> Result<Vec<String>> {
    let rows = nm(&[
        "--escape",
        "yes",
        "--get-values",
        "IN-USE,SSID,BSSID,FREQ,CHAN,MODE,RATE",
        "device",
        "wifi",
        "list",
        "ifname",
        interface,
        "--rescan",
        "no",
    ])?;
    rows.lines()
        .map(split_row)
        .find(|r| r.len() == 7 && r[0] == "*")
        .ok_or_else(|| "device has no active access point".into())
}

fn identity(interface: &str, expected: &str) -> Result<Identity> {
    let state = field("device", interface, "GENERAL.STATE")?;
    if state.split_whitespace().next() != Some("100") {
        return Err("selected device is not activated".into());
    }
    let active_path = field("device", interface, "GENERAL.CON-PATH")?;
    if active_path.is_empty() || active_path == "--" || active_path == "/" {
        return Err("device has no active connection object".into());
    }
    let uuid = field("device", interface, "GENERAL.CON-UUID")?;
    if uuid.is_empty() || uuid == "--" {
        return Err("device has no active profile".into());
    }
    let kind = field("connection", &uuid, "connection.type")?;
    let bssid = if kind == "802-11-wireless" {
        if expected.is_empty() {
            return Err("empty expected SSID requires a wired device".into());
        }
        let ap = active_ap(interface)?;
        if ap[1] != expected {
            return Err("active SSID changed".into());
        }
        ap[2].clone()
    } else {
        if kind != "802-3-ethernet" || !expected.is_empty() {
            return Err("selected device does not match expected network type/SSID".into());
        }
        String::new()
    };
    if field("device", interface, "GENERAL.CON-UUID")? != uuid
        || field("device", interface, "GENERAL.CON-PATH")? != active_path
    {
        return Err("active profile changed while resolving device".into());
    }
    Ok(Identity {
        interface: interface.into(),
        uuid,
        ssid: expected.into(),
        bssid,
        active_path,
    })
}

fn revalidate(original: &Identity) -> Result<()> {
    if identity(&original.interface, &original.ssid)? != *original {
        return Err("active network changed; discard result and retry".into());
    }
    Ok(())
}

fn security(id: &Identity) -> Result<String> {
    if id.bssid.is_empty() {
        return Ok("wired".into());
    }
    field("connection", &id.uuid, "802-11-wireless-security.key-mgmt")
}

fn password(id: &Identity, security: &str) -> Result<String> {
    let key = match security {
        "" => return Ok(String::new()),
        "wpa-psk" | "sae" => "802-11-wireless-security.psk".to_owned(),
        "none" => {
            let index = field(
                "connection",
                &id.uuid,
                "802-11-wireless-security.wep-tx-keyidx",
            )?;
            if !matches!(index.as_str(), "0" | "1" | "2" | "3") {
                return Err("invalid WEP key index".into());
            }
            format!("802-11-wireless-security.wep-key{index}")
        }
        "owe" => return Err("Enhanced Open (OWE) secret/QR is unsupported".into()),
        "wired" => return Err("wired connections have no Wi-Fi secret/QR".into()),
        _ => return Err("enterprise/managed Wi-Fi secret/QR is unsupported".into()),
    };
    let value = nm(&[
        "--show-secrets",
        "--get-values",
        &key,
        "connection",
        "show",
        "uuid",
        &id.uuid,
    ])?;
    if value.is_empty() {
        return Err("saved Wi-Fi secret unavailable (agent-owned or permission denied)".into());
    }
    Ok(value)
}

fn info(id: &Identity, security: &str) -> Result<Value> {
    let dev = |key| field("device", &id.interface, key);
    let mut value = json!({
        "interface": id.interface, "ssid": id.ssid,
        "profileUuid": id.uuid, "profileId": field("connection", &id.uuid, "connection.id")?,
        "security": security, "ipv4": dev("IP4.ADDRESS")?, "ipv6": dev("IP6.ADDRESS")?,
        "dns": format!("{}\n{}", dev("IP4.DNS")?, dev("IP6.DNS")?).trim(),
        "gateway4": dev("IP4.GATEWAY")?, "gateway6": dev("IP6.GATEWAY")?,
        "mac": dev("GENERAL.HWADDR")?,
        "bssid": id.bssid, "frequency": "", "band": "", "channel": "", "mode": "", "rate": ""
    });
    if !id.bssid.is_empty() {
        let ap = active_ap(&id.interface)?;
        if ap[1] != id.ssid || ap[2] != id.bssid {
            return Err("active access point changed".into());
        }
        for (key, index) in [("frequency", 3), ("channel", 4), ("mode", 5), ("rate", 6)] {
            value[key] = json!(ap[index]);
        }
        let frequency = ap[3]
            .split_whitespace()
            .next()
            .unwrap_or("")
            .parse()
            .unwrap_or(0);
        value["band"] = json!(if frequency == 0 {
            ""
        } else {
            frequency_band(frequency)
        });
    }
    Ok(value)
}

fn qr_png(payload: &str) -> Result<Vec<u8>> {
    let mut child =
        Command::new(env::var_os("NETWORK_MANAGER_QRENCODE").unwrap_or_else(|| "qrencode".into()))
            .args(["-t", "PNG", "-s", "10", "-m", "4", "-o", "-"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| "could not start qrencode")?;
    let written = child.stdin.take().unwrap().write_all(payload.as_bytes());
    let output = child.wait_with_output().map_err(|_| "qrencode failed")?;
    if written.is_err() || !output.status.success() || output.stdout.is_empty() {
        return Err("qrencode failed to create image".into());
    }
    Ok(output.stdout)
}

fn connect_wep(interface: &str, expected: &str) -> Result<Value> {
    // A single bounded JSON document, not a command line or a readline script.
    let mut input = String::new();
    std::io::stdin()
        .take(4097)
        .read_to_string(&mut input)
        .map_err(|_| "could not read WEP credential input")?;
    if input.len() > 4096 {
        return Err("WEP credential input too large".into());
    }
    let value: Value = serde_json::from_str(&input).map_err(|_| "invalid WEP credential JSON")?;
    let object = value.as_object().ok_or("expected WEP credential object")?;
    let password = object
        .get("password")
        .and_then(Value::as_str)
        .ok_or("missing WEP password string")?;
    if object.len() != 1 || password.is_empty() || password.chars().any(char::is_control) {
        return Err("invalid WEP password input".into());
    }
    if interface.is_empty() || expected.is_empty() || expected.len() > 32 {
        return Err("WEP requires an interface and nonempty SSID".into());
    }
    let rows = nm(&[
        "--escape",
        "yes",
        "--get-values",
        "SSID,BSSID,SECURITY",
        "device",
        "wifi",
        "list",
        "ifname",
        interface,
        "--rescan",
        "no",
    ])?;
    let ap = rows
        .lines()
        .map(split_row)
        .find(|r| r.len() == 3 && r[0] == expected && r[2] == "WEP")
        .ok_or("no cached WEP access point for selected interface/SSID")?;
    let bssid = &ap[1];
    if bssid.len() != 17
        || !bssid
            .split(':')
            .all(|part| part.len() == 2 && part.bytes().all(|c| c.is_ascii_hexdigit()))
    {
        return Err("invalid cached WEP access point BSSID".into());
    }
    // nmcli selects a compatible available saved profile, or creates one. Both
    // the device and AP are explicit; no hidden=yes (which would trigger a scan).
    // --ask's secret agent reads stdin via readline. Suppress both streams,
    // including readline echo and all authentication/permission diagnostics.
    let mut child =
        Command::new(env::var_os("NETWORK_MANAGER_NMCLI").unwrap_or_else(|| "nmcli".into()))
            .env("LC_ALL", "C")
            .args([
                "--ask", "--wait", "60", "device", "wifi", "connect", bssid, "ifname", interface,
                "bssid", bssid,
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| "could not start WEP activation")?;
    let written = writeln!(child.stdin.take().unwrap(), "{password}");
    let status = child
        .wait()
        .map_err(|_| "could not wait for WEP activation")?;
    // A saved credential can complete activation without consuming stdin.
    if !status.success() {
        return Err(
            "WEP activation failed (credential rejected, permission denied, or AP unavailable)"
                .into(),
        );
    }
    let _ = written;
    let id = identity(interface, expected)?;
    if !id.bssid.eq_ignore_ascii_case(bssid)
        || security(&id)? != "none"
        || field("connection", &id.uuid, "802-11-wireless.ssid")? != expected
    {
        return Err("activated network does not match selected WEP access point/profile".into());
    }
    revalidate(&id)?;
    Ok(
        json!({"success": true, "interface": interface, "ssid": expected,
        "bssid": id.bssid, "profileUuid": id.uuid}),
    )
}

fn run() -> Result<Value> {
    let args: Vec<_> = env::args().skip(1).collect();
    let pinned = matches!(args.first().map(String::as_str), Some("secret" | "qr"));
    if args.len() != if pinned { 5 } else { 3 }
        || !matches!(args[0].as_str(), "info" | "secret" | "qr" | "connect-wep")
    {
        return Err("usage: network-manager info|connect-wep <interface> <expected-ssid>; secret|qr <interface> <expected-ssid> <expected-profile-uuid> <expected-bssid>".into());
    }
    if pinned && (args[3].is_empty() || args[4].is_empty()) {
        return Err("secret/QR requires nonempty expected profile UUID and BSSID".into());
    }
    if args[0] == "connect-wep" {
        return connect_wep(&args[1], &args[2]);
    }
    let id = identity(&args[1], &args[2])?;
    if pinned && (id.uuid != args[3] || id.bssid != args[4]) {
        return Err("active network does not match expected profile/access point".into());
    }
    let security = security(&id)?;
    let mut result = if args[0] == "info" {
        info(&id, &security)?
    } else {
        let password = password(&id, &security)?;
        revalidate(&id)?;
        if args[0] == "secret" {
            json!({"password": password})
        } else {
            let hidden = field("connection", &id.uuid, "802-11-wireless.hidden")? == "yes";
            let auth = match security.as_str() {
                "" => "nopass",
                "none" => "WEP",
                _ => "WPA",
            };
            let payload = wifi_qr_payload(
                &id.ssid,
                auth,
                if auth == "nopass" {
                    None
                } else {
                    Some(&password)
                },
                hidden,
            );
            json!({"image": format!("data:image/png;base64,{}", STANDARD.encode(qr_png(&payload)?))})
        }
    };
    revalidate(&id)?;
    if pinned {
        result["interface"] = json!(id.interface);
        result["ssid"] = json!(id.ssid);
        result["profileUuid"] = json!(id.uuid);
        result["bssid"] = json!(id.bssid);
    }
    Ok(result)
}

fn main() {
    match run() {
        Ok(value) => println!("{value}"),
        Err(error) => {
            println!("{}", json!({"error": error}));
            eprintln!("network-manager: {error}");
            std::process::exit(2);
        }
    }
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
    fn escape(value: &str) -> String {
        let mut result = String::new();
        for c in value.chars() {
            if matches!(c, '\\' | ';' | ',' | ':' | '"') {
                result.push('\\');
            }
            result.push(c);
        }
        result
    }
    let mut result = format!("WIFI:T:{};S:{};", escape(authentication), escape(ssid));
    if let Some(password) = password {
        result.push_str(&format!("P:{};", escape(password)));
    }
    result.push_str(if hidden { "H:true;;" } else { "H:false;;" });
    result
}

#[cfg(test)]
mod tests;
