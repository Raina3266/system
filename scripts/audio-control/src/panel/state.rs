//! What the panel asks for, and what it gets back.
//!
//! Every reading and every action crosses this boundary, so the UI thread
//! never touches PulseAudio or BlueZ: a card that has gone away mid-scan
//! stalls the worker, not the panel.

use crate::model::{AudioEntry, BluetoothEntry, Mode, StreamEntry};

/// A pairing prompt, flattened so the UI does not need the agent's types.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Prompt {
    pub message: String,
    /// `None` for an informational prompt, such as a passkey to type on the
    /// device itself, which has no answer to send back.
    pub kind: Option<crate::model::CodeKind>,
    pub address: String,
}

/// Everything the four tabs draw, read in one pass.
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub outputs: Vec<AudioEntry>,
    pub inputs: Vec<AudioEntry>,
    pub streams: Vec<StreamEntry>,
    pub devices: Vec<BluetoothEntry>,
    pub powered: bool,
    pub scanning: bool,
    pub prompt: Option<Prompt>,
    /// Why the last action failed, shown on whichever tab is up.
    pub notice: Option<String>,
    /// Why the Bluetooth list is empty. Only the Pair tab shows it: a machine
    /// with no adapter should not put an error over its volume sliders.
    pub bluetooth_notice: Option<String>,
}

/// One thing the user did.
#[derive(Clone, Debug)]
pub enum Command {
    Refresh,
    SetDefault(AudioEntry),
    SetVolume(AudioEntry, u8),
    ToggleMute(AudioEntry),
    SetStreamVolume(StreamEntry, u8),
    ToggleStreamMute(StreamEntry),
    MoveStream(StreamEntry, String),
    SetPowered(bool),
    Scan,
    Connect(BluetoothEntry),
    Disconnect(BluetoothEntry),
    Forget(BluetoothEntry),
    AnswerPrompt(Option<String>),
}

/// Which tab is showing, and what it is looking at.
#[derive(Clone, Debug, Default)]
pub struct View {
    pub mode: Option<ModeTab>,
    /// The stream whose destination is being chosen, if the Play tab is
    /// showing its device picker rather than the stream list.
    pub routing: Option<StreamEntry>,
}

pub type ModeTab = Mode;

/// The tabs, in the order they are drawn.
pub const TABS: [Mode; 4] = [Mode::Bluetooth, Mode::Output, Mode::Input, Mode::Playback];

impl View {
    pub fn tab(&self) -> Mode {
        self.mode.unwrap_or(Mode::Output)
    }
}
