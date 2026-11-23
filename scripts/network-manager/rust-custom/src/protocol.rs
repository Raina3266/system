//! JSONL request schema, method validation, and request bounds.

use serde::Deserialize;
use serde_json::{Value, json};

use crate::bus::{Failure, NetResult};
use crate::connection::Secret;
use crate::model::Identity;

pub(crate) const REQUEST_LIMIT: usize = 65536;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WireRequest {
    #[serde(rename = "expectedStamp")]
    pub(crate) expected_stamp: Option<String>,
    #[serde(default)]
    pub(crate) id: Option<Value>,
    pub(crate) method: String,
    #[serde(default)]
    pub(crate) args: Vec<Value>,
}
pub(crate) enum Method {
    Activate {
        key: String,
        profile_uuid: String,
        password: Option<Secret>,
    },
    Disconnect(String),
    Forget(String),
    ToggleWifi,
    ReadSecret(String, Identity),
    ReadSavedSecret(String, String),
}
impl Method {
    pub(crate) fn parse(method: &str, mut args: Vec<Value>) -> NetResult<Self> {
        let key = || {
            args.first()
                .and_then(Value::as_str)
                .filter(|key| !key.is_empty() && key.len() <= 4096)
                .map(str::to_owned)
                .ok_or_else(Failure::invalid)
        };
        match method {
            "activate" if matches!(args.len(), 1 | 2) => {
                let key = key()?;
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase", deny_unknown_fields)]
                struct Options {
                    #[serde(default)]
                    profile_uuid: String,
                    #[serde(default)]
                    password: Option<String>,
                }
                let options: Options = serde_json::from_value(if args.len() == 2 {
                    args.remove(1)
                } else {
                    json!({})
                })
                .map_err(|_| Failure::invalid())?;
                if options.profile_uuid.len() > 128 {
                    return Err(Failure::invalid());
                }
                Ok(Self::Activate {
                    key,
                    profile_uuid: options.profile_uuid,
                    password: options.password.map(Secret),
                })
            }
            "disconnect" if args.len() == 1 => Ok(Self::Disconnect(key()?)),
            "forget" if args.len() == 1 => Ok(Self::Forget(key()?)),
            "toggleWifi" if args.is_empty() => Ok(Self::ToggleWifi),
            "readSecret" if args.len() == 2 => {
                let key = key()?;
                let identity: Identity =
                    serde_json::from_value(args.remove(1)).map_err(|_| Failure::invalid())?;
                Ok(Self::ReadSecret(key, identity))
            }
            "readSavedSecret" if args.len() == 2 => {
                let key = key()?;
                let uuid = args[1]
                    .as_str()
                    .filter(|uuid| !uuid.is_empty() && uuid.len() <= 128)
                    .ok_or_else(Failure::invalid)?
                    .to_owned();
                Ok(Self::ReadSavedSecret(key, uuid))
            }
            _ => Err(Failure::invalid()),
        }
    }
    pub(crate) fn key(&self) -> Option<&str> {
        match self {
            Self::Activate { key, .. }
            | Self::Disconnect(key)
            | Self::Forget(key)
            | Self::ReadSecret(key, _)
            | Self::ReadSavedSecret(key, _) => Some(key),
            Self::ToggleWifi => None,
        }
    }
    pub(crate) fn mutation(&self) -> bool {
        matches!(
            self,
            Self::Activate { .. } | Self::Disconnect(_) | Self::Forget(_)
        )
    }
}
pub(crate) fn valid_id(id: &Value) -> bool {
    id.as_str()
        .is_some_and(|id| !id.is_empty() && id.len() <= 128)
        || id.as_u64().is_some_and(|id| id <= 9_007_199_254_740_991)
}
