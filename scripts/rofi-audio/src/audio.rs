use std::{cell::Cell, io, rc::Rc};

use libpulse_binding::context::introspect::Introspector;
use libpulse_binding::def::PortAvailable;
use libpulse_binding::operation::Operation;
use libpulse_binding::volume::{ChannelVolumes, Volume};
use pulsectl::controllers::types::{ApplicationInfo, DeviceInfo};
use pulsectl::controllers::{AppControl, DeviceControl, SinkController, SourceController};

use crate::AppResult;
use crate::model::{
    AudioEntry, AudioKind, ChoiceEntry, ChoiceList, StreamEntry, hex_encode, short_device_name,
};

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
        let handler = match self {
            Self::Sink(controller) => &mut controller.handler,
            Self::Source(controller) => &mut controller.handler,
        };
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

pub fn move_stream(entry: &StreamEntry, device_name: &str) -> AppResult<()> {
    let mut controller = Controller::create(AudioKind::Output)?;
    let app = controller.check_stream(entry)?;
    let device = controller.device_by_name(device_name)?;
    controller.change(|api, done| api.move_sink_input_by_index(app.index, device.index, Some(done)))
}

pub fn port_choices(entry: &AudioEntry) -> AppResult<ChoiceList> {
    let device = Controller::create(entry.kind)?.device_by_name(&entry.name)?;
    let active = device.active_port.as_ref().and_then(|p| p.name.as_deref());
    let mut entries: Vec<_> = device
        .ports
        .iter()
        .filter_map(|port| {
            let name = port.name.as_deref()?;
            let label = port.description.as_deref().unwrap_or(name);
            let available = port.available != PortAvailable::No;
            Some(ChoiceEntry {
                key: format!("port:{}", hex_encode(name)),
                label: format!("{label}{}", if available { "" } else { " (unplugged)" }),
                active: active == Some(name),
                enabled: available,
            })
        })
        .collect();
    entries.sort_by(|a, b| b.active.cmp(&a.active).then_with(|| a.label.cmp(&b.label)));
    Ok(ChoiceList {
        title: format!("Port for {}", entry.label),
        entries,
    })
}

pub fn set_port(entry: &AudioEntry, key: &str) -> AppResult<()> {
    let mut controller = Controller::create(entry.kind)?;
    let device = controller.device_by_name(&entry.name)?;
    let port = device
        .ports
        .iter()
        .find(|p| {
            p.name
                .as_ref()
                .is_some_and(|name| format!("port:{}", hex_encode(name)) == key)
        })
        .filter(|p| p.available != PortAvailable::No)
        .and_then(|p| p.name.as_deref())
        .ok_or_else(|| io::Error::other("The selected port is no longer available"))?;
    controller.change(|api, done| match entry.kind {
        AudioKind::Output => api.set_sink_port_by_index(device.index, port, Some(done)),
        AudioKind::Input => api.set_source_port_by_index(device.index, port, Some(done)),
    })
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
    let mut controller = Controller::create(kind)?;
    let default_name = controller.default_name()?;
    let mut entries: Vec<_> = controller
        .list_devices()?
        .into_iter()
        .filter(|device| kind == AudioKind::Output || device.monitor.is_none())
        .filter_map(|device| entry(kind, &device, default_name.as_deref()))
        .collect();
    entries.sort_by(|left, right| {
        left.description
            .to_lowercase()
            .cmp(&right.description.to_lowercase())
            .then_with(|| left.name.cmp(&right.name))
    });
    Ok(entries)
}

pub fn set_default(entry: &AudioEntry) -> AppResult<()> {
    let mut controller = Controller::create(entry.kind)?;
    if controller.set_default_device(&entry.name)? {
        return Ok(());
    }
    Err(io::Error::other("PulseAudio rejected the default device change").into())
}

/// Applies a relative volume change to one device and returns the new level.
/// The step is applied to the value read back here rather than to the rendered
/// row, so repeated presses never drift out of sync with the server.
pub fn nudge_volume(entry: &AudioEntry, delta: i16) -> AppResult<u8> {
    let mut controller = Controller::create(entry.kind)?;
    let device = controller.device_by_name(&entry.name)?;
    let volumes = adjusted_volume(device.volume, entry.kind, delta)?;
    controller.change(|api, done| match entry.kind {
        AudioKind::Output => api.set_sink_volume_by_index(device.index, &volumes, Some(done)),
        AudioKind::Input => api.set_source_volume_by_index(device.index, &volumes, Some(done)),
    })?;
    Ok(percent(&volumes))
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
        description,
    })
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
