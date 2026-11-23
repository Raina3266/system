//! Native D-Bus names, bounded value conversion, and secret-safe failures.

use std::collections::{BTreeMap, HashMap};
use std::time::Duration;

use dbus::arg::{ArgType, PropMap, RefArg, Variant};
use dbus::{Message, MessageType, Path};
use serde::Serialize;
use serde_json::{Value, json};

pub(crate) const NM: &str = "org.freedesktop.NetworkManager";
pub(crate) const ROOT: &str = "/org/freedesktop/NetworkManager";
pub(crate) const OBJECT_ROOT: &str = "/org/freedesktop";
pub(crate) const DEVICE: &str = "org.freedesktop.NetworkManager.Device";
pub(crate) const WIRELESS: &str = "org.freedesktop.NetworkManager.Device.Wireless";
pub(crate) const ETHERNET: &str = "org.freedesktop.NetworkManager.Device.Wired";
pub(crate) const AP_INTERFACE: &str = "org.freedesktop.NetworkManager.AccessPoint";
pub(crate) const ACTIVE: &str = "org.freedesktop.NetworkManager.Connection.Active";
pub(crate) const CONNECTION: &str = "org.freedesktop.NetworkManager.Settings.Connection";
pub(crate) const OBJECT_MANAGER: &str = "org.freedesktop.DBus.ObjectManager";
pub(crate) const DBUS: &str = "org.freedesktop.DBus";
pub(crate) const DBUS_PATH: &str = "/org/freedesktop/DBus";
const MAX_OBJECTS: usize = 8192;
pub(crate) const READ_TIME: Duration = Duration::from_secs(5);
pub(crate) type Settings = HashMap<String, PropMap>;
pub(crate) type Managed = HashMap<Path<'static>, HashMap<String, PropMap>>;
pub(crate) type Objects = BTreeMap<String, BTreeMap<String, Value>>;
pub(crate) type NetResult<T> = Result<T, Failure>;

#[derive(Clone, Debug, Serialize, PartialEq)]
pub(crate) struct Failure {
    pub(crate) message: &'static str,
    pub(crate) code: &'static str,
}
impl Failure {
    pub(crate) fn new(message: &'static str) -> Self {
        Self {
            message,
            code: "NETWORK_ERROR",
        }
    }
    pub(crate) fn authentication(message: &'static str) -> Self {
        Self {
            message,
            code: "AUTHENTICATION",
        }
    }
    pub(crate) fn cancelled() -> Self {
        Self {
            message: "The network request was cancelled.",
            code: "CANCELLED",
        }
    }
    pub(crate) fn timeout() -> Self {
        Self {
            message: "The network request timed out. Try again.",
            code: "TIMEOUT",
        }
    }
    pub(crate) fn changed() -> Self {
        Self::new(
            "The selected saved profile or connection changed. Reopen network information and try again.",
        )
    }
    pub(crate) fn unavailable() -> Self {
        Self {
            message: "The network device is no longer available.",
            code: "UNAVAILABLE",
        }
    }
    pub(crate) fn invalid() -> Self {
        Self {
            message: "Invalid network backend request.",
            code: "INVALID_REQUEST",
        }
    }
    pub(crate) fn native(message: &mut Message) -> Option<Self> {
        if message.msg_type() != MessageType::Error {
            return None;
        }
        // Only the error *name* is examined. Its diagnostic body can contain
        // passwords, and must never be formatted, returned, or logged.
        let authentication = message
            .as_result()
            .err()
            .and_then(|error| {
                error.name().map(|name| {
                    name.rsplit('.').next().is_some_and(|suffix| {
                        matches!(
                            suffix,
                            "NoSecrets" | "InvalidSecrets" | "LoginFailed" | "AuthenticationFailed"
                        )
                    })
                })
            })
            .unwrap_or(false);
        let text =
            "The network operation failed. Check permissions and the saved network settings.";
        Some(if authentication {
            Self::authentication(text)
        } else {
            Self::new(text)
        })
    }
}

pub(crate) fn dbus_path(path: &str) -> NetResult<Path<'static>> {
    Path::new(path.to_owned()).map_err(|_| Failure::changed())
}
pub(crate) fn call(owner: &str, path: &str, interface: &str, method: &str) -> NetResult<Message> {
    Message::new_method_call(owner, path, interface, method).map_err(|_| Failure::changed())
}
fn raw_json(arg: &dyn RefArg, depth: usize) -> Value {
    if depth > 12 {
        return Value::Null;
    }
    match arg.arg_type() {
        ArgType::Variant => arg
            .as_iter()
            .and_then(|mut iter| iter.next())
            .map_or(Value::Null, |value| raw_json(value, depth + 1)),
        ArgType::Boolean => json!(arg.as_i64().unwrap_or(0) != 0),
        ArgType::String | ArgType::ObjectPath | ArgType::Signature => {
            json!(arg.as_str().unwrap_or(""))
        }
        ArgType::Double => json!(arg.as_f64().unwrap_or(0.0)),
        ArgType::Array if arg.signature().starts_with("a{") => {
            let mut result = serde_json::Map::new();
            if let Some(mut iter) = arg.as_iter() {
                while let (Some(key), Some(value)) = (iter.next(), iter.next()) {
                    if result.len() >= MAX_OBJECTS {
                        break;
                    }
                    if let Some(key) = key.as_str() {
                        result.insert(key.to_owned(), raw_json(value, depth + 1));
                    }
                }
            }
            Value::Object(result)
        }
        ArgType::Array | ArgType::Struct | ArgType::DictEntry => Value::Array(
            arg.as_iter()
                .map(|iter| {
                    iter.take(MAX_OBJECTS)
                        .map(|value| raw_json(value, depth + 1))
                        .collect()
                })
                .unwrap_or_default(),
        ),
        _ => arg
            .as_u64()
            .map_or_else(|| json!(arg.as_i64().unwrap_or(0)), |value| json!(value)),
    }
}
fn properties_json(properties: &PropMap) -> Value {
    Value::Object(
        properties
            .iter()
            .map(|(name, value)| (name.clone(), raw_json(&*value.0, 0)))
            .collect(),
    )
}
pub(crate) fn managed_json(objects: Managed) -> NetResult<Objects> {
    if objects.len() > MAX_OBJECTS {
        return Err(Failure::unavailable());
    }
    Ok(objects
        .into_iter()
        .map(|(path, interfaces)| {
            (
                path.to_string(),
                interfaces
                    .into_iter()
                    .map(|(name, properties)| (name, properties_json(&properties)))
                    .collect(),
            )
        })
        .collect())
}
pub(crate) fn settings_json(settings: &Settings) -> Value {
    Value::Object(
        settings
            .iter()
            .map(|(name, properties)| (name.clone(), properties_json(properties)))
            .collect(),
    )
}
pub(crate) fn clone_settings(settings: &Settings) -> Settings {
    settings
        .iter()
        .map(|(name, properties)| {
            (
                name.clone(),
                properties
                    .iter()
                    .map(|(key, value)| (key.clone(), Variant(value.0.box_clone())))
                    .collect(),
            )
        })
        .collect()
}
pub(crate) fn insert<T: RefArg + 'static>(
    settings: &mut Settings,
    section: &str,
    key: &str,
    value: T,
) {
    settings
        .entry(section.to_owned())
        .or_default()
        .insert(key.to_owned(), Variant(Box::new(value)));
}
pub(crate) fn strip_secrets(settings: &mut Settings) {
    for properties in settings.values_mut() {
        properties.retain(|name, _| {
            !matches!(
                name.as_str(),
                "psk"
                    | "password"
                    | "password-raw"
                    | "leap-password"
                    | "private-key-password"
                    | "phase2-private-key-password"
                    | "pin"
                    | "secrets"
                    | "wep-key0"
                    | "wep-key1"
                    | "wep-key2"
                    | "wep-key3"
            )
        });
    }
}
pub(crate) fn text(value: &Value, key: &str) -> String {
    value[key].as_str().unwrap_or("").to_owned()
}
pub(crate) fn number(value: &Value, key: &str) -> u32 {
    value[key].as_u64().unwrap_or(0).min(u32::MAX as u64) as u32
}
pub(crate) fn boolean(value: &Value, key: &str) -> bool {
    value[key].as_bool().unwrap_or(false)
}
pub(crate) fn strings(value: &Value, key: &str) -> Vec<String> {
    value[key]
        .as_array()
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}
pub(crate) fn bytes(value: &Value, key: &str) -> Vec<u8> {
    value[key]
        .as_array()
        .map(|values| {
            values
                .iter()
                .filter_map(|value| value.as_u64().and_then(|value| u8::try_from(value).ok()))
                .collect()
        })
        .unwrap_or_default()
}
pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
