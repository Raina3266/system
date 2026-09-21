//! Client-side lifecycle for the companion preview panel.

use std::env;
use std::ffi::OsString;
use std::fs;
use std::io::{self, Read, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::Duration;

use crate::ipc::{
    CLOSE, CONTENT_IMAGE, CONTENT_NONE, CONTENT_TEXT, HEADER_SIZE, MAX_PAYLOAD_BYTES, PANEL_STATE,
    PREPARE_SWITCH, SAVE_AND_CLOSE, SWITCH_READY, SWITCH_REJECTED, SWITCH_SAME_ITEM, UPDATE_IMAGE,
    UPDATE_TEXT,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PanelContent {
    EditableText(String),
    ReadOnlyText(String),
    Image(PathBuf),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PanelSnapshot {
    Text { id: u64, text: String },
    Image { id: u64 },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SwitchReply {
    Rejected,
    SameItem,
    Ready(Option<PanelSnapshot>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SaveResult {
    NoPanel,
    Closed(Option<PanelSnapshot>),
}

#[derive(Clone)]
pub struct PanelClient {
    socket: PathBuf,
    executable: OsString,
    environment_prefix: String,
}

impl PanelClient {
    pub fn new(
        socket_name: &str,
        executable: impl Into<OsString>,
        environment_prefix: impl Into<String>,
    ) -> io::Result<Self> {
        let runtime = env::var_os("XDG_RUNTIME_DIR")
            .ok_or_else(|| io::Error::other("XDG_RUNTIME_DIR is not set"))?;
        Ok(Self {
            socket: PathBuf::from(runtime)
                .join(format!("{socket_name}-preview-{}.sock", std::process::id())),
            executable: executable.into(),
            environment_prefix: environment_prefix.into(),
        })
    }

    pub fn is_open(&self) -> bool {
        self.socket.exists()
    }

    pub fn cleanup(&self) -> io::Result<()> {
        if let Err(error) = fs::remove_file(&self.socket)
            && error.kind() != io::ErrorKind::NotFound
        {
            return Err(error);
        }
        Ok(())
    }

    pub fn open(&self, id: u64, title: &str, content: &PanelContent) -> io::Result<()> {
        self.cleanup()?;
        let mut command = Command::new(&self.executable);
        command.args(["--stdin", "--title", title]);
        if is_read_only(content) {
            command.arg("--read-only");
        }
        command.arg("--panel");
        self.append_geometry_overrides(&mut command);
        command
            .arg("--listen")
            .arg(&self.socket)
            .stdin(Stdio::piped())
            .stdout(Stdio::null());
        let mut child = command.spawn()?;
        let mut input = child
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("rofi-preview-shared standard input is unavailable"))?;
        match content {
            PanelContent::EditableText(text) | PanelContent::ReadOnlyText(text) => {
                input.write_all(text.as_bytes())?;
            }
            PanelContent::Image(_) => {}
        }
        drop(input);

        if let Err(error) = wait_for_socket(&mut child, &self.socket) {
            self.close_silently();
            return Err(error);
        }
        if !self.update(id, 0, content)? {
            self.close_silently();
            return Err(io::Error::other(
                "rofi-preview-shared closed before displaying the selected item",
            ));
        }
        drop(child);
        Ok(())
    }

    pub fn update(&self, id: u64, serial: u64, content: &PanelContent) -> io::Result<bool> {
        let mut payload = id.to_be_bytes().to_vec();
        let operation = match content {
            PanelContent::EditableText(text) | PanelContent::ReadOnlyText(text) => {
                payload.extend_from_slice(text.as_bytes());
                UPDATE_TEXT
            }
            PanelContent::Image(path) => {
                payload.extend_from_slice(path.as_os_str().as_bytes());
                UPDATE_IMAGE
            }
        };
        self.send(operation, serial, &payload)
    }

    pub fn prepare_switch(&self, id: u64, serial: u64) -> io::Result<Option<SwitchReply>> {
        self.request(PREPARE_SWITCH, serial, &id.to_be_bytes())
    }

    pub fn save_and_close(&self) -> io::Result<SaveResult> {
        let Some(reply) = self.request(SAVE_AND_CLOSE, 0, &[])? else {
            return Ok(SaveResult::NoPanel);
        };
        wait_for_socket_removal(&self.socket)?;
        self.cleanup()?;
        match reply {
            SwitchReply::Ready(snapshot) => Ok(SaveResult::Closed(snapshot)),
            SwitchReply::Rejected | SwitchReply::SameItem => Err(io::Error::other(
                "preview panel returned an invalid save response",
            )),
        }
    }

    pub fn close(&self) -> io::Result<()> {
        if self.send(CLOSE, 0, &[])? {
            wait_for_socket_removal(&self.socket)?;
        }
        self.cleanup()
    }

    pub fn close_silently(&self) {
        let _ = self.send(CLOSE, 0, &[]);
        let _ = wait_for_socket_removal(&self.socket);
        let _ = self.cleanup();
    }

    fn append_geometry_overrides(&self, command: &mut Command) {
        for (suffix, legacy_suffix, option) in [
            ("LAUNCHER_WIDTH", Some("ROFI_WIDTH"), "--companion-width"),
            ("PREVIEW_WIDTH", None, "--width"),
            ("PREVIEW_HEIGHT", None, "--height"),
            ("PREVIEW_SIDE", None, "--side"),
            ("PREVIEW_GAP", None, "--gap"),
        ] {
            let value =
                env::var_os(format!("{}_{suffix}", self.environment_prefix)).or_else(|| {
                    legacy_suffix.and_then(|suffix| {
                        env::var_os(format!("{}_{suffix}", self.environment_prefix))
                    })
                });
            if let Some(value) = value {
                command.arg(option).arg(value);
            }
        }
    }

    fn request(
        &self,
        operation: u8,
        serial: u64,
        payload: &[u8],
    ) -> io::Result<Option<SwitchReply>> {
        let Some(mut stream) = connect(&self.socket)? else {
            return Ok(None);
        };
        stream.set_read_timeout(Some(Duration::from_secs(5)))?;
        write_frame(&mut stream, operation, serial, payload)?;
        read_switch_reply(&mut stream, serial).map(Some)
    }

    fn send(&self, operation: u8, serial: u64, payload: &[u8]) -> io::Result<bool> {
        let Some(mut stream) = connect(&self.socket)? else {
            return Ok(false);
        };
        if let Err(error) = write_frame(&mut stream, operation, serial, payload) {
            if matches!(
                error.kind(),
                io::ErrorKind::BrokenPipe
                    | io::ErrorKind::ConnectionAborted
                    | io::ErrorKind::ConnectionReset
            ) {
                return Ok(false);
            }
            return Err(error);
        }
        Ok(true)
    }
}

fn is_read_only(content: &PanelContent) -> bool {
    !matches!(content, PanelContent::EditableText(_))
}

fn connect(path: &Path) -> io::Result<Option<UnixStream>> {
    match UnixStream::connect(path) {
        Ok(stream) => Ok(Some(stream)),
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
            ) =>
        {
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

fn wait_for_socket(child: &mut Child, path: &Path) -> io::Result<()> {
    for _ in 0..100 {
        if path.exists() {
            return Ok(());
        }
        if let Some(status) = child.try_wait()? {
            return Err(io::Error::other(format!(
                "rofi-preview-shared exited before opening its socket ({status})"
            )));
        }
        thread::sleep(Duration::from_millis(10));
    }
    let _ = child.kill();
    let _ = child.wait();
    Err(io::Error::other(
        "rofi-preview-shared did not open its socket within one second",
    ))
}

fn wait_for_socket_removal(path: &Path) -> io::Result<()> {
    for _ in 0..200 {
        if !path.exists() {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(10));
    }
    Err(io::Error::other(format!(
        "rofi-preview-shared did not close socket {} within two seconds",
        path.display()
    )))
}

pub(crate) fn read_switch_reply(
    mut reader: impl Read,
    expected_serial: u64,
) -> io::Result<SwitchReply> {
    let mut header = [0_u8; HEADER_SIZE];
    reader.read_exact(&mut header)?;
    if header[0] != PANEL_STATE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "preview panel returned an unknown response",
        ));
    }
    let serial = u64::from_be_bytes(header[1..9].try_into().expect("fixed serial length"));
    if serial != expected_serial {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "preview panel returned a mismatched selection serial",
        ));
    }
    let length = u64::from_be_bytes(header[9..17].try_into().expect("fixed payload length"));
    let length = usize::try_from(length)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "response is too large"))?;
    if !(10..=MAX_PAYLOAD_BYTES).contains(&length) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "preview panel returned an invalid state response",
        ));
    }

    let mut bytes = vec![0; length];
    reader.read_exact(&mut bytes)?;
    let disposition = bytes[0];
    let kind = bytes[1];
    let id = u64::from_be_bytes(bytes[2..10].try_into().expect("fixed item ID length"));
    let content = &bytes[10..];

    match disposition {
        SWITCH_REJECTED if kind == CONTENT_NONE && id == 0 && content.is_empty() => {
            Ok(SwitchReply::Rejected)
        }
        SWITCH_SAME_ITEM if kind == CONTENT_NONE && id == 0 && content.is_empty() => {
            Ok(SwitchReply::SameItem)
        }
        SWITCH_READY => match kind {
            CONTENT_NONE if id == 0 && content.is_empty() => Ok(SwitchReply::Ready(None)),
            CONTENT_TEXT => String::from_utf8(content.to_vec())
                .map(|text| SwitchReply::Ready(Some(PanelSnapshot::Text { id, text })))
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error)),
            CONTENT_IMAGE if content.is_empty() => {
                Ok(SwitchReply::Ready(Some(PanelSnapshot::Image { id })))
            }
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "preview panel returned an invalid content state",
            )),
        },
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "preview panel returned an invalid switch response",
        )),
    }
}

pub(crate) fn write_frame(
    mut writer: impl Write,
    operation: u8,
    serial: u64,
    payload: &[u8],
) -> io::Result<()> {
    writer.write_all(&[operation])?;
    writer.write_all(&serial.to_be_bytes())?;
    writer.write_all(&(payload.len() as u64).to_be_bytes())?;
    writer.write_all(payload)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn close_and_save_frames_have_no_payload() {
        for operation in [CLOSE, SAVE_AND_CLOSE] {
            let mut frame = Vec::new();
            write_frame(&mut frame, operation, 0, &[]).unwrap();
            assert_eq!(frame.len(), HEADER_SIZE);
            assert_eq!(frame[0], operation);
            assert_eq!(u64::from_be_bytes(frame[9..17].try_into().unwrap()), 0);
        }
    }

    #[test]
    fn panel_response_preserves_item_ownership_and_complete_buffer() {
        let text = "first line\n\tsecond  line\n中文 👩🏽‍💻\n";
        let mut payload = vec![SWITCH_READY, CONTENT_TEXT];
        payload.extend_from_slice(&73_u64.to_be_bytes());
        payload.extend_from_slice(text.as_bytes());
        let mut frame = Vec::new();
        write_frame(&mut frame, PANEL_STATE, 29, &payload).unwrap();

        assert_eq!(
            read_switch_reply(frame.as_slice(), 29).unwrap(),
            SwitchReply::Ready(Some(PanelSnapshot::Text {
                id: 73,
                text: text.to_owned(),
            }))
        );
    }

    #[test]
    fn rejected_and_same_item_responses_are_distinct() {
        for (disposition, expected) in [
            (SWITCH_REJECTED, SwitchReply::Rejected),
            (SWITCH_SAME_ITEM, SwitchReply::SameItem),
        ] {
            let mut payload = vec![disposition, CONTENT_NONE];
            payload.extend_from_slice(&0_u64.to_be_bytes());
            let mut frame = Vec::new();
            write_frame(&mut frame, PANEL_STATE, 7, &payload).unwrap();
            assert_eq!(read_switch_reply(frame.as_slice(), 7).unwrap(), expected);
        }
    }

    #[test]
    fn only_editable_text_arms_the_editor() {
        assert!(!is_read_only(&PanelContent::EditableText(String::new())));
        assert!(is_read_only(&PanelContent::ReadOnlyText(String::new())));
        assert!(is_read_only(&PanelContent::Image(PathBuf::from(
            "image.png"
        ))));
    }
}
