use std::ffi::OsString;
use std::fs;
use std::io::{self, Read};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::ffi::OsStringExt;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::thread;

const UPDATE_TEXT: u8 = 1;
const CLOSE: u8 = 2;
const UPDATE_IMAGE: u8 = 3;
const HEADER_SIZE: usize = 17;
const MAX_PAYLOAD_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Message {
    UpdateText { serial: u64, text: String },
    UpdateImage { serial: u64, path: PathBuf },
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
                match read_message(&mut connection) {
                    Ok(message) => {
                        let close = message == Message::Close;
                        if sender.send(message).is_err() || close {
                            break;
                        }
                    }
                    Err(error) => eprintln!("preview-panel: read live update: {error}"),
                }
            }
        })
    {
        let _ = fs::remove_file(&socket_path);
        return Err(error);
    }

    Ok((
        receiver,
        SocketGuard {
            path: socket_path,
        },
    ))
}

fn read_message(mut reader: impl Read) -> io::Result<Message> {
    let mut header = [0_u8; HEADER_SIZE];
    reader.read_exact(&mut header)?;
    let operation = header[0];
    let serial = u64::from_be_bytes(header[1..9].try_into().expect("fixed serial length"));
    let length = u64::from_be_bytes(
        header[9..17]
            .try_into()
            .expect("fixed payload length"),
    );
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
            let mut bytes = vec![0; length];
            reader.read_exact(&mut bytes)?;
            String::from_utf8(bytes)
                .map(|text| Message::UpdateText { serial, text })
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
        }
        UPDATE_IMAGE => {
            let mut bytes = vec![0; length];
            reader.read_exact(&mut bytes)?;
            Ok(Message::UpdateImage {
                serial,
                path: PathBuf::from(OsString::from_vec(bytes)),
            })
        }
        CLOSE if length == 0 => Ok(Message::Close),
        CLOSE => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "close message must have an empty payload",
        )),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unknown message operation",
        )),
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    fn frame(operation: u8, serial: u64, payload: &[u8]) -> Vec<u8> {
        let mut bytes = vec![operation];
        bytes.extend_from_slice(&serial.to_be_bytes());
        bytes.extend_from_slice(&(payload.len() as u64).to_be_bytes());
        bytes.extend_from_slice(payload);
        bytes
    }

    #[test]
    fn update_preserves_whitespace_and_unicode_exactly() {
        let text = "heading\r\n\t  first    value\n\n中文 👩🏽‍💻  \n";
        let message = read_message(Cursor::new(frame(UPDATE_TEXT, 17, text.as_bytes()))).unwrap();
        assert_eq!(
            message,
            Message::UpdateText {
                serial: 17,
                text: text.to_owned()
            }
        );
    }

    #[test]
    fn image_update_preserves_the_cached_file_path() {
        let path = "/home/raina/.local/share/rofi-clipboard/images/7.png";
        assert_eq!(
            read_message(Cursor::new(frame(UPDATE_IMAGE, 19, path.as_bytes()))).unwrap(),
            Message::UpdateImage {
                serial: 19,
                path: PathBuf::from(path),
            }
        );
    }

    #[test]
    fn close_requires_an_empty_payload() {
        assert_eq!(
            read_message(Cursor::new(frame(CLOSE, 0, &[]))).unwrap(),
            Message::Close
        );
        let error = read_message(Cursor::new(frame(CLOSE, 0, b"unexpected"))).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn rejects_unknown_operations() {
        let error = read_message(Cursor::new(frame(99, 0, &[]))).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }
}
