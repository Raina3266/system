use std::ffi::OsString;
use std::fs;
use std::io::{self, Read, Write};
use std::os::unix::ffi::OsStringExt;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

const UPDATE_TEXT: u8 = 1;
const CLOSE: u8 = 2;
const UPDATE_IMAGE: u8 = 3;
const SAVE_AND_CLOSE: u8 = 4;
const PANEL_STATE: u8 = 5;
const PREPARE_SWITCH: u8 = 6;
const HEADER_SIZE: usize = 17;
const ITEM_ID_SIZE: usize = 8;
const MAX_PAYLOAD_BYTES: usize = 64 * 1024 * 1024;
const SAVE_RESPONSE_TIMEOUT: Duration = Duration::from_secs(5);

const SWITCH_REJECTED: u8 = 0;
const SWITCH_SAME_ITEM: u8 = 1;
const SWITCH_READY: u8 = 2;
const CONTENT_NONE: u8 = 0;
const CONTENT_TEXT: u8 = 1;
const CONTENT_IMAGE: u8 = 2;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ContentSnapshot {
    Text { id: u64, text: String },
    Image { id: u64 },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SwitchReply {
    Rejected,
    SameItem,
    Ready(Option<ContentSnapshot>),
}

#[derive(Debug)]
pub enum Message {
    UpdateText {
        serial: u64,
        id: u64,
        text: String,
    },
    UpdateImage {
        serial: u64,
        id: u64,
        path: PathBuf,
    },
    PrepareSwitch {
        serial: u64,
        target_id: u64,
        reply: Sender<SwitchReply>,
    },
    SaveAndClose {
        reply: Sender<Option<ContentSnapshot>>,
    },
    Close,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum Request {
    UpdateText { serial: u64, id: u64, text: String },
    UpdateImage { serial: u64, id: u64, path: PathBuf },
    PrepareSwitch { serial: u64, target_id: u64 },
    SaveAndClose,
    Close,
}

pub struct SocketGuard {
    path: PathBuf,
}

impl Drop for SocketGuard {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_file(&self.path)
            && error.kind() != io::ErrorKind::NotFound
        {
            eprintln!(
                "preview-panel: failed to remove socket {}: {error}",
                self.path.display()
            );
        }
    }
}

pub fn bind(path: &Path) -> io::Result<(Receiver<Message>, SocketGuard)> {
    let listener = UnixListener::bind(path)?;
    if let Err(error) = fs::set_permissions(path, fs::Permissions::from_mode(0o600)) {
        let _ = fs::remove_file(path);
        return Err(error);
    }
    let (sender, receiver) = mpsc::channel();
    let socket_path = path.to_path_buf();

    if let Err(error) = thread::Builder::new()
        .name("preview-panel-ipc".to_owned())
        .spawn(move || {
            for connection in listener.incoming() {
                let mut connection = match connection {
                    Ok(connection) => connection,
                    Err(error) => {
                        eprintln!("preview-panel: accept live update: {error}");
                        continue;
                    }
                };
                match handle_connection(&mut connection, &sender) {
                    Ok(true) => break,
                    Ok(false) => {}
                    Err(error) => eprintln!("preview-panel: handle live update: {error}"),
                }
            }
        })
    {
        let _ = fs::remove_file(&socket_path);
        return Err(error);
    }

    Ok((receiver, SocketGuard { path: socket_path }))
}

fn handle_connection(stream: &mut UnixStream, sender: &Sender<Message>) -> io::Result<bool> {
    match read_request(&mut *stream)? {
        Request::UpdateText { serial, id, text } => {
            send_to_ui(sender, Message::UpdateText { serial, id, text })?;
            Ok(false)
        }
        Request::UpdateImage { serial, id, path } => {
            send_to_ui(sender, Message::UpdateImage { serial, id, path })?;
            Ok(false)
        }
        Request::PrepareSwitch { serial, target_id } => {
            let (reply, response) = mpsc::channel();
            send_to_ui(
                sender,
                Message::PrepareSwitch {
                    serial,
                    target_id,
                    reply,
                },
            )?;
            let response = response
                .recv_timeout(SAVE_RESPONSE_TIMEOUT)
                .map_err(|error| {
                    io::Error::new(
                        io::ErrorKind::TimedOut,
                        format!("wait for current panel state: {error}"),
                    )
                })?;
            write_panel_state(stream, serial, &response)?;
            Ok(false)
        }
        Request::SaveAndClose => {
            let (reply, response) = mpsc::channel();
            send_to_ui(sender, Message::SaveAndClose { reply })?;
            let snapshot = response
                .recv_timeout(SAVE_RESPONSE_TIMEOUT)
                .map_err(|error| {
                    io::Error::new(
                        io::ErrorKind::TimedOut,
                        format!("wait for current panel state: {error}"),
                    )
                })?;
            write_panel_state(stream, 0, &SwitchReply::Ready(snapshot))?;
            Ok(true)
        }
        Request::Close => {
            let _ = sender.send(Message::Close);
            Ok(true)
        }
    }
}

fn send_to_ui(sender: &Sender<Message>, message: Message) -> io::Result<()> {
    sender
        .send(message)
        .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "preview window has closed"))
}

fn read_request(mut reader: impl Read) -> io::Result<Request> {
    let mut header = [0_u8; HEADER_SIZE];
    reader.read_exact(&mut header)?;
    let operation = header[0];
    let serial = u64::from_be_bytes(header[1..9].try_into().expect("fixed serial length"));
    let length = u64::from_be_bytes(header[9..17].try_into().expect("fixed payload length"));
    let length = usize::try_from(length)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "message is too large"))?;
    if length > MAX_PAYLOAD_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "message is too large",
        ));
    }

    match operation {
        UPDATE_TEXT => {
            let (id, bytes) = read_item_payload(&mut reader, length)?;
            String::from_utf8(bytes)
                .map(|text| Request::UpdateText { serial, id, text })
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
        }
        UPDATE_IMAGE => {
            let (id, bytes) = read_item_payload(&mut reader, length)?;
            Ok(Request::UpdateImage {
                serial,
                id,
                path: PathBuf::from(OsString::from_vec(bytes)),
            })
        }
        PREPARE_SWITCH if length == ITEM_ID_SIZE => {
            let mut bytes = [0_u8; ITEM_ID_SIZE];
            reader.read_exact(&mut bytes)?;
            Ok(Request::PrepareSwitch {
                serial,
                target_id: u64::from_be_bytes(bytes),
            })
        }
        PREPARE_SWITCH => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "prepare-switch message must contain one item ID",
        )),
        CLOSE if length == 0 => Ok(Request::Close),
        CLOSE => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "close message must have an empty payload",
        )),
        SAVE_AND_CLOSE if length == 0 => Ok(Request::SaveAndClose),
        SAVE_AND_CLOSE => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "save-and-close message must have an empty payload",
        )),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unknown message operation",
        )),
    }
}

fn read_item_payload(mut reader: impl Read, length: usize) -> io::Result<(u64, Vec<u8>)> {
    if length < ITEM_ID_SIZE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "item update is missing its clipboard item ID",
        ));
    }
    let mut id = [0_u8; ITEM_ID_SIZE];
    reader.read_exact(&mut id)?;
    let mut bytes = vec![0; length - ITEM_ID_SIZE];
    reader.read_exact(&mut bytes)?;
    Ok((u64::from_be_bytes(id), bytes))
}

fn write_panel_state(mut writer: impl Write, serial: u64, reply: &SwitchReply) -> io::Result<()> {
    let mut payload = Vec::new();
    match reply {
        SwitchReply::Rejected => {
            payload.extend_from_slice(&[SWITCH_REJECTED, CONTENT_NONE]);
            payload.extend_from_slice(&0_u64.to_be_bytes());
        }
        SwitchReply::SameItem => {
            payload.extend_from_slice(&[SWITCH_SAME_ITEM, CONTENT_NONE]);
            payload.extend_from_slice(&0_u64.to_be_bytes());
        }
        SwitchReply::Ready(snapshot) => {
            payload.push(SWITCH_READY);
            match snapshot {
                None => {
                    payload.push(CONTENT_NONE);
                    payload.extend_from_slice(&0_u64.to_be_bytes());
                }
                Some(ContentSnapshot::Text { id, text }) => {
                    payload.push(CONTENT_TEXT);
                    payload.extend_from_slice(&id.to_be_bytes());
                    payload.extend_from_slice(text.as_bytes());
                }
                Some(ContentSnapshot::Image { id }) => {
                    payload.push(CONTENT_IMAGE);
                    payload.extend_from_slice(&id.to_be_bytes());
                }
            }
        }
    }

    writer.write_all(&[PANEL_STATE])?;
    writer.write_all(&serial.to_be_bytes())?;
    writer.write_all(&(payload.len() as u64).to_be_bytes())?;
    writer.write_all(&payload)
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;
    use std::os::unix::net::UnixStream;

    use super::*;

    fn frame(operation: u8, serial: u64, payload: &[u8]) -> Vec<u8> {
        let mut bytes = vec![operation];
        bytes.extend_from_slice(&serial.to_be_bytes());
        bytes.extend_from_slice(&(payload.len() as u64).to_be_bytes());
        bytes.extend_from_slice(payload);
        bytes
    }

    fn item_frame(operation: u8, serial: u64, id: u64, payload: &[u8]) -> Vec<u8> {
        let mut item = id.to_be_bytes().to_vec();
        item.extend_from_slice(payload);
        frame(operation, serial, &item)
    }

    #[test]
    fn update_preserves_whitespace_and_unicode_exactly() {
        let text = "heading\r\n\t  first    value\n\n中文 👩🏽‍💻  \n";
        let message = read_request(Cursor::new(item_frame(
            UPDATE_TEXT,
            17,
            42,
            text.as_bytes(),
        )))
        .unwrap();
        assert_eq!(
            message,
            Request::UpdateText {
                serial: 17,
                id: 42,
                text: text.to_owned()
            }
        );
    }

    #[test]
    fn image_update_preserves_the_cached_file_path() {
        let path = "/home/raina/.local/share/rofi-clipboard/images/7.png";
        assert_eq!(
            read_request(Cursor::new(item_frame(
                UPDATE_IMAGE,
                19,
                7,
                path.as_bytes()
            )))
            .unwrap(),
            Request::UpdateImage {
                serial: 19,
                id: 7,
                path: PathBuf::from(path),
            }
        );
    }

    #[test]
    fn prepare_switch_preserves_target_id_and_serial() {
        assert_eq!(
            read_request(Cursor::new(frame(
                PREPARE_SWITCH,
                23,
                &91_u64.to_be_bytes()
            )))
            .unwrap(),
            Request::PrepareSwitch {
                serial: 23,
                target_id: 91,
            }
        );
    }

    #[test]
    fn close_requires_an_empty_payload() {
        assert_eq!(
            read_request(Cursor::new(frame(CLOSE, 0, &[]))).unwrap(),
            Request::Close
        );
        let error = read_request(Cursor::new(frame(CLOSE, 0, b"unexpected"))).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn save_and_close_requires_an_empty_payload() {
        assert_eq!(
            read_request(Cursor::new(frame(SAVE_AND_CLOSE, 0, &[]))).unwrap(),
            Request::SaveAndClose
        );
        let error = read_request(Cursor::new(frame(SAVE_AND_CLOSE, 0, b"unexpected"))).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn panel_state_response_preserves_item_id_and_complete_buffer() {
        let text = "first line\n\tsecond  line\n中文 👩🏽‍💻\n";
        let mut response = Vec::new();
        write_panel_state(
            &mut response,
            31,
            &SwitchReply::Ready(Some(ContentSnapshot::Text {
                id: 77,
                text: text.to_owned(),
            })),
        )
        .unwrap();

        assert_eq!(response[0], PANEL_STATE);
        assert_eq!(u64::from_be_bytes(response[1..9].try_into().unwrap()), 31);
        assert_eq!(
            u64::from_be_bytes(response[9..17].try_into().unwrap()),
            (10 + text.len()) as u64
        );
        assert_eq!(response[17], SWITCH_READY);
        assert_eq!(response[18], CONTENT_TEXT);
        assert_eq!(u64::from_be_bytes(response[19..27].try_into().unwrap()), 77);
        assert_eq!(&response[27..], text.as_bytes());
    }

    #[test]
    fn save_request_round_trip_returns_the_ui_buffer() {
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let (sender, receiver) = mpsc::channel();
        let server = std::thread::spawn(move || handle_connection(&mut server, &sender).unwrap());
        client.write_all(&frame(SAVE_AND_CLOSE, 0, &[])).unwrap();

        let edited = "first line\n\tsecond  line\n中文 👩🏽‍💻\n";
        match receiver.recv_timeout(Duration::from_secs(1)).unwrap() {
            Message::SaveAndClose { reply } => reply
                .send(Some(ContentSnapshot::Text {
                    id: 55,
                    text: edited.to_owned(),
                }))
                .unwrap(),
            message => panic!("expected save request, got {message:?}"),
        }

        let mut response = Vec::new();
        client.read_to_end(&mut response).unwrap();
        assert!(server.join().unwrap());
        assert_eq!(response[0], PANEL_STATE);
        assert_eq!(response[17], SWITCH_READY);
        assert_eq!(response[18], CONTENT_TEXT);
        assert_eq!(u64::from_be_bytes(response[19..27].try_into().unwrap()), 55);
        assert_eq!(&response[27..], edited.as_bytes());
    }

    #[test]
    fn rejects_unknown_operations() {
        let error = read_request(Cursor::new(frame(99, 0, &[]))).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }
}
