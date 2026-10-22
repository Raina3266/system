//! Physical rows and profile/rollback policy. Every mutation is an Astal RPC.

use std::{
    collections::HashMap,
    thread,
    time::{Duration, Instant},
};

use serde::Serialize;

use crate::{
    AppResult,
    devices::{Metadata, Profile, Route},
    model::{
        AudioKind, Card, Choice, Identity, Node, Snapshot, hex_encode, short_device_name,
        single_line,
    },
};

#[derive(Clone, Debug)]
pub struct State {
    pub snapshot: Snapshot,
    pub metadata: Metadata,
}

impl State {
    pub fn load(snapshot: Snapshot, kind: AudioKind) -> AppResult<Self> {
        snapshot.validate()?;
        let metadata = Metadata::load(&snapshot, kind)?;
        Ok(Self { snapshot, metadata })
    }
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(tag = "operation", rename_all = "lowercase")]
pub enum Action {
    Profile { card: Identity, profile: i32 },
    Route { node: Identity, route: i32 },
    Default { node: Identity },
}

pub trait Backend {
    fn observe(&mut self) -> AppResult<State>;
    fn action(&mut self, action: Action) -> AppResult<()>;
    fn pause(&mut self) {
        thread::sleep(Duration::from_millis(50));
    }
}

pub fn physical_key(card: &str, port: &str) -> String {
    format!("card-output:{}:port:{}", hex_encode(card), hex_encode(port))
}

fn node_key(snapshot: &Snapshot, node: &Node) -> String {
    let base = format!("{}:{}", node.kind.key_prefix(), hex_encode(&node.name));
    if node.name.is_empty()
        || snapshot
            .nodes
            .iter()
            .filter(|other| other.kind == node.kind && other.name == node.name)
            .count()
            > 1
    {
        format!("{base}:node:{}:{}", node.id, hex_encode(&node.serial))
    } else {
        base
    }
}

fn card_for<'a>(snapshot: &'a Snapshot, node: &Node) -> Option<&'a Card> {
    snapshot
        .cards
        .iter()
        .find(|card| Some(card.id) == node.card_id && card.serial == node.card_serial)
}

fn route_available(state: &State, card: &Card, route: &Route) -> bool {
    let mut native = state
        .snapshot
        .nodes
        .iter()
        .filter(|node| node.card_id == Some(card.id) && node.card_serial == card.serial)
        .flat_map(|node| &node.routes)
        .filter(|native| native.index == route.index && native.name == route.name)
        .peekable();
    if native.peek().is_some() {
        native.any(|native| native.available)
    } else {
        route.available
    }
}

fn route_label<'a>(state: &'a State, card: &Card, route: &'a Route) -> &'a str {
    let native = state
        .snapshot
        .nodes
        .iter()
        .filter(|node| node.card_id == Some(card.id) && node.card_serial == card.serial)
        .flat_map(|node| &node.routes)
        .find(|native| native.index == route.index && native.name == route.name);
    if let Some(native) = native {
        if native.description.is_empty() {
            &native.name
        } else {
            &native.description
        }
    } else if route.description.is_empty() {
        &route.name
    } else {
        &route.description
    }
}

fn card_label(snapshot: &Snapshot, card: &Card) -> String {
    snapshot
        .nodes
        .iter()
        .find(|node| node.card_id == Some(card.id) && !node.description.is_empty())
        .map(|node| short_device_name(&node.description, None))
        .unwrap_or_else(|| card.name.clone())
}

pub fn rows(state: &State, kind: AudioKind) -> AppResult<Vec<Choice>> {
    let snapshot = &state.snapshot;
    let mut choices = Vec::new();
    let mut labels: HashMap<_, _> = snapshot
        .cards
        .iter()
        .map(|card| (card.name.clone(), card_label(snapshot, card)))
        .collect();
    for node in snapshot.nodes.iter().filter(|node| node.kind == kind) {
        let card = card_for(snapshot, node);
        let description = if node.description.is_empty() {
            &node.name
        } else {
            &node.description
        };
        let base = Choice {
            key: node_key(snapshot, node),
            label: short_device_name(description, None),
            description: description.clone(),
            name: node.name.clone(),
            card: card.map(|card| card.name.clone()).unwrap_or_default(),
            port: false,
            port_name: String::new(),
            port_active: false,
            node_id: Some(node.id),
            node_serial: node.serial.clone(),
            card_id: node.card_id,
            card_serial: node.card_serial.clone(),
            route_id: None,
        };
        labels.insert(
            node.name.clone(),
            short_device_name(&node.description, None),
        );
        let associated: Vec<_> = card
            .into_iter()
            .flat_map(|card| state.metadata.routes_for(card, node))
            .collect();
        let routes: Vec<_> = associated
            .iter()
            .copied()
            .filter_map(|route| {
                node.routes
                    .iter()
                    .find(|native| native.index == route.index && native.name == route.name)
                    .map(|native| (route, native))
            })
            .collect();
        if routes.is_empty() {
            // Card-wide native routes may omit this endpoint's associations;
            // known unavailable ports must not become device-only choices.
            if card.is_some_and(|card| {
                !associated.is_empty()
                    && associated
                        .iter()
                        .all(|route| !route_available(state, card, route))
            }) {
                continue;
            }
            // Missing associations/native route objects are not an excuse to
            // invent a route action. The native endpoint still supports defaults.
            choices.push(base);
        } else {
            for (route, native) in routes.into_iter().filter(|(_, native)| native.available) {
                let label = if native.description.is_empty() {
                    &native.name
                } else {
                    &native.description
                };
                choices.push(Choice {
                    key: format!("{}:port:{}", base.key, hex_encode(&native.name)),
                    label: single_line(label, usize::MAX),
                    description: format!("{} — {label}", base.description),
                    port: true,
                    port_name: native.name.clone(),
                    port_active: card
                        .is_some_and(|card| state.metadata.port_active(card, node, route)),
                    route_id: Some(native.index),
                    ..base.clone()
                });
            }
        }
    }
    if kind == AudioKind::Output {
        for card in snapshot
            .cards
            .iter()
            .filter(|card| card.name.starts_with("alsa_card."))
        {
            let label = card_label(snapshot, card);
            labels.insert(card.name.clone(), label.clone());
            let Some(data) = state.metadata.card(card) else {
                continue;
            };
            for route in data.routes.iter().filter(|route| route.kind == kind) {
                let matching: Vec<_> = choices
                    .iter()
                    .enumerate()
                    .filter(|(_, row)| {
                        row.card_id == Some(card.id)
                            && row.card_serial == card.serial
                            && row.port
                            && row.route_id == Some(route.index)
                            && row.port_name == route.name
                    })
                    .map(|(index, _)| index)
                    .collect();
                match matching.as_slice() {
                    [index] => choices[*index].key = physical_key(&card.name, &route.name),
                    [] if route_available(state, card, route)
                        && profile_for(state, card, route).is_some() =>
                    {
                        let port_label = route_label(state, card, route);
                        choices.push(Choice {
                            key: physical_key(&card.name, &route.name),
                            label: single_line(port_label, usize::MAX),
                            description: format!("{label} — {port_label}"),
                            name: String::new(),
                            card: card.name.clone(),
                            port: true,
                            port_name: route.name.clone(),
                            port_active: false,
                            node_id: None,
                            node_serial: String::new(),
                            card_id: Some(card.id),
                            card_serial: card.serial.clone(),
                            route_id: Some(route.index),
                        });
                    }
                    _ => {}
                }
            }
        }
    }
    if choices.len() > 4096 {
        return Err("Too many physical audio choices".into());
    }
    choices.sort_by(|left, right| {
        left.description
            .to_lowercase()
            .cmp(&right.description.to_lowercase())
            .then_with(|| left.name.cmp(&right.name))
            .then_with(|| left.key.cmp(&right.key))
    });
    clarify_selection_labels(&mut choices, &labels);
    Ok(choices)
}

pub(crate) fn clarify_selection_labels(choices: &mut [Choice], labels: &HashMap<String, String>) {
    let names: Vec<_> = choices.iter().map(|row| row.label.to_lowercase()).collect();
    for (row, label) in choices.iter_mut().zip(&names) {
        if names.iter().filter(|other| *other == label).count() > 1
            && let Some(device) = labels.get(if row.card.is_empty() {
                &row.name
            } else {
                &row.card
            })
            && !row.label.eq_ignore_ascii_case(device)
        {
            row.label = format!("{} — {device}", row.label);
        }
    }
    let names: Vec<_> = choices
        .iter()
        .map(|row| single_line(&row.label, 48).to_lowercase())
        .collect();
    let mut used: std::collections::HashSet<_> = names.iter().cloned().collect();
    for (row, label) in choices.iter_mut().zip(&names) {
        if names.iter().filter(|other| *other == label).count() > 1 {
            let mut number = 1;
            loop {
                let candidate = format!("#{number} {}", row.label);
                if used.insert(single_line(&candidate, 48).to_lowercase()) {
                    row.label = candidate;
                    break;
                }
                number += 1;
            }
        }
    }
}

/// Keep all current microphone devices/ports, then prefer profiles retaining
/// the most existing ports (including HDMI), ahead of profile priority.
pub(crate) fn profile_for<'a>(state: &'a State, card: &Card, route: &Route) -> Option<&'a Profile> {
    if !card.name.starts_with("alsa_card.")
        || route.kind != AudioKind::Output
        || !route_available(state, card, route)
    {
        return None;
    }
    let data = state.metadata.card(card)?;
    let active = data
        .profiles
        .iter()
        .find(|profile| profile.index == card.active_profile)?;
    data.profiles
        .iter()
        .filter(|profile| {
            profile.available
                && !profile.sinks.is_empty()
                && card
                    .profiles
                    .iter()
                    .any(|native| native.index == profile.index)
                && route.profiles.contains(&profile.index)
                && route
                    .devices
                    .iter()
                    .any(|device| profile.sinks.contains(device))
                && profile.sources.len() >= active.sources.len()
                && data
                    .routes
                    .iter()
                    .filter(|port| {
                        port.kind == AudioKind::Input
                            && port.profiles.contains(&card.active_profile)
                    })
                    .all(|port| {
                        port.profiles.contains(&profile.index)
                            && port
                                .devices
                                .iter()
                                .any(|device| profile.sources.contains(device))
                    })
                && state
                    .snapshot
                    .nodes
                    .iter()
                    .filter(|node| node.kind == AudioKind::Input && node.card_id == Some(card.id))
                    .all(|node| {
                        profile.index == card.active_profile
                            || node
                                .profile_device
                                .is_some_and(|device| profile.sources.contains(&device))
                    })
        })
        .max_by(|left, right| {
            let rank = |profile: &Profile| {
                (
                    profile.index == card.active_profile,
                    data.routes
                        .iter()
                        .filter(|port| {
                            port.profiles.contains(&card.active_profile)
                                && port.profiles.contains(&profile.index)
                        })
                        .count(),
                    profile.priority,
                )
            };
            rank(left)
                .cmp(&rank(right))
                .then_with(|| right.name.cmp(&left.name))
        })
}

#[derive(Clone)]
struct Guard {
    card: Identity,
    name: String,
    profile: i32,
}

impl Guard {
    fn new(card: &Card, profile: i32) -> Self {
        Self {
            card: card.identity(),
            name: card.name.clone(),
            profile,
        }
    }
    fn check<'a>(&self, state: &'a State) -> AppResult<&'a Card> {
        let card = state.snapshot.card(&self.card)?;
        if card.name != self.name || card.active_profile != self.profile {
            return Err("The audio profile changed elsewhere".into());
        }
        Ok(card)
    }
}

fn binding_matches(original: &Choice, current: &Choice) -> AppResult<()> {
    if original.card_id != current.card_id
        || original.card_serial != current.card_serial
        || original.card != current.card
        || original.port != current.port
        || original.port_name != current.port_name
        || original.route_id != current.route_id
        || (original.node_id.is_some()
            && (original.node_id != current.node_id
                || original.node_serial != current.node_serial
                || original.name != current.name))
    {
        return Err("The selected audio device disappeared or was replaced".into());
    }
    Ok(())
}

fn current_choice(state: &State, kind: AudioKind, target: &Choice) -> AppResult<Choice> {
    let row = rows(state, kind)?
        .into_iter()
        .find(|row| row.key == target.key)
        .ok_or("The selected audio device or port is no longer available")?;
    binding_matches(target, &row)?;
    Ok(row)
}

const POLL_ATTEMPTS: usize = 40;
const POLL_TIMEOUT: Duration = Duration::from_secs(4);

fn wait_for<T>(
    backend: &mut impl Backend,
    mut check: impl FnMut(&State) -> AppResult<Option<T>>,
) -> AppResult<T> {
    let deadline = Instant::now() + POLL_TIMEOUT;
    for attempt in 0..POLL_ATTEMPTS {
        if attempt > 0 && Instant::now() >= deadline {
            break;
        }
        let state = backend.observe()?;
        if let Some(value) = check(&state)? {
            return Ok(value);
        }
        backend.pause();
    }
    Err("Timed out waiting for the selected audio endpoint/route".into())
}

fn live_choice(
    state: &State,
    kind: AudioKind,
    target: &Choice,
    guard: Option<&Guard>,
) -> AppResult<Choice> {
    if let Some(guard) = guard {
        guard.check(state)?;
    }
    let row = current_choice(state, kind, target)?;
    let identity = Identity {
        id: row.node_id.ok_or("The selected endpoint is not live")?,
        serial: row.node_serial.clone(),
    };
    let node = state.snapshot.node(&identity)?;
    if node.kind != kind || node.name != row.name {
        return Err("The selected endpoint changed identity".into());
    }
    Ok(row)
}

fn apply_live(
    backend: &mut impl Backend,
    kind: AudioKind,
    target: &Choice,
    guard: Option<&Guard>,
) -> AppResult<()> {
    let current = backend.observe()?;
    let row = live_choice(&current, kind, target, guard)?;
    let node = Identity {
        id: row.node_id.ok_or("The selected endpoint is not live")?,
        serial: row.node_serial.clone(),
    };
    if row.port && !row.port_active {
        backend.action(Action::Route {
            node: node.clone(),
            route: row.route_id.ok_or("Missing native route index")?,
        })?;
        wait_for(backend, |state| {
            let row = live_choice(state, kind, target, guard)?;
            Ok(row.port_active.then_some(()))
        })?;
    }
    let current = backend.observe()?;
    let row = live_choice(&current, kind, target, guard)?;
    if row.port && !row.port_active {
        return Err("The selected endpoint route changed elsewhere".into());
    }
    backend.action(Action::Default { node: node.clone() })?;
    let current = backend.observe()?;
    live_choice(&current, kind, target, guard)?;
    if current.snapshot.default_node(kind).map(Node::identity) != Some(node) {
        return Err("Astal did not confirm the requested default endpoint".into());
    }
    Ok(())
}

fn output_for(state: &State, card: &Card, target: &Choice) -> AppResult<Option<Choice>> {
    let matching: Vec<_> = rows(state, AudioKind::Output)?
        .into_iter()
        .filter(|row| {
            row.card_id == Some(card.id)
                && row.card_serial == card.serial
                && row.node_id.is_some()
                && row.port_name == target.port_name
                && row.route_id == target.route_id
        })
        .collect();
    if matching.len() > 1 {
        return Err("More than one endpoint matches this card port".into());
    }
    Ok(matching.into_iter().next())
}

fn restore(
    backend: &mut impl Backend,
    original: &Card,
    selected: i32,
    previous: Option<&Node>,
) -> AppResult<()> {
    let state = backend.observe()?;
    let current = state.snapshot.card(&original.identity())?;
    if current.name != original.name {
        return Err("The sound card changed identity during restoration".into());
    }
    if current.active_profile != selected {
        return Err("Profile changed elsewhere; not restoring it".into());
    }
    if !current
        .profiles
        .iter()
        .any(|profile| profile.index == original.active_profile)
        || !state.metadata.card(current).is_some_and(|data| {
            data.profiles
                .iter()
                .any(|profile| profile.index == original.active_profile && profile.available)
        })
    {
        return Err("Previous profile is no longer available".into());
    }
    backend.action(Action::Profile {
        card: current.identity(),
        profile: original.active_profile,
    })?;
    let guard = Guard::new(original, original.active_profile);
    let restored = wait_for(backend, |state| {
        let card = state.snapshot.card(&original.identity())?;
        if card.active_profile == selected {
            return Ok(None);
        }
        guard.check(state)?;
        let Some(previous) = previous else {
            return Ok(Some(None));
        };
        let exact = state
            .snapshot
            .nodes
            .iter()
            .find(|node| node.identity() == previous.identity());
        // Only endpoints on the switched card are expected to be recreated.
        // Do not rebind an unrelated/virtual endpoint solely by its display name.
        let matching: Vec<_> = state
            .snapshot
            .nodes
            .iter()
            .filter(|node| {
                previous.card_id == Some(original.id)
                    && previous.card_serial == original.serial
                    && node.card_id == previous.card_id
                    && node.card_serial == previous.card_serial
                    && node.name == previous.name
                    && node.kind == previous.kind
                    && node.profile_device == previous.profile_device
            })
            .collect();
        if matching.len() > 1 {
            return Err("Previous default endpoint is ambiguous".into());
        }
        Ok(exact
            .or_else(|| matching.first().copied())
            .map(|node| Some(node.identity())))
    })?;
    if let Some(node) = restored {
        let state = backend.observe()?;
        guard.check(&state)?;
        state.snapshot.node(&node)?;
        backend.action(Action::Default { node })?;
    }
    Ok(())
}

pub fn select(
    backend: &mut impl Backend,
    initial: &State,
    kind: AudioKind,
    key: &str,
) -> AppResult<()> {
    let target = rows(initial, kind)?
        .into_iter()
        .find(|row| row.key == key)
        .ok_or("The selected audio device is no longer available")?;
    let state = backend.observe()?;
    let target = current_choice(&state, kind, &target)?;
    if target.node_id.is_some() {
        let guard = target
            .card_id
            .and_then(|id| state.snapshot.cards.iter().find(|card| card.id == id))
            .map(|card| Guard::new(card, card.active_profile));
        return apply_live(backend, kind, &target, guard.as_ref());
    }
    if kind != AudioKind::Output {
        return Err("Only ALSA output profiles can be activated automatically".into());
    }
    let original = state
        .snapshot
        .card(&Identity {
            id: target.card_id.ok_or("Missing sound card identity")?,
            serial: target.card_serial.clone(),
        })?
        .clone();
    let route = state
        .metadata
        .card(&original)
        .and_then(|data| {
            data.routes.iter().find(|route| {
                Some(route.index) == target.route_id && route.name == target.port_name
            })
        })
        .ok_or("The selected physical port disappeared")?;
    let profile = profile_for(&state, &original, route)
        .ok_or("No compatible profile for the selected output")?
        .index;
    let previous = state.snapshot.default_node(AudioKind::Output).cloned();
    let switched = profile != original.active_profile;
    let result = (|| {
        if switched {
            backend.action(Action::Profile {
                card: original.identity(),
                profile,
            })?;
        }
        let guard = Guard::new(&original, profile);
        let output = wait_for(backend, |state| {
            let card = state.snapshot.card(&original.identity())?;
            if card.active_profile == original.active_profile && switched {
                return Ok(None);
            }
            guard.check(state)?;
            let route = state
                .metadata
                .card(card)
                .and_then(|data| {
                    data.routes.iter().find(|route| {
                        Some(route.index) == target.route_id
                            && route.name == target.port_name
                            && route_available(state, card, route)
                    })
                })
                .ok_or("The selected output is no longer available")?;
            if !route.profiles.contains(&profile) {
                return Err("The selected port changed profiles".into());
            }
            output_for(state, card, &target)
        })?;
        apply_live(backend, AudioKind::Output, &output, Some(&guard))
    })();
    if let Err(error) = result {
        if switched {
            // A rejected action may have changed nothing, or may have applied
            // before its acknowledgement failed. Only recover our own profile.
            let current = backend.observe()?;
            let card = current.snapshot.card(&original.identity())?;
            if card.active_profile == original.active_profile {
                return Err(error);
            }
            let recovery = match restore(backend, &original, profile, previous.as_ref()) {
                Ok(()) => "Previous profile restored".to_owned(),
                Err(restore) => format!("Could not restore: {restore}"),
            };
            return Err(format!("{error}. {recovery}").into());
        }
        return Err(error);
    }
    Ok(())
}
