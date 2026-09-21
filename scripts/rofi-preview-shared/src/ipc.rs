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

pub(crate) const UPDATE_TEXT: u8 = 1;
pub(crate) const CLOSE: u8 = 2;
pub(crate) const UPDATE_IMAGE: u8 = 3;
pub(crate) const SAVE_AND_CLOSE: u8 = 4;
pub(crate) const PANEL_STATE: u8 = 5;
pub(crate) const PREPARE_SWITCH: u8 = 6;
pub(crate) const UPDATE_NETWORK: u8 = 7;
const HEADER_SIZE: usize = 17;
const ITEM_ID_SIZE: usize = 8;
const NETWORK_PREFIX_SIZE: usize = ITEM_ID_SIZE * 2;
const MAX_PAYLOAD_BYTES: usize = 64 * 1024 * 1024;
const SAVE_RESPONSE_TIMEOUT: Duration = Duration::from_secs(5);

const SWITCH_REJECTED: u8 = 0;
const SWITCH_SAME_ITEM: u8 = 1;
pub(crate) const SWITCH_READY: u8 = 2;
const CONTENT_NONE: u8 = 0;
pub(crate) const CONTENT_TEXT: u8 = 1;
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
    UpdateNetwork {
        serial: u64,
        id: u64,
        details: String,
        png: Vec<u8>,
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
pub(crate) enum Request {
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
    UpdateNetwork {
        serial: u64,
        id: u64,
        details: String,
        png: Vec<u8>,
    },
    PrepareSwitch {
        serial: u64,
        target_id: u64,
    },
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
                "rofi-preview-shared: failed to remove socket {}: {error}",
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
        .name("rofi-preview-shared-ipc".to_owned())
        .spawn(move || {
            for connection in listener.incoming() {
                let mut connection = match connection {
                    Ok(connection) => connection,
                    Err(error) => {
                        eprintln!("rofi-preview-shared: accept live update: {error}");
                        continue;
                    }
                };
                match handle_connection(&mut connection, &sender) {
                    Ok(true) => break,
                    Ok(false) => {}
                    Err(error) => eprintln!("rofi-preview-shared: handle live update: {error}"),
                }
            }
        })
    {
        let _ = fs::remove_file(&socket_path);
        return Err(error);
    }

    Ok((receiver, SocketGuard { path: socket_path }))
}

pub(crate) fn handle_connection(
    stream: &mut UnixStream,
    sender: &Sender<Message>,
) -> io::Result<bool> {
    match read_request(&mut *stream)? {
        Request::UpdateText { serial, id, text } => {
            send_to_ui(sender, Message::UpdateText { serial, id, text })?;
            Ok(false)
        }
        Request::UpdateImage { serial, id, path } => {
            send_to_ui(sender, Message::UpdateImage { serial, id, path })?;
            Ok(false)
        }
        Request::UpdateNetwork {
            serial,
            id,
            details,
            png,
        } => {
            send_to_ui(
                sender,
                Message::UpdateNetwork {
                    serial,
                    id,
                    details,
                    png,
                },
            )?;
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
            let response = receive_panel_response(response)?;
            write_panel_state(stream, serial, &response)?;
            Ok(false)
        }
        Request::SaveAndClose => {
            let (reply, response) = mpsc::channel();
            send_to_ui(sender, Message::SaveAndClose { reply })?;
            let snapshot = receive_panel_response(response)?;
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

fn receive_panel_response<T>(response: Receiver<T>) -> io::Result<T> {
    response
        .recv_timeout(SAVE_RESPONSE_TIMEOUT)
        .map_err(|error| {
            io::Error::new(
                io::ErrorKind::TimedOut,
                format!("wait for current panel state: {error}"),
            )
        })
}

pub(crate) fn read_request(mut reader: impl Read) -> io::Result<Request> {
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
        UPDATE_NETWORK => {
            let (id, details, png) = read_network_payload(&mut reader, length)?;
            Ok(Request::UpdateNetwork {
                serial,
                id,
                details,
                png,
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

fn read_network_payload(
    mut reader: impl Read,
    length: usize,
) -> io::Result<(u64, String, Vec<u8>)> {
    if length < NETWORK_PREFIX_SIZE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "network update is missing its item ID or details length",
        ));
    }
    let mut id = [0_u8; ITEM_ID_SIZE];
    reader.read_exact(&mut id)?;
    let mut details_length = [0_u8; ITEM_ID_SIZE];
    reader.read_exact(&mut details_length)?;
    let details_length = usize::try_from(u64::from_be_bytes(details_length))
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "network details are too large"))?;
    let remaining = length - NETWORK_PREFIX_SIZE;
    if details_length > remaining {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "network details length exceeds the message payload",
        ));
    }
    let mut details = vec![0; details_length];
    reader.read_exact(&mut details)?;
    let details = String::from_utf8(details)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let mut png = vec![0; remaining - details_length];
    reader.read_exact(&mut png)?;
    Ok((u64::from_be_bytes(id), details, png))
}

pub(crate) fn write_panel_state(
    mut writer: impl Write,
    serial: u64,
    reply: &SwitchReply,
) -> io::Result<()> {
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
