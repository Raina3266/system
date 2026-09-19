//! HID++ battery bridge: kernel power supplies -> BlueZ `Battery1`.
//!
//! Some Logitech devices (the MX Master 3 among them) answer the standard
//! GATT Battery Service with a permanent 0%. The real level only travels over
//! Logitech's proprietary HID++ protocol, which the kernel hid-logitech-hidpp
//! driver already decodes into `/sys/class/power_supply/hidpp_battery_*`.
//! BlueZ never learns that value, so everything reading `org.bluez.Battery1`
//! (Wayle, GNOME, UPower) shows the flat lie.
//!
//! This module republishes those levels through BlueZ's Battery Provider
//! API. Provider batteries and the GATT one compete for the device's
//! `Battery1` on a first-come basis — BlueZ ignores a provider battery when
//! one is already registered for the device path — and the GATT stub always
//! arrives first on connect. So provider objects for paired Logitech HID
//! devices are registered *before* any connection and never voluntarily
//! given up: a device without a kernel supply holds a placeholder object
//! (no `Percentage`) that claims the slot, and the object is swapped for one
//! carrying the real level the moment a supply appears.
//!
//! Losing the race anyway — a fresh pairing connects before its device is
//! even known, or the provider starts while a device is connected — is not
//! permanent: bluetoothd never re-processes an object it has already seen,
//! but it does unregister the GATT stub's battery on every disconnect, so
//! each cycle re-exports batteries the daemon is not surfacing until one
//! lands in a freed slot. From then on the stub is the one being ignored.
//!
//! The BlueZ D-Bus policy only lets root register a provider, so this
//! subcommand is driven by the system service `bt-battery-provider`, never
//! by the user session.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::Path;
use std::time::Duration;

use zbus::fdo::{DBusProxy, ObjectManager, ObjectManagerProxy};
use zbus::names::{BusName, OwnedInterfaceName};
use zbus::zvariant::{ObjectPath, OwnedObjectPath, OwnedValue};
use zbus::{Connection, Proxy, interface};

use crate::AppResult;

/// HID++ levels move at the speed of discharge; a slow poll keeps D-Bus
/// chatter near zero while still catching a reconnect within half a minute.
const POLL_INTERVAL: Duration = Duration::from_secs(30);
/// While the daemon is not surfacing our battery, retry fast enough to
/// claim the device's slot inside a disconnect window only seconds long.
const RETRY_INTERVAL: Duration = Duration::from_secs(2);
/// The kernel driver names its supplies `hidpp_battery_<n>`.
const POWER_SUPPLIES: &str = "/sys/class/power_supply";
/// Provider root on the system bus; battery objects live beneath it. D-Bus
/// object paths allow only `[A-Za-z0-9_]` per segment, so no hyphens here.
const PROVIDER_ROOT: &str = "/bt_battery/hidpp";
/// Reported as `Battery1.Source` so the origin is diagnosable at a glance.
const SOURCE: &str = "HID++";
/// The name bluetoothd owns; watched so a daemon restart gets re-registered.
const BLUEZ: &str = "org.bluez";
/// Device1.Modialias prefix for Logitech, the only vendor of HID++ devices.
const LOGITECH_MODALIAS: &str = "usb:v046D";
/// The Human Interface Device service UUID; separates mice and keyboards
/// from Logitech audio devices, which HID++ cannot serve.
const HID_SERVICE_UUID: &str = "00001812-0000-1000-8000-00805f9b34fb";

/// One kernel HID++ power supply. The hidpp driver reports the peer's
/// Bluetooth address as the serial, which ties the supply to a BlueZ device.
struct HidppSupply {
    /// Normalized peer address.
    address: String,
    /// Charge percentage, when the driver could establish one.
    percentage: Option<u8>,
}

/// A paired Logitech HID device known to BlueZ, connected or not.
struct BluezDevice {
    /// Normalized device address.
    address: String,
    /// The device's own object path, reported through `Device`.
    path: OwnedObjectPath,
}

/// Provider battery carrying a real level.
struct HidppBattery {
    device: OwnedObjectPath,
    percentage: u8,
}

#[interface(name = "org.bluez.BatteryProvider1")]
impl HidppBattery {
    /// The BlueZ device this battery belongs to.
    #[zbus(property)]
    fn device(&self) -> OwnedObjectPath {
        self.device.clone()
    }

    /// Charge percentage (0-100).
    #[zbus(property)]
    fn percentage(&self) -> u8 {
        self.percentage
    }

    /// Where this report comes from.
    #[zbus(property)]
    fn source(&self) -> String {
        SOURCE.to_owned()
    }
}

/// Placeholder claiming a device's battery slot while no kernel supply
/// exists. Carries no `Percentage` — BlueZ explicitly tolerates its absence —
/// so the GATT stub cannot register, yet nothing false is displayed either.
struct HidppBatteryPending {
    device: OwnedObjectPath,
}

#[interface(name = "org.bluez.BatteryProvider1")]
impl HidppBatteryPending {
    /// The BlueZ device this battery belongs to.
    #[zbus(property)]
    fn device(&self) -> OwnedObjectPath {
        self.device.clone()
    }

    /// Where this report comes from.
    #[zbus(property)]
    fn source(&self) -> String {
        SOURCE.to_owned()
    }
}

/// Long-running provider loop; see the module docs. Never returns on its own.
pub async fn run() -> AppResult<()> {
    let connection = Connection::system().await?;
    // bluetoothd walks the provider root through its object manager, so one
    // has to be served there before any battery objects appear.
    let root = ObjectPath::try_from(PROVIDER_ROOT)?;
    connection.object_server().at(root, ObjectManager).await?;
    // Normalized address -> (device path, exported percentage).
    let mut exported: HashMap<String, (OwnedObjectPath, Option<u8>)> = HashMap::new();
    // Unique name bluetoothd ran under when we last registered. A different
    // owner means the daemon restarted and forgot the registration.
    let mut registered_for: Option<String> = None;
    loop {
        // A failed cycle (bluetoothd mid-restart, sysfs hiccup) is logged and
        // retried; only setup errors before the loop are fatal. With every
        // battery surfaced the slow poll is enough; otherwise retry fast
        // enough to catch a short disconnect window.
        let surfaced = match cycle(&connection, &mut exported, &mut registered_for).await {
            Ok(surfaced) => surfaced,
            Err(error) => {
                eprintln!("audio-control: battery provider cycle failed: {error}");
                false
            }
        };
        tokio::time::sleep(if surfaced {
            POLL_INTERVAL
        } else {
            RETRY_INTERVAL
        })
        .await;
    }
}

async fn cycle(
    connection: &Connection,
    exported: &mut HashMap<String, (OwnedObjectPath, Option<u8>)>,
    registered_for: &mut Option<String>,
) -> AppResult<bool> {
    let Some(owner) = bluez_owner(connection).await? else {
        // bluetoothd is down; our objects stay exported and the next owner
        // gets a fresh registration. Nothing to retry against until then.
        *registered_for = None;
        return Ok(true);
    };
    if registered_for.as_ref() != Some(&owner) {
        register_providers(connection).await?;
        *registered_for = Some(owner);
    }
    let supplies = discover_supplies()?;
    let devices = bluez_devices(connection).await?;
    reconcile(connection, exported, &supplies, &devices).await
}

/// Current unique owner of org.bluez, or `None` while bluetoothd is down.
async fn bluez_owner(connection: &Connection) -> AppResult<Option<String>> {
    let dbus = DBusProxy::new(connection).await?;
    let bluez = BusName::try_from(BLUEZ)?;
    Ok(dbus
        .get_name_owner(bluez)
        .await
        .ok()
        .map(|owner| owner.to_string()))
}

/// Register the provider root with every adapter. BlueZ serves
/// `BatteryProviderManager1` on the adapter paths only, never on the daemon
/// root.
async fn register_providers(connection: &Connection) -> AppResult<()> {
    let mut registered = false;
    // The API takes an object path, not a string; a string argument makes
    // GDBus answer UnknownMethod as if the method did not exist.
    let provider_root = ObjectPath::try_from(PROVIDER_ROOT)?;
    for adapter in bluez_adapters(connection).await? {
        let path = adapter.to_string();
        let proxy = Proxy::new(
            connection,
            BLUEZ,
            path.as_str(),
            "org.bluez.BatteryProviderManager1",
        )
        .await?;
        match proxy
            .call::<_, _, ()>("RegisterBatteryProvider", &provider_root)
            .await
        {
            Ok(()) => registered = true,
            Err(error) => eprintln!("audio-control: registering with {path} failed: {error}"),
        }
    }
    if !registered {
        return Err("no adapter accepted the battery provider registration".into());
    }
    Ok(())
}

/// ObjectManager snapshot of everything bluetoothd exports.
async fn managed_objects(
    connection: &Connection,
) -> AppResult<HashMap<OwnedObjectPath, HashMap<OwnedInterfaceName, HashMap<String, OwnedValue>>>> {
    let manager = ObjectManagerProxy::new(connection, BLUEZ, "/").await?;
    Ok(manager.get_managed_objects().await?)
}

/// The property set of one interface, looked up by name.
fn interface_properties<'a>(
    interfaces: &'a HashMap<OwnedInterfaceName, HashMap<String, OwnedValue>>,
    name: &str,
) -> Option<&'a HashMap<String, OwnedValue>> {
    interfaces
        .iter()
        .find(|(interface, _)| interface.as_str() == name)
        .map(|(_, properties)| properties)
}

/// Every paired Logitech HID device bluetoothd knows, connected or not.
async fn bluez_devices(connection: &Connection) -> AppResult<Vec<BluezDevice>> {
    let mut devices = Vec::new();
    for (path, interfaces) in managed_objects(connection).await? {
        let Some(device) = interface_properties(&interfaces, "org.bluez.Device1") else {
            continue;
        };
        let Some(address) = property_string(device, "Address")
            .as_deref()
            .and_then(normalize_address)
        else {
            continue;
        };
        // Unpaired devices are temporary and would be rejected by bluetoothd;
        // non-HID Logitech devices (audio) have batteries HID++ cannot serve.
        if !property_bool(device, "Paired").unwrap_or(false) {
            continue;
        }
        let modalias = property_string(device, "Modalias");
        let uuids = property_strings(device, "UUIDs").unwrap_or_default();
        if !is_logitech_hid(modalias.as_deref(), &uuids) {
            continue;
        }
        devices.push(BluezDevice { address, path });
    }
    Ok(devices)
}

/// Object paths of every adapter.
async fn bluez_adapters(connection: &Connection) -> AppResult<Vec<OwnedObjectPath>> {
    Ok(managed_objects(connection)
        .await?
        .into_iter()
        .filter(|(_, interfaces)| interface_properties(interfaces, "org.bluez.Adapter1").is_some())
        .map(|(path, _)| path)
        .collect())
}

fn property_string(interface: &HashMap<String, OwnedValue>, name: &str) -> Option<String> {
    String::try_from(interface.get(name)?.clone()).ok()
}

fn property_bool(interface: &HashMap<String, OwnedValue>, name: &str) -> Option<bool> {
    bool::try_from(interface.get(name)?.clone()).ok()
}

fn property_strings(interface: &HashMap<String, OwnedValue>, name: &str) -> Option<Vec<String>> {
    Vec::<String>::try_from(interface.get(name)?.clone()).ok()
}

/// A Logitech device that speaks HID-over-Bluetooth; the modalias vendor
/// prefix and the HID service UUID together keep audio devices out.
fn is_logitech_hid(modalias: Option<&str>, uuids: &[String]) -> bool {
    modalias.is_some_and(|alias| alias.starts_with(LOGITECH_MODALIAS))
        && uuids.iter().any(|uuid| uuid == HID_SERVICE_UUID)
}

/// Every HID++ power supply currently present, straight from sysfs.
fn discover_supplies() -> io::Result<Vec<HidppSupply>> {
    let mut supplies = Vec::new();
    for entry in fs::read_dir(POWER_SUPPLIES)?.flatten() {
        let file_name = entry.file_name();
        let Some(name) = file_name.to_str() else {
            continue;
        };
        if !name.starts_with("hidpp_battery_") {
            continue;
        }
        let dir = entry.path();
        // Receiver-attached devices report model serials rather than
        // addresses; they cannot match a BlueZ device and drop out here.
        let Some(address) = read_trimmed(&dir.join("serial_number"))
            .as_deref()
            .and_then(normalize_address)
        else {
            continue;
        };
        supplies.push(HidppSupply {
            address,
            percentage: supply_percentage(&dir),
        });
    }
    Ok(supplies)
}

fn read_trimmed(path: &Path) -> Option<String> {
    fs::read_to_string(path)
        .ok()
        .map(|text| text.trim().to_owned())
}

/// Percentage for one supply: the driver's exact number when present, else a
/// coarse mapping of the level word. Unknown levels stay `None` so the caller
/// keeps a percentage-less placeholder rather than guessing a number.
fn supply_percentage(dir: &Path) -> Option<u8> {
    if let Some(capacity) = read_trimmed(&dir.join("capacity")) {
        if let Ok(value) = capacity.parse::<u8>() {
            return Some(value.min(100));
        }
    }
    let level =
        read_trimmed(&dir.join("capacity_level")).or_else(|| read_trimmed(&dir.join("status")))?;
    level_percentage(&level)
}

/// Coarse HID++ level words -> percentages. The numbers are judgements, not
/// measurements; they only exist because `Battery1` wants a byte.
fn level_percentage(level: &str) -> Option<u8> {
    match level.trim().to_ascii_lowercase().as_str() {
        "full" => Some(100),
        "high" => Some(80),
        "normal" => Some(50),
        "low" => Some(20),
        "critical" => Some(5),
        _ => None,
    }
}

/// Collapse a serial that looks like a Bluetooth address to bare uppercase
/// hex. Receiver-attached devices report model serials instead of addresses,
/// so they can never match a BlueZ device and are naturally ignored.
fn normalize_address(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    let mut hex = String::with_capacity(12);
    for character in raw.chars() {
        match character {
            '0'..='9' | 'a'..='f' | 'A'..='F' => hex.push(character.to_ascii_uppercase()),
            ':' | '-' => {}
            _ => return None,
        }
    }
    (hex.len() == 12).then_some(hex)
}

/// Bring exported battery objects in line with the paired Logitech HID
/// devices bluetoothd knows, and report whether the daemon is currently
/// surfacing every one of them. Objects are kept for disconnected devices
/// too — dropping one would hand the `Battery1` slot to the lying GATT stub
/// until the next full cycle. A changed percentage — or a battery bluetoothd
/// is not surfacing, since it never re-processes an object it has already
/// seen — swaps the object, which the daemon observes through its
/// object-manager view of the provider root; no reliance on property-change
/// signal plumbing.
async fn reconcile(
    connection: &Connection,
    exported: &mut HashMap<String, (OwnedObjectPath, Option<u8>)>,
    supplies: &[HidppSupply],
    devices: &[BluezDevice],
) -> AppResult<bool> {
    let mut desired: HashMap<String, (OwnedObjectPath, Option<u8>)> = HashMap::new();
    for device in devices {
        let percentage = supplies
            .iter()
            .find(|supply| supply.address == device.address)
            .and_then(|supply| supply.percentage);
        desired.insert(device.address.clone(), (device.path.clone(), percentage));
    }

    for key in exported.keys().cloned().collect::<Vec<_>>() {
        if !desired.contains_key(&key) {
            let (_, percentage) = &exported[&key];
            remove_battery(connection, &key, *percentage).await?;
            exported.remove(&key);
            eprintln!("audio-control: battery provider: {key} removed");
        }
    }
    let mut surfaced = true;
    for (key, (device, percentage)) in desired {
        let unchanged = exported
            .get(&key)
            .is_some_and(|(current, current_percentage)| {
                *current == device && *current_percentage == percentage
            });
        if unchanged && battery_surfaced(connection, &device).await {
            continue;
        }
        // Either the desired state changed or the daemon is not surfacing
        // this battery; (re-)exporting re-enters the first-come race, which
        // claims the slot the moment the GATT stub frees it at disconnect.
        // Registration itself is only observable on the next cycle.
        surfaced = false;
        if let Some((_, previous)) = exported.get(&key) {
            remove_battery(connection, &key, *previous).await?;
        }
        let path = battery_path(&key)?;
        match percentage {
            Some(percentage) => {
                connection
                    .object_server()
                    .at(
                        path,
                        HidppBattery {
                            device: device.clone(),
                            percentage,
                        },
                    )
                    .await?;
            }
            None => {
                connection
                    .object_server()
                    .at(
                        path,
                        HidppBatteryPending {
                            device: device.clone(),
                        },
                    )
                    .await?;
            }
        }
        // Retry re-exports of an unchanged state are silent; only real
        // transitions are worth a journal line.
        if !unchanged {
            eprintln!(
                "audio-control: battery provider: {key} -> {}",
                percentage
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "pending".to_owned())
            );
        }
        exported.insert(key, (device, percentage));
    }
    Ok(surfaced)
}

async fn remove_battery(
    connection: &Connection,
    key: &str,
    percentage: Option<u8>,
) -> AppResult<()> {
    let path = battery_path(key)?;
    let server = connection.object_server();
    match percentage {
        Some(_) => {
            server.remove::<HidppBattery, _>(path).await?;
        }
        None => {
            server.remove::<HidppBatteryPending, _>(path).await?;
        }
    }
    Ok(())
}

/// True when bluetoothd is surfacing this provider's battery as the device's
/// `Battery1`. The daemon answers `Source` from whichever battery holds the
/// device's slot, so anything else — the GATT stub's `"GATT Battery
/// Service"`, or no interface at all — means the slot must be re-entered.
async fn battery_surfaced(connection: &Connection, device: &OwnedObjectPath) -> bool {
    let Ok(proxy) = Proxy::new(connection, BLUEZ, device.as_str(), "org.bluez.Battery1").await
    else {
        return false;
    };
    matches!(
        proxy.get_property::<String>("Source").await,
        Ok(source) if source == SOURCE
    )
}

/// Battery object path under the provider root for one normalized address.
fn battery_path(key: &str) -> AppResult<OwnedObjectPath> {
    Ok(OwnedObjectPath::try_from(format!("{PROVIDER_ROOT}/{key}"))?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bluetooth_addresses_collapse_to_bare_hex() {
        assert_eq!(
            normalize_address("c8:1a:7b:31:09:df").as_deref(),
            Some("C81A7B3109DF")
        );
        assert_eq!(
            normalize_address("C8-1A-7B-31-09-DF").as_deref(),
            Some("C81A7B3109DF")
        );
    }

    #[test]
    fn receiver_serials_are_not_addresses() {
        assert_eq!(normalize_address("4516LGN8"), None);
        assert_eq!(normalize_address(""), None);
    }

    #[test]
    fn level_words_map_to_coarse_percentages() {
        assert_eq!(level_percentage("Full\n"), Some(100));
        assert_eq!(level_percentage("critical"), Some(5));
        assert_eq!(level_percentage("unknown"), None);
    }

    #[test]
    fn battery_paths_sit_under_the_provider_root() {
        let path = battery_path("C81A7B3109DF").unwrap();
        assert_eq!(path.as_str(), "/bt_battery/hidpp/C81A7B3109DF");
    }

    #[test]
    fn only_logitech_hid_devices_are_pre_empted() {
        let hid = vec![HID_SERVICE_UUID.to_owned()];
        assert!(is_logitech_hid(Some("usb:v046DpB023d0015"), &hid));
        // Logitech, but audio: no HID service, no HID++ battery.
        assert!(!is_logitech_hid(Some("usb:v046DpB040d0015"), &[]));
        // HID, but not Logitech.
        assert!(!is_logitech_hid(Some("usb:v8087p0026d0015"), &hid));
        assert!(!is_logitech_hid(None, &hid));
    }
}
