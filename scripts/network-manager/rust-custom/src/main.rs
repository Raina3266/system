use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use std::{
    env,
    ffi::{OsStr, OsString},
    io::{Read, Write},
    process::{Command, Stdio},
};

type Result<T> = std::result::Result<T, &'static str>;

const MAX_INPUT: usize = 16 * 1024;
const MAX_PNG: usize = 1024 * 1024;
const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";

fn validate_args(mut args: impl Iterator<Item = OsString>) -> Result<()> {
    if args.next().as_deref() != Some(OsStr::new("qr")) || args.next().is_some() {
        return Err("usage: network-manager qr (JSON on stdin)");
    }
    Ok(())
}

fn read_bounded(reader: impl Read, limit: usize, error: &'static str) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| error)?;
    if bytes.len() > limit {
        return Err(error);
    }
    Ok(bytes)
}

fn qr_payload(reader: impl Read) -> Result<String> {
    let input = read_bounded(reader, MAX_INPUT, "could not read bounded QR input")?;
    // Do not expose serde diagnostics: they can quote credential input.
    let value: Value = serde_json::from_slice(&input).map_err(|_| "invalid QR JSON")?;
    let object = value.as_object().ok_or("invalid QR request")?;
    if object.keys().any(|key| {
        !matches!(
            key.as_str(),
            "ssid" | "authentication" | "password" | "hidden"
        )
    }) {
        return Err("invalid QR request fields");
    }
    let ssid = object
        .get("ssid")
        .and_then(Value::as_str)
        .ok_or("invalid QR SSID")?;
    if ssid.is_empty() || ssid.len() > 32 || ssid.chars().any(char::is_control) {
        return Err("invalid QR SSID");
    }
    let authentication = object
        .get("authentication")
        .and_then(Value::as_str)
        .filter(|auth| matches!(*auth, "nopass" | "WPA" | "WEP"))
        .ok_or("invalid QR authentication")?;
    let hidden = object
        .get("hidden")
        .and_then(Value::as_bool)
        .ok_or("invalid QR hidden flag")?;
    let password = match object.get("password") {
        Some(Value::String(password)) => password.as_str(),
        None if authentication == "nopass" => "",
        _ => return Err("invalid QR password"),
    };
    if password.chars().any(char::is_control) || (authentication != "nopass" && password.is_empty())
    {
        return Err("invalid QR password");
    }
    Ok(wifi_qr_payload(
        ssid,
        authentication,
        (authentication != "nopass").then_some(password),
        hidden,
    ))
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
    let mut result = format!("WIFI:T:{authentication};S:{};", escape(ssid));
    if let Some(password) = password {
        result.push_str(&format!("P:{};", escape(password)));
    }
    result.push_str(if hidden { "H:true;;" } else { "H:false;;" });
    result
}

fn qr_png(payload: &str) -> Result<Vec<u8>> {
    let mut child =
        Command::new(env::var_os("NETWORK_MANAGER_QRENCODE").unwrap_or_else(|| "qrencode".into()))
            .args(["-t", "PNG", "-s", "10", "-m", "4", "-o", "-"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| "could not start QR encoder")?;
    // The credential-bearing payload goes only through stdin, never argv.
    let written = child.stdin.take().unwrap().write_all(payload.as_bytes());
    let png = read_bounded(
        child.stdout.take().unwrap(),
        MAX_PNG,
        "could not read bounded QR image",
    );
    if written.is_err() || png.is_err() {
        // Stop an overproducing encoder before waiting for it to exit.
        let _ = child.kill();
        let _ = child.wait();
        return Err("QR encoding failed");
    }
    let status = child.wait().map_err(|_| "QR encoding failed")?;
    let png = png?;
    if !status.success() || !png.starts_with(PNG_SIGNATURE) {
        return Err("QR encoding failed");
    }
    Ok(png)
}

fn run() -> Result<Value> {
    validate_args(env::args_os().skip(1))?;
    let payload = qr_payload(std::io::stdin().lock())?;
    Ok(json!({"image": format!("data:image/png;base64,{}", STANDARD.encode(qr_png(&payload)?))}))
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

#[cfg(test)]
mod tests;
