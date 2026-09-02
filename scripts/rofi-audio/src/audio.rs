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

mod profiles;

/// Volume step, in percent, for one press of the volume buttons.
pub const STEP: i16 = 5;

/// `SinkController` and `SourceController` implement the same `DeviceControl`
/// trait but are distinct types, so one small enum lets every operation below
/// be written once for outputs and inputs alike.
enum Controller {
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

fn available_port<'a>(ports: &'a [DevicePortInfo], name: &str) -> AppResult<&'a str> {
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

fn stream_identity(index: u32, client: Option<u32>, serial: &str) -> String {
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
fn clarify_selection_labels(entries: &mut [AudioEntry], device_labels: &HashMap<String, String>) {
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

fn require_live_output(entry: &AudioEntry) -> AppResult<()> {
    if entry.inactive() {
        return Err(io::Error::other("Select this output first").into());
    }
    Ok(())
}

fn volume_target(current: u8, kind: AudioKind, delta: i16) -> u8 {
    (i16::from(current) + delta).clamp(0, kind.maximum()) as u8
}

fn adjusted_volume(
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

fn port_rows(base: AudioEntry, ports: &[DevicePortInfo], active: Option<&str>) -> Vec<AudioEntry> {
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

fn percent(volumes: &ChannelVolumes) -> u8 {
    let normal = f64::from(Volume::NORMAL.0);
    let average = f64::from(volumes.avg().0);
    ((average / normal) * 100.0).round().clamp(0.0, 255.0) as u8
}

fn from_percent(percent: u8) -> Volume {
    let normal = f64::from(Volume::NORMAL.0);
    Volume((normal * f64::from(percent) / 100.0).round() as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(kind: AudioKind) -> AudioEntry {
        AudioEntry {
            key: format!("{}:{}", kind.key_prefix(), hex_encode("built-in")),
            kind,
            name: "built-in".into(),
            card: None,
            description: "Built-in Audio Analog Stereo".into(),
            label: "Built-in Audio".into(),
            volume: 65,
            muted: true,
            default: true,
            port: None,
        }
    }

    fn port(name: &str, label: &str, available: PortAvailable) -> DevicePortInfo {
        DevicePortInfo {
            name: Some(name.into()),
            description: Some(label.into()),
            priority: 100,
            available,
        }
    }

    #[test]
    fn output_and_input_ports_are_distinct_rows_with_only_the_active_default_marked() {
        for (kind, labels) in [
            (AudioKind::Output, ["Speakers", "Headphones"]),
            (AudioKind::Input, ["Internal microphone", "Microphone jack"]),
        ] {
            let ports = [
                port("internal", labels[0], PortAvailable::Yes),
                port("jack", labels[1], PortAvailable::Yes),
            ];
            let rows = port_rows(device(kind), &ports, Some("jack"));
            assert_eq!(rows.len(), 2);
            assert_ne!(rows[0].key, rows[1].key);
            assert_eq!(rows[0].port.as_deref(), Some("internal"));
            assert_eq!(rows[1].port.as_deref(), Some("jack"));
            assert!(!rows[0].default);
            assert!(rows[1].default);
            for (row, label) in rows.iter().zip(labels) {
                assert_eq!(row.name, "built-in");
                assert_eq!(row.label, label);
                assert!(row.description.contains(label));
                assert_eq!(row.volume, 65);
                assert!(row.muted);
            }
            let mut other = device(kind);
            other.default = false;
            assert!(
                port_rows(other, &ports, Some("jack"))
                    .iter()
                    .all(|r| !r.default)
            );
            assert!(
                port_rows(device(kind), &ports, None)
                    .iter()
                    .all(|r| !r.default)
            );
        }
    }

    #[test]
    fn unavailable_ports_are_hidden_but_unknown_availability_is_allowed() {
        let ports = [
            port("speaker", "Speakers", PortAvailable::Unknown),
            port("jack", "Headphones", PortAvailable::No),
        ];
        let rows = port_rows(device(AudioKind::Output), &ports, Some("speaker"));
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].port.as_deref(), Some("speaker"));
        // Do not add a generic row that would bypass unavailable-port checks.
        assert!(port_rows(device(AudioKind::Output), &ports[1..], Some("jack")).is_empty());
    }

    #[test]
    fn devices_without_named_ports_keep_a_single_device_row() {
        let base = device(AudioKind::Output);
        let mut unnamed = port("", "", PortAvailable::Unknown);
        unnamed.name = None;
        for ports in [
            vec![],
            vec![unnamed],
            vec![port("", "", PortAvailable::Unknown)],
        ] {
            let rows = port_rows(base.clone(), &ports, None);
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].key, base.key);
            assert_eq!(rows[0].label, base.label);
            assert!(rows[0].port.is_none());
            assert!(rows[0].default);
        }
    }

    #[test]
    fn port_identity_uses_names_not_labels_and_survives_active_port_changes() {
        let ports = [
            port("jack:one;🎧", "Headphones", PortAvailable::Yes),
            port("jack:two", "Headphones", PortAvailable::Yes),
        ];
        let before = port_rows(device(AudioKind::Output), &ports, Some("jack:one;🎧"));
        let after = port_rows(device(AudioKind::Output), &ports, Some("jack:two"));
        assert_ne!(before[0].key, before[1].key);
        assert_eq!(before[0].key, after[0].key);
        assert_eq!(before[1].key, after[1].key);
        assert!(before[0].key.is_ascii());
        assert!(!before[0].key.contains(';'));
        let mut other = device(AudioKind::Output);
        other.key = format!("sink:{}", hex_encode("usb"));
        assert_ne!(before[0].key, port_rows(other, &ports, None)[0].key);
        assert_ne!(
            before[0].key,
            port_rows(device(AudioKind::Input), &ports, None)[0].key
        );
    }

    #[test]
    fn routing_choices_mark_only_the_streams_active_port() {
        for on_stream_device in [true, false] {
            let mut base = device(AudioKind::Output);
            base.default = on_stream_device;
            let ports = [
                port("speaker", "Speaker", PortAvailable::Unknown),
                port("jack", "Headphones", PortAvailable::Yes),
            ];
            let choices: Vec<_> = port_rows(base, &ports, Some("jack"))
                .into_iter()
                .map(crate::model::ChoiceEntry::route)
                .collect();
            assert_eq!(choices.len(), 2);
            assert_eq!(choices[0].label, "Speaker");
            assert_eq!(choices[1].label, "Headphones");
            assert!(!choices[0].active);
            assert_eq!(choices[1].active, on_stream_device);
            assert!(choices.iter().all(|choice| choice.enabled));
            assert_ne!(choices[0].key, choices[1].key);
        }
    }

    #[test]
    fn routing_choices_without_ports_keep_the_device_name() {
        let mut base = device(AudioKind::Output);
        base.label = "WH-1000XM4".into();
        let rows = port_rows(base.clone(), &[], None);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].label, "WH-1000XM4");
        assert_eq!(rows[0].key, base.key);
    }

    #[test]
    fn routing_choices_disambiguate_identical_port_names() {
        let mut usb = device(AudioKind::Output);
        usb.name = "usb".into();
        usb.key = format!("sink:{}", hex_encode(&usb.name));
        let ports = [port("speaker", "Speakers", PortAvailable::Unknown)];
        let mut rows: Vec<_> = [device(AudioKind::Output), usb]
            .into_iter()
            .flat_map(|base| port_rows(base, &ports, Some("speaker")))
            .collect();
        clarify_selection_labels(
            &mut rows,
            &HashMap::from([
                ("built-in".into(), "Built-in Audio".into()),
                ("usb".into(), "USB Audio".into()),
            ]),
        );
        assert_eq!(rows[0].label, "Speakers — Built-in Audio");
        assert_eq!(rows[1].label, "Speakers — USB Audio");
        assert!(
            rows.iter()
                .all(|row| row.port.as_deref() == Some("speaker"))
        );
        assert_ne!(rows[0].key, rows[1].key);
    }

    #[test]
    fn long_hardware_names_do_not_hide_the_port() {
        let mut base = device(AudioKind::Output);
        base.description = "An extremely long descriptive device name for a sound card".into();
        let ports = [port("jack", "Headphones", PortAvailable::Yes)];
        let rows = port_rows(base, &ports, Some("jack"));
        assert_eq!(rows[0].label, "Headphones");
        assert!(rows[0].row_label().ends_with("Headphones"));
    }

    #[test]
    fn hdmi_port_numbers_survive_without_repeated_chipset_names() {
        let mut base = device(AudioKind::Output);
        base.description = "Alder Lake PCH-P HDMI / DisplayPort".into();
        let ports = [
            port("hdmi-1", "HDMI / DisplayPort 1", PortAvailable::Yes),
            port("hdmi-2", "HDMI / DisplayPort 2", PortAvailable::Yes),
        ];
        let mut rows = port_rows(base, &ports, Some("hdmi-1"));
        clarify_selection_labels(&mut rows, &HashMap::new());
        assert_eq!(rows[0].label, "HDMI / DisplayPort 1");
        assert_eq!(rows[1].label, "HDMI / DisplayPort 2");
        assert!(rows[0].description.contains("Alder Lake PCH-P"));
        assert!(rows[0].row_label().ends_with('1'));
        assert!(rows[1].row_label().ends_with('2'));
    }

    #[test]
    fn identical_port_labels_only_add_hardware_context_when_needed() {
        for kind in [AudioKind::Output, AudioKind::Input] {
            let ports = [port("jack", "Headphones", PortAvailable::Yes)];
            let mut usb = device(kind);
            usb.name = "usb".into();
            usb.key = format!("{}:{}", kind.key_prefix(), hex_encode(&usb.name));
            let mut rows = port_rows(device(kind), &ports, Some("jack"));
            rows.extend(port_rows(usb, &ports, Some("jack")));
            let keys: Vec<_> = rows.iter().map(|e| e.key.clone()).collect();
            let names = HashMap::from([
                ("built-in".into(), "Built-in Audio".into()),
                ("usb".into(), "USB Headset".into()),
            ]);
            clarify_selection_labels(&mut rows, &names);
            assert_eq!(rows[0].label, "Headphones — Built-in Audio");
            assert_eq!(rows[1].label, "Headphones — USB Headset");
            assert_eq!(rows.iter().map(|e| e.key.clone()).collect::<Vec<_>>(), keys);
            assert!(rows.iter().all(|e| e.port.as_deref() == Some("jack")));
        }
    }

    #[test]
    fn identical_models_and_clipped_labels_still_have_distinct_rows() {
        let ports = [
            port("jack-1", "Headphones", PortAvailable::Yes),
            port("jack-2", "Headphones", PortAvailable::Yes),
        ];
        let mut rows = port_rows(device(AudioKind::Output), &ports, Some("jack-1"));
        clarify_selection_labels(&mut rows, &HashMap::new());
        assert_eq!(rows[0].label, "#1 Headphones");
        assert_eq!(rows[1].label, "#2 Headphones");
        assert_ne!(rows[0].key, rows[1].key);

        for (i, row) in rows.iter_mut().enumerate() {
            row.label = format!("{} {i}", "Long device name ".repeat(10));
        }
        clarify_selection_labels(&mut rows, &HashMap::new());
        assert_ne!(rows[0].row_label(), rows[1].row_label());
    }

    #[test]
    fn unique_bluetooth_usb_and_virtual_device_names_are_unchanged() {
        let mut rows: Vec<_> = ["WH-1000XM4", "Scarlett 2i2 USB", "Virtual Output"]
            .into_iter()
            .map(|label| AudioEntry {
                label: label.into(),
                ..device(AudioKind::Output)
            })
            .collect();
        clarify_selection_labels(&mut rows, &HashMap::new());
        assert_eq!(rows[0].label, "WH-1000XM4");
        assert_eq!(rows[1].label, "Scarlett 2i2 USB");
        assert_eq!(rows[2].label, "Virtual Output");
    }

    #[test]
    fn selecting_a_missing_or_unplugged_port_is_rejected() {
        let ports = [
            port("speaker", "Speakers", PortAvailable::Unknown),
            port("jack", "Headphones", PortAvailable::No),
        ];
        assert_eq!(available_port(&ports, "speaker").unwrap(), "speaker");
        assert!(available_port(&ports, "jack").is_err());
        assert!(available_port(&ports, "removed").is_err());
    }

    #[test]
    fn percentages_round_trip_through_pulseaudio_volume_units() {
        for level in [0_u8, 5, 33, 50, 66, 100, 125, 150] {
            let mut volumes = ChannelVolumes::default();
            volumes.set(2, from_percent(level));
            assert_eq!(percent(&volumes), level);
        }
    }

    #[test]
    fn normal_volume_is_exactly_one_hundred_percent() {
        let mut volumes = ChannelVolumes::default();
        volumes.set(2, Volume::NORMAL);
        assert_eq!(percent(&volumes), 100);
        assert_eq!(from_percent(100), Volume::NORMAL);
        assert_eq!(from_percent(0), Volume::MUTED);
    }

    #[test]
    fn output_can_amplify_but_input_stays_at_one_hundred() {
        assert_eq!(volume_target(100, AudioKind::Output, STEP), 105);
        assert_eq!(volume_target(148, AudioKind::Output, STEP), 150);
        assert_eq!(volume_target(150, AudioKind::Output, STEP), 150);
        assert_eq!(volume_target(100, AudioKind::Input, STEP), 100);
        assert_eq!(volume_target(97, AudioKind::Input, STEP), 100);
        assert_eq!(volume_target(3, AudioKind::Output, -STEP), 0);
        assert_eq!(volume_target(0, AudioKind::Input, -STEP), 0);
    }

    #[test]
    fn nudges_preserve_existing_channel_balance() {
        let mut volume = ChannelVolumes::default();
        volume.set(2, from_percent(60));
        volume.get_mut()[0] = from_percent(30);
        let adjusted = adjusted_volume(volume, AudioKind::Output, STEP).unwrap();
        assert_eq!(percent(&adjusted), 50);
        assert!((i64::from(adjusted.get()[0].0) * 2 - i64::from(adjusted.get()[1].0)).abs() <= 2);
        let limited = adjusted_volume(adjusted, AudioKind::Output, 200).unwrap();
        assert!(limited.max().0 <= from_percent(150).0);
    }

    #[test]
    fn zero_volume_can_be_raised() {
        let mut volume = ChannelVolumes::default();
        volume.set(2, Volume::MUTED);
        assert_eq!(
            percent(&adjusted_volume(volume, AudioKind::Output, STEP).unwrap()),
            5
        );
    }

    #[test]
    fn stream_keys_separate_clients_and_reused_indices() {
        let key = stream_identity(7, Some(3), "100");
        assert!(key.starts_with("playback:"));
        assert_ne!(key, stream_identity(7, Some(4), "100"));
        assert_ne!(key, stream_identity(7, Some(3), "101"));
    }
}
