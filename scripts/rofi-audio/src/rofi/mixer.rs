//! Audio tab interaction and the in-place routing picker. The small backend
//! interface lets navigation and action dispatch be tested without audio hardware.
use std::io;

use crate::AppResult;
use crate::audio;
use crate::model::{
    AudioEntry, AudioKind, BACK_KEY, ChoiceEntry, ChoiceList, Devices, Mode, Picker, hex_decode,
};

use super::{
    RETV_ACTIVATE, RETV_BACK, RETV_MUTE, RETV_ROUTE, RETV_VOLUME_DOWN, RETV_VOLUME_UP, UiState,
};

#[derive(Debug, PartialEq)]
enum Mutation {
    Volume(i16),
    Mute,
    Default,
    Route(String),
}

trait Backend {
    fn snapshot(&mut self, mode: Mode) -> AppResult<Devices>;
    fn choices(&mut self, mode: Mode, before: &Devices, picker: &Picker) -> AppResult<ChoiceList>;
    fn apply(&mut self, before: &Devices, key: &str, mutation: Mutation) -> AppResult<()>;
}

struct Pulse;

fn gone() -> crate::AppError {
    io::Error::other("The selected device or stream is no longer available").into()
}

impl Backend for Pulse {
    fn snapshot(&mut self, mode: Mode) -> AppResult<Devices> {
        let kind = mode.audio_kind().ok_or_else(gone)?;
        if mode.is_stream() {
            Ok(Devices::Streams(audio::streams()?))
        } else {
            Ok(Devices::Audio(audio::selections(kind)?))
        }
    }

    fn choices(&mut self, mode: Mode, before: &Devices, picker: &Picker) -> AppResult<ChoiceList> {
        match picker {
            Picker::Route(key) if mode.is_stream() => {
                let stream = before.stream(key).ok_or_else(gone)?;
                let devices = audio::snapshot(AudioKind::Output)?;
                Ok(ChoiceList {
                    title: format!("Output for {}", stream.application),
                    entries: devices
                        .into_iter()
                        .map(|device| ChoiceEntry {
                            active: device.name == stream.device_name,
                            key: device.key,
                            label: device.description,
                            enabled: true,
                        })
                        .collect(),
                })
            }
            _ => Err(gone()),
        }
    }

    fn apply(&mut self, before: &Devices, key: &str, mutation: Mutation) -> AppResult<()> {
        if let Some(entry) = before.audio(key) {
            match mutation {
                Mutation::Volume(delta) => {
                    audio::nudge_volume(entry, delta)?;
                    Ok(())
                }
                Mutation::Mute => audio::toggle_mute(entry),
                Mutation::Default => audio::set_default(entry),
                Mutation::Route(_) => Err(gone()),
            }
        } else if let Some(entry) = before.stream(key) {
            match mutation {
                Mutation::Volume(delta) => audio::nudge_stream_volume(entry, delta),
                Mutation::Mute => audio::toggle_stream_mute(entry),
                Mutation::Route(key) => {
                    let name = key
                        .strip_prefix("sink:")
                        .and_then(hex_decode)
                        .ok_or_else(gone)?;
                    audio::move_stream(entry, &name)
                }
                _ => Err(gone()),
            }
        } else {
            Err(gone())
        }
    }
}

pub(super) fn run(mode: Mode, retv: u8, selected: Option<&str>, state: &mut UiState) -> Devices {
    run_with(&mut Pulse, mode, retv, selected, state)
}

fn close_picker(state: &mut UiState) {
    state.selection = state.picker.take().map(|picker| picker.target().to_owned());
}

fn run_with(
    backend: &mut impl Backend,
    mode: Mode,
    retv: u8,
    selected: Option<&str>,
    state: &mut UiState,
) -> Devices {
    let before = match backend.snapshot(mode) {
        Ok(snapshot) => snapshot,
        Err(error) => {
            eprintln!("rofi-audio: cannot list audio: {error}");
            close_picker(state);
            state.set_message("Audio service is unavailable.");
            return if mode.is_stream() {
                Devices::Streams(Vec::new())
            } else {
                Devices::Audio(Vec::new())
            };
        }
    };

    if let Some(picker) = state.picker.clone() {
        if retv == RETV_BACK || (retv == RETV_ACTIVATE && selected == Some(BACK_KEY)) {
            close_picker(state);
            return before;
        }
        let choices = match backend.choices(mode, &before, &picker) {
            Ok(choices) => choices,
            Err(error) => {
                close_picker(state);
                state.set_message(error.to_string());
                return before;
            }
        };
        if retv == RETV_ACTIVATE {
            let choice =
                selected.and_then(|key| choices.entries.iter().find(|c| c.key == key && c.enabled));
            if let Some(choice) = choice {
                let mutation = Mutation::Route(choice.key.clone());
                match backend.apply(&before, picker.target(), mutation) {
                    Ok(()) => {
                        close_picker(state);
                        return backend.snapshot(mode).unwrap_or(before);
                    }
                    Err(error) => state.set_message(format!("Cannot apply selection: {error}")),
                }
            } else {
                state.set_message("That choice is no longer available.");
            }
        } else if matches!(
            retv,
            RETV_VOLUME_UP | RETV_VOLUME_DOWN | RETV_MUTE | RETV_ROUTE
        ) {
            state.set_message("Choose a row with Enter, or go Back.");
        }
        return Devices::Choices(choices);
    }

    let key = selected.unwrap_or_default();
    let picker = match retv {
        RETV_ACTIVATE | RETV_ROUTE if mode.is_stream() => Some(Picker::Route(key.to_owned())),
        _ => None,
    };
    if let Some(picker) = picker {
        match backend.choices(mode, &before, &picker) {
            Ok(choices) => {
                state.selection = Some(
                    choices
                        .entries
                        .iter()
                        .find(|c| c.active && c.enabled)
                        .map(|c| c.key.clone())
                        .unwrap_or_else(|| BACK_KEY.to_owned()),
                );
                state.picker = Some(picker);
                return Devices::Choices(choices);
            }
            Err(error) => state.set_message(error.to_string()),
        }
        return before;
    }

    let mutation = match retv {
        RETV_ACTIVATE if !mode.is_stream() => Some(Mutation::Default),
        RETV_VOLUME_UP => Some(Mutation::Volume(audio::STEP)),
        RETV_VOLUME_DOWN => Some(Mutation::Volume(-audio::STEP)),
        RETV_MUTE => Some(Mutation::Mute),
        RETV_ROUTE => {
            state.set_message("Device routing applies to Playback.");
            None
        }
        super::RETV_SCAN | super::RETV_FORGET => {
            state.set_message("Scan and Forget apply to Bluetooth.");
            None
        }
        _ => None,
    };
    if let Some(mutation) = mutation {
        let is_default = mutation == Mutation::Default;
        match backend.apply(&before, key, mutation) {
            Ok(()) => {
                let mut after = backend.snapshot(mode).unwrap_or(before);
                // pipewire-pulse acknowledges a default change before its
                // metadata update becomes visible to introspection.
                if is_default
                    && let Devices::Audio(entries) = &mut after
                    && entries.iter().any(|e| e.key == key)
                {
                    apply_chosen_default(entries, key);
                }
                return after;
            }
            Err(error) => state.set_message(format!("Cannot change audio: {error}")),
        }
    }
    before
}

pub(super) fn apply_chosen_default(entries: &mut [AudioEntry], chosen: &str) {
    for entry in entries {
        entry.default = entry.key == chosen;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Fake {
        actions: Vec<(String, Mutation)>,
        missing: bool,
        reject: bool,
    }

    impl Backend for Fake {
        fn snapshot(&mut self, _: Mode) -> AppResult<Devices> {
            Ok(Devices::Streams(Vec::new()))
        }
        fn choices(&mut self, _: Mode, _: &Devices, _: &Picker) -> AppResult<ChoiceList> {
            if self.missing {
                return Err(gone());
            }
            Ok(ChoiceList {
                title: "Choose device".into(),
                entries: vec![
                    ChoiceEntry {
                        key: "chosen".into(),
                        label: "Headphones".into(),
                        active: true,
                        enabled: true,
                    },
                    ChoiceEntry {
                        key: "unplugged".into(),
                        label: "Unplugged".into(),
                        active: false,
                        enabled: false,
                    },
                ],
            })
        }
        fn apply(&mut self, _: &Devices, key: &str, mutation: Mutation) -> AppResult<()> {
            if self.reject {
                return Err(io::Error::other("rejected").into());
            }
            self.actions.push((key.to_owned(), mutation));
            Ok(())
        }
    }

    #[test]
    fn activating_a_stream_opens_routes_without_setting_a_default() {
        let mut backend = Fake::default();
        let mut state = UiState::default();
        let rows = run_with(
            &mut backend,
            Mode::Playback,
            RETV_ACTIVATE,
            Some("stream"),
            &mut state,
        );
        assert!(matches!(rows, Devices::Choices(_)));
        assert_eq!(state.picker, Some(Picker::Route("stream".into())));
        assert_eq!(state.selection.as_deref(), Some("chosen"));
        assert!(backend.actions.is_empty());
        run_with(
            &mut backend,
            Mode::Playback,
            RETV_ACTIVATE,
            Some("chosen"),
            &mut state,
        );
        assert_eq!(
            backend.actions,
            vec![("stream".into(), Mutation::Route("chosen".into()))]
        );
        assert!(state.picker.is_none());
        assert_eq!(state.selection.as_deref(), Some("stream"));
    }

    #[test]
    fn route_button_does_not_open_a_picker_on_device_tabs() {
        for mode in [Mode::Output, Mode::Input] {
            let mut backend = Fake::default();
            let mut state = UiState::default();
            run_with(&mut backend, mode, RETV_ROUTE, Some("device"), &mut state);
            assert!(state.picker.is_none());
            assert_eq!(
                state.message.as_deref(),
                Some("Device routing applies to Playback.")
            );
            assert!(backend.actions.is_empty());
        }
    }

    #[test]
    fn port_rows_activate_directly_without_opening_a_picker() {
        for mode in [Mode::Output, Mode::Input] {
            let mut backend = Fake::default();
            let mut state = UiState::default();
            run_with(
                &mut backend,
                mode,
                RETV_ACTIVATE,
                Some("device:port:chosen"),
                &mut state,
            );
            assert_eq!(
                backend.actions,
                vec![("device:port:chosen".into(), Mutation::Default)]
            );
            assert!(state.picker.is_none());
        }
    }

    #[test]
    fn back_and_disappearing_streams_leave_picker_without_mutations() {
        for (retv, key, missing) in [
            (RETV_BACK, "chosen", false),
            (RETV_ACTIVATE, BACK_KEY, false),
            (super::super::RETV_REFRESH, "chosen", true),
        ] {
            let mut backend = Fake {
                missing,
                ..Default::default()
            };
            let mut state = UiState {
                picker: Some(Picker::Route("stream".into())),
                ..Default::default()
            };
            run_with(&mut backend, Mode::Playback, retv, Some(key), &mut state);
            assert!(state.picker.is_none());
            assert!(backend.actions.is_empty());
        }
    }

    #[test]
    fn missing_disabled_or_rejected_choices_cannot_succeed() {
        for (key, reject) in [("gone", false), ("unplugged", false), ("chosen", true)] {
            let mut backend = Fake {
                reject,
                ..Default::default()
            };
            let mut state = UiState {
                picker: Some(Picker::Route("stream".into())),
                ..Default::default()
            };
            run_with(
                &mut backend,
                Mode::Playback,
                RETV_ACTIVATE,
                Some(key),
                &mut state,
            );
            assert!(state.picker.is_some());
            assert!(state.message.is_some());
            assert!(backend.actions.is_empty());
        }
    }

    #[test]
    fn mute_and_volume_dispatch_to_the_selected_row() {
        for mode in [Mode::Output, Mode::Input, Mode::Playback] {
            let mut backend = Fake::default();
            let mut state = UiState::default();
            run_with(&mut backend, mode, RETV_MUTE, Some("row"), &mut state);
            run_with(&mut backend, mode, RETV_VOLUME_UP, Some("row"), &mut state);
            run_with(
                &mut backend,
                mode,
                RETV_VOLUME_DOWN,
                Some("row"),
                &mut state,
            );
            assert_eq!(
                backend.actions,
                vec![
                    ("row".into(), Mutation::Mute),
                    ("row".into(), Mutation::Volume(5)),
                    ("row".into(), Mutation::Volume(-5))
                ]
            );
        }
    }

    #[test]
    fn activating_a_device_still_sets_the_default() {
        for mode in [Mode::Output, Mode::Input] {
            let mut backend = Fake::default();
            let mut state = UiState::default();
            run_with(
                &mut backend,
                mode,
                RETV_ACTIVATE,
                Some("device"),
                &mut state,
            );
            assert_eq!(backend.actions, vec![("device".into(), Mutation::Default)]);
            assert!(state.picker.is_none());
        }
    }

    #[test]
    fn volume_buttons_do_not_act_on_picker_rows() {
        let mut backend = Fake::default();
        let mut state = UiState {
            picker: Some(Picker::Route("stream".into())),
            ..Default::default()
        };
        run_with(
            &mut backend,
            Mode::Playback,
            RETV_VOLUME_UP,
            Some("chosen"),
            &mut state,
        );
        assert!(backend.actions.is_empty());
        assert!(state.picker.is_some());
    }
}
