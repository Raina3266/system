//! Unit tests, one module per feature source file.

mod model {
    use nmrs::SecurityFeatures;

    use crate::model::*;

    #[test]
    fn security_names_distinguish_wpa2_and_wpa3() {
        let mut mixed = SecurityFeatures::default();
        mixed.privacy = true;
        mixed.psk = true;
        mixed.sae = true;
        mixed.ccmp = true;
        assert_eq!(security_label(&mixed), "WPA2/WPA3");

        let mut enterprise = SecurityFeatures::default();
        enterprise.privacy = true;
        enterprise.eap_suite_b_192 = true;
        enterprise.ccmp = true;
        assert_eq!(security_label(&enterprise), "WPA3 Enterprise");
    }

    #[test]
    fn signal_ordering_ignores_drift_inside_one_bar() {
        // Two readings that draw the same icon have to compare equal, or the
        // timed refresh would keep swapping their rows.
        assert_eq!(signal_bars(77), signal_bars(94));
        assert_eq!(signal_icon(77), signal_icon(94));
        assert!(signal_bars(51) > signal_bars(50));
        assert_eq!(signal_bars(0), 0);
    }
}

mod network {
    use crate::network::*;

    #[test]
    fn waybar_json_escapes_markup_tooltips_safely() {
        assert_eq!(
            json_escape("A \"network\"\nline"),
            "A \\\"network\\\"\\nline"
        );
    }
}

mod wayle {
    use crate::wayle::*;

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
