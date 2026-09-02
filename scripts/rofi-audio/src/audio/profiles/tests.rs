use super::*;

const CARD: &str = "alsa_card.test";
const SPEAKERS: &str = "HiFi (HDMI1, HDMI2, HDMI3, Mic1, Mic2, Speaker)";
const HEADPHONES: &str = "HiFi (HDMI1, HDMI2, HDMI3, Headphones, Mic1, Mic2)";

fn card() -> Card {
    let profile = |name: &str| Profile {
        name: name.into(),
        available: true,
        sinks: 4,
        sources: 2,
        priority: 100,
    };
    let port = |name: &str, output, available, profiles: Vec<String>| Port {
        name: name.into(),
        label: name.trim_start_matches("[Out] ").into(),
        output,
        available,
        profiles,
    };
    let both = vec![SPEAKERS.into(), HEADPHONES.into()];
    Card {
        index: 7,
        name: CARD.into(),
        label: "Alder Lake PCH-P High Definition Audio Controller".into(),
        active: SPEAKERS.into(),
        profiles: vec![profile(SPEAKERS), profile(HEADPHONES)],
        ports: vec![
            port(
                "[Out] Speaker",
                true,
                PortAvailable::Unknown,
                vec![SPEAKERS.into()],
            ),
            port(
                "[Out] Headphones",
                true,
                PortAvailable::Yes,
                vec![HEADPHONES.into()],
            ),
            port("[Out] HDMI1", true, PortAvailable::Yes, both.clone()),
            port("[Out] HDMI2", true, PortAvailable::Yes, both.clone()),
            port("[Out] HDMI3", true, PortAvailable::No, both.clone()),
            port("[In] Mic1", false, PortAvailable::Unknown, both.clone()),
            port("[In] Mic2", false, PortAvailable::Yes, both),
        ],
    }
}

fn output(card: u32, name: &str, port: &str) -> Output {
    Output {
        card: Some(card),
        name: name.into(),
        ports: vec![port.into()],
    }
}

fn row(name: &str, port: &str) -> AudioEntry {
    AudioEntry {
        key: format!("sink:{}:port:{}", hex_encode(name), hex_encode(port)),
        kind: AudioKind::Output,
        name: name.into(),
        card: None,
        description: format!("Alder Lake PCH-P — {port}"),
        label: port.trim_start_matches("[Out] ").into(),
        volume: 60,
        muted: false,
        default: true,
        port: Some(port.into()),
    }
}

#[test]
fn both_outputs_remain_visible_with_stable_keys_across_profile_changes() {
    let mut card = card();
    let mut before = vec![row("speaker-sink", "[Out] Speaker")];
    complete_outputs(
        &[card.clone()],
        &[output(7, "speaker-sink", "[Out] Speaker")],
        &mut before,
    );
    let speaker = before.iter().find(|e| e.label == "Speaker").unwrap();
    let headphone = before.iter().find(|e| e.label == "Headphones").unwrap();
    assert!(!speaker.inactive());
    assert!(headphone.inactive());
    assert_eq!(headphone.row_label(), "󰕾   —   Headphones");
    assert!(!headphone.default);
    assert!(require_live_output(headphone).is_err());
    assert!(require_live_output(speaker).is_ok());
    assert!(
        before
            .iter()
            .all(|e| !e.label.contains("Mic") && !e.label.contains("HDMI3"))
    );
    let speaker_key = speaker.key.clone();
    let headphone_key = headphone.key.clone();

    card.active = HEADPHONES.into();
    card.index = 12; // Indexes can change; only card and port names are identity.
    let mut after = vec![row("different-headphone-sink", "[Out] Headphones")];
    complete_outputs(
        &[card],
        &[output(12, "different-headphone-sink", "[Out] Headphones")],
        &mut after,
    );
    let speaker = after.iter().find(|e| e.label == "Speaker").unwrap();
    let headphone = after.iter().find(|e| e.label == "Headphones").unwrap();
    assert_eq!(speaker.key, speaker_key);
    assert_eq!(headphone.key, headphone_key);
    assert!(speaker.inactive());
    assert!(!headphone.inactive());
    assert!(headphone.default);
}

#[test]
fn disconnected_ports_and_unavailable_profiles_are_not_offered() {
    for unavailable_profile in [false, true] {
        let mut card = card();
        if unavailable_profile {
            card.profiles[1].available = false;
        } else {
            card.ports[1].available = PortAvailable::No;
        }
        let mut rows = Vec::new();
        complete_outputs(&[card], &[], &mut rows);
        assert!(!rows.iter().any(|row| row.label == "Headphones"));
    }
}

#[test]
fn profile_selection_preserves_microphones_and_never_changes_bluetooth_codecs() {
    let mut card = card();
    assert_eq!(profile_for(&card, &card.ports[1]), Some(HEADPHONES));
    assert_eq!(profile_for(&card, &card.ports[2]), Some(SPEAKERS));
    card.profiles[1].sources = 0;
    assert_eq!(profile_for(&card, &card.ports[1]), None);
    card.profiles[1].sources = 2;
    card.ports[5].profiles = vec![SPEAKERS.into()];
    assert_eq!(profile_for(&card, &card.ports[1]), None);
    card.name = "bluez_card.test".into();
    assert_eq!(profile_for(&card, &card.ports[0]), None);
    let mut rows = Vec::new();
    complete_outputs(&[card], &[], &mut rows);
    assert!(rows.is_empty());
}

#[test]
fn profiles_retaining_hdmi_ports_win_over_higher_priority_reduced_profiles() {
    let mut card = card();
    let minimal = "headphones-and-mics-only";
    card.profiles.push(Profile {
        name: minimal.into(),
        available: true,
        sinks: 1,
        sources: 2,
        priority: 999,
    });
    for port in &mut card.ports {
        if port.name == "[Out] Headphones" || !port.output {
            port.profiles.push(minimal.into());
        }
    }
    assert_eq!(profile_for(&card, &card.ports[1]), Some(HEADPHONES));
}

#[test]
fn identical_port_names_on_other_cards_never_supply_the_target() {
    assert!(
        find_output(
            vec![output(8, "wrong", "[Out] Headphones")],
            7,
            "[Out] Headphones"
        )
        .unwrap()
        .is_none()
    );
    assert!(
        find_output(
            vec![
                output(7, "one", "[Out] Headphones"),
                output(7, "two", "[Out] Headphones")
            ],
            7,
            "[Out] Headphones"
        )
        .is_err()
    );
    assert_ne!(
        key(CARD, "[Out] Speaker"),
        key("alsa_card.other", "[Out] Speaker")
    );
    assert!(key("alsa_card.🎧;x", "[Out] Jack:1").is_ascii());
}

struct Fake {
    card: Card,
    events: Vec<String>,
    default: String,
    delay: usize,
    never_appears: bool,
    reject_profile: bool,
    reject_port: bool,
    reject_default: bool,
    external_change: bool,
}

impl Default for Fake {
    fn default() -> Self {
        Self {
            card: card(),
            events: Vec::new(),
            default: "speaker-sink".into(),
            delay: 0,
            never_appears: false,
            reject_profile: false,
            reject_port: false,
            reject_default: false,
            external_change: false,
        }
    }
}

impl Backend for Fake {
    fn card(&mut self, name: &str) -> AppResult<Card> {
        assert_eq!(name, CARD);
        Ok(self.card.clone())
    }

    fn outputs(&mut self) -> AppResult<Vec<Output>> {
        let mut outputs = vec![Output {
            card: None,
            name: "virtual".into(),
            ports: Vec::new(),
        }];
        if self.card.active == SPEAKERS {
            outputs.push(output(self.card.index, "speaker-sink", "[Out] Speaker"));
        } else if self.card.active == HEADPHONES {
            if self.external_change {
                self.card.active = "changed-elsewhere".into();
            } else if self.delay > 0 {
                self.delay -= 1;
            } else if !self.never_appears {
                outputs.push(output(
                    self.card.index,
                    "new-headphone-sink",
                    "[Out] Headphones",
                ));
            }
        }
        Ok(outputs)
    }

    fn default_output(&mut self) -> AppResult<Option<String>> {
        Ok(Some(self.default.clone()))
    }

    fn set_profile(&mut self, _: &str, profile: &str) -> AppResult<()> {
        self.events.push(format!("profile:{profile}"));
        if self.reject_profile {
            return Err(io::Error::other("Profile rejected").into());
        }
        self.card.active = profile.into();
        Ok(())
    }

    fn set_port(&mut self, output: &Output, port: &str) -> AppResult<()> {
        self.events.push(format!("port:{}:{port}", output.name));
        if self.reject_port {
            return Err(io::Error::other("Port rejected").into());
        }
        Ok(())
    }

    fn set_default(&mut self, name: &str) -> AppResult<()> {
        self.events.push(format!("default:{name}"));
        if self.reject_default && name == "new-headphone-sink" {
            return Err(io::Error::other("Default rejected").into());
        }
        self.default = name.into();
        Ok(())
    }

    fn pause(&mut self) {}
}

#[test]
fn switching_waits_for_the_new_sink_then_sets_port_before_default() {
    let mut fake = Fake {
        delay: 3,
        ..Default::default()
    };
    activate_with(&mut fake, CARD, "[Out] Headphones").unwrap();
    assert_eq!(
        fake.events,
        [
            format!("profile:{HEADPHONES}"),
            "port:new-headphone-sink:[Out] Headphones".into(),
            "default:new-headphone-sink".into()
        ]
    );
    assert_eq!(fake.default, "new-headphone-sink");
    fake.events.clear();
    activate_with(&mut fake, CARD, "[Out] Speaker").unwrap();
    assert_eq!(
        fake.events,
        [
            format!("profile:{SPEAKERS}"),
            "port:speaker-sink:[Out] Speaker".into(),
            "default:speaker-sink".into()
        ]
    );
}

#[test]
fn live_outputs_do_not_trigger_profile_changes() {
    let mut fake = Fake::default();
    activate_with(&mut fake, CARD, "[Out] Speaker").unwrap();
    assert_eq!(
        fake.events,
        ["port:speaker-sink:[Out] Speaker", "default:speaker-sink"]
    );
}

#[test]
fn unplugged_or_removed_ports_do_not_trigger_any_mutation() {
    let mut fake = Fake::default();
    fake.card.ports[1].available = PortAvailable::No;
    assert!(activate_with(&mut fake, CARD, "[Out] Headphones").is_err());
    assert!(activate_with(&mut fake, CARD, "removed").is_err());
    assert!(fake.events.is_empty());
}

#[test]
fn rejected_profile_never_attempts_port_or_default_changes() {
    let mut fake = Fake {
        reject_profile: true,
        ..Default::default()
    };
    assert!(activate_with(&mut fake, CARD, "[Out] Headphones").is_err());
    assert_eq!(fake.events, [format!("profile:{HEADPHONES}")]);
    assert_eq!(fake.card.active, SPEAKERS);
}

#[test]
fn timeouts_and_rejected_followup_actions_restore_profile_and_default() {
    for (never_appears, reject_port, reject_default) in [
        (true, false, false),
        (false, true, false),
        (false, false, true),
    ] {
        let mut fake = Fake {
            never_appears,
            reject_port,
            reject_default,
            ..Default::default()
        };
        let error = activate_with(&mut fake, CARD, "[Out] Headphones").unwrap_err();
        assert!(error.to_string().contains("Previous profile restored"));
        assert_eq!(fake.card.active, SPEAKERS);
        assert_eq!(fake.default, "speaker-sink");
        assert!(
            fake.events
                .ends_with(&[format!("profile:{SPEAKERS}"), "default:speaker-sink".into()])
        );
    }
}

#[test]
fn rollback_can_restore_a_virtual_default_without_a_sound_card() {
    let mut fake = Fake {
        default: "virtual".into(),
        never_appears: true,
        ..Default::default()
    };
    let error = activate_with(&mut fake, CARD, "[Out] Headphones").unwrap_err();
    assert!(error.to_string().contains("Previous profile restored"));
    assert_eq!(fake.default, "virtual");
}

#[test]
fn rollback_does_not_overwrite_a_newer_external_profile_choice() {
    let mut fake = Fake {
        external_change: true,
        ..Default::default()
    };
    let error = activate_with(&mut fake, CARD, "[Out] Headphones").unwrap_err();
    assert!(error.to_string().contains("changed elsewhere"));
    assert_eq!(fake.card.active, "changed-elsewhere");
    assert_eq!(fake.events, [format!("profile:{HEADPHONES}")]);
}
