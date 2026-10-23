use super::*;
use std::io::Cursor;

fn request() -> Value {
    json!({"ssid": "Cafe", "authentication": "WPA", "password": "secret", "hidden": false})
}

fn payload(value: Value) -> Result<String> {
    qr_payload(value.to_string().as_bytes())
}

#[test]
fn payload_escapes_every_wifi_qr_delimiter() {
    assert_eq!(
        wifi_qr_payload("Cafe;Guest,:\\\"", "WPA", Some("a:b,c\\d;\""), false),
        "WIFI:T:WPA;S:Cafe\\;Guest\\,\\:\\\\\\\";P:a\\:b\\,c\\\\d\\;\\\";H:false;;"
    );
}

#[test]
fn payload_preserves_utf8_and_whitespace() {
    assert_eq!(
        payload(
            json!({"ssid": " café ", "authentication": "WEP", "password": " key ", "hidden": true})
        )
        .unwrap(),
        "WIFI:T:WEP;S: café ;P: key ;H:true;;"
    );
}

#[test]
fn open_networks_need_no_password_and_never_encode_one() {
    for password in [None, Some(json!("")), Some(json!("not-encoded"))] {
        let mut value = json!({"ssid": "Hidden", "authentication": "nopass", "hidden": true});
        if let Some(password) = password {
            value["password"] = password;
        }
        assert_eq!(payload(value).unwrap(), "WIFI:T:nopass;S:Hidden;H:true;;");
    }
}

#[test]
fn ssid_limit_is_utf8_bytes_not_characters() {
    let mut value = request();
    for ssid in ["a".repeat(32), "é".repeat(16)] {
        value["ssid"] = json!(ssid);
        assert!(payload(value.clone()).is_ok());
    }
    for ssid in [String::new(), "a".repeat(33), "é".repeat(17)] {
        value["ssid"] = json!(ssid);
        assert!(payload(value.clone()).is_err());
    }
}

#[test]
fn control_characters_in_credentials_are_rejected() {
    for field in ["ssid", "password"] {
        for control in ['\0', '\n', '\r', '\t', '\u{7f}', '\u{85}'] {
            let mut value = request();
            value[field] = json!(format!("before{control}after"));
            assert!(
                payload(value).is_err(),
                "accepted {field} control character"
            );
        }
    }
}

#[test]
fn only_supported_authentication_and_nonempty_secured_passwords_are_accepted() {
    for authentication in ["WPA", "WEP", "nopass"] {
        let mut value = request();
        value["authentication"] = json!(authentication);
        assert!(payload(value).is_ok());
    }
    for authentication in ["", "wpa", "WPA2", "sae", "WPA;S:injected"] {
        let mut value = request();
        value["authentication"] = json!(authentication);
        assert!(payload(value).is_err());
    }
    for authentication in ["WPA", "WEP"] {
        let mut value = request();
        value["authentication"] = json!(authentication);
        value["password"] = json!("");
        assert!(payload(value.clone()).is_err());
        value.as_object_mut().unwrap().remove("password");
        assert!(payload(value).is_err());
    }
}

#[test]
fn fields_are_strictly_typed_and_unknown_fields_are_rejected() {
    for field in ["ssid", "authentication", "password", "hidden"] {
        for invalid in [Value::Null, json!([]), json!({}), json!(7)] {
            let mut value = request();
            value[field] = invalid;
            assert!(payload(value).is_err());
        }
        let mut value = request();
        value.as_object_mut().unwrap().remove(field);
        assert!(payload(value).is_err());
    }
    for field in ["ssid", "authentication", "password"] {
        let mut value = request();
        value[field] = json!(true);
        assert!(payload(value).is_err());
    }
    let mut value = request();
    value["hidden"] = json!("false");
    assert!(payload(value).is_err());
    let mut value = request();
    value["interface"] = json!("wlan0");
    assert!(payload(value).is_err());
}

#[test]
fn malformed_nonobject_and_invalid_utf8_input_is_rejected_without_echo() {
    for input in [
        &b""[..],
        b"{private-sentinel",
        b"null",
        b"[]",
        b"true",
        b"\"private-sentinel\"",
        b"{} {}",
        b"{\"ssid\":\"\xff\"}",
        br#"{"ssid":"\ud800"}"#,
    ] {
        let error = qr_payload(input).unwrap_err();
        assert!(!error.contains("private-sentinel"));
    }
}

#[test]
fn input_bound_is_inclusive_and_does_not_read_unbounded_streams() {
    let mut input = request().to_string().into_bytes();
    input.resize(MAX_INPUT, b' ');
    assert!(qr_payload(input.as_slice()).is_ok());
    input.resize(MAX_INPUT * 2, b' ');
    let mut cursor = Cursor::new(input);
    assert!(qr_payload(&mut cursor).is_err());
    assert_eq!(cursor.position(), MAX_INPUT as u64 + 1);
}

#[test]
fn output_reads_are_bounded() {
    assert_eq!(read_bounded(&b"1234"[..], 4, "bounded").unwrap(), b"1234");
    let mut cursor = Cursor::new(vec![0; MAX_PNG * 2]);
    assert_eq!(
        read_bounded(&mut cursor, MAX_PNG, "bounded"),
        Err("bounded")
    );
    assert_eq!(cursor.position(), MAX_PNG as u64 + 1);
}

#[test]
fn only_the_bare_qr_command_is_accepted() {
    assert!(validate_args([OsString::from("qr")].into_iter()).is_ok());
    for args in [
        vec![],
        vec!["info"],
        vec!["secret", "wlan0", "ssid", "uuid", "bssid"],
        vec!["connect-wep", "wlan0", "ssid"],
        vec!["identity"],
        vec!["nmcli"],
        vec!["qr", "wlan0", "ssid", "uuid", "bssid"],
        vec!["qr", "private-sentinel"],
        vec!["qr", "--password", "private-sentinel"],
        vec!["--help"],
    ] {
        let error = validate_args(args.into_iter().map(OsString::from)).unwrap_err();
        assert!(!error.contains("private-sentinel"));
    }
}
