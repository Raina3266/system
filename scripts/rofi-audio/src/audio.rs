use std::io;

use libpulse_binding::volume::{ChannelVolumes, Volume};
use pulsectl::controllers::types::DeviceInfo;
use pulsectl::controllers::{DeviceControl, SinkController, SourceController};

use crate::AppResult;
use crate::model::{AudioEntry, AudioKind, hex_encode, short_device_name};

/// Volume step, in percent, for one press of the volume buttons.
pub const STEP: i16 = 5;
/// Volume ceiling. PulseAudio happily amplifies past 100%; the old wpctl
/// script capped at 1.0 and this keeps that behaviour.
pub const MAXIMUM: i16 = 100;

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

    fn set_device_volume_by_index(&mut self, index: u32, volume: &ChannelVolumes) {
        match self {
            Self::Sink(controller) => controller.set_device_volume_by_index(index, volume),
            Self::Source(controller) => controller.set_device_volume_by_index(index, volume),
        }
    }
}

pub fn snapshot(kind: AudioKind) -> AppResult<Vec<AudioEntry>> {
    let mut controller = Controller::create(kind)?;
    let default_name = controller.default_name()?;
    let mut entries: Vec<_> = controller
        .list_devices()?
        .into_iter()
        // Every sink has a matching `.monitor` source that records what it is
        // playing. Those are not microphones, so they stay out of the Input
        // tab. (For a sink, `monitor` names its own monitor source instead.)
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
    let target = (percent(&device.volume) as i16 + delta).clamp(0, MAXIMUM);
    let mut volumes = device.volume;
    volumes.set(volumes.len(), from_percent(target as u8));
    controller.set_device_volume_by_index(device.index, &volumes);
    Ok(target as u8)
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
        for level in [0_u8, 5, 33, 50, 66, 100] {
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
    fn volume_steps_stay_inside_the_zero_to_hundred_range() {
        let clamp = |current: i16, delta: i16| (current + delta).clamp(0, MAXIMUM);
        assert_eq!(clamp(100, STEP), 100);
        assert_eq!(clamp(0, -STEP), 0);
        assert_eq!(clamp(97, STEP), 100);
        assert_eq!(clamp(3, -STEP), 0);
    }
}
