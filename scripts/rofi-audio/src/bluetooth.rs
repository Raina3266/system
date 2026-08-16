use std::env;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::time::Duration;

use bluer::agent::{Agent, ReqError, ReqResult};
use bluer::{Adapter, Address, Device, ErrorKind, Session};
use futures_util::StreamExt;

use crate::AppResult;
use crate::model::{BluetoothEntry, CodeKind, hex_decode, hex_encode};

const DEFAULT_SCAN_SECONDS: u64 = 6;
/// How long the detached `connect-bg` agent waits for the user to type a
/// pairing code before giving up and letting BlueZ cancel the attempt.
const CODE_TIMEOUT: Duration = Duration::from_secs(120);
const CODE_POLL: Duration = Duration::from_millis(100);
const REQUEST_FILENAME: &str = "rofi-audio-pair-request";
const RESPONSE_FILENAME: &str = "rofi-audio-pair-response";

pub struct Backend {
    session: Session,
    adapter: Adapter,
}

impl Backend {
    pub async fn new() -> AppResult<Self> {
        let session = Session::new().await?;
        let adapter = session.default_adapter().await?;
        Ok(Self { session, adapter })
    }

    pub fn device(&self, address: &str) -> AppResult<Device> {
        Ok(self.adapter.device(address.parse::<Address>()?)?)
    }

    pub async fn is_powered(&self) -> AppResult<bool> {
        Ok(self.adapter.is_powered().await?)
    }

    /// Every action in the Bluetooth tab implies the radio should be on, so
    /// scanning or connecting powers the adapter up instead of refusing.
    pub async fn power_on(&self) -> AppResult<bool> {
        if self.adapter.is_powered().await? {
            return Ok(false);
        }
        self.adapter.set_powered(true).await?;
        Ok(true)
    }

    pub async fn set_powered(&self, powered: bool) -> AppResult<()> {
        Ok(self.adapter.set_powered(powered).await?)
    }

    pub async fn snapshot(&self) -> AppResult<Vec<BluetoothEntry>> {
        // Every property is its own D-Bus round trip, and a scan can leave
        // fifty devices behind. Reading them serially is what made the menu
        // stall after a scan; the connection multiplexes, so issuing all the
        // reads at once turns hundreds of sequential trips into a few rounds.
        let reads = self
            .adapter
            .device_addresses()
            .await?
            .into_iter()
            .filter_map(|address| {
                let device = self.adapter.device(address).ok()?;
                Some(async move { read_entry(&device, address).await })
            });
        let mut entries: Vec<_> = futures_util::future::join_all(reads)
            .await
            .into_iter()
            // A device can disappear between the address listing and the
            // property reads; skip it rather than failing the whole render.
            .flatten()
            // Discovery turns up a long tail of devices that never resolve a
            // name — beacons, cars, laptops in the next flat. Without a name
            // there is nothing to pick out of the list, so they are only kept
            // once they are paired.
            .filter(|entry| entry.named || entry.paired || entry.connected)
            .collect();
        entries.sort_by(|left, right| {
            left.rank()
                .cmp(&right.rank())
                .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
                .then_with(|| left.address.cmp(&right.address))
        });
        Ok(entries)
    }

    /// Runs a bounded discovery window. BlueZ discovers for as long as the
    /// event stream is alive, so the timeout is what stops the scan.
    pub async fn scan(&self) -> AppResult<()> {
        let events = self.adapter.discover_devices().await?;
        let seconds = env::var("ROFI_AUDIO_SCAN_SECONDS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(DEFAULT_SCAN_SECONDS);
        let _ = tokio::time::timeout(Duration::from_secs(seconds), async move {
            let mut events = std::pin::pin!(events);
            while events.next().await.is_some() {}
        })
        .await;
        Ok(())
    }

    pub async fn disconnect(&self, entry: &BluetoothEntry) -> AppResult<()> {
        Ok(self.device(&entry.address)?.disconnect().await?)
    }

    pub async fn forget(&self, entry: &BluetoothEntry) -> AppResult<()> {
        Ok(self.adapter.remove_device(entry.address.parse()?).await?)
    }

    /// Pairs when needed, then connects. Registering our own agent on this
    /// D-Bus connection makes BlueZ route this pairing's PIN and passkey
    /// requests to us instead of to the session-wide default agent.
    pub async fn pair_and_connect(&self, device: &Device) -> AppResult<()> {
        if !device.is_paired().await? {
            let _agent = self.session.register_agent(pairing_agent()).await?;
            match device.pair().await {
                Ok(()) => {}
                // BlueZ reports an already-known device as AlreadyExists; that
                // is the state we wanted, so fall through to Connect.
                Err(error) if error.kind == ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error.into()),
            }
            // Trusting lets the device reconnect on its own afterwards.
            let _ = device.set_trusted(true).await;
        }
        match device.connect().await {
            Ok(())
            | Err(bluer::Error {
                kind: ErrorKind::AlreadyConnected,
                ..
            }) => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    pub async fn name_of(&self, address: &str) -> String {
        match self.device(address) {
            Ok(device) => device_name(&device, address)
                .await
                .unwrap_or_else(|| address.to_owned()),
            Err(_) => address.to_owned(),
        }
    }

    /// Powered state plus the names of everything currently connected, for the
    /// Waybar tooltip.
    pub async fn status(&self) -> AppResult<(bool, Vec<String>)> {
        let powered = self.adapter.is_powered().await?;
        if !powered {
            return Ok((false, Vec::new()));
        }
        let checks = self
            .adapter
            .device_addresses()
            .await?
            .into_iter()
            .filter_map(|address| {
                let device = self.adapter.device(address).ok()?;
                Some(async move {
                    if device.is_connected().await.unwrap_or(false) {
                        let address = address.to_string();
                        Some(
                            device_name(&device, &address)
                                .await
                                .unwrap_or_else(|| address.clone()),
                        )
                    } else {
                        None
                    }
                })
            });
        let mut connected: Vec<_> = futures_util::future::join_all(checks)
            .await
            .into_iter()
            .flatten()
            .collect();
        connected.sort();
        Ok((powered, connected))
    }
}

async fn read_entry(device: &Device, address: Address) -> AppResult<BluetoothEntry> {
    let address = address.to_string();
    // Concurrent within one device too: six properties, one round trip's worth
    // of latency instead of six.
    let (name, icon, connected, paired, battery) = futures_util::join!(
        device_name(device, &address),
        device.icon(),
        device.is_connected(),
        device.is_paired(),
        device.battery_percentage(),
    );
    Ok(BluetoothEntry {
        key: format!("bt:{}", hex_encode(&address)),
        named: name.is_some(),
        name: name.unwrap_or_else(|| address.clone()),
        icon: icon.ok().flatten(),
        connected: connected?,
        paired: paired?,
        battery: battery.ok().flatten(),
        address,
    })
}

/// Alias first (it reflects the user-visible name and falls back to the remote
/// name), then the remote name. `None` when BlueZ has resolved neither, which
/// is how `snapshot` recognises a device worth hiding.
async fn device_name(device: &Device, address: &str) -> Option<String> {
    if let Ok(alias) = device.alias().await
        && !alias.is_empty()
        && alias != address
    {
        return Some(alias);
    }
    match device.name().await {
        Ok(Some(name)) if !name.is_empty() && name != address => Some(name),
        _ => None,
    }
}

pub fn error_message(name: &str, error: &(dyn std::error::Error + 'static)) -> String {
    if let Some(error) = error.downcast_ref::<bluer::Error>() {
        return match error.kind {
            ErrorKind::AuthenticationCanceled | ErrorKind::AuthenticationRejected => {
                format!("Pairing with {name} was rejected.")
            }
            ErrorKind::AuthenticationFailed => {
                format!("Incorrect pairing code for {name}.")
            }
            ErrorKind::AuthenticationTimeout => format!("Pairing with {name} timed out."),
            ErrorKind::ConnectionAttemptFailed => {
                format!("{name} refused the connection. Make sure it is on and in range.")
            }
            ErrorKind::DoesNotExist => format!("{name} is no longer available."),
            ErrorKind::NotReady => "The Bluetooth adapter is not ready.".to_owned(),
            _ => format!("Cannot connect to {name}: {error}"),
        };
    }
    format!("Cannot connect to {name}: {error}")
}

// ---------------------------------------------------------------------------
// Pairing agent
//
// The Rofi script is a fresh short-lived process per keystroke, so it cannot
// host the agent that BlueZ calls back into during pairing. The detached
// `connect-bg` process owns the agent instead, and the two halves talk through
// a pair of files in $XDG_RUNTIME_DIR: the agent writes a request, the script
// renders it as a prompt, and the script writes back the typed answer.
// ---------------------------------------------------------------------------

/// A prompt raised by the agent while pairing.
pub struct PairRequest {
    /// `None` for informational prompts that need no answer, such as a passkey
    /// the user has to type on the *device* rather than here.
    pub kind: Option<CodeKind>,
    pub address: String,
    pub message: String,
}

fn pairing_agent() -> Agent {
    Agent {
        // Not the default agent: this one only handles pairings that this
        // process starts, leaving incoming pairings to the session agent.
        request_default: false,
        request_pin_code: Some(Box::new(|request| {
            Box::pin(async move { ask(CodeKind::Pin, request.device.to_string()).await })
        })),
        request_passkey: Some(Box::new(|request| {
            Box::pin(async move {
                let code = ask(CodeKind::Passkey, request.device.to_string()).await?;
                code.trim().parse::<u32>().map_err(|_| ReqError::Rejected)
            })
        })),
        display_pin_code: Some(Box::new(|request| {
            Box::pin(async move {
                announce(
                    &request.device.to_string(),
                    format!("Type {} on the device to finish pairing.", request.pincode),
                );
                Ok(())
            })
        })),
        display_passkey: Some(Box::new(|request| {
            Box::pin(async move {
                announce(
                    &request.device.to_string(),
                    format!(
                        "Type {:06} on the device to finish pairing.",
                        request.passkey
                    ),
                );
                Ok(())
            })
        })),
        // Numeric comparison and plain authorization are confirmed for the
        // user: they started this pairing from the menu a moment ago.
        request_confirmation: Some(Box::new(|request| {
            Box::pin(async move {
                announce(
                    &request.device.to_string(),
                    format!("Confirming passkey {:06}…", request.passkey),
                );
                Ok(())
            })
        })),
        request_authorization: Some(Box::new(|_| Box::pin(async { Ok(()) }))),
        authorize_service: Some(Box::new(|_| Box::pin(async { Ok(()) }))),
        ..Default::default()
    }
}

/// Publishes a prompt and waits for the Rofi script to answer it.
async fn ask(kind: CodeKind, address: String) -> ReqResult<String> {
    let _ = fs::remove_file(response_path().ok_or(ReqError::Canceled)?);
    write_request(&PairRequest {
        kind: Some(kind),
        message: kind.prompt(&address),
        address,
    })
    .map_err(|_| ReqError::Canceled)?;

    let deadline = std::time::Instant::now() + CODE_TIMEOUT;
    while std::time::Instant::now() < deadline {
        if let Some(answer) = take_response() {
            return answer.ok_or(ReqError::Rejected);
        }
        tokio::time::sleep(CODE_POLL).await;
    }
    clear_request();
    Err(ReqError::Canceled)
}

/// Publishes a prompt that needs no answer; the script shows it as a message.
fn announce(address: &str, message: String) {
    let _ = write_request(&PairRequest {
        kind: None,
        address: address.to_owned(),
        message,
    });
}

fn runtime_path(filename: &str) -> Option<PathBuf> {
    Some(PathBuf::from(env::var_os("XDG_RUNTIME_DIR")?).join(filename))
}

fn request_path() -> Option<PathBuf> {
    runtime_path(REQUEST_FILENAME)
}

fn response_path() -> Option<PathBuf> {
    runtime_path(RESPONSE_FILENAME)
}

/// Temp file plus rename, so a concurrent reader never sees a partial prompt.
fn write_atomic(path: &PathBuf, contents: &str) -> io::Result<()> {
    let temporary = path.with_extension("tmp");
    fs::write(&temporary, contents)?;
    fs::rename(&temporary, path)
}

/// Wire format: one line for the kind, then the address and the human-readable
/// message hex-encoded so neither can smuggle a newline into the file.
fn encode_request(request: &PairRequest) -> String {
    format!(
        "{}\n{}\n{}\n",
        request.kind.map(CodeKind::name).unwrap_or("display"),
        hex_encode(&request.address),
        hex_encode(&request.message),
    )
}

fn decode_request(contents: &str) -> Option<PairRequest> {
    let mut lines = contents.lines();
    let (kind, address, message) = (lines.next()?, lines.next()?, lines.next()?);
    Some(PairRequest {
        // Anything that is not a code kind — "display" — is informational.
        kind: kind.parse::<CodeKind>().ok(),
        address: hex_decode(address)?,
        message: hex_decode(message)?,
    })
}

fn encode_response(code: Option<&str>) -> String {
    match code {
        Some(code) => format!("code\n{}\n", hex_encode(code)),
        None => "cancel\n\n".to_owned(),
    }
}

fn decode_response(contents: &str) -> Option<Option<String>> {
    let mut lines = contents.lines();
    match (lines.next()?, lines.next().unwrap_or_default()) {
        ("code", code) => Some(hex_decode(code)),
        _ => Some(None),
    }
}

fn write_request(request: &PairRequest) -> io::Result<()> {
    let path = request_path().ok_or_else(|| io::Error::other("XDG_RUNTIME_DIR is not set"))?;
    write_atomic(&path, &encode_request(request))
}

pub fn clear_request() {
    if let Some(path) = request_path() {
        let _ = fs::remove_file(path);
    }
}

/// Reads and consumes a pending prompt. Prompts are single-use: the script
/// keeps the resulting state in `ROFI_DATA` from there on.
pub fn take_request() -> Option<PairRequest> {
    let path = request_path()?;
    let contents = fs::read_to_string(&path).ok()?;
    let _ = fs::remove_file(&path);
    decode_request(&contents)
}

/// Answers a prompt. `None` cancels the pairing.
pub fn answer_request(code: Option<&str>) {
    let Some(path) = response_path() else {
        return;
    };
    let _ = write_atomic(&path, &encode_response(code));
}

fn take_response() -> Option<Option<String>> {
    let path = response_path()?;
    let contents = fs::read_to_string(&path).ok()?;
    let _ = fs::remove_file(&path);
    decode_response(&contents)
}

/// Drops any prompt or answer left behind by an abandoned pairing, so the next
/// launch never renders a stale password box.
pub fn cleanup() {
    clear_request();
    if let Some(path) = response_path() {
        let _ = fs::remove_file(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pair_prompts_round_trip_through_the_wire_format() {
        let encoded = encode_request(&PairRequest {
            kind: Some(CodeKind::Passkey),
            address: "AA:BB:CC:DD:EE:FF".to_owned(),
            message: "Please type in the 6-digit passkey for Café 🎧.".to_owned(),
        });
        let request = decode_request(&encoded).unwrap();
        assert_eq!(request.kind, Some(CodeKind::Passkey));
        assert_eq!(request.address, "AA:BB:CC:DD:EE:FF");
        assert_eq!(
            request.message,
            "Please type in the 6-digit passkey for Café 🎧."
        );
    }

    #[test]
    fn display_prompts_carry_no_code_kind_so_they_never_open_an_input_box() {
        let encoded = encode_request(&PairRequest {
            kind: None,
            address: "AA:BB:CC:DD:EE:FF".to_owned(),
            message: "Type 042311 on the device to finish pairing.".to_owned(),
        });
        assert!(encoded.starts_with("display\n"));
        let request = decode_request(&encoded).unwrap();
        assert_eq!(request.kind, None);
        assert_eq!(
            request.message,
            "Type 042311 on the device to finish pairing."
        );
    }

    #[test]
    fn multi_line_prompts_cannot_break_the_record_layout() {
        let encoded = encode_request(&PairRequest {
            kind: Some(CodeKind::Pin),
            address: "AA:BB:CC:DD:EE:FF".to_owned(),
            message: "First line\nSecond line".to_owned(),
        });
        assert_eq!(encoded.lines().count(), 3);
        assert_eq!(
            decode_request(&encoded).unwrap().message,
            "First line\nSecond line"
        );
    }

    #[test]
    fn answers_and_cancellations_are_distinguishable() {
        assert_eq!(
            decode_response(&encode_response(Some("042311"))),
            Some(Some("042311".to_owned()))
        );
        assert_eq!(decode_response(&encode_response(None)), Some(None));
        assert_eq!(decode_response(""), None);
    }

    #[test]
    fn truncated_prompts_are_rejected_rather_than_half_applied() {
        assert!(decode_request("passkey\n4141\n").is_none());
        assert!(decode_request("passkey\nnot-hex\n4141\n").is_none());
    }
}
