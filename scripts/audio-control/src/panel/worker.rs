//! The thread that talks to PulseAudio and BlueZ.
//!
//! Both are blocking from the panel's point of view — PulseAudio genuinely so,
//! BlueZ because its futures need a runtime — and a device that has gone away
//! can take seconds to time out. Keeping all of it here means the panel keeps
//! drawing while that happens.

use std::sync::mpsc::{Receiver, Sender};
use std::thread;

use tokio::runtime::Runtime;

use super::state::{Command, Prompt, Snapshot};
use crate::model::{AudioKind, Mode};
use crate::{audio, bluetooth};

pub fn spawn(commands: Receiver<Command>, snapshots: Sender<Snapshot>) {
    thread::spawn(move || {
        let Ok(runtime) = Runtime::new() else {
            return;
        };
        while let Ok(command) = commands.recv() {
            let notice = runtime.block_on(apply(command));
            if snapshots.send(runtime.block_on(read(notice))).is_err() {
                return;
            }
        }
    });
}

/// Carries out one action, returning anything worth telling the user.
async fn apply(command: Command) -> Option<String> {
    match command {
        Command::Refresh => None,
        Command::SetDefault(entry) => {
            failure("Could not switch device", audio::set_default(&entry))
        }
        Command::SetVolume(entry, target) => {
            // The domain layer nudges rather than sets, so that the step is
            // applied to the level read back from the server rather than to
            // the one this panel last drew.
            let delta = i16::from(target) - i16::from(entry.volume);
            failure(
                "Could not change volume",
                audio::nudge_volume(&entry, delta).map(|_| ()),
            )
        }
        Command::ToggleMute(entry) => failure("Could not mute", audio::toggle_mute(&entry)),
        Command::SetStreamVolume(entry, target) => {
            let current = entry.volume.unwrap_or(0);
            let delta = i16::from(target) - i16::from(current);
            failure(
                "Could not change volume",
                audio::nudge_stream_volume(&entry, delta),
            )
        }
        Command::ToggleStreamMute(entry) => {
            failure("Could not mute", audio::toggle_stream_mute(&entry))
        }
        Command::MoveStream(entry, destination) => failure(
            "Could not move the stream",
            audio::move_stream(&entry, &destination),
        ),
        Command::SetPowered(on) => {
            backend_action(|backend| async move { backend.set_powered(on).await }).await
        }
        Command::Scan => {
            backend_action(|backend| async move {
                backend.power_on().await?;
                backend.scan().await
            })
            .await
        }
        Command::Connect(entry) => {
            backend_action(|backend| async move {
                let device = backend.device(&entry.address)?;
                backend.pair_and_connect(&device).await
            })
            .await
        }
        Command::Disconnect(entry) => {
            backend_action(|backend| async move { backend.disconnect(&entry).await }).await
        }
        Command::Forget(entry) => {
            backend_action(|backend| async move { backend.forget(&entry).await }).await
        }
        Command::AnswerPrompt(answer) => {
            bluetooth::answer_request(answer.as_deref());
            None
        }
    }
}

async fn backend_action<F, Fut>(action: F) -> Option<String>
where
    F: FnOnce(bluetooth::Backend) -> Fut,
    Fut: Future<Output = crate::AppResult<()>>,
{
    match bluetooth::Backend::new().await {
        Ok(backend) => failure("Bluetooth", action(backend).await),
        Err(error) => Some(format!("Bluetooth unavailable: {error}")),
    }
}

fn failure(context: &str, result: crate::AppResult<()>) -> Option<String> {
    result.err().map(|error| format!("{context}: {error}"))
}

async fn read(notice: Option<String>) -> Snapshot {
    let (outputs, inputs) = (
        audio::selections(AudioKind::Output).unwrap_or_default(),
        audio::selections(AudioKind::Input).unwrap_or_default(),
    );
    let streams = audio::streams().unwrap_or_default();

    // A scan window that has run out should stop the Scan button pulsing even
    // if nothing else changed.
    bluetooth::expire_scanning();
    let (powered, devices, bluetooth_notice) = match bluetooth::Backend::new().await {
        Ok(backend) => {
            let powered = backend.is_powered().await.unwrap_or(false);
            match backend.snapshot().await {
                Ok(devices) => (powered, devices, None),
                Err(error) => (powered, Vec::new(), Some(format!("Bluetooth: {error}"))),
            }
        }
        Err(error) => (
            false,
            Vec::new(),
            Some(format!("Bluetooth unavailable: {error}")),
        ),
    };

    Snapshot {
        outputs,
        inputs,
        streams,
        devices,
        powered,
        scanning: bluetooth::is_scanning(),
        prompt: bluetooth::take_request().map(|request| Prompt {
            message: request.message,
            kind: request.kind,
            address: request.address,
        }),
        notice,
        bluetooth_notice,
    }
}

/// The devices a tab lists.
pub fn entries(snapshot: &Snapshot, mode: Mode) -> &[crate::model::AudioEntry] {
    match mode {
        Mode::Input => &snapshot.inputs,
        _ => &snapshot.outputs,
    }
}
