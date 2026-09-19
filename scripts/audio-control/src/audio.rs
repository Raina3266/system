use std::{cell::Cell, collections::HashMap, io, rc::Rc};

use libpulse_binding::context::introspect::Introspector;
use libpulse_binding::def::PortAvailable;
use libpulse_binding::operation::Operation;
use libpulse_binding::volume::{ChannelVolumes, Volume};
use pulsectl::controllers::types::{ApplicationInfo, DeviceInfo, DevicePortInfo};
use pulsectl::controllers::{AppControl, DeviceControl, SinkController, SourceController};

use crate::AppResult;
use crate::model::{
    AudioEntry, AudioKind, StreamEntry, hex_encode, short_device_name, single_line,
};

/// Volume step, in percent, for one press of the volume buttons.
pub const STEP: i16 = 5;

/// `SinkController` and `SourceController` implement the same `DeviceControl`
/// trait but are distinct types, so one small enum lets every operation below
/// be written once for outputs and inputs alike.
pub(crate) enum Controller {
    Sink(SinkController),
    Source(SourceController),
}

impl Controller {
    fn handler(&mut self) -> &mut pulsectl::Handler {
        match self {
            Self::Sink(controller) => &mut controller.handler,
            Self::Source(controller) => &mut controller.handler,
        }
    }

    fn create(kind: AudioKind) -> AppResult<Self> {
        Ok(match kind {
            AudioKind::Output => Self::Sink(SinkController::create()?),
            AudioKind::Input => Self::Source(SourceController::create()?),
        })
    }

    fn list_devices(&mut self) -> AppResult<Vec<DeviceInfo>> {
        Ok(match self {
            Self::Sink(controller) => controller.list_devices()?,
            Self::Source(controller) => controller.list_devices()?,
        })
    }

    fn device_by_name(&mut self, name: &str) -> AppResult<DeviceInfo> {
        Ok(match self {
            Self::Sink(controller) => controller.get_device_by_name(name)?,
            Self::Source(controller) => controller.get_device_by_name(name)?,
        })
    }

    /// Read straight off the server rather than through
    /// `get_default_device`, which unwraps the server's name and panics when
    /// no default is set.
    fn default_name(&mut self) -> AppResult<Option<String>> {
        Ok(match self {
            Self::Sink(controller) => controller.get_server_info()?.default_sink_name,
            Self::Source(controller) => controller.get_server_info()?.default_source_name,
        })
    }

    fn set_default_device(&mut self, name: &str) -> AppResult<bool> {
        Ok(match self {
            Self::Sink(controller) => controller.set_default_device(name)?,
            Self::Source(controller) => controller.set_default_device(name)?,
        })
    }

    /// Check both transport completion and the server's acknowledgement. The
    /// convenience volume/mute setters in pulsectl discard operation errors.
    fn change(
        &mut self,
        make_operation: impl FnOnce(
            &mut Introspector,
            Box<dyn FnMut(bool)>,
        ) -> Operation<dyn FnMut(bool)>,
    ) -> AppResult<()> {
        let accepted = Rc::new(Cell::new(false));
        let result = accepted.clone();
        let handler = self.handler();
        let op = make_operation(&mut handler.introspect, Box::new(move |ok| result.set(ok)));
        handler.wait_for_operation(op)?;
        if !accepted.get() {
            return Err(io::Error::other("Audio server rejected the change").into());
        }
        Ok(())
    }

    fn check_stream(&mut self, entry: &StreamEntry) -> AppResult<ApplicationInfo> {
        let Self::Sink(controller) = self else {
            return Err(io::Error::other("Playback requires an output controller").into());
        };
        let app = controller.get_app_by_index(entry.index)?;
        if stream_key(&app) != entry.key {
            return Err(io::Error::other("The selected stream has ended").into());
        }
        Ok(app)
    }
}

pub fn toggle_mute(entry: &AudioEntry) -> AppResult<()> {
    require_live_output(entry)?;
    let mut controller = Controller::create(entry.kind)?;
    let device = controller.device_by_name(&entry.name)?;
    controller.change(|api, done| match entry.kind {
        AudioKind::Output => api.set_sink_mute_by_index(device.index, !device.mute, Some(done)),
        AudioKind::Input => api.set_source_mute_by_index(device.index, !device.mute, Some(done)),
    })
}

pub fn toggle_stream_mute(entry: &StreamEntry) -> AppResult<()> {
    let mut controller = Controller::create(AudioKind::Output)?;
    let app = controller.check_stream(entry)?;
    controller.change(|api, done| api.set_sink_input_mute(app.index, !app.mute, Some(done)))
}

pub fn nudge_stream_volume(entry: &StreamEntry, delta: i16) -> AppResult<()> {
    let mut controller = Controller::create(AudioKind::Output)?;
    let app = controller.check_stream(entry)?;
    if !app.has_volume || !app.volume_writable {
        return Err(io::Error::other("This stream does not support volume changes").into());
    }
    let volumes = adjusted_volume(app.volume, AudioKind::Output, delta)?;
    controller.change(|api, done| api.set_sink_input_volume(app.index, &volumes, Some(done)))
}

pub fn move_stream(entry: &StreamEntry, destination_key: &str) -> AppResult<()> {
    let mut controller = Controller::create(AudioKind::Output)?;
    controller.check_stream(entry)?;
    let destination = routing_devices(&entry.device_name)?
        .into_iter()
        .find(|device| device.key == destination_key)
        .ok_or_else(|| io::Error::other("The selected output is no longer available"))?;
    if let (Some(card), Some(port)) = (destination.card.as_deref(), destination.port.as_deref()) {
        return profiles::route(&mut controller, card, port, entry);
    }
    let device = controller.device_by_name(&destination.name)?;
    if let Some(port) = destination.port.as_deref() {
        let port = available_port(&device.ports, port)?;
        if device.active_port.as_ref().and_then(|p| p.name.as_deref()) != Some(port) {
            controller.change(|api, done| {
                api.set_sink_port_by_name(&destination.name, port, Some(done))
            })?;
        }
    }
    move_stream_to(&mut controller, entry, &destination.name)
}

fn move_stream_to(
    controller: &mut Controller,
    entry: &StreamEntry,
    device_name: &str,
) -> AppResult<()> {
    let app = controller.check_stream(entry)?;
    let device = controller.device_by_name(device_name)?;
    if app.connection_id == device.index {
        return Ok(());
    }
    controller.change(|api, done| api.move_sink_input_by_index(app.index, device.index, Some(done)))
}

pub(crate) fn available_port<'a>(ports: &'a [DevicePortInfo], name: &str) -> AppResult<&'a str> {
    ports
        .iter()
        .find(|p| p.name.as_deref() == Some(name))
        .filter(|p| p.available != PortAvailable::No)
        .and_then(|p| p.name.as_deref())
        .ok_or_else(|| io::Error::other("The selected port is no longer available").into())
}

pub fn streams() -> AppResult<Vec<StreamEntry>> {
    let mut controller = SinkController::create()?;
    let devices = controller.list_devices()?;
    let mut entries: Vec<_> = controller
        .list_applications()?
        .iter()
        .map(|app| {
            let device = devices.iter().find(|d| d.index == app.connection_id);
            let device_entry = device.and_then(|d| entry(AudioKind::Output, d, None));
            let application = app
                .proplist
                .get_str("application.name")
                .filter(|name| !name.is_empty())
                .or_else(|| app.proplist.get_str("application.process.binary"))
                .unwrap_or_else(|| "Audio stream".into());
            StreamEntry {
                key: stream_key(app),
                index: app.index,
                application,
                name: app.name.clone().unwrap_or_default(),
                device_name: device_entry
                    .as_ref()
                    .map(|d| d.name.clone())
                    .unwrap_or_default(),
                device_label: device_entry
                    .map(|d| d.label)
                    .unwrap_or_else(|| "Unknown device".into()),
                volume: app.has_volume.then(|| percent(&app.volume)),
                muted: app.mute,
                corked: app.corked,
            }
        })
        .collect();
    entries.sort_by(|a, b| {
        a.application
            .to_lowercase()
            .cmp(&b.application.to_lowercase())
            .then_with(|| a.name.cmp(&b.name))
            .then_with(|| a.index.cmp(&b.index))
    });
    Ok(entries)
}

fn stream_key(app: &ApplicationInfo) -> String {
    stream_identity(
        app.index,
        app.client,
        &app.proplist.get_str("object.serial").unwrap_or_default(),
    )
}

pub(crate) fn stream_identity(index: u32, client: Option<u32>, serial: &str) -> String {
    // The PipeWire object serial protects a picker from stream-index reuse.
    // Native PulseAudio uses monotonically allocated stream/client indices.
    format!(
        "playback:{index}:{}:{}",
        client.map(|v| v.to_string()).unwrap_or_default(),
        hex_encode(serial)
    )
}

pub fn snapshot(kind: AudioKind) -> AppResult<Vec<AudioEntry>> {
    snapshot_with_rows(kind, DeviceRows::Devices, None)
}

/// Output/Input rows expose physical ports without inventing independent
/// devices. Waybar still keeps one row per device.
pub fn selections(kind: AudioKind) -> AppResult<Vec<AudioEntry>> {
    snapshot_with_rows(kind, DeviceRows::Ports, None)
}

/// Include ports from compatible inactive profiles, marking the stream's
/// current port rather than the system default.
pub fn routing_devices(current_device: &str) -> AppResult<Vec<AudioEntry>> {
    snapshot_with_rows(AudioKind::Output, DeviceRows::Ports, Some(current_device))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DeviceRows {
    Devices,
    Ports,
}

fn snapshot_with_rows(
    kind: AudioKind,
    rows: DeviceRows,
    current_device: Option<&str>,
) -> AppResult<Vec<AudioEntry>> {
    let mut controller = Controller::create(kind)?;
    let default_name = match current_device {
        Some(name) => Some(name.to_owned()),
        None => controller.default_name()?,
    };
    let mut device_labels = HashMap::new();
    let devices = controller.list_devices()?;
    let mut entries: Vec<_> = devices
        .iter()
        .filter(|device| kind == AudioKind::Output || device.monitor.is_none())
        .flat_map(|device| {
            let Some(base) = entry(kind, device, default_name.as_deref()) else {
                return Vec::new();
            };
            if rows != DeviceRows::Devices {
                device_labels.insert(
                    base.name.clone(),
                    short_device_name(&base.description, None),
                );
            }
            if rows == DeviceRows::Ports {
                port_rows(
                    base,
                    &device.ports,
                    device.active_port.as_ref().and_then(|p| p.name.as_deref()),
                )
            } else {
                vec![base]
            }
        })
        .collect();
    if kind == AudioKind::Output && rows == DeviceRows::Ports {
        let cards = profiles::cards(&mut controller)?;
        profiles::complete_outputs(&cards, &profiles::outputs(&devices), &mut entries);
        for card in cards {
            device_labels.insert(card.name, short_device_name(&card.label, None));
        }
    }
    entries.sort_by(|left, right| {
        left.description
            .to_lowercase()
            .cmp(&right.description.to_lowercase())
            .then_with(|| left.name.cmp(&right.name))
            .then_with(|| left.key.cmp(&right.key))
    });
    if rows != DeviceRows::Devices {
        clarify_selection_labels(&mut entries, &device_labels);
    }
    Ok(entries)
}

/// Ports carry the useful difference (Speakers, Headphones, HDMI/DisplayPort
/// number). Only repeat the hardware name when two choices would otherwise
/// look identical. Keep all identifiers and full search descriptions intact.
pub(crate) fn clarify_selection_labels(
    entries: &mut [AudioEntry],
    device_labels: &HashMap<String, String>,
) {
    let labels: Vec<_> = entries.iter().map(|e| e.label.to_lowercase()).collect();
    for (entry, label) in entries.iter_mut().zip(&labels) {
        if labels.iter().filter(|other| *other == label).count() > 1
            && let Some(device) = device_labels.get(entry.card.as_deref().unwrap_or(&entry.name))
            && !entry.label.eq_ignore_ascii_case(device)
        {
            entry.label = format!("{} — {device}", entry.label);
        }
    }

    // Identical models/port descriptions can still collide. Their sorted row
    // order is deterministic; a small number distinguishes them without using
    // a long PulseAudio node name. Actions still use the original stable key.
    let labels: Vec<_> = entries
        .iter()
        .map(|e| single_line(&e.label, 48).to_lowercase())
        .collect();
    let mut used: std::collections::HashSet<_> = labels.iter().cloned().collect();
    for (entry, label) in entries.iter_mut().zip(&labels) {
        if labels.iter().filter(|other| *other == label).count() > 1 {
            let mut number = 1;
            loop {
                // Prefix the number so even a clipped row stays distinct.
                let candidate = format!("#{number} {}", entry.label);
                if used.insert(single_line(&candidate, 48).to_lowercase()) {
                    entry.label = candidate;
                    break;
                }
                number += 1;
            }
        }
    }
}

pub fn set_default(entry: &AudioEntry) -> AppResult<()> {
    let mut controller = Controller::create(entry.kind)?;
    if entry.kind == AudioKind::Output
        && let (Some(card), Some(port)) = (entry.card.as_deref(), entry.port.as_deref())
    {
        return profiles::activate(&mut controller, card, port);
    }
    // Revalidate against the server: a jack can be unplugged after rendering.
    // Switch the port first, so a rejected switch never changes the default.
    let device = controller.device_by_name(&entry.name)?;
    if let Some(name) = entry.port.as_deref() {
        let port = available_port(&device.ports, name)?;
        controller.change(|api, done| match entry.kind {
            AudioKind::Output => api.set_sink_port_by_index(device.index, port, Some(done)),
            AudioKind::Input => api.set_source_port_by_index(device.index, port, Some(done)),
        })?;
    }
    if controller.set_default_device(&entry.name)? {
        return Ok(());
    }
    Err(io::Error::other("PulseAudio rejected the default device change").into())
}

/// Applies a relative volume change to one device and returns the new level.
/// The step is applied to the value read back here rather than to the rendered
/// row, so repeated presses never drift out of sync with the server.
pub fn nudge_volume(entry: &AudioEntry, delta: i16) -> AppResult<u8> {
    require_live_output(entry)?;
    let mut controller = Controller::create(entry.kind)?;
    let device = controller.device_by_name(&entry.name)?;
    let volumes = adjusted_volume(device.volume, entry.kind, delta)?;
    controller.change(|api, done| match entry.kind {
        AudioKind::Output => api.set_sink_volume_by_index(device.index, &volumes, Some(done)),
        AudioKind::Input => api.set_source_volume_by_index(device.index, &volumes, Some(done)),
    })?;
    Ok(percent(&volumes))
}

pub(crate) fn require_live_output(entry: &AudioEntry) -> AppResult<()> {
    if entry.inactive() {
        return Err(io::Error::other("Select this output first").into());
    }
    Ok(())
}

pub(crate) fn volume_target(current: u8, kind: AudioKind, delta: i16) -> u8 {
    (i16::from(current) + delta).clamp(0, kind.maximum()) as u8
}

pub(crate) fn adjusted_volume(
    mut volume: ChannelVolumes,
    kind: AudioKind,
    delta: i16,
) -> AppResult<ChannelVolumes> {
    if volume.len() == 0 {
        return Err(io::Error::other("This device has no volume channels").into());
    }
    let target = from_percent(volume_target(percent(&volume), kind, delta));
    let average = volume.avg().0;
    // Preserve an existing balance set in another mixer. Also cap the loudest
    // channel so an uneven channel layout cannot bypass the volume ceiling.
    let peak = if average == 0 {
        target.0
    } else {
        ((f64::from(target.0) * f64::from(volume.max().0) / f64::from(average)).round() as u32)
            .min(from_percent(kind.maximum() as u8).0)
    };
    volume
        .scale(Volume(peak))
        .ok_or_else(|| io::Error::other("Invalid channel volume"))?;
    Ok(volume)
}

/// The default output and input, for the Waybar module.
pub fn defaults() -> (Option<AudioEntry>, Option<AudioEntry>) {
    (default_of(AudioKind::Output), default_of(AudioKind::Input))
}

fn default_of(kind: AudioKind) -> Option<AudioEntry> {
    snapshot(kind).ok()?.into_iter().find(|entry| entry.default)
}

fn entry(kind: AudioKind, device: &DeviceInfo, default_name: Option<&str>) -> Option<AudioEntry> {
    let name = device.name.clone()?;
    let description = device
        .description
        .clone()
        .filter(|description| !description.is_empty())
        .unwrap_or_else(|| name.clone());
    let port = device
        .active_port
        .as_ref()
        .and_then(|port| port.description.as_deref());
    Some(AudioEntry {
        key: format!("{}:{}", kind.key_prefix(), hex_encode(&name)),
        default: default_name == Some(name.as_str()),
        volume: percent(&device.volume),
        muted: device.mute,
        label: short_device_name(&description, port),
        kind,
        name,
        card: None,
        description,
        port: None,
    })
}

pub(crate) fn port_rows(
    base: AudioEntry,
    ports: &[DevicePortInfo],
    active: Option<&str>,
) -> Vec<AudioEntry> {
    // USB/Bluetooth/virtual devices without named ports still get one row.
    if !ports
        .iter()
        .any(|p| p.name.as_deref().is_some_and(|name| !name.is_empty()))
    {
        return vec![base];
    }
    ports
        .iter()
        .filter_map(|port| {
            let name = port.name.as_deref().filter(|name| !name.is_empty())?;
            if port.available == PortAvailable::No {
                return None;
            }
            let label = port
                .description
                .as_deref()
                .filter(|label| !label.is_empty())
                .unwrap_or(name);
            Some(AudioEntry {
                key: format!("{}:port:{}", base.key, hex_encode(name)),
                port: Some(name.to_owned()),
                description: format!("{} — {label}", base.description),
                // The port already says what this row selects. Do not repeat
                // the controller description or truncate HDMI port numbers.
                label: single_line(label, usize::MAX),
                default: base.default && active == Some(name),
                ..base.clone()
            })
        })
        .collect()
}

pub(crate) fn percent(volumes: &ChannelVolumes) -> u8 {
    let normal = f64::from(Volume::NORMAL.0);
    let average = f64::from(volumes.avg().0);
    ((average / normal) * 100.0).round().clamp(0.0, 255.0) as u8
}

pub(crate) fn from_percent(percent: u8) -> Volume {
    let normal = f64::from(Volume::NORMAL.0);
    Volume((normal * f64::from(percent) / 100.0).round() as u32)
}

pub(crate) mod profiles {
    //! ALSA outputs that are exposed by mutually exclusive card profiles.
    //! Card/port names are the identity; sink names and indexes are re-resolved.
    use std::{cell::RefCell, thread, time::Duration};

    use libpulse_binding::{callbacks::ListResult, context::introspect::CardInfo, direction};

    use super::*;

    #[derive(Clone, Debug)]
    pub(crate) struct Card {
        pub(crate) index: u32,
        pub name: String,
        pub label: String,
        pub(crate) active: String,
        pub(crate) profiles: Vec<Profile>,
        pub(crate) ports: Vec<Port>,
    }

    #[derive(Clone, Debug)]
    pub(crate) struct Profile {
        pub(crate) name: String,
        pub(crate) available: bool,
        pub(crate) sinks: u32,
        pub(crate) sources: u32,
        pub(crate) priority: u32,
    }

    #[derive(Clone, Debug)]
    pub(crate) struct Port {
        pub(crate) name: String,
        pub(crate) label: String,
        pub(crate) output: bool,
        pub(crate) available: PortAvailable,
        pub(crate) profiles: Vec<String>,
    }

    #[derive(Clone, Debug)]
    pub(crate) struct Output {
        pub(crate) card: Option<u32>,
        pub(crate) name: String,
        pub(crate) ports: Vec<String>,
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
                .ok_or_else(|| {
                    io::Error::other("The selected output is no longer available").into()
                })
        }
    }

    pub(crate) fn cards(controller: &mut Controller) -> AppResult<Vec<Card>> {
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

    pub(crate) fn outputs(devices: &[DeviceInfo]) -> Vec<Output> {
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

    pub(crate) fn key(card: &str, port: &str) -> String {
        format!("card-output:{}:port:{}", hex_encode(card), hex_encode(port))
    }

    /// Prefer the current profile, then the one retaining the most existing ports.
    /// Never automatically drop microphone ports/input devices of the current profile.
    /// Bluetooth codecs and profiles without an explicit port association are excluded.
    pub(crate) fn profile_for<'a>(card: &'a Card, port: &Port) -> Option<&'a str> {
        if !card.name.starts_with("alsa_card.")
            || !port.output
            || port.available == PortAvailable::No
        {
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
                                p.profiles.contains(&card.active)
                                    && p.profiles.contains(&profile.name)
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

    pub(crate) fn complete_outputs(
        cards: &[Card],
        outputs: &[Output],
        entries: &mut Vec<AudioEntry>,
    ) {
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

    pub(crate) trait Backend {
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

    pub(crate) fn find_output(
        outputs: Vec<Output>,
        card: u32,
        port: &str,
    ) -> AppResult<Option<Output>> {
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
                return Err(
                    io::Error::other("Profile changed elsewhere during restoration").into(),
                );
            }
            backend.pause();
        }
        Err(io::Error::other("Previous profile or default output did not return").into())
    }

    pub(crate) fn activate(controller: &mut Controller, card: &str, port: &str) -> AppResult<()> {
        activate_with(controller, card, port)
    }

    pub(crate) fn route(
        controller: &mut Controller,
        card: &str,
        port: &str,
        stream: &StreamEntry,
    ) -> AppResult<()> {
        activate_with_action(controller, card, port, |controller, output| {
            move_stream_to(controller, stream, output)
        })
    }

    pub(crate) fn activate_with(
        backend: &mut impl Backend,
        card_name: &str,
        port_name: &str,
    ) -> AppResult<()> {
        activate_with_action(backend, card_name, port_name, |backend, output| {
            backend.set_default(output)
        })
    }

    pub(crate) fn activate_with_action<B: Backend>(
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
                let recovery = match rollback(backend, &card, profile, previous_default.as_deref())
                {
                    Ok(()) => "Previous profile restored".to_owned(),
                    Err(restore) => format!("Could not restore: {restore}"),
                };
                return Err(io::Error::other(format!("{error}. {recovery}")).into());
            }
            return Err(error);
        }
        Ok(())
    }
}
