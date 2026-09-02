//! ALSA outputs that are exposed by mutually exclusive card profiles.
//! Card/port names are the identity; sink names and indexes are re-resolved.
use std::{cell::RefCell, thread, time::Duration};

use libpulse_binding::{callbacks::ListResult, context::introspect::CardInfo, direction};

use super::*;

#[derive(Clone, Debug)]
pub(super) struct Card {
    index: u32,
    pub name: String,
    pub label: String,
    active: String,
    profiles: Vec<Profile>,
    ports: Vec<Port>,
}

#[derive(Clone, Debug)]
struct Profile {
    name: String,
    available: bool,
    sinks: u32,
    sources: u32,
    priority: u32,
}

#[derive(Clone, Debug)]
struct Port {
    name: String,
    label: String,
    output: bool,
    available: PortAvailable,
    profiles: Vec<String>,
}

#[derive(Clone, Debug)]
pub(super) struct Output {
    card: Option<u32>,
    name: String,
    ports: Vec<String>,
}

impl Card {
    fn from_info(info: &CardInfo<'_>) -> Option<Self> {
        let name = info
            .name
            .as_deref()
            .filter(|name| !name.is_empty())?
            .to_owned();
        Some(Self {
            index: info.index,
            label: info
                .proplist
                .get_str("device.description")
                .unwrap_or_else(|| name.clone()),
            name,
            active: info
                .active_profile
                .as_ref()
                .and_then(|p| p.name.as_deref())
                .unwrap_or_default()
                .into(),
            profiles: info
                .profiles
                .iter()
                .filter_map(|p| {
                    Some(Profile {
                        name: p.name.as_deref()?.into(),
                        available: p.available,
                        sinks: p.n_sinks,
                        sources: p.n_sources,
                        priority: p.priority,
                    })
                })
                .collect(),
            ports: info
                .ports
                .iter()
                .filter_map(|p| {
                    let name = p.name.as_deref().filter(|name| !name.is_empty())?;
                    Some(Port {
                        name: name.into(),
                        label: p
                            .description
                            .as_deref()
                            .filter(|label| !label.is_empty())
                            .unwrap_or(name)
                            .into(),
                        output: p.direction.contains(direction::FlagSet::OUTPUT),
                        available: p.available,
                        profiles: p
                            .profiles
                            .iter()
                            .filter_map(|profile| profile.name.as_deref().map(str::to_owned))
                            .collect(),
                    })
                })
                .collect(),
        })
    }

    fn output_port(&self, name: &str) -> AppResult<&Port> {
        if !self.name.starts_with("alsa_card.") {
            return Err(io::Error::other(
                "Automatic profile switching is only supported for ALSA outputs",
            )
            .into());
        }
        self.ports
            .iter()
            .find(|p| p.name == name && p.output && p.available != PortAvailable::No)
            .ok_or_else(|| io::Error::other("The selected output is no longer available").into())
    }
}

pub(super) fn cards(controller: &mut Controller) -> AppResult<Vec<Card>> {
    let result = Rc::new(RefCell::new(Vec::new()));
    let result_cb = result.clone();
    let complete = Rc::new(Cell::new(false));
    let complete_cb = complete.clone();
    let handler = controller.handler();
    let op = handler
        .introspect
        .get_card_info_list(move |item| match item {
            ListResult::Item(info) => {
                if let Some(card) = Card::from_info(info) {
                    result_cb.borrow_mut().push(card);
                }
            }
            ListResult::End => complete_cb.set(true),
            ListResult::Error => complete_cb.set(false),
        });
    handler.wait_for_operation(op)?;
    if !complete.get() {
        return Err(io::Error::other("Cannot read audio card profiles").into());
    }
    let cards = std::mem::take(&mut *result.borrow_mut());
    Ok(cards)
}

pub(super) fn outputs(devices: &[DeviceInfo]) -> Vec<Output> {
    devices
        .iter()
        .filter_map(|device| {
            Some(Output {
                card: device.card,
                name: device.name.as_ref()?.clone(),
                ports: device
                    .ports
                    .iter()
                    .filter(|p| p.available != PortAvailable::No)
                    .filter_map(|p| p.name.clone())
                    .collect(),
            })
        })
        .collect()
}

fn key(card: &str, port: &str) -> String {
    format!("card-output:{}:port:{}", hex_encode(card), hex_encode(port))
}

/// Prefer the current profile, then the one retaining the most existing ports.
/// Never automatically drop microphone ports/input devices of the current profile.
/// Bluetooth codecs and profiles without an explicit port association are excluded.
fn profile_for<'a>(card: &'a Card, port: &Port) -> Option<&'a str> {
    if !card.name.starts_with("alsa_card.") || !port.output || port.available == PortAvailable::No {
        return None;
    }
    let active = card.profiles.iter().find(|p| p.name == card.active);
    card.profiles
        .iter()
        .filter(|profile| {
            profile.available
                && profile.sinks > 0
                && port.profiles.contains(&profile.name)
                && active.is_none_or(|active| profile.sources >= active.sources)
                && card
                    .ports
                    .iter()
                    .filter(|p| !p.output && p.profiles.contains(&card.active))
                    .all(|p| p.profiles.contains(&profile.name))
        })
        .max_by(|left, right| {
            let rank = |profile: &Profile| {
                (
                    profile.name == card.active,
                    card.ports
                        .iter()
                        .filter(|p| {
                            p.profiles.contains(&card.active) && p.profiles.contains(&profile.name)
                        })
                        .count(),
                    profile.priority,
                )
            };
            rank(left)
                .cmp(&rank(right))
                .then_with(|| right.name.cmp(&left.name))
        })
        .map(|profile| profile.name.as_str())
}

pub(super) fn complete_outputs(cards: &[Card], outputs: &[Output], entries: &mut Vec<AudioEntry>) {
    for card in cards
        .iter()
        .filter(|card| card.name.starts_with("alsa_card."))
    {
        for port in card
            .ports
            .iter()
            .filter(|p| p.output && p.available != PortAvailable::No)
        {
            let matches: Vec<_> = entries
                .iter()
                .enumerate()
                .filter(|(_, entry)| {
                    entry.port.as_deref() == Some(port.name.as_str())
                        && outputs.iter().any(|output| {
                            output.card == Some(card.index) && output.name == entry.name
                        })
                })
                .map(|(index, _)| index)
                .collect();
            match matches.as_slice() {
                [index] => {
                    let entry = &mut entries[*index];
                    entry.key = key(&card.name, &port.name);
                    entry.card = Some(card.name.clone());
                }
                [] if profile_for(card, port).is_some() => entries.push(AudioEntry {
                    key: key(&card.name, &port.name),
                    kind: AudioKind::Output,
                    name: String::new(),
                    card: Some(card.name.clone()),
                    description: format!("{} — {}", card.label, port.label),
                    label: single_line(&port.label, usize::MAX),
                    // Not a real sink yet: display an em dash, not a made-up volume.
                    volume: 0,
                    muted: false,
                    default: false,
                    port: Some(port.name.clone()),
                }),
                // A card port shared by multiple live sinks is ambiguous. Keep
                // their existing device-specific rows rather than merging them.
                _ => {}
            }
        }
    }
}

trait Backend {
    fn card(&mut self, name: &str) -> AppResult<Card>;
    fn outputs(&mut self) -> AppResult<Vec<Output>>;
    fn default_output(&mut self) -> AppResult<Option<String>>;
    fn set_profile(&mut self, card: &str, profile: &str) -> AppResult<()>;
    fn set_port(&mut self, output: &Output, port: &str) -> AppResult<()>;
    fn set_default(&mut self, name: &str) -> AppResult<()>;
    fn pause(&mut self) {
        thread::sleep(Duration::from_millis(50));
    }
}

impl Backend for Controller {
    fn card(&mut self, name: &str) -> AppResult<Card> {
        cards(self)?
            .into_iter()
            .find(|card| card.name == name)
            .ok_or_else(|| io::Error::other("The selected sound card disappeared").into())
    }

    fn outputs(&mut self) -> AppResult<Vec<Output>> {
        Ok(outputs(&self.list_devices()?))
    }

    fn default_output(&mut self) -> AppResult<Option<String>> {
        self.default_name()
    }

    fn set_profile(&mut self, card: &str, profile: &str) -> AppResult<()> {
        self.change(|api, done| api.set_card_profile_by_name(card, profile, Some(done)))
    }

    fn set_port(&mut self, output: &Output, port: &str) -> AppResult<()> {
        let device = self.device_by_name(&output.name)?;
        if device.card != output.card {
            return Err(io::Error::other("The selected output changed sound cards").into());
        }
        let port = available_port(&device.ports, port)?;
        if device.active_port.as_ref().and_then(|p| p.name.as_deref()) == Some(port) {
            return Ok(());
        }
        self.change(|api, done| api.set_sink_port_by_name(&output.name, port, Some(done)))
    }

    fn set_default(&mut self, name: &str) -> AppResult<()> {
        if self.set_default_device(name)? {
            Ok(())
        } else {
            Err(io::Error::other("Audio server rejected the default output").into())
        }
    }
}

fn find_output(outputs: Vec<Output>, card: u32, port: &str) -> AppResult<Option<Output>> {
    let mut matching = outputs
        .into_iter()
        .filter(|o| o.card == Some(card) && o.ports.iter().any(|p| p == port));
    let output = matching.next();
    if matching.next().is_some() {
        return Err(io::Error::other("More than one output matches this card port").into());
    }
    Ok(output)
}

const POLL_ATTEMPTS: usize = 40;

fn wait_for_output(
    backend: &mut impl Backend,
    original: &Card,
    port: &str,
    profile: &str,
) -> AppResult<Output> {
    for _ in 0..POLL_ATTEMPTS {
        let card = backend.card(&original.name)?;
        card.output_port(port)?;
        if card.active == profile {
            if let Some(output) = find_output(backend.outputs()?, card.index, port)? {
                return Ok(output);
            }
        } else if card.active != original.active {
            return Err(io::Error::other("The audio profile changed elsewhere").into());
        }
        backend.pause();
    }
    Err(io::Error::other("Timed out waiting for the selected output").into())
}

fn rollback(
    backend: &mut impl Backend,
    original: &Card,
    selected: &str,
    default: Option<&str>,
) -> AppResult<()> {
    let current = backend.card(&original.name)?;
    // Do not overwrite another application's/user's newer profile choice.
    if current.active != selected {
        return Err(io::Error::other("Profile changed elsewhere; not restoring it").into());
    }
    if !current
        .profiles
        .iter()
        .any(|p| p.name == original.active && p.available)
    {
        return Err(io::Error::other("Previous profile is no longer available").into());
    }
    backend.set_profile(&original.name, &original.active)?;
    for _ in 0..POLL_ATTEMPTS {
        let current = backend.card(&original.name)?;
        if current.active == original.active {
            match default {
                None => return Ok(()),
                Some(name) if backend.outputs()?.iter().any(|o| o.name == name) => {
                    return backend.set_default(name);
                }
                _ => {}
            }
        } else if current.active != selected {
            return Err(io::Error::other("Profile changed elsewhere during restoration").into());
        }
        backend.pause();
    }
    Err(io::Error::other("Previous profile or default output did not return").into())
}

pub(super) fn activate(controller: &mut Controller, card: &str, port: &str) -> AppResult<()> {
    activate_with(controller, card, port)
}

pub(super) fn route(
    controller: &mut Controller,
    card: &str,
    port: &str,
    stream: &StreamEntry,
) -> AppResult<()> {
    activate_with_action(controller, card, port, |controller, output| {
        move_stream_to(controller, stream, output)
    })
}

fn activate_with(backend: &mut impl Backend, card_name: &str, port_name: &str) -> AppResult<()> {
    activate_with_action(backend, card_name, port_name, |backend, output| {
        backend.set_default(output)
    })
}

fn activate_with_action<B: Backend>(
    backend: &mut B,
    card_name: &str,
    port_name: &str,
    finish: impl FnOnce(&mut B, &str) -> AppResult<()>,
) -> AppResult<()> {
    let card = backend.card(card_name)?;
    let port = card.output_port(port_name)?;
    if let Some(output) = find_output(backend.outputs()?, card.index, port_name)? {
        backend.set_port(&output, port_name)?;
        return finish(backend, &output.name);
    }
    let profile = profile_for(&card, port)
        .ok_or_else(|| io::Error::other("No compatible profile for the selected output"))?;
    let previous_default = backend.default_output()?;
    let switched = profile != card.active;
    if switched {
        backend.set_profile(card_name, profile)?;
    }
    let result = (|| {
        let output = wait_for_output(backend, &card, port_name, profile)?;
        backend.set_port(&output, port_name)?;
        finish(backend, &output.name)
    })();
    if let Err(error) = result {
        if switched {
            let recovery = match rollback(backend, &card, profile, previous_default.as_deref()) {
                Ok(()) => "Previous profile restored".to_owned(),
                Err(restore) => format!("Could not restore: {restore}"),
            };
            return Err(io::Error::other(format!("{error}. {recovery}")).into());
        }
        return Err(error);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
