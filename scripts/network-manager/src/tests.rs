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
    assert_eq!(frequency_band(2437), "2.4 GHz");
    assert_eq!(frequency_band(5180), "5 GHz");
    assert_eq!(frequency_band(6115), "6 GHz");
}

#[test]
fn payload_marks_hidden_open_networks() {
    assert_eq!(
        wifi_qr_payload("Hidden", "nopass", None, true),
        "WIFI:T:nopass;S:Hidden;H:true;;"
    );
}

#[test]
fn ap_rows_preserve_ssid_and_bssid_delimiters() {
    assert_eq!(
        split_row(r"*:Cafe\:Guest:aa\:bb\:cc:6115 MHz:33:Infra:100 Mbit/s")[1],
        "Cafe:Guest"
    );
}

#[test]
fn identity_includes_interface_profile_and_access_point() {
    let original = Identity {
        interface: "wlan0".into(),
        uuid: "active-not-same-name".into(),
        ssid: "duplicate".into(),
        bssid: "first".into(),
        active_path: "/active/1".into(),
    };
    for updated in [
        Identity {
            interface: "wlan1".into(),
            ..original.clone()
        },
        Identity {
            uuid: "other-same-ssid".into(),
            ..original.clone()
        },
        Identity {
            bssid: "roamed".into(),
            ..original.clone()
        },
        Identity {
            active_path: "/active/2".into(),
            ..original.clone()
        },
    ] {
        assert!(original != updated);
    }
}
