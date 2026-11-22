use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::env;
use std::ffi::OsString;
use std::fs;
use std::io::{self, Write};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::{Path, PathBuf};
use std::process::{self, Command, Stdio};

#[derive(Deserialize)]
struct Config {
    name: String,
    commands: Vec<String>,
    strict_help: bool,
    check_response: bool,
    astal: PathBuf,
    bundle: PathBuf,
    environment: BTreeMap<String, String>,
    forced_environment: BTreeMap<String, String>,
}

impl Config {
    fn usage(&self) -> String {
        let commands = self
            .commands
            .iter()
            .map(|command| match command.as_str() {
                "toggle" => "toggle [monitor]",
                command => command,
            })
            .collect::<Vec<_>>()
            .join(" | ");
        format!("Usage: {} {commands}", self.name)
    }

    fn child_command(&self, executable: &Path) -> Command {
        let mut command = Command::new(executable);
        for (key, value) in &self.environment {
            if env::var_os(key).is_none_or(|value| value.is_empty()) {
                command.env(key, value);
            }
        }
        command.envs(&self.forced_environment);
        command
    }
}

enum Action<'a> {
    Help,
    Run,
    Request(Request<'a>),
}

#[derive(Serialize)]
struct Request<'a> {
    command: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    monitor: Option<&'a str>,
}

fn parse_action<'a>(config: &Config, args: &'a [OsString]) -> Result<Action<'a>, ()> {
    let command = args.first().and_then(|arg| arg.to_str()).ok_or(())?;
    if matches!(command, "-h" | "--help") {
        return if !config.strict_help || args.len() == 1 {
            Ok(Action::Help)
        } else {
            Err(())
        };
    }
    if !config.commands.iter().any(|allowed| allowed == command)
        || args.len() > if command == "toggle" { 2 } else { 1 }
    {
        return Err(());
    }
    if command == "run" {
        Ok(Action::Run)
    } else {
        Ok(Action::Request(Request {
            command,
            monitor: args
                .get(1)
                .map(|arg| arg.to_str().ok_or(()))
                .transpose()?
                .filter(|monitor| !monitor.is_empty()),
        }))
    }
}

fn exec_error(name: &str, executable: &Path, error: io::Error) -> i32 {
    eprintln!(
        "{name}: cannot execute '{}': {error}; check the configured executable and its permissions",
        executable.display()
    );
    match error.kind() {
        io::ErrorKind::NotFound => 127,
        io::ErrorKind::PermissionDenied => 126,
        // ENOEXEC: the file exists but is not an executable program.
        _ if error.raw_os_error() == Some(8) => 126,
        _ => 1,
    }
}

fn has_runtime_dir(command: &Command) -> bool {
    command
        .get_envs()
        .find(|(key, _)| *key == "XDG_RUNTIME_DIR")
        .map(|(_, value)| value.is_some_and(|value| !value.is_empty()))
        .unwrap_or_else(|| env::var_os("XDG_RUNTIME_DIR").is_some_and(|value| !value.is_empty()))
}

fn response_body(response: &[u8]) -> &[u8] {
    // Shell command substitution removes every trailing LF, but not CR or spaces.
    let end = response
        .iter()
        .rposition(|byte| *byte != b'\n')
        .map_or(0, |index| index + 1);
    &response[..end]
}

fn response_has_error(response: &[u8]) -> bool {
    serde_json::from_slice::<serde_json::Value>(response)
        .ok()
        .is_some_and(|value| {
            value
                .as_object()
                .is_some_and(|object| object.contains_key("error"))
        })
}

fn launch(config: &Config, action: Action<'_>) -> i32 {
    let is_run = matches!(action, Action::Run);
    let (mut command, executable) = match action {
        Action::Help => {
            println!("{}", config.usage());
            return 0;
        }
        Action::Run => {
            let command = config.child_command(&config.bundle);
            if !has_runtime_dir(&command) {
                eprintln!("{}: XDG_RUNTIME_DIR is required", config.name);
                return 1;
            }
            (command, &config.bundle)
        }
        Action::Request(request) => {
            let mut command = config.child_command(&config.astal);
            command.args([
                "--instance",
                &config.name,
                &serde_json::to_string(&request).expect("request contains only strings"),
            ]);
            (command, &config.astal)
        }
    };

    if is_run || !config.check_response {
        return exec_error(&config.name, executable, command.exec());
    }

    let output = match command
        .stdin(Stdio::inherit())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .output()
    {
        Ok(output) => output,
        Err(error) => return exec_error(&config.name, executable, error),
    };
    if !output.status.success() {
        return output
            .status
            .code()
            .unwrap_or_else(|| output.status.signal().map_or(1, |signal| 128 + signal));
    }
    let response = response_body(&output.stdout);
    let mut stdout = io::stdout().lock();
    if let Err(error) = stdout
        .write_all(response)
        .and_then(|()| stdout.write_all(b"\n"))
        .and_then(|()| stdout.flush())
    {
        eprintln!("{}: cannot write response: {error}", config.name);
        return 1;
    }
    i32::from(response_has_error(response))
}

fn main() {
    process::exit(main_status(env::args_os().skip(1)));
}

fn backend_status(mut args: impl Iterator<Item = OsString>) -> i32 {
    match args.next().as_deref().and_then(std::ffi::OsStr::to_str) {
        Some("network") if args.next().is_none() => network_backend::run(),
        Some("dashboard") if args.next().is_none() => dashboard_backend::run(),
        _ => {
            eprintln!("Usage: panel-launcher backend {{network|dashboard}}");
            2
        }
    }
}

fn main_status(mut args: impl Iterator<Item = OsString>) -> i32 {
    let launcher_usage = "Usage: panel-launcher --config PATH COMMAND [monitor]";
    let first = args.next();
    if first.as_deref() == Some(std::ffi::OsStr::new("runtime")) {
        return desktop_runtime::status(&args.collect::<Vec<_>>());
    }
    if first.as_deref() == Some(std::ffi::OsStr::new("backend")) {
        return backend_status(args);
    }
    if first.as_deref() != Some(std::ffi::OsStr::new("--config")) {
        eprintln!("{launcher_usage}");
        return 2;
    }
    let Some(path) = args.next().map(PathBuf::from) else {
        eprintln!("{launcher_usage}");
        return 2;
    };
    let contents = match fs::read(&path) {
        Ok(contents) => contents,
        Err(error) => {
            eprintln!(
                "panel-launcher: cannot read config '{}': {error}; pass a readable JSON file with --config",
                path.display()
            );
            return 1;
        }
    };
    let config: Config = match serde_json::from_slice(&contents) {
        Ok(config) => config,
        Err(error) => {
            eprintln!(
                "panel-launcher: invalid config '{}': {error}; check the generated launcher configuration",
                path.display()
            );
            return 1;
        }
    };
    let args: Vec<_> = args.collect();
    match parse_action(&config, &args) {
        Ok(action) => launch(&config, action),
        Err(()) => {
            eprintln!("{}", config.usage());
            2
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configuration_command_parsing_remains_independent_of_backend_dispatch() {
        let mut config = Config {
            name: "fixture-panel".to_owned(),
            commands: ["run", "toggle", "close", "status", "toggleWifi"]
                .into_iter()
                .map(str::to_owned)
                .collect(),
            strict_help: true,
            check_response: true,
            astal: PathBuf::from("/fixture/astal"),
            bundle: PathBuf::from("/fixture/bundle"),
            environment: BTreeMap::new(),
            forced_environment: BTreeMap::new(),
        };
        let args = [
            OsString::from("toggle"),
            OsString::from("monitor with spaces"),
        ];
        let Action::Request(request) =
            parse_action(&config, &args).unwrap_or_else(|()| panic!("valid toggle"))
        else {
            panic!("expected IPC action");
        };
        assert_eq!(
            serde_json::to_value(request).unwrap(),
            serde_json::json!({"command":"toggle","monitor":"monitor with spaces"})
        );
        assert!(matches!(
            parse_action(&config, &[OsString::from("run")]),
            Ok(Action::Run)
        ));
        for args in [
            vec![],
            vec![OsString::from("backend")],
            vec![OsString::from("close"), OsString::from("extra")],
            vec![OsString::from("--help"), OsString::from("extra")],
        ] {
            assert!(parse_action(&config, &args).is_err());
        }
        config.strict_help = false;
        assert!(matches!(
            parse_action(
                &config,
                &[OsString::from("--help"), OsString::from("extra")]
            ),
            Ok(Action::Help)
        ));
        assert_eq!(
            backend_status([OsString::from("network"), OsString::from("extra")].into_iter()),
            2
        );
        assert_eq!(
            config.usage(),
            "Usage: fixture-panel run | toggle [monitor] | close | status | toggleWifi"
        );
    }

    #[test]
    fn shell_response_newline_semantics() {
        for (input, expected) in [
            (b"".as_slice(), b"".as_slice()),
            (b"\n\n", b""),
            (b"ok\n\n", b"ok"),
            (b"ok\r\n", b"ok\r"),
            (b"ok \n", b"ok "),
            (b"one\ntwo\n", b"one\ntwo"),
            (b"\xff\n", b"\xff"),
        ] {
            assert_eq!(response_body(input), expected);
        }
    }

    #[test]
    fn only_top_level_error_keys_are_failures() {
        for response in [
            r#"{"error":null}"#,
            r#"{"error":false}"#,
            r#"{"error":0}"#,
            r#"{"error":""}"#,
            r#"{"error":{}}"#,
            r#"{"result":"ok","error":[]}"#,
        ] {
            assert!(response_has_error(response.as_bytes()), "{response}");
        }
        for response in [
            "ok",
            "",
            "not json",
            "null",
            "false",
            r#""error""#,
            r#"{"result":{"error":true}}"#,
            r#"[{"error":true}]"#,
            r#"{"error":true} trailing"#,
        ] {
            assert!(!response_has_error(response.as_bytes()), "{response}");
        }
    }
}

// Kept self-contained so other backend domains can extend backend_status without
// changing the launcher/configuration path or sharing NetworkManager state.
mod network_backend {
    use dbus::arg::{ArgType, PropMap, RefArg, Variant};
    use dbus::channel::{BusType, Channel, Sender};
    use dbus::{Message, MessageType, Path};
    use serde::{Deserialize, Serialize};
    use serde_json::{Value, json};
    use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
    use std::hash::{Hash, Hasher};
    use std::io;
    use std::time::{Duration, Instant};

    const NM: &str = "org.freedesktop.NetworkManager";
    const ROOT: &str = "/org/freedesktop/NetworkManager";
    const OBJECT_ROOT: &str = "/org/freedesktop";
    #[cfg(test)]
    const SETTINGS_PATH: &str = "/org/freedesktop/NetworkManager/Settings";
    const DEVICE: &str = "org.freedesktop.NetworkManager.Device";
    const WIRELESS: &str = "org.freedesktop.NetworkManager.Device.Wireless";
    const ETHERNET: &str = "org.freedesktop.NetworkManager.Device.Wired";
    const AP_INTERFACE: &str = "org.freedesktop.NetworkManager.AccessPoint";
    const ACTIVE: &str = "org.freedesktop.NetworkManager.Connection.Active";
    const CONNECTION: &str = "org.freedesktop.NetworkManager.Settings.Connection";
    const OBJECT_MANAGER: &str = "org.freedesktop.DBus.ObjectManager";
    const DBUS: &str = "org.freedesktop.DBus";
    const DBUS_PATH: &str = "/org/freedesktop/DBus";
    const WIFI: &str = "802-11-wireless";
    const WIRED: &str = "802-3-ethernet";
    const SECURITY: &str = "802-11-wireless-security";
    const PSK: u32 = 0x100;
    const EAP: u32 = 0x200;
    const SAE: u32 = 0x400;
    const OWE: u32 = 0x800;
    const OWE_TM: u32 = 0x1000;
    const SUITE_B: u32 = 0x2000;
    const REQUEST_LIMIT: usize = 65536;
    const MAX_OPERATIONS: usize = 32;
    const MAX_OBJECTS: usize = 8192;
    const MAX_PROFILES: usize = 512;
    const MAX_DEVICES: usize = 64;
    const OUTPUT_LIMIT: usize = 1024 * 1024;
    const REQUEST_TIME: Duration = Duration::from_secs(30);
    const READ_TIME: Duration = Duration::from_secs(5);
    const SCAN_TIME: Duration = Duration::from_secs(15);
    type Settings = HashMap<String, PropMap>;
    type Managed = HashMap<Path<'static>, HashMap<String, PropMap>>;
    type Objects = BTreeMap<String, BTreeMap<String, Value>>;
    type NetResult<T> = Result<T, Failure>;

    #[derive(Clone, Debug, Serialize, PartialEq)]
    struct Failure {
        message: &'static str,
        code: &'static str,
    }
    impl Failure {
        fn new(message: &'static str) -> Self {
            Self {
                message,
                code: "NETWORK_ERROR",
            }
        }
        fn authentication(message: &'static str) -> Self {
            Self {
                message,
                code: "AUTHENTICATION",
            }
        }
        fn cancelled() -> Self {
            Self {
                message: "The network request was cancelled.",
                code: "CANCELLED",
            }
        }
        fn timeout() -> Self {
            Self {
                message: "The network request timed out. Try again.",
                code: "TIMEOUT",
            }
        }
        fn changed() -> Self {
            Self::new(
                "The selected saved profile or connection changed. Reopen network information and try again.",
            )
        }
        fn unavailable() -> Self {
            Self {
                message: "The network device is no longer available.",
                code: "UNAVAILABLE",
            }
        }
        fn invalid() -> Self {
            Self {
                message: "Invalid network backend request.",
                code: "INVALID_REQUEST",
            }
        }
        fn native(message: &mut Message) -> Option<Self> {
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
                                "NoSecrets"
                                    | "InvalidSecrets"
                                    | "LoginFailed"
                                    | "AuthenticationFailed"
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

    fn dbus_path(path: &str) -> NetResult<Path<'static>> {
        Path::new(path.to_owned()).map_err(|_| Failure::changed())
    }
    fn call(owner: &str, path: &str, interface: &str, method: &str) -> NetResult<Message> {
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
    fn managed_json(objects: Managed) -> NetResult<Objects> {
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
    fn settings_json(settings: &Settings) -> Value {
        Value::Object(
            settings
                .iter()
                .map(|(name, properties)| (name.clone(), properties_json(properties)))
                .collect(),
        )
    }
    fn clone_settings(settings: &Settings) -> Settings {
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
    fn insert<T: RefArg + 'static>(settings: &mut Settings, section: &str, key: &str, value: T) {
        settings
            .entry(section.to_owned())
            .or_default()
            .insert(key.to_owned(), Variant(Box::new(value)));
    }
    fn strip_secrets(settings: &mut Settings) {
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
    fn text(value: &Value, key: &str) -> String {
        value[key].as_str().unwrap_or("").to_owned()
    }
    fn number(value: &Value, key: &str) -> u32 {
        value[key].as_u64().unwrap_or(0).min(u32::MAX as u64) as u32
    }
    fn boolean(value: &Value, key: &str) -> bool {
        value[key].as_bool().unwrap_or(false)
    }
    fn strings(value: &Value, key: &str) -> Vec<String> {
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
    fn bytes(value: &Value, key: &str) -> Vec<u8> {
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
    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }
    unsafe extern "C" {
        fn iconv_open(to: *const libc::c_char, from: *const libc::c_char) -> *mut libc::c_void;
        fn iconv_close(converter: *mut libc::c_void) -> libc::c_int;
        fn iconv(
            converter: *mut libc::c_void,
            input: *mut *mut libc::c_char,
            input_left: *mut usize,
            output: *mut *mut libc::c_char,
            output_left: *mut usize,
        ) -> usize;
    }
    fn ssid_convert(bytes: &[u8], encoding: &str) -> Option<String> {
        let encoding = std::ffi::CString::new(encoding).ok()?;
        let converter = unsafe { iconv_open(c"UTF-8".as_ptr(), encoding.as_ptr()) };
        if converter as usize == usize::MAX {
            return None;
        }
        let mut input = bytes.as_ptr().cast_mut().cast::<libc::c_char>();
        let mut input_left = bytes.len();
        let mut result = vec![0u8; bytes.len().checked_mul(6)?.checked_add(32)?];
        let mut output = result.as_mut_ptr().cast::<libc::c_char>();
        let mut output_left = result.len();
        let converted = unsafe {
            iconv(
                converter,
                &mut input,
                &mut input_left,
                &mut output,
                &mut output_left,
            )
        };
        unsafe {
            iconv_close(converter);
        }
        if converted == usize::MAX || input_left != 0 {
            return None;
        }
        result.truncate(result.len() - output_left);
        String::from_utf8(result)
            .ok()
            .map(|text| text.split('\0').next().unwrap_or("").to_owned())
    }
    fn ssid_encodings(locale: &str) -> Vec<String> {
        let language = locale.split('.').next().unwrap_or("").to_ascii_lowercase();
        let names: &[&str] = match language.as_str() {
            "zh_cn" | "zh_sg" => &["euc-cn", "gb2312", "gb18030"],
            "zh_hk" => &["big5", "euc-tw", "big5-hkcs"],
            "zh_mo" | "zh_tw" => &["big5", "euc-tw"],
            _ => match language.get(..2).unwrap_or("") {
                "ar" => &["iso-8859-6", "windows-1256"],
                "be" | "mk" | "ru" | "sr" => &["koi8-r", "windows-1251", "iso-8859-5"],
                "bg" => &["windows-1251", "koi8-r", "iso-8859-5"],
                "cs" | "hr" | "hu" | "pl" | "ro" | "sh" | "sk" | "sl" => {
                    &["iso-8859-2", "windows-1250"]
                }
                "el" => &["iso-8859-7", "windows-1253"],
                "et" | "lt" | "lv" => &["iso-8859-4", "windows-1257"],
                "he" | "iw" => &["iso-8859-8", "windows-1255"],
                "ja" => &["euc-jp", "shift_jis", "iso-2022-jp"],
                "ko" => &["euc-kr", "iso-2022-kr", "johab"],
                "th" => &["iso-8859-11", "windows-874"],
                "tr" => &["iso-8859-9", "windows-1254"],
                "uk" => &["koi8-u", "koi8-r", "windows-1251"],
                _ => &[],
            },
        };
        if !names.is_empty() {
            return names.iter().map(|name| (*name).to_owned()).collect();
        }
        vec![
            locale
                .split_once('.')
                .map(|(_, encoding)| encoding.split('@').next().unwrap_or("UTF-8"))
                .unwrap_or("UTF-8")
                .to_owned(),
            "iso-8859-1".to_owned(),
            "windows-1251".to_owned(),
        ]
    }
    fn ssid_text(bytes: &[u8]) -> String {
        // Match NM's locale-guided display conversion, but never use its text as
        // identity: different SSID byte strings can have the same visible name.
        if !bytes.contains(&0)
            && let Ok(text) = std::str::from_utf8(bytes)
        {
            return text.to_owned();
        }
        static ENCODINGS: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
        let encodings = ENCODINGS.get_or_init(|| {
            let locale = ["LC_ALL", "LC_CTYPE", "LANG"]
                .iter()
                .find_map(|name| std::env::var(name).ok())
                .unwrap_or_default();
            ssid_encodings(&locale)
        });
        encodings
            .iter()
            .find_map(|encoding| ssid_convert(bytes, encoding))
            .unwrap_or_else(|| {
                bytes
                    .iter()
                    .take_while(|byte| **byte != 0)
                    .map(|byte| {
                        if (0x20..=0x7e).contains(byte) {
                            *byte as char
                        } else {
                            '?'
                        }
                    })
                    .collect()
            })
    }
    fn object<'a>(objects: &'a Objects, path: &str, interface: &str) -> &'a Value {
        objects
            .get(path)
            .and_then(|interfaces| interfaces.get(interface))
            .unwrap_or(&Value::Null)
    }
    fn non_root(path: &str) -> bool {
        !path.is_empty() && path != "/"
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
    #[serde(rename_all = "lowercase")]
    enum Security {
        Open,
        Wep,
        Psk,
        Sae,
        Owe,
        Enterprise,
        Unknown,
    }
    impl Security {
        fn name(self) -> &'static str {
            match self {
                Self::Open => "open",
                Self::Wep => "wep",
                Self::Psk => "psk",
                Self::Sae => "sae",
                Self::Owe => "owe",
                Self::Enterprise => "enterprise",
                Self::Unknown => "unknown",
            }
        }
        fn label(self) -> &'static str {
            match self {
                Self::Open => "Open",
                Self::Wep => "WEP",
                Self::Psk => "WPA/WPA2",
                Self::Sae => "WPA3",
                Self::Owe => "Enhanced Open (OWE)",
                Self::Enterprise => "Enterprise",
                Self::Unknown => "Unknown",
            }
        }
        fn password(self) -> bool {
            matches!(self, Self::Wep | Self::Psk | Self::Sae)
        }
        fn shareable(self) -> bool {
            self == Self::Open || self.password()
        }
    }
    #[derive(Clone, Debug)]
    struct AccessPoint {
        path: String,
        ssid: Vec<u8>,
        bssid: String,
        flags: u32,
        wpa: u32,
        rsn: u32,
        strength: u32,
        frequency: u32,
        mode: u32,
    }
    impl AccessPoint {
        fn parse(path: &str, properties: &Value) -> Self {
            Self {
                path: path.to_owned(),
                ssid: bytes(properties, "Ssid"),
                bssid: text(properties, "HwAddress"),
                flags: number(properties, "Flags"),
                wpa: number(properties, "WpaFlags"),
                rsn: number(properties, "RsnFlags"),
                strength: number(properties, "Strength").min(100),
                frequency: number(properties, "Frequency"),
                mode: number(properties, "Mode"),
            }
        }
        fn security(&self) -> Security {
            let flags = self.wpa | self.rsn;
            if flags & (EAP | SUITE_B) != 0 {
                Security::Enterprise
            } else if flags & SAE != 0 {
                Security::Sae
            } else if flags & PSK != 0 {
                Security::Psk
            } else if flags & (OWE | OWE_TM) != 0 {
                Security::Owe
            } else if flags != 0 {
                Security::Unknown
            } else if self.flags & 1 != 0 {
                Security::Wep
            } else {
                Security::Open
            }
        }
    }
    struct Profile {
        path: String,
        incarnation: u64,
        settings: Settings,
        value: Value,
        version: Option<u64>,
        uuid: String,
        id: String,
        kind: String,
        ssid: Vec<u8>,
        security: Security,
    }
    impl Profile {
        fn parse(
            path: String,
            incarnation: u64,
            mut settings: Settings,
            properties: &Value,
        ) -> Self {
            strip_secrets(&mut settings);
            let value = settings_json(&settings);
            let security = if value.get("802-1x").is_some() {
                Security::Enterprise
            } else if value.get(SECURITY).is_none() {
                Security::Open
            } else {
                match value[SECURITY]["key-mgmt"].as_str().unwrap_or("") {
                    "none" => Security::Wep,
                    "wpa-psk" | "wpa-none" => Security::Psk,
                    "sae" => Security::Sae,
                    "owe" => Security::Owe,
                    "ieee8021x" | "wpa-eap" | "wpa-eap-suite-b-192" => Security::Enterprise,
                    _ => Security::Unknown,
                }
            };
            Self {
                uuid: text(&value["connection"], "uuid"),
                id: text(&value["connection"], "id"),
                kind: text(&value["connection"], "type"),
                ssid: bytes(&value[WIFI], "ssid"),
                version: properties["VersionId"].as_u64(),
                path,
                incarnation,
                settings,
                value,
                security,
            }
        }
        fn hidden(&self) -> bool {
            boolean(&self.value[WIFI], "hidden")
        }
        fn key_index(&self) -> u32 {
            number(&self.value[SECURITY], "wep-tx-keyidx")
        }
        fn label(&self) -> String {
            let setting = &self.value[SECURITY];
            let protocols = strings(setting, "proto");
            let protocol = if protocols.iter().any(|proto| proto == "rsn")
                && !protocols.iter().any(|proto| proto == "wpa")
            {
                "WPA2"
            } else if protocols.iter().any(|proto| proto == "wpa")
                && !protocols.iter().any(|proto| proto == "rsn")
            {
                "WPA"
            } else {
                "WPA/WPA2"
            };
            match setting["key-mgmt"].as_str().unwrap_or("") {
                "wpa-none" => "WPA".to_owned(),
                "wpa-psk" => protocol.to_owned(),
                "wpa-eap" => format!("{protocol} Enterprise"),
                "wpa-eap-suite-b-192" => "WPA3 Enterprise".to_owned(),
                "ieee8021x" => if setting["auth-alg"] == "leap" {
                    "LEAP"
                } else {
                    "Dynamic WEP"
                }
                .to_owned(),
                _ => self.security.label().to_owned(),
            }
        }
        fn context(&self) -> Value {
            authentication_context(&self.value)
        }
    }
    fn authentication_context(value: &Value) -> Value {
        let mut context = value.clone();
        if let Some(connection) = context["connection"].as_object_mut() {
            for field in [
                "id",
                "timestamp",
                "autoconnect",
                "autoconnect-priority",
                "autoconnect-retries",
            ] {
                connection.remove(field);
            }
        }
        if let Some(security) = context[SECURITY].as_object_mut() {
            for field in [
                "psk",
                "psk-flags",
                "wep-key0",
                "wep-key1",
                "wep-key2",
                "wep-key3",
                "wep-key-flags",
                "leap-password",
                "leap-password-flags",
            ] {
                security.remove(field);
            }
        }
        context
    }
    #[derive(Clone, Debug)]
    struct ActiveConnection {
        path: String,
        uuid: String,
        id: String,
        kind: String,
        profile: String,
        state: u32,
        reason: u32,
        devices: Vec<String>,
        ip4: String,
        ip6: String,
    }
    impl ActiveConnection {
        fn parse(path: &str, value: &Value, reason: u32) -> Self {
            Self {
                path: path.to_owned(),
                uuid: text(value, "Uuid"),
                id: text(value, "Id"),
                kind: text(value, "Type"),
                profile: text(value, "Connection"),
                state: number(value, "State"),
                reason,
                devices: strings(value, "Devices"),
                ip4: text(value, "Ip4Config"),
                ip6: text(value, "Ip6Config"),
            }
        }
    }
    #[derive(Clone, Debug)]
    struct DeviceState {
        path: String,
        incarnation: u64,
        iface: String,
        wireless: bool,
        state: u32,
        reason: u32,
        active: Option<ActiveConnection>,
        active_ap: String,
        aps: Vec<AccessPoint>,
        link: Value,
        ip4: Value,
        ip6: Value,
    }
    impl DeviceState {
        fn changing(&self) -> bool {
            (40..100).contains(&self.state)
        }
    }
    #[derive(Default)]
    struct Snapshot {
        owner: String,
        wifi_enabled: bool,
        wifi_hardware_enabled: bool,
        devices: Vec<DeviceState>,
        profiles: Vec<Profile>,
        active: BTreeMap<String, ActiveConnection>,
    }
    impl Snapshot {
        fn build(
            owner: String,
            objects: &Objects,
            mut settings: BTreeMap<String, Settings>,
            incarnations: &BTreeMap<String, u64>,
            reasons: &BTreeMap<String, u32>,
        ) -> NetResult<Self> {
            let manager = object(objects, ROOT, NM);
            if manager.is_null() {
                return Err(Failure::unavailable());
            }
            let mut result = Self {
                owner,
                wifi_enabled: boolean(manager, "WirelessEnabled"),
                wifi_hardware_enabled: boolean(manager, "WirelessHardwareEnabled"),
                ..Self::default()
            };
            for (path, interfaces) in objects {
                if let Some(value) = interfaces.get(ACTIVE) {
                    result.active.insert(
                        path.clone(),
                        ActiveConnection::parse(path, value, *reasons.get(path).unwrap_or(&0)),
                    );
                }
                if interfaces.contains_key(CONNECTION)
                    && let Some(settings) = settings.remove(path)
                {
                    result.profiles.push(Profile::parse(
                        path.clone(),
                        *incarnations.get(path).unwrap_or(&0),
                        settings,
                        object(objects, path, CONNECTION),
                    ));
                }
            }
            let paths = strings(manager, "Devices");
            if paths.len() > MAX_DEVICES {
                return Err(Failure::unavailable());
            }
            for path in paths {
                let properties = object(objects, &path, DEVICE);
                let kind = number(properties, "DeviceType");
                if !matches!(kind, 1 | 2) {
                    continue;
                }
                let wireless = kind == 2;
                let link = object(objects, &path, if wireless { WIRELESS } else { ETHERNET });
                if properties.is_null() || link.is_null() {
                    continue;
                }
                let active_path = text(properties, "ActiveConnection");
                let active = result.active.get(&active_path).cloned();
                let active_ap = text(link, "ActiveAccessPoint");
                let mut aps = Vec::new();
                if wireless {
                    let mut paths = strings(link, "AccessPoints");
                    if non_root(&active_ap) && !paths.contains(&active_ap) {
                        paths.push(active_ap.clone());
                    }
                    for ap in paths {
                        let value = object(objects, &ap, AP_INTERFACE);
                        if !value.is_null() {
                            aps.push(AccessPoint::parse(&ap, value));
                        }
                    }
                }
                let ip = |field, interface| {
                    let path = text(properties, field);
                    // Device and ActiveConnection can temporarily describe different
                    // requests. Never publish the device's unrelated IP configuration.
                    if active.as_ref().is_some_and(|active| {
                        path.as_str()
                            == if field == "Ip4Config" {
                                active.ip4.as_str()
                            } else {
                                active.ip6.as_str()
                            }
                    }) {
                        object(objects, &path, interface).clone()
                    } else {
                        Value::Null
                    }
                };
                result.devices.push(DeviceState {
                    path: path.clone(),
                    incarnation: *incarnations.get(&path).unwrap_or(&0),
                    iface: text(properties, "Interface"),
                    wireless,
                    state: number(properties, "State"),
                    reason: properties["StateReason"]
                        .get(1)
                        .and_then(Value::as_u64)
                        .unwrap_or(0) as u32,
                    active: active.clone(),
                    active_ap,
                    aps,
                    link: link.clone(),
                    ip4: ip("Ip4Config", "org.freedesktop.NetworkManager.IP4Config"),
                    ip6: ip("Ip6Config", "org.freedesktop.NetworkManager.IP6Config"),
                });
            }
            Ok(result)
        }
        fn unique_profile(&self, uuid: &str) -> Option<&Profile> {
            let mut profiles = self.profiles.iter().filter(|profile| profile.uuid == uuid);
            let profile = profiles.next()?;
            if profiles.next().is_some() {
                None
            } else {
                Some(profile)
            }
        }
    }

    fn normalize_mac(value: &str) -> String {
        value
            .chars()
            .filter(|ch| ch.is_ascii_hexdigit())
            .flat_map(char::to_lowercase)
            .collect()
    }
    fn setting_mac(value: &Value, field: &str) -> String {
        value[field]
            .as_str()
            .map(normalize_mac)
            .unwrap_or_else(|| hex(&bytes(value, field)))
    }
    fn device_compatible(device: &DeviceState, profile: &Profile) -> bool {
        if profile.kind != if device.wireless { WIFI } else { WIRED } || profile.uuid.is_empty() {
            return false;
        }
        let connection = &profile.value["connection"];
        let iface = text(connection, "interface-name");
        if !iface.is_empty() && iface != device.iface {
            return false;
        }
        let setting = &profile.value[if device.wireless { WIFI } else { WIRED }];
        let permanent = text(&device.link, "PermHwAddress");
        let actual = normalize_mac(if permanent.is_empty() {
            device.link["HwAddress"].as_str().unwrap_or("")
        } else {
            &permanent
        });
        let mac = setting_mac(setting, "mac-address");
        if !mac.is_empty() && mac != actual {
            return false;
        }
        for field in ["mac-address-blacklist", "mac-address-denylist"] {
            if strings(setting, field)
                .iter()
                .any(|mac| normalize_mac(mac) == actual)
            {
                return false;
            }
        }
        if !device.wireless {
            let subchannels = strings(setting, "s390-subchannels");
            if !subchannels.is_empty() && subchannels != strings(&device.link, "S390Subchannels") {
                return false;
            }
        } else {
            let caps = number(&device.link, "WirelessCapabilities");
            let required = match setting["mode"].as_str().unwrap_or("infrastructure") {
                "infrastructure" => 0,
                "adhoc" => 0x80,
                "ap" => 0x40,
                "mesh" => 0x1000,
                _ => return false,
            };
            if required != 0 && caps & required == 0 {
                return false;
            }
            let security = &profile.value[SECURITY];
            if matches!(
                security["key-mgmt"].as_str(),
                Some("wpa-psk" | "wpa-eap" | "wpa-eap-suite-b-192")
            ) {
                if caps & 0x3c == 0 {
                    return false;
                }
                let protocols = strings(security, "proto");
                if protocols.iter().any(|proto| proto == "rsn")
                    && !protocols.iter().any(|proto| proto == "wpa")
                    && caps & 0x28 == 0
                {
                    return false;
                }
            }
        }
        true
    }
    fn frequency_channel(frequency: u32) -> u32 {
        match frequency {
            2484 => 14,
            2412..=2472 if (frequency - 2407).is_multiple_of(5) => (frequency - 2407) / 5,
            4910..=4980 if (frequency - 4000).is_multiple_of(5) => (frequency - 4000) / 5,
            5000..=5895 if (frequency - 5000).is_multiple_of(5) => (frequency - 5000) / 5,
            5935 => 2,
            5955..=7115 if (frequency - 5950).is_multiple_of(5) => (frequency - 5950) / 5,
            58320..=69120 if (frequency - 56160).is_multiple_of(2160) => (frequency - 56160) / 2160,
            _ => 0,
        }
    }
    fn ap_compatible(ap: &AccessPoint, profile: &Profile) -> bool {
        let wireless = &profile.value[WIFI];
        if profile.kind != WIFI || profile.ssid != ap.ssid {
            return false;
        }
        let bssid = setting_mac(wireless, "bssid");
        if !bssid.is_empty() && bssid != normalize_mac(&ap.bssid) {
            return false;
        }
        if ap.bssid.is_empty() || ap.mode == 0 {
            return false;
        }
        let mode = wireless["mode"].as_str().unwrap_or("infrastructure");
        if mode == "ap" {
            return false;
        }
        if ap.mode
            != match mode {
                "infrastructure" => 2,
                "adhoc" => 1,
                "ap" => 3,
                "mesh" => 4,
                _ => return false,
            }
        {
            return false;
        }
        let band = wireless["band"].as_str().unwrap_or("");
        if !match band {
            "" => true,
            "bg" => (2400..2500).contains(&ap.frequency),
            "a" => (4900..5900).contains(&ap.frequency),
            "6GHz" | "6ghz" => (5925..=7125).contains(&ap.frequency),
            _ => false,
        } {
            return false;
        }
        let channel = number(wireless, "channel");
        if channel != 0 && channel != frequency_channel(ap.frequency) {
            return false;
        }
        let security = &profile.value[SECURITY];
        let key = security["key-mgmt"].as_str().unwrap_or("");
        if key.is_empty() {
            return profile.security == Security::Open
                && (ap.wpa == OWE_TM
                    || ap.rsn == OWE_TM
                    || (ap.flags & 1 == 0 && ap.wpa == 0 && ap.rsn == 0));
        }
        if key == "none" {
            return ap.flags & 1 != 0 && ap.wpa == 0 && ap.rsn == 0;
        }
        if key == "ieee8021x" {
            if ap.flags & 1 == 0 {
                return false;
            }
            if ap.wpa == 0 {
                return true;
            }
            let ciphers_match = |field: &str, shift: u32| {
                let allowed = strings(security, field);
                allowed.is_empty()
                    || allowed.iter().any(|cipher| {
                        ap.wpa
                            & (match cipher.as_str() {
                                "wep40" => 1u32,
                                "wep104" => 2,
                                _ => 0,
                            } << shift)
                            != 0
                    })
            };
            return ap.wpa & EAP != 0
                && ap.wpa & 3 != 0
                && ap.wpa & 0x30 != 0
                && ciphers_match("pairwise", 0)
                && ciphers_match("group", 4);
        }
        let protocols = strings(security, "proto");
        let pairwise = strings(security, "pairwise");
        let group = strings(security, "group");
        let required = match key {
            "wpa-psk" | "wpa-none" => PSK,
            "sae" => SAE,
            "owe" => OWE | OWE_TM,
            "wpa-eap" => EAP,
            "wpa-eap-suite-b-192" => SUITE_B,
            _ => return false,
        };
        let ciphers = |flags: u32, names: &[String], group: bool| {
            names.is_empty()
                || names.iter().any(|name| {
                    let bit = match name.as_str() {
                        "wep40" => 1,
                        "wep104" => 2,
                        "tkip" => 4,
                        "ccmp" => 8,
                        _ => 0,
                    };
                    flags & if group { bit << 4 } else { bit } != 0
                })
        };
        [("wpa", ap.wpa), ("rsn", ap.rsn)]
            .iter()
            .any(|(protocol, flags)| {
                (protocols.is_empty() || protocols.iter().any(|allowed| allowed == protocol))
                    && (!matches!(key, "sae" | "owe" | "wpa-eap-suite-b-192") || *protocol == "rsn")
                    && (number(security, "pmf") != 3 || *protocol == "rsn")
                    && flags & required != 0
                    && ciphers(*flags, &pairwise, false)
                    && ciphers(*flags, &group, true)
            })
    }

    #[derive(Clone, Debug)]
    struct Entry {
        key: String,
        device: usize,
        ssid: Vec<u8>,
        security: Security,
        aps: Vec<usize>,
        profiles: Vec<usize>,
    }
    fn entry_key(device: &DeviceState, ssid: &[u8], security: Security) -> String {
        if device.wireless {
            json!([device.iface, "wifi", hex(ssid), security.name()]).to_string()
        } else {
            json!([device.iface, "wired"]).to_string()
        }
    }
    fn reconcile(snapshot: &Snapshot) -> Vec<Entry> {
        let mut entries = BTreeMap::<String, Entry>::new();
        for (index, device) in snapshot.devices.iter().enumerate() {
            let mut add = |ssid: &[u8], security| {
                let key = entry_key(device, ssid, security);
                entries.entry(key.clone()).or_insert_with(|| Entry {
                    key,
                    device: index,
                    ssid: ssid.to_vec(),
                    security,
                    aps: Vec::new(),
                    profiles: Vec::new(),
                });
            };
            if !device.wireless {
                add(&[], Security::Open);
            }
            for (ap_index, ap) in device.aps.iter().enumerate() {
                let fallback = device.active.as_ref().and_then(|active| {
                    snapshot
                        .profiles
                        .iter()
                        .find(|profile| profile.path == active.profile)
                });
                let ssid = if ap.ssid.is_empty() && ap.path == device.active_ap {
                    fallback.map_or(ap.ssid.as_slice(), |profile| profile.ssid.as_slice())
                } else {
                    &ap.ssid
                };
                if ssid.is_empty() {
                    continue;
                }
                let key = entry_key(device, ssid, ap.security());
                entries
                    .entry(key.clone())
                    .or_insert_with(|| Entry {
                        key,
                        device: index,
                        ssid: ssid.to_vec(),
                        security: ap.security(),
                        aps: Vec::new(),
                        profiles: Vec::new(),
                    })
                    .aps
                    .push(ap_index);
            }
            for (profile_index, profile) in snapshot.profiles.iter().enumerate() {
                if !device_compatible(device, profile)
                    || (device.wireless && profile.ssid.is_empty())
                {
                    continue;
                }
                let matches: Vec<String> = entries
                    .values()
                    .filter(|entry| {
                        entry.device == index
                            && (!device.wireless
                                || (entry.ssid == profile.ssid
                                    && entry
                                        .aps
                                        .iter()
                                        .any(|ap| ap_compatible(&device.aps[*ap], profile))))
                    })
                    .map(|entry| entry.key.clone())
                    .collect();
                if matches.is_empty() {
                    let ssid = if device.wireless {
                        profile.ssid.as_slice()
                    } else {
                        &[]
                    };
                    let security = if device.wireless {
                        profile.security
                    } else {
                        Security::Open
                    };
                    let key = entry_key(device, ssid, security);
                    entries
                        .entry(key.clone())
                        .or_insert_with(|| Entry {
                            key,
                            device: index,
                            ssid: ssid.to_vec(),
                            security,
                            aps: Vec::new(),
                            profiles: Vec::new(),
                        })
                        .profiles
                        .push(profile_index);
                } else {
                    for key in matches {
                        entries.get_mut(&key).unwrap().profiles.push(profile_index);
                    }
                }
            }
        }
        for entry in entries.values_mut() {
            let active_uuid = snapshot.devices[entry.device]
                .active
                .as_ref()
                .map(|active| active.uuid.as_str())
                .unwrap_or("");
            entry.profiles.sort_by(|left, right| {
                let left = &snapshot.profiles[*left];
                let right = &snapshot.profiles[*right];
                (right.uuid == active_uuid)
                    .cmp(&(left.uuid == active_uuid))
                    .then_with(|| {
                        right.value["connection"]["timestamp"]
                            .as_u64()
                            .unwrap_or(0)
                            .cmp(&left.value["connection"]["timestamp"].as_u64().unwrap_or(0))
                    })
                    .then_with(|| left.uuid.cmp(&right.uuid))
                    .then_with(|| left.path.cmp(&right.path))
            });
        }
        entries.into_values().collect()
    }
    impl Entry {
        fn targets(&self, snapshot: &Snapshot) -> bool {
            let device = &snapshot.devices[self.device];
            let Some(active) = &device.active else {
                return false;
            };
            if !device.wireless {
                return active.kind == WIRED;
            }
            // AP and ActiveConnection notifications are independently ordered.
            // A matching old AP must not authorize acting on a newer profile.
            if !snapshot
                .unique_profile(&active.uuid)
                .is_some_and(|profile| {
                    profile.path == active.profile
                        && profile.kind == WIFI
                        && profile.ssid == self.ssid
                })
            {
                return false;
            }
            if let Some(ap) = device
                .aps
                .iter()
                .find(|ap| ap.path == device.active_ap && !ap.ssid.is_empty())
            {
                return self.ssid == ap.ssid && self.security == ap.security();
            }
            self.profiles
                .iter()
                .any(|profile| snapshot.profiles[*profile].uuid == active.uuid)
        }
        fn connected(&self, snapshot: &Snapshot) -> bool {
            let device = &snapshot.devices[self.device];
            self.targets(snapshot)
                && device.state == 100
                && device.active.as_ref().is_some_and(|active| {
                    active.state == 2
                        && active.devices.contains(&device.path)
                        && (!device.wireless
                            || snapshot
                                .unique_profile(&active.uuid)
                                .is_some_and(|profile| {
                                    profile.path == active.profile
                                        && profile.kind == WIFI
                                        && profile.ssid == self.ssid
                                        && device_compatible(device, profile)
                                        && device
                                            .aps
                                            .iter()
                                            .find(|ap| ap.path == device.active_ap)
                                            .is_none_or(|ap| {
                                                ap.ssid.is_empty() || ap_compatible(ap, profile)
                                            })
                                }))
                })
        }
        fn changing(&self, snapshot: &Snapshot) -> bool {
            let device = &snapshot.devices[self.device];
            self.targets(snapshot) && (device.changing() || device.state == 110)
        }
        fn best_ap<'a>(
            &self,
            snapshot: &'a Snapshot,
            profile: Option<&Profile>,
        ) -> Option<&'a AccessPoint> {
            let device = &snapshot.devices[self.device];
            self.aps
                .iter()
                .map(|index| &device.aps[*index])
                .filter(|ap| profile.is_none_or(|profile| ap_compatible(ap, profile)))
                .min_by(|left, right| {
                    (right.path == device.active_ap)
                        .cmp(&(left.path == device.active_ap))
                        .then_with(|| right.strength.cmp(&left.strength))
                        .then_with(|| left.path.cmp(&right.path))
                })
        }
        fn saved_identity(&self, snapshot: &Snapshot) -> String {
            let device = &snapshot.devices[self.device];
            let mut profiles: Vec<_> = self
                .profiles
                .iter()
                .map(|index| {
                    let profile = &snapshot.profiles[*index];
                    (
                        profile.uuid.clone(),
                        profile.path.clone(),
                        profile.incarnation,
                    )
                })
                .collect();
            profiles.sort();
            json!([snapshot.owner, device.path, device.incarnation, profiles]).to_string()
        }
        fn security_label(&self, snapshot: &Snapshot, ap: Option<&AccessPoint>) -> String {
            let device = &snapshot.devices[self.device];
            let profiles: Vec<_> = self
                .profiles
                .iter()
                .map(|index| &snapshot.profiles[*index])
                .filter(|profile| ap.is_none_or(|ap| ap_compatible(ap, profile)))
                .collect();
            let labels: BTreeSet<_> = profiles.iter().map(|profile| profile.label()).collect();
            let saved_label = profiles
                .iter()
                .find(|profile| {
                    device
                        .active
                        .as_ref()
                        .is_some_and(|active| active.uuid == profile.uuid)
                })
                .map(|profile| profile.label())
                .or_else(|| {
                    if labels.len() == 1 {
                        labels.first().cloned()
                    } else {
                        None
                    }
                });
            let Some(ap) = ap else {
                return saved_label.unwrap_or_else(|| self.security.label().to_owned());
            };
            match ap.security() {
                Security::Psk => if ap.rsn & PSK != 0 { "WPA2" } else { "WPA" }.to_owned(),
                Security::Enterprise => if ap.rsn & SUITE_B != 0 {
                    "WPA3 Enterprise"
                } else if ap.rsn & EAP != 0 {
                    "WPA2 Enterprise"
                } else if ap.wpa & EAP != 0 {
                    "WPA Enterprise"
                } else {
                    "Enterprise"
                }
                .to_owned(),
                Security::Wep => saved_label
                    .filter(|label| matches!(label.as_str(), "Dynamic WEP" | "LEAP"))
                    .unwrap_or_else(|| "WEP".to_owned()),
                security => security.label().to_owned(),
            }
        }
        fn identity(&self, snapshot: &Snapshot) -> Option<Identity> {
            if !self.connected(snapshot) {
                return None;
            }
            let device = &snapshot.devices[self.device];
            let active = device.active.as_ref()?;
            Some(Identity {
                interface: device.iface.clone(),
                ssid: if device.wireless {
                    ssid_text(&self.ssid)
                } else {
                    String::new()
                },
                profile_uuid: active.uuid.clone(),
                bssid: device
                    .aps
                    .iter()
                    .find(|ap| device.wireless && ap.path == device.active_ap)
                    .map(|ap| ap.bssid.clone())
                    .unwrap_or_default(),
                active_path: active.path.clone(),
            })
        }
        fn details(&self, snapshot: &Snapshot) -> Value {
            let device = &snapshot.devices[self.device];
            let Some(identity) = self.identity(snapshot) else {
                return self.profiles.first().map(|index| { let profile = &snapshot.profiles[*index]; json!({
                    "ssid": if device.wireless { ssid_text(&self.ssid) } else { String::new() }, "interface": device.iface,
                    "profileId": profile.id, "profileUuid": profile.uuid, "identity": null,
                }) }).unwrap_or(Value::Null);
            };
            let addresses = |config: &Value| {
                config["AddressData"]
                    .as_array()
                    .map(|addresses| {
                        addresses
                            .iter()
                            .filter_map(|address| {
                                Some(format!(
                                    "{}/{}",
                                    address["address"].as_str()?,
                                    address["prefix"].as_u64()?
                                ))
                            })
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default()
            };
            let nameservers = |config: &Value| {
                let mut values: Vec<String> = config["NameserverData"]
                    .as_array()
                    .map(|servers| {
                        servers
                            .iter()
                            .filter_map(|server| server["address"].as_str().map(str::to_owned))
                            .collect()
                    })
                    .unwrap_or_default();
                if values.is_empty()
                    && let Some(servers) = config["Nameservers"].as_array()
                {
                    for server in servers {
                        if let Some(server) = server.as_u64() {
                            values.push(
                                std::net::Ipv4Addr::from((server as u32).to_ne_bytes()).to_string(),
                            );
                        } else if let Some(octets) = server.as_array() {
                            let bytes: Vec<u8> = octets
                                .iter()
                                .filter_map(|value| {
                                    value.as_u64().and_then(|byte| u8::try_from(byte).ok())
                                })
                                .collect();
                            if let Ok(bytes) = <[u8; 16]>::try_from(bytes) {
                                values.push(std::net::Ipv6Addr::from(bytes).to_string());
                            }
                        }
                    }
                }
                values
            };
            let mut dns = nameservers(&device.ip4);
            dns.extend(nameservers(&device.ip6));
            let mut seen = BTreeSet::new();
            dns.retain(|address| seen.insert(address.clone()));
            let ap = device
                .aps
                .iter()
                .find(|ap| device.wireless && ap.path == device.active_ap);
            let frequency = ap.map_or(0, |ap| ap.frequency);
            let rate = number(
                &device.link,
                if device.wireless { "Bitrate" } else { "Speed" },
            ) as f64
                / if device.wireless { 1000.0 } else { 1.0 };
            let mut details = serde_json::to_value(&identity).unwrap();
            let fields = json!({ "identity": identity, "profileId": device.active.as_ref().map(|active| &active.id),
                "ipv4": addresses(&device.ip4), "ipv6": addresses(&device.ip6),
                "prefix": device.ip4["AddressData"].get(0).and_then(|address| address.get("prefix")).cloned().unwrap_or(json!("")),
                "gateway4": text(&device.ip4, "Gateway"), "gateway6": text(&device.ip6, "Gateway"), "dns": dns,
                "mac": text(&device.link, "HwAddress"),
                "frequency": if frequency == 0 { String::new() } else { format!("{frequency} MHz") },
                "band": if frequency == 0 { "" } else if frequency >= 59400 { "60 GHz" } else if frequency >= 5925 { "6 GHz" } else if frequency >= 4900 { "5 GHz" } else { "2.4 GHz" },
                "channel": if frequency == 0 { String::new() } else { frequency_channel(frequency).to_string() },
                "mode": ap.map_or("", |ap| match ap.mode { 1 => "Ad-Hoc", 2 => "Infrastructure", 3 => "Access point", 4 => "Mesh", _ => "Unknown" }),
                "rate": if rate == 0.0 { String::new() } else { format!("{} Mbit/s", (rate * 10.0).round() / 10.0) },
            });
            details
                .as_object_mut()
                .unwrap()
                .extend(fields.as_object().unwrap().clone());
            details
        }
    }
    #[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Identity {
        interface: String,
        ssid: String,
        profile_uuid: String,
        bssid: String,
        active_path: String,
    }

    fn profile_shareable(
        snapshot: &Snapshot,
        entry: &Entry,
        profile: &Profile,
        busy: bool,
    ) -> bool {
        let device = &snapshot.devices[entry.device];
        device.wireless
            && entry.security.shareable()
            && profile.security.shareable()
            && profile.kind == WIFI
            && device_compatible(device, profile)
            && snapshot
                .unique_profile(&profile.uuid)
                .is_some_and(|current| current.path == profile.path)
            && !entry.changing(snapshot)
            && !busy
    }
    fn state_json(
        snapshot: &Snapshot,
        entries: &[Entry],
        busy: &BTreeMap<String, String>,
    ) -> Value {
        let mut identities = serde_json::Map::new();
        let mut details = serde_json::Map::new();
        let rows: Vec<_> = entries.iter().map(|entry| {
            let device = &snapshot.devices[entry.device];
            let connected = entry.connected(snapshot);
            let device_busy = busy.contains_key(&device.path);
            let changing = entry.changing(snapshot) || busy.get(&device.path).is_some_and(|key| key == &entry.key);
            let known = !entry.profiles.is_empty();
            let ap = entry.best_ap(snapshot, None);
            let radios = !device.wireless || (snapshot.wifi_enabled && snapshot.wifi_hardware_enabled);
            let supports_creation = !device.wireless || (ap.is_some_and(|ap| ap.mode == 2) && entry.security != Security::Enterprise && entry.security != Security::Unknown);
            let can_activate = !connected && !changing && !device_busy && radios && (known || supports_creation);
            let accepts_password = device.wireless && entry.security.password() && entry.profiles.len() <= 1
                && entry.profiles.first().is_none_or(|index| snapshot.profiles[*index].security.password());
            let profiles: Vec<_> = entry.profiles.iter().map(|index| {
                let profile = &snapshot.profiles[*index];
                let selected = !connected || device.active.as_ref().is_some_and(|active| active.profile == profile.path && active.uuid == profile.uuid);
                json!({ "id": profile.id, "uuid": profile.uuid, "canShare": selected && profile_shareable(snapshot, entry, profile, device_busy) })
            }).collect();
            let can_share = profiles.iter().any(|profile| profile["canShare"] == true);
            let mut uuids: Vec<_> = entry.profiles.iter().map(|index| snapshot.profiles[*index].uuid.clone()).collect(); uuids.sort();
            let profile_stamp = json!(uuids).to_string();
            let saved_identity = entry.saved_identity(snapshot);
            let saved_stamp = if known && !connected { json!([entry.key, device.iface,
                if device.wireless { ssid_text(&entry.ssid) } else { String::new() }, device.wireless, entry.security,
                changing, profile_stamp, saved_identity]).to_string() } else { String::new() };
            let credential_stamp = json!([entry.key, saved_identity, profile_stamp, accepts_password,
                snapshot.wifi_enabled, snapshot.wifi_hardware_enabled]).to_string();
            identities.insert(entry.key.clone(), json!(entry.identity(snapshot)));
            let information = entry.details(snapshot);
            if information.is_object() { details.insert(entry.key.clone(), information); }
            let mut contexts: Vec<_> = entry.profiles.iter().map(|index| {
                let profile = &snapshot.profiles[*index]; (profile.uuid.clone(), profile.context().to_string())
            }).collect();
            contexts.sort();
            let mut stamp = std::collections::hash_map::DefaultHasher::new();
            (entry.key.as_str(), saved_identity.as_str(), contexts, snapshot.wifi_enabled, snapshot.wifi_hardware_enabled).hash(&mut stamp);
            let selection_stamp = format!("{:016x}", stamp.finish());
            json!({ "key": entry.key, "name": if device.wireless { ssid_text(&entry.ssid) } else { device.iface.clone() },
                "ssid": if device.wireless { ssid_text(&entry.ssid) } else { String::new() }, "interface": device.iface,
                "kind": if device.wireless { "WI-FI" } else { "WIRED" }, "wireless": device.wireless,
                "connected": connected, "changing": changing, "known": known,
                "strength": if device.wireless { ap.map(|ap| ap.strength) } else { Some(0) },
                "security": if device.wireless { entry.security_label(snapshot, ap) } else { "Wired".to_owned() },
                "securityType": entry.security, "profiles": profiles, "savedIdentity": saved_identity,
                "canActivate": can_activate, "canDisconnect": connected && !changing && !device_busy,
                "canForget": known && !changing && !device_busy, "canShare": can_share,
                "canShowInfo": connected || known, "acceptsPassword": accepts_password,
                "requiresPassword": !known && accepts_password, "profileStamp": profile_stamp,
                "savedInfoStamp": saved_stamp, "credentialStamp": credential_stamp, "selectionStamp": selection_stamp,
            })
        }).collect();
        json!({ "type": "state", "rows": rows, "available": !snapshot.owner.is_empty() && !snapshot.devices.is_empty(),
            "wifiAvailable": !snapshot.owner.is_empty() && snapshot.devices.iter().any(|device| device.wireless),
            "wifiEnabled": snapshot.wifi_enabled, "wifiHardwareEnabled": snapshot.wifi_hardware_enabled,
            "identities": identities, "details": details })
    }

    struct Secret(String);
    impl Drop for Secret {
        fn drop(&mut self) {
            // Request-local credentials are not part of the reconciled cache.
            unsafe {
                self.0.as_bytes_mut().fill(0);
            }
        }
    }
    fn set_password(settings: &mut Settings, password: &str) -> NetResult<()> {
        if password.is_empty() || password.contains('\0') {
            return Err(Failure::authentication("Enter a valid network password."));
        }
        let value = settings_json(settings);
        let security = value[SECURITY]["key-mgmt"].as_str().unwrap_or("");
        if value.get("802-1x").is_some() {
            return Err(Failure::new(
                "Configure authentication for this network in a network connection editor.",
            ));
        }
        match security {
            "none" => {
                let raw = (matches!(password.len(), 5 | 13)
                    && password.bytes().all(|byte| (0x20..=0x7e).contains(&byte)))
                    || (matches!(password.len(), 10 | 26)
                        && password.bytes().all(|byte| byte.is_ascii_hexdigit()));
                if !raw && password.len() > 64 {
                    return Err(Failure::authentication(
                        "Enter a valid WEP key or passphrase.",
                    ));
                }
                let index = number(&value[SECURITY], "wep-tx-keyidx");
                if index > 3 {
                    return Err(Failure::authentication("Enter a valid network password."));
                }
                insert(
                    settings,
                    SECURITY,
                    "wep-key-type",
                    if raw { 1u32 } else { 2u32 },
                );
                insert(
                    settings,
                    SECURITY,
                    &format!("wep-key{index}"),
                    password.to_owned(),
                );
                insert(settings, SECURITY, "wep-key-flags", 0u32);
            }
            "wpa-psk" | "wpa-none" | "sae" => {
                if security != "sae"
                    && !((8..=63).contains(&password.len())
                        || (password.len() == 64
                            && password.bytes().all(|byte| byte.is_ascii_hexdigit())))
                {
                    return Err(Failure::authentication(
                        "Enter a valid WPA password (8–63 characters or 64 hexadecimal digits).",
                    ));
                }
                insert(settings, SECURITY, "psk", password.to_owned());
                insert(settings, SECURITY, "psk-flags", 0u32);
            }
            _ => {
                return Err(Failure::new(
                    "Configure authentication for this network in a network connection editor.",
                ));
            }
        }
        Ok(())
    }
    fn random_uuid() -> NetResult<String> {
        let mut bytes = [0u8; 16];
        if unsafe { libc::getrandom(bytes.as_mut_ptr().cast(), bytes.len(), libc::GRND_NONBLOCK) }
            != bytes.len() as isize
        {
            return Err(Failure::new(
                "The connection could not be started. Try again.",
            ));
        }
        bytes[6] = (bytes[6] & 15) | 0x40;
        bytes[8] = (bytes[8] & 63) | 0x80;
        let value = hex(&bytes);
        Ok(format!(
            "{}-{}-{}-{}-{}",
            &value[..8],
            &value[8..12],
            &value[12..16],
            &value[16..20],
            &value[20..]
        ))
    }
    fn new_connection(
        snapshot: &Snapshot,
        entry: &Entry,
        password: Option<&str>,
    ) -> NetResult<(String, Settings)> {
        let device = &snapshot.devices[entry.device];
        let mut settings = Settings::new();
        let uuid = random_uuid()?;
        insert(
            &mut settings,
            "connection",
            "id",
            if device.wireless {
                ssid_text(&entry.ssid)
            } else {
                format!("Wired {}", device.iface)
            },
        );
        insert(&mut settings, "connection", "uuid", uuid.clone());
        insert(
            &mut settings,
            "connection",
            "type",
            if device.wireless { WIFI } else { WIRED }.to_owned(),
        );
        insert(
            &mut settings,
            "connection",
            "interface-name",
            device.iface.clone(),
        );
        insert(&mut settings, "connection", "autoconnect", true);
        insert(&mut settings, "ipv4", "method", "auto".to_owned());
        insert(&mut settings, "ipv6", "method", "auto".to_owned());
        if !device.wireless {
            if password.is_some() {
                return Err(Failure::new(
                    "Configure wired authentication in a network connection editor.",
                ));
            }
            settings.insert(WIRED.to_owned(), PropMap::new());
        } else {
            let ap = entry.best_ap(snapshot, None).ok_or_else(|| {
                Failure::new("This network is no longer in range. Refresh and try again.")
            })?;
            if ap.mode != 2 {
                return Err(Failure::new(
                    "Configure this network mode in a network connection editor.",
                ));
            }
            if entry.ssid.is_empty() || entry.ssid.len() > 32 {
                return Err(Failure::new(
                    "The network settings are not supported. Configure a supported saved profile first.",
                ));
            }
            insert(&mut settings, WIFI, "ssid", entry.ssid.clone());
            insert(&mut settings, WIFI, "mode", "infrastructure".to_owned());
            match entry.security {
                Security::Open => (),
                Security::Wep => {
                    insert(&mut settings, SECURITY, "key-mgmt", "none".to_owned());
                    insert(&mut settings, SECURITY, "auth-alg", "open".to_owned());
                }
                Security::Psk => insert(&mut settings, SECURITY, "key-mgmt", "wpa-psk".to_owned()),
                Security::Sae | Security::Owe => {
                    insert(
                        &mut settings,
                        SECURITY,
                        "key-mgmt",
                        if entry.security == Security::Sae {
                            "sae"
                        } else {
                            "owe"
                        }
                        .to_owned(),
                    );
                    insert(&mut settings, SECURITY, "pmf", 3u32);
                }
                _ => {
                    return Err(Failure::new(
                        "Create a suitable saved authentication profile before connecting.",
                    ));
                }
            }
            if entry.security.password() {
                set_password(
                    &mut settings,
                    password.ok_or_else(|| {
                        Failure::authentication("A password is required for this network.")
                    })?,
                )?;
            } else if password.is_some() {
                return Err(Failure::new(
                    "This network does not accept an inline password. Configure its saved profile in a network connection editor.",
                ));
            }
        }
        Ok((uuid, settings))
    }

    #[derive(Clone, Debug)]
    struct ProfileGuard {
        path: String,
        incarnation: u64,
        uuid: String,
        context: Value,
    }
    impl ProfileGuard {
        fn capture(profile: &Profile) -> Self {
            Self {
                path: profile.path.clone(),
                incarnation: profile.incarnation,
                uuid: profile.uuid.clone(),
                context: profile.context(),
            }
        }
        fn validate<'a>(&self, snapshot: &'a Snapshot) -> NetResult<&'a Profile> {
            snapshot
                .unique_profile(&self.uuid)
                .filter(|profile| {
                    profile.path == self.path
                        && profile.incarnation == self.incarnation
                        && profile.context() == self.context
                })
                .ok_or_else(Failure::changed)
        }
    }
    #[derive(Clone, Debug)]
    struct EntryGuard {
        key: String,
        owner: String,
        device: String,
        incarnation: u64,
        iface: String,
        wireless: bool,
        ssid: Vec<u8>,
        security: Security,
        profiles: Vec<ProfileGuard>,
        active: Option<String>,
        active_uuid: Option<String>,
    }
    impl EntryGuard {
        fn capture(entry: &Entry, snapshot: &Snapshot) -> Self {
            let device = &snapshot.devices[entry.device];
            Self {
                key: entry.key.clone(),
                owner: snapshot.owner.clone(),
                device: device.path.clone(),
                incarnation: device.incarnation,
                iface: device.iface.clone(),
                wireless: device.wireless,
                ssid: entry.ssid.clone(),
                security: entry.security,
                profiles: entry
                    .profiles
                    .iter()
                    .map(|index| ProfileGuard::capture(&snapshot.profiles[*index]))
                    .collect(),
                active: device.active.as_ref().map(|active| active.path.clone()),
                active_uuid: device.active.as_ref().map(|active| active.uuid.clone()),
            }
        }
        fn entry<'a>(&self, snapshot: &Snapshot, entries: &'a [Entry]) -> NetResult<&'a Entry> {
            let entry = entries
                .iter()
                .find(|entry| entry.key == self.key)
                .ok_or_else(Failure::changed)?;
            let device = &snapshot.devices[entry.device];
            if snapshot.owner != self.owner
                || device.path != self.device
                || device.incarnation != self.incarnation
                || device.iface != self.iface
                || device.wireless != self.wireless
                || entry.ssid != self.ssid
                || entry.security != self.security
            {
                return Err(Failure::changed());
            }
            Ok(entry)
        }
        fn device<'a>(&self, snapshot: &'a Snapshot) -> NetResult<&'a DeviceState> {
            snapshot
                .devices
                .iter()
                .find(|device| {
                    snapshot.owner == self.owner
                        && device.path == self.device
                        && device.incarnation == self.incarnation
                        && device.iface == self.iface
                        && device.wireless == self.wireless
                })
                .ok_or_else(Failure::unavailable)
        }
        fn current_active(&self, snapshot: &Snapshot) -> NetResult<()> {
            let current = self
                .device(snapshot)?
                .active
                .as_ref()
                .map(|active| &active.path);
            if current.is_some()
                && (current != self.active.as_ref()
                    || self
                        .device(snapshot)?
                        .active
                        .as_ref()
                        .map(|active| &active.uuid)
                        != self.active_uuid.as_ref())
            {
                return Err(Failure::changed());
            }
            Ok(())
        }
    }
    #[derive(Clone, Debug)]
    struct SecretGuard {
        entry: EntryGuard,
        profile: ProfileGuard,
        identity: Option<Identity>,
        saved_identity: String,
        radios: (bool, bool),
        security: Security,
        hidden: bool,
        key_index: u32,
    }
    impl SecretGuard {
        fn capture(
            snapshot: &Snapshot,
            entries: &[Entry],
            key: &str,
            expected: Option<Identity>,
            uuid: Option<&str>,
            busy: &BTreeMap<String, String>,
        ) -> NetResult<Self> {
            let entry = entries
                .iter()
                .find(|entry| entry.key == key)
                .ok_or_else(Failure::changed)?;
            let device = &snapshot.devices[entry.device];
            let profile = if let Some(expected) = &expected {
                if entry.identity(snapshot).as_ref() != Some(expected) {
                    return Err(Failure::new(
                        "The active connection changed. Reopen network information and try again.",
                    ));
                }
                let active = device.active.as_ref().ok_or_else(Failure::changed)?;
                snapshot
                    .unique_profile(&expected.profile_uuid)
                    .filter(|profile| profile.path == active.profile)
            } else {
                uuid.filter(|uuid| !uuid.is_empty())
                    .and_then(|uuid| snapshot.unique_profile(uuid))
                    .filter(|profile| {
                        entry
                            .profiles
                            .iter()
                            .any(|index| snapshot.profiles[*index].path == profile.path)
                    })
            }
            .ok_or_else(|| Failure::new("The selected saved profile is no longer available."))?;
            if !profile_shareable(snapshot, entry, profile, busy.contains_key(&device.path)) {
                return Err(Failure::new(
                    "Sharing credentials is not supported for this network.",
                ));
            }
            let guard = Self {
                entry: EntryGuard::capture(entry, snapshot),
                profile: ProfileGuard::capture(profile),
                identity: expected,
                saved_identity: entry.saved_identity(snapshot),
                radios: (snapshot.wifi_enabled, snapshot.wifi_hardware_enabled),
                security: profile.security,
                hidden: profile.hidden(),
                key_index: profile.key_index(),
            };
            guard.validate(snapshot, entries, busy)?;
            Ok(guard)
        }
        fn validate(
            &self,
            snapshot: &Snapshot,
            entries: &[Entry],
            busy: &BTreeMap<String, String>,
        ) -> NetResult<()> {
            let entry = self.entry.entry(snapshot, entries)?;
            let device = &snapshot.devices[entry.device];
            let profile = self.profile.validate(snapshot)?;
            if !profile_shareable(snapshot, entry, profile, busy.contains_key(&device.path))
                || profile.ssid != entry.ssid
                || profile.security != self.security
                || profile.hidden() != self.hidden
                || profile.key_index() != self.key_index
                || self.radios != (snapshot.wifi_enabled, snapshot.wifi_hardware_enabled)
            {
                return Err(Failure::changed());
            }
            if let Some(identity) = &self.identity {
                if entry.identity(snapshot).as_ref() != Some(identity)
                    || !device.active.as_ref().is_some_and(|active| {
                        active.profile == self.profile.path && active.uuid == self.profile.uuid
                    })
                {
                    return Err(Failure::new(
                        "The active connection changed. Reopen network information and try again.",
                    ));
                }
            } else if entry.connected(snapshot)
                || entry.changing(snapshot)
                || entry.identity(snapshot).is_some()
                || entry.saved_identity(snapshot) != self.saved_identity
            {
                return Err(Failure::changed());
            }
            Ok(())
        }
        fn credentials(&self, password: &str) -> Value {
            json!({ "password": password, "authentication": if self.security == Security::Open { "nopass" }
                else if self.security == Security::Wep { "WEP" } else { "WPA" }, "hidden": self.hidden })
        }
    }

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct WireRequest {
        #[serde(rename = "expectedStamp")]
        expected_stamp: Option<String>,
        #[serde(default)]
        id: Option<Value>,
        method: String,
        #[serde(default)]
        args: Vec<Value>,
    }
    enum Method {
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
        fn parse(method: &str, mut args: Vec<Value>) -> NetResult<Self> {
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
        fn key(&self) -> Option<&str> {
            match self {
                Self::Activate { key, .. }
                | Self::Disconnect(key)
                | Self::Forget(key)
                | Self::ReadSecret(key, _)
                | Self::ReadSavedSecret(key, _) => Some(key),
                Self::ToggleWifi => None,
            }
        }
        fn mutation(&self) -> bool {
            matches!(
                self,
                Self::Activate { .. } | Self::Disconnect(_) | Self::Forget(_)
            )
        }
    }
    fn valid_id(id: &Value) -> bool {
        id.as_str()
            .is_some_and(|id| !id.is_empty() && id.len() <= 128)
            || id.as_u64().is_some_and(|id| id <= 9_007_199_254_740_991)
    }
    struct ActivationWatch {
        path: String,
        requested_uuid: String,
        profile: Option<ProfileGuard>,
        added_profile: Option<String>,
        seen_target: bool,
        seen_progress: bool,
        seen_active: bool,
    }
    enum Phase {
        Start(Method),
        SecretStart(SecretGuard),
        AwaitSecret(SecretGuard),
        SecretFinish(SecretGuard, Secret),
        AwaitUpdate(ProfileGuard),
        ActivateSaved(ProfileGuard),
        AwaitActivation {
            requested_uuid: String,
            profile: Option<ProfileGuard>,
            added: bool,
        },
        WaitActivation(ActivationWatch),
        AwaitDisconnect {
            active: Option<String>,
            forget: Option<VecDeque<ProfileGuard>>,
        },
        VerifyDisconnect {
            active: Option<String>,
            forget: Option<VecDeque<ProfileGuard>>,
        },
        DeleteNext(VecDeque<ProfileGuard>),
        AwaitDelete(ProfileGuard, VecDeque<ProfileGuard>),
        VerifyDelete(ProfileGuard, VecDeque<ProfileGuard>),
        AwaitToggle(bool),
        VerifyToggle(bool),
    }
    struct Operation {
        id: Value,
        owner: String,
        guard: Option<EntryGuard>,
        mutation: bool,
        deadline: Instant,
        after_refresh: u64,
        phase: Phase,
    }
    enum PendingKind {
        Match,
        Owner,
        Objects(u64),
        Settings(u64, String),
        Operation(String),
        Scan(String, u64),
    }
    struct Pending {
        deadline: Instant,
        kind: PendingKind,
    }
    struct Refresh {
        id: u64,
        revision: u64,
        objects: Option<Objects>,
        settings: BTreeMap<String, Settings>,
        remaining: usize,
    }
    struct Engine {
        owner: String,
        snapshot: Snapshot,
        entries: Vec<Entry>,
        incarnations: BTreeMap<String, u64>,
        incarnation_serial: u64,
        reasons: BTreeMap<String, u32>,
        operations: BTreeMap<String, Operation>,
        pending: HashMap<u32, Pending>,
        busy: BTreeMap<String, String>,
        opened: bool,
        scans: BTreeMap<String, (u64, Instant)>,
        revision: u64,
        refresh_serial: u64,
        completed_refresh: u64,
        refresh: Option<Refresh>,
        dirty: bool,
        refresh_at: Instant,
        subscriptions: usize,
        owner_pending: bool,
        output: VecDeque<Value>,
        last_state: Value,
        stopped: bool,
    }
    impl Engine {
        fn new(now: Instant) -> Self {
            let mut engine = Self {
                owner: String::new(),
                snapshot: Snapshot::default(),
                entries: Vec::new(),
                incarnations: BTreeMap::new(),
                incarnation_serial: 0,
                reasons: BTreeMap::new(),
                operations: BTreeMap::new(),
                pending: HashMap::new(),
                busy: BTreeMap::new(),
                opened: false,
                scans: BTreeMap::new(),
                revision: 0,
                refresh_serial: 0,
                completed_refresh: 0,
                refresh: None,
                dirty: false,
                refresh_at: now,
                subscriptions: 0,
                owner_pending: false,
                output: VecDeque::new(),
                last_state: Value::Null,
                stopped: false,
            };
            engine.publish();
            engine
        }
        fn publish(&mut self) {
            if self.stopped {
                return;
            }
            let state = state_json(&self.snapshot, &self.entries, &self.busy);
            if state != self.last_state {
                self.last_state = state.clone();
                // Unsolicited snapshots are replaceable; request replies are not.
                self.output.retain(|value| value["type"] != "state");
                self.output.push_back(state);
            }
        }
        fn reply(&mut self, id: Value, result: NetResult<Value>) {
            if self.stopped {
                return;
            }
            self.output.push_back(match result {
                Ok(result) => json!({ "id": id, "result": result }),
                Err(error) => json!({ "id": id, "error": { "message": error.message, "code": error.code.to_ascii_lowercase() } }),
            });
        }
        fn mark_dirty(&mut self, now: Instant) {
            self.dirty = true;
            self.refresh_at = now;
        }
        fn send(
            &mut self,
            bus: &impl Sender,
            message: Message,
            kind: PendingKind,
            deadline: Instant,
        ) -> NetResult<()> {
            if self.stopped {
                return Err(Failure::cancelled());
            }
            let serial = bus.send(message).map_err(|_| Failure::unavailable())?;
            self.pending.insert(serial, Pending { deadline, kind });
            Ok(())
        }
        fn bootstrap(&mut self, bus: &impl Sender, now: Instant) -> NetResult<()> {
            for rule in [
                "type='signal',sender='org.freedesktop.DBus',path='/org/freedesktop/DBus',interface='org.freedesktop.DBus',member='NameOwnerChanged',arg0='org.freedesktop.NetworkManager'",
                "type='signal',sender='org.freedesktop.NetworkManager',path_namespace='/org/freedesktop'",
            ] {
                self.send(
                    bus,
                    call(DBUS, DBUS_PATH, DBUS, "AddMatch")?.append1(rule),
                    PendingKind::Match,
                    now + READ_TIME,
                )?;
                self.subscriptions += 1;
            }
            self.send(
                bus,
                call(DBUS, DBUS_PATH, DBUS, "GetNameOwner")?.append1(NM),
                PendingKind::Owner,
                now + READ_TIME,
            )?;
            self.owner_pending = true;
            Ok(())
        }
        fn enqueue(&mut self, line: &[u8], now: Instant) {
            if self.stopped {
                return;
            }
            if self.output.len() >= 128 {
                self.stop();
                return;
            }
            let request = if line.len() <= REQUEST_LIMIT {
                serde_json::from_slice::<WireRequest>(line).ok()
            } else {
                None
            };
            let Some(request) = request else {
                self.reply(Value::Null, Err(Failure::invalid()));
                return;
            };
            let id = request.id.filter(valid_id);
            if request.method == "setOpened" {
                if request.args.len() != 1 || !request.args[0].is_boolean() {
                    self.reply(id.unwrap_or(Value::Null), Err(Failure::invalid()));
                    return;
                }
                self.opened = request.args[0] == true;
                if !self.opened {
                    self.stop_scans();
                    // Already-sent calls cannot be undone; discard their continuations.
                    let pending: Vec<_> = self.operations.keys().cloned().collect();
                    for key in pending {
                        self.finish(&key, Err(Failure::cancelled()));
                    }
                }
                if let Some(id) = id {
                    self.reply(id, Ok(Value::Null));
                }
                return;
            }
            if request.method == "cancel" {
                if request.args.len() != 1 || !valid_id(&request.args[0]) {
                    self.reply(id.unwrap_or(Value::Null), Err(Failure::invalid()));
                    return;
                }
                self.finish(&request.args[0].to_string(), Err(Failure::cancelled()));
                if let Some(id) = id {
                    self.reply(id, Ok(Value::Null));
                }
                return;
            }
            let Some(id) = id else {
                self.reply(Value::Null, Err(Failure::invalid()));
                return;
            };
            let operation_key = id.to_string();
            if self.operations.contains_key(&operation_key) {
                self.reply(
                    id,
                    Err(Failure {
                        message: "This network request ID is already in use.",
                        code: "DUPLICATE_ID",
                    }),
                );
                return;
            }
            if self.operations.len() >= MAX_OPERATIONS {
                self.reply(
                    id,
                    Err(Failure {
                        message: "Too many network requests are pending.",
                        code: "BUSY",
                    }),
                );
                return;
            }
            let method = match Method::parse(&request.method, request.args) {
                Ok(method) => method,
                Err(error) => {
                    self.reply(id, Err(error));
                    return;
                }
            };
            let guard = if let Some(key) = method.key() {
                let Some(entry) = self.entries.iter().find(|entry| entry.key == key) else {
                    self.reply(
                        id,
                        Err(Failure::new(
                            "The selected network is no longer available. Refresh and try again.",
                        )),
                    );
                    return;
                };
                let sensitive = matches!(
                    &method,
                    Method::Forget(_)
                        | Method::ReadSecret(..)
                        | Method::ReadSavedSecret(..)
                        | Method::Activate {
                            password: Some(_),
                            ..
                        }
                );
                if sensitive {
                    let stamp = self.last_state["rows"]
                        .as_array()
                        .and_then(|rows| rows.iter().find(|row| row["key"] == key))
                        .and_then(|row| row["selectionStamp"].as_str());
                    if stamp.is_none() || stamp != request.expected_stamp.as_deref() {
                        self.reply(id, Err(Failure::changed()));
                        return;
                    }
                }
                Some(EntryGuard::capture(entry, &self.snapshot))
            } else {
                None
            };
            if method.mutation()
                && let Some(guard) = &guard
                && self.busy.contains_key(&guard.device)
            {
                self.reply(
                    id,
                    Err(Failure {
                        message: "Wait for the current network operation to finish.",
                        code: "BUSY",
                    }),
                );
                return;
            }
            if matches!(method, Method::ToggleWifi)
                && self.operations.values().any(|operation| {
                    matches!(
                        operation.phase,
                        Phase::Start(Method::ToggleWifi)
                            | Phase::AwaitToggle(_)
                            | Phase::VerifyToggle(_)
                    )
                })
            {
                self.reply(
                    id,
                    Err(Failure {
                        message: "Wait for the current network operation to finish.",
                        code: "BUSY",
                    }),
                );
                return;
            }
            let mutation = method.mutation();
            let phase = match &method {
                Method::ReadSecret(key, identity) => SecretGuard::capture(
                    &self.snapshot,
                    &self.entries,
                    key,
                    Some(identity.clone()),
                    None,
                    &self.busy,
                )
                .map(Phase::SecretStart),
                Method::ReadSavedSecret(key, uuid) => SecretGuard::capture(
                    &self.snapshot,
                    &self.entries,
                    key,
                    None,
                    Some(uuid),
                    &self.busy,
                )
                .map(Phase::SecretStart),
                _ => Ok(Phase::Start(method)),
            };
            let phase = match phase {
                Ok(phase) => phase,
                Err(error) => {
                    self.reply(id, Err(error));
                    return;
                }
            };
            if matches!(&phase, Phase::Start(method) if method.mutation())
                && let Some(guard) = &guard
            {
                self.busy.insert(guard.device.clone(), guard.key.clone());
            }
            self.operations.insert(
                operation_key,
                Operation {
                    id,
                    owner: self.owner.clone(),
                    guard,
                    mutation,
                    deadline: now + REQUEST_TIME,
                    after_refresh: self.refresh_serial + 1,
                    phase,
                },
            );
            self.mark_dirty(now);
            self.publish();
        }
        fn finish(&mut self, key: &str, result: NetResult<Value>) {
            let Some(operation) = self.operations.remove(key) else {
                return;
            };
            self.pending.retain(
                |_, pending| !matches!(&pending.kind, PendingKind::Operation(id) if id == key),
            );
            if operation.mutation
                && let Some(guard) = &operation.guard
                && self.busy.get(&guard.device) == Some(&guard.key)
            {
                self.busy.remove(&guard.device);
            }
            self.reply(operation.id.clone(), result);
            self.publish();
        }
        fn stop_scans(&mut self) {
            self.scans.clear();
            self.pending
                .retain(|_, pending| !matches!(pending.kind, PendingKind::Scan(..)));
        }
        fn stop(&mut self) {
            self.stopped = true;
            self.opened = false;
            self.operations.clear();
            self.pending.clear();
            self.busy.clear();
            self.scans.clear();
            self.refresh = None;
            self.output.clear();
            self.entries.clear();
            self.snapshot = Snapshot::default();
        }
        fn set_owner(&mut self, owner: String, now: Instant) {
            if self.owner == owner {
                return;
            }
            let keys: Vec<_> = self.operations.keys().cloned().collect();
            for key in keys {
                self.finish(&key, Err(Failure::unavailable()));
            }
            self.pending
                .retain(|_, pending| matches!(pending.kind, PendingKind::Match));
            self.owner = owner;
            self.owner_pending = false;
            self.refresh = None;
            self.incarnations.clear();
            self.reasons.clear();
            self.snapshot = Snapshot::default();
            self.entries.clear();
            self.stop_scans();
            self.revision += 1;
            self.mark_dirty(now);
            self.publish();
        }
        fn start_refresh(&mut self, bus: &impl Sender, now: Instant) -> NetResult<()> {
            self.refresh_serial += 1;
            let id = self.refresh_serial;
            self.send(
                bus,
                call(
                    &self.owner,
                    OBJECT_ROOT,
                    OBJECT_MANAGER,
                    "GetManagedObjects",
                )?,
                PendingKind::Objects(id),
                now + READ_TIME,
            )?;
            self.refresh = Some(Refresh {
                id,
                revision: self.revision,
                objects: None,
                settings: BTreeMap::new(),
                remaining: 0,
            });
            Ok(())
        }
        fn complete_refresh(&mut self, now: Instant) -> NetResult<()> {
            let Some(refresh) = self.refresh.take() else {
                return Ok(());
            };
            if refresh.revision != self.revision {
                self.mark_dirty(now);
                return Ok(());
            }
            let objects = refresh.objects.ok_or_else(Failure::unavailable)?;
            let members: BTreeSet<String> = objects
                .iter()
                .filter(|(_, interfaces)| {
                    interfaces.contains_key(DEVICE) || interfaces.contains_key(CONNECTION)
                })
                .map(|(path, _)| path.clone())
                .collect();
            self.incarnations.retain(|path, _| members.contains(path));
            for path in members {
                if !self.incarnations.contains_key(&path) {
                    self.incarnation_serial += 1;
                    self.incarnations.insert(path, self.incarnation_serial);
                }
            }
            self.reasons.retain(|path, _| {
                objects
                    .get(path)
                    .is_some_and(|interfaces| interfaces.contains_key(ACTIVE))
            });
            self.snapshot = Snapshot::build(
                self.owner.clone(),
                &objects,
                refresh.settings,
                &self.incarnations,
                &self.reasons,
            )?;
            self.entries = reconcile(&self.snapshot);
            self.completed_refresh = refresh.id;
            self.dirty = false;
            self.publish();
            Ok(())
        }
        fn refresh_failed(&mut self, now: Instant) {
            self.refresh = None;
            self.pending.retain(|_, pending| {
                !matches!(
                    pending.kind,
                    PendingKind::Objects(_) | PendingKind::Settings(..)
                )
            });
            let keys: Vec<_> = self.operations.keys().cloned().collect();
            for key in keys {
                self.finish(&key, Err(Failure::unavailable()));
            }
            self.snapshot = Snapshot::default();
            self.entries.clear();
            self.stop_scans();
            self.dirty = false;
            self.refresh_at = now;
            self.publish();
        }
        fn receive(&mut self, bus: &impl Sender, mut message: Message, now: Instant) {
            if self.stopped {
                return;
            }
            if message.msg_type() == MessageType::Signal {
                self.signal(&message, now);
                return;
            }
            let Some(pending) = message
                .get_reply_serial()
                .and_then(|serial| self.pending.remove(&serial))
            else {
                if let Some(reply) = dbus::channel::default_reply(&message) {
                    let _ = bus.send(reply);
                }
                return;
            };
            let expected_sender = if matches!(pending.kind, PendingKind::Match | PendingKind::Owner)
            {
                DBUS
            } else {
                self.owner.as_str()
            };
            // A real bus always supplies an authenticated sender header. Synthetic
            // fixture replies may omit it, but cannot arrive over a bus socket.
            if message
                .sender()
                .is_some_and(|sender| &*sender != expected_sender)
            {
                self.pending
                    .insert(message.get_reply_serial().unwrap(), pending);
                return;
            }
            let failure = Failure::native(&mut message);
            match pending.kind {
                PendingKind::Match => {
                    self.subscriptions = self.subscriptions.saturating_sub(1);
                    if failure.is_some() {
                        self.stop();
                    }
                }
                PendingKind::Owner => {
                    self.owner_pending = false;
                    let owner = if failure.is_none() {
                        message.read1::<String>().unwrap_or_default()
                    } else {
                        String::new()
                    };
                    self.set_owner(owner, now);
                }
                PendingKind::Objects(id) => {
                    if !self
                        .refresh
                        .as_ref()
                        .is_some_and(|refresh| refresh.id == id)
                    {
                        return;
                    }
                    let objects = if failure.is_none() {
                        message
                            .read1::<Managed>()
                            .ok()
                            .and_then(|objects| managed_json(objects).ok())
                    } else {
                        None
                    };
                    let Some(objects) = objects else {
                        self.refresh_failed(now);
                        return;
                    };
                    let paths: Vec<_> = objects
                        .iter()
                        .filter(|(_, interfaces)| interfaces.contains_key(CONNECTION))
                        .map(|(path, _)| path.clone())
                        .collect();
                    if paths.len() > MAX_PROFILES {
                        self.refresh_failed(now);
                        return;
                    }
                    let refresh = self.refresh.as_mut().unwrap();
                    refresh.remaining = paths.len();
                    refresh.objects = Some(objects);
                    for path in &paths {
                        let result = call(&self.owner, path, CONNECTION, "GetSettings").and_then(
                            |message| {
                                self.send(
                                    bus,
                                    message,
                                    PendingKind::Settings(id, path.clone()),
                                    now + READ_TIME,
                                )
                            },
                        );
                        if result.is_err() {
                            self.refresh_failed(now);
                            return;
                        }
                    }
                    if paths.is_empty() && self.complete_refresh(now).is_err() {
                        self.refresh_failed(now);
                    }
                }
                PendingKind::Settings(id, path) => {
                    let Some(refresh) = self.refresh.as_mut().filter(|refresh| refresh.id == id)
                    else {
                        return;
                    };
                    if failure.is_none()
                        && let Ok(mut settings) = message.read1::<Settings>()
                    {
                        strip_secrets(&mut settings);
                        refresh.settings.insert(path, settings);
                    }
                    refresh.remaining = refresh.remaining.saturating_sub(1);
                    if refresh.remaining == 0 && self.complete_refresh(now).is_err() {
                        self.refresh_failed(now);
                    }
                }
                PendingKind::Operation(key) => {
                    if let Some(failure) = failure {
                        self.finish(&key, Err(failure));
                        return;
                    }
                    let Some(mut operation) = self.operations.remove(&key) else {
                        return;
                    };
                    let result = self.operation_reply(&mut operation, &message, now);
                    self.operations.insert(key.clone(), operation);
                    if let Err(error) = result {
                        self.finish(&key, Err(error));
                    }
                }
                PendingKind::Scan(_, _) => (),
            }
        }
        fn signal(&mut self, message: &Message, now: Instant) {
            let sender = message
                .sender()
                .map(|sender| sender.to_string())
                .unwrap_or_default();
            let interface = message
                .interface()
                .map(|interface| interface.to_string())
                .unwrap_or_default();
            let member = message
                .member()
                .map(|member| member.to_string())
                .unwrap_or_default();
            if sender == DBUS && interface == DBUS && member == "NameOwnerChanged" {
                if let Ok((name, _, owner)) = message.read3::<String, String, String>()
                    && name == NM
                {
                    self.set_owner(owner, now);
                }
                return;
            }
            if sender != self.owner || self.owner.is_empty() {
                return;
            }
            let path = message
                .path()
                .map(|path| path.to_string())
                .unwrap_or_default();
            if interface == ACTIVE
                && member == "StateChanged"
                && let Ok((_, reason)) = message.read2::<u32, u32>()
            {
                self.reasons.insert(path.clone(), reason);
            }
            if interface == OBJECT_MANAGER && member == "InterfacesRemoved" {
                if let Ok((path, interfaces)) = message.read2::<Path<'static>, Vec<String>>()
                    && interfaces
                        .iter()
                        .any(|interface| matches!(interface.as_str(), DEVICE | CONNECTION))
                {
                    self.incarnations.remove::<str>(&path);
                }
            } else if member == "DeviceRemoved" || member == "ConnectionRemoved" {
                if let Ok(path) = message.read1::<Path<'static>>() {
                    self.incarnations.remove::<str>(&path);
                }
            } else if interface == CONNECTION && member == "Removed" {
                self.incarnations.remove(&path);
            }
            self.revision += 1;
            self.mark_dirty(now);
        }
        fn operation_reply(
            &mut self,
            operation: &mut Operation,
            message: &Message,
            now: Instant,
        ) -> NetResult<()> {
            let phase = std::mem::replace(&mut operation.phase, Phase::VerifyToggle(false));
            operation.phase = match phase {
                Phase::AwaitUpdate(profile) => Phase::ActivateSaved(profile),
                Phase::AwaitActivation {
                    requested_uuid,
                    profile,
                    added,
                } => {
                    let (added_profile, path) = if added {
                        let (profile, active) = message
                            .read2::<Path<'static>, Path<'static>>()
                            .map_err(|_| Failure::changed())?;
                        (Some(profile.to_string()), active.to_string())
                    } else {
                        (
                            None,
                            message
                                .read1::<Path<'static>>()
                                .map_err(|_| Failure::changed())?
                                .to_string(),
                        )
                    };
                    if !non_root(&path) {
                        return Err(Failure::new(
                            "The connection could not be started. Try again.",
                        ));
                    }
                    Phase::WaitActivation(ActivationWatch {
                        path,
                        requested_uuid,
                        profile,
                        added_profile,
                        seen_target: false,
                        seen_progress: false,
                        seen_active: false,
                    })
                }
                Phase::AwaitDisconnect { active, forget } => {
                    Phase::VerifyDisconnect { active, forget }
                }
                Phase::AwaitDelete(profile, remaining) => Phase::VerifyDelete(profile, remaining),
                Phase::AwaitSecret(guard) => {
                    let secrets = message.read1::<Settings>().map_err(|_| Failure::new("The saved password is not available. Check the saved network settings."))?;
                    let key = if guard.security == Security::Wep {
                        format!("wep-key{}", guard.key_index)
                    } else {
                        "psk".to_owned()
                    };
                    let password = secrets.get(SECURITY).and_then(|security| security.get(&key)).and_then(|value| value.0.as_str())
                        .filter(|password| !password.is_empty() && password.len() <= REQUEST_LIMIT).ok_or_else(|| Failure::new("The saved password is not available. Check the saved network settings."))?;
                    Phase::SecretFinish(guard, Secret(password.to_owned()))
                }
                Phase::AwaitToggle(enabled) => Phase::VerifyToggle(enabled),
                _ => return Err(Failure::changed()),
            };
            operation.after_refresh = self.refresh_serial + 1;
            self.mark_dirty(now);
            Ok(())
        }

        fn send_operation(
            &mut self,
            bus: &impl Sender,
            key: &str,
            operation: &Operation,
            message: Message,
        ) -> NetResult<()> {
            self.send(
                bus,
                message,
                PendingKind::Operation(key.to_owned()),
                operation.deadline,
            )
        }
        fn check_radios(&self, guard: &EntryGuard) -> NetResult<()> {
            if guard.wireless
                && (!self.snapshot.wifi_enabled || !self.snapshot.wifi_hardware_enabled)
            {
                return Err(Failure::new(
                    "Enable Wi-Fi and check the hardware switch before connecting.",
                ));
            }
            Ok(())
        }
        fn activate_saved(
            &mut self,
            bus: &impl Sender,
            key: &str,
            operation: &mut Operation,
            profile_guard: ProfileGuard,
        ) -> NetResult<()> {
            let guard = operation.guard.as_ref().ok_or_else(Failure::changed)?;
            let entry = guard.entry(&self.snapshot, &self.entries)?;
            self.check_radios(guard)?;
            let profile = profile_guard.validate(&self.snapshot)?;
            if !entry
                .profiles
                .iter()
                .any(|index| self.snapshot.profiles[*index].path == profile.path)
            {
                return Err(Failure::changed());
            }
            let device = &self.snapshot.devices[entry.device];
            let ap = entry
                .best_ap(&self.snapshot, Some(profile))
                .map_or("/", |ap| ap.path.as_str());
            let message = call(&self.owner, ROOT, NM, "ActivateConnection")?.append3(
                dbus_path(&profile.path)?,
                dbus_path(&device.path)?,
                dbus_path(ap)?,
            );
            operation.phase = Phase::AwaitActivation {
                requested_uuid: profile.uuid.clone(),
                profile: Some(profile_guard),
                added: false,
            };
            self.send_operation(bus, key, operation, message)
        }
        fn step(
            &mut self,
            bus: &impl Sender,
            key: &str,
            operation: &mut Operation,
            now: Instant,
        ) -> NetResult<Option<Value>> {
            if operation.owner != self.owner || self.owner.is_empty() {
                return Err(Failure::unavailable());
            }
            if let Some(guard) = &operation.guard {
                guard.device(&self.snapshot)?;
            }
            let phase = std::mem::replace(&mut operation.phase, Phase::VerifyToggle(false));
            match phase {
                Phase::Start(Method::Activate {
                    profile_uuid,
                    password,
                    ..
                }) => {
                    let guard = operation.guard.as_ref().ok_or_else(Failure::changed)?;
                    let entry = guard.entry(&self.snapshot, &self.entries)?;
                    self.check_radios(guard)?;
                    if entry.changing(&self.snapshot) || entry.connected(&self.snapshot) {
                        return Err(Failure::new(
                            "Wait for the current network operation to finish.",
                        ));
                    }
                    if password.is_some() && (guard.profiles.len() > 1 || entry.profiles.len() > 1)
                    {
                        return Err(Failure::new(
                            "Multiple saved profiles match this network. Update the intended profile in a network connection editor.",
                        ));
                    }
                    let selected = if profile_uuid.is_empty() {
                        guard.profiles.first()
                    } else {
                        Some(
                            guard
                                .profiles
                                .iter()
                                .find(|profile| profile.uuid == profile_uuid)
                                .ok_or_else(|| {
                                    Failure::new(
                                        "The selected saved profile is no longer available.",
                                    )
                                })?,
                        )
                    };
                    if let Some(selected) = selected {
                        let profile = selected.validate(&self.snapshot)?;
                        if !entry
                            .profiles
                            .iter()
                            .any(|index| self.snapshot.profiles[*index].path == profile.path)
                        {
                            return Err(Failure::changed());
                        }
                        if let Some(password) = password {
                            let version = profile.version.filter(|version| *version != 0).ok_or_else(|| Failure::new("This NetworkManager version does not support guarded profile updates. Use a network connection editor."))?;
                            let mut settings = clone_settings(&profile.settings);
                            set_password(&mut settings, &password.0)?;
                            let mut expected = selected.clone();
                            expected.context = authentication_context(&settings_json(&settings));
                            let mut arguments = PropMap::new();
                            arguments.insert("version-id".to_owned(), Variant(Box::new(version)));
                            let message = call(&self.owner, &profile.path, CONNECTION, "Update2")?
                                .append3(settings, 1u32, arguments);
                            operation.phase = Phase::AwaitUpdate(expected);
                            self.send_operation(bus, key, operation, message)?;
                        } else {
                            self.activate_saved(bus, key, operation, selected.clone())?;
                        }
                    } else {
                        if !entry.profiles.is_empty() {
                            return Err(Failure::changed());
                        }
                        let (uuid, settings) = new_connection(
                            &self.snapshot,
                            entry,
                            password.as_ref().map(|password| password.0.as_str()),
                        )?;
                        let device = &self.snapshot.devices[entry.device];
                        let ap = entry
                            .best_ap(&self.snapshot, None)
                            .map_or("/", |ap| ap.path.as_str());
                        let message = call(&self.owner, ROOT, NM, "AddAndActivateConnection")?
                            .append3(settings, dbus_path(&device.path)?, dbus_path(ap)?);
                        operation.phase = Phase::AwaitActivation {
                            requested_uuid: uuid,
                            profile: None,
                            added: true,
                        };
                        self.send_operation(bus, key, operation, message)?;
                    }
                }
                Phase::ActivateSaved(profile) => {
                    let guard = operation.guard.as_ref().ok_or_else(Failure::changed)?;
                    let entry = guard.entry(&self.snapshot, &self.entries)?;
                    if entry.profiles.len() != 1 {
                        return Err(Failure::new(
                            "The saved profile selection changed. Update the intended profile in a network connection editor.",
                        ));
                    }
                    self.activate_saved(bus, key, operation, profile)?;
                }
                Phase::WaitActivation(mut watch) => {
                    let guard = operation.guard.as_ref().ok_or_else(Failure::changed)?;
                    let device = guard.device(&self.snapshot)?;
                    if guard.wireless
                        && (!self.snapshot.wifi_enabled || !self.snapshot.wifi_hardware_enabled)
                    {
                        return Err(Failure::new(
                            "Wi-Fi was disabled during the connection request.",
                        ));
                    }
                    if let Some(active) = self.snapshot.active.get(&watch.path) {
                        watch.seen_active = true;
                        if active.uuid != watch.requested_uuid
                            || active.kind != if guard.wireless { WIFI } else { WIRED }
                            || !active.devices.contains(&guard.device)
                        {
                            return Err(Failure::new(
                                "The selected saved profile changed during activation. Refresh and try again.",
                            ));
                        }
                        if let Some(profile) = &watch.profile {
                            profile.validate(&self.snapshot)?;
                            if active.profile != profile.path {
                                return Err(Failure::changed());
                            }
                        } else if let Some(path) = &watch.added_profile {
                            let profile = self
                                .snapshot
                                .unique_profile(&watch.requested_uuid)
                                .filter(|profile| {
                                    &profile.path == path
                                        && profile.kind == active.kind
                                        && profile.ssid == guard.ssid
                                        && device_compatible(device, profile)
                                })
                                .ok_or_else(Failure::changed)?;
                            watch.profile = Some(ProfileGuard::capture(profile));
                            if active.profile != *path {
                                return Err(Failure::changed());
                            }
                        }
                        let target = device
                            .active
                            .as_ref()
                            .is_some_and(|current| current.path == watch.path);
                        if target && device.changing() {
                            watch.seen_progress = true;
                        }
                        if matches!(active.state, 3 | 4)
                            || (target && watch.seen_progress && device.state == 120)
                        {
                            let message = "The connection failed. Check authentication and the saved network settings.";
                            return Err(
                                if matches!(device.reason, 7..=11)
                                    || matches!(active.reason, 9 | 10)
                                {
                                    Failure::authentication(message)
                                } else {
                                    Failure::new(message)
                                },
                            );
                        }
                        if target {
                            watch.seen_target = true;
                        } else if device.active.is_some() && watch.seen_target {
                            return Err(Failure::new("Another connection replaced this request."));
                        }
                        if active.state == 2 && device.state == 100 && target {
                            let entry = guard.entry(&self.snapshot, &self.entries)?;
                            if !entry.targets(&self.snapshot) || !entry.connected(&self.snapshot) {
                                return Err(Failure::new(
                                    "The selected network changed during activation.",
                                ));
                            }
                            return Ok(Some(Value::Null));
                        }
                    } else if watch.seen_active {
                        return Err(Failure::new("Another connection replaced this request."));
                    }
                    operation.phase = Phase::WaitActivation(watch);
                    operation.after_refresh = self.refresh_serial + 1;
                    self.dirty = true;
                    self.refresh_at = now + Duration::from_millis(250);
                }
                Phase::Start(Method::Disconnect(_)) | Phase::Start(Method::Forget(_)) => {
                    let forget = matches!(phase, Phase::Start(Method::Forget(_)));
                    let guard = operation.guard.as_ref().ok_or_else(Failure::changed)?;
                    let entry = guard.entry(&self.snapshot, &self.entries)?;
                    if entry.changing(&self.snapshot) {
                        return Err(Failure::new(
                            "Wait for the current network operation to finish.",
                        ));
                    }
                    let profiles = if forget {
                        if guard.profiles.is_empty() {
                            return Err(Failure::new(
                                "The selected saved profile is no longer available.",
                            ));
                        }
                        for profile in &guard.profiles {
                            if self
                                .snapshot
                                .profiles
                                .iter()
                                .any(|current| current.uuid == profile.uuid)
                            {
                                let profile = profile.validate(&self.snapshot)?;
                                if !entry.profiles.iter().any(|index| {
                                    self.snapshot.profiles[*index].path == profile.path
                                }) {
                                    return Err(Failure::changed());
                                }
                            }
                        }
                        Some(VecDeque::from(guard.profiles.clone()))
                    } else {
                        None
                    };
                    if entry.targets(&self.snapshot) {
                        guard.current_active(&self.snapshot)?;
                        let active = self.snapshot.devices[entry.device]
                            .active
                            .as_ref()
                            .map(|active| active.path.clone());
                        let message = call(&self.owner, &guard.device, DEVICE, "Disconnect")?;
                        operation.phase = Phase::AwaitDisconnect {
                            active,
                            forget: profiles,
                        };
                        self.send_operation(bus, key, operation, message)?;
                    } else if let Some(profiles) = profiles {
                        operation.phase = Phase::DeleteNext(profiles);
                    } else {
                        return Ok(Some(Value::Null));
                    }
                }
                Phase::VerifyDisconnect { active, forget } => {
                    let guard = operation.guard.as_ref().ok_or_else(Failure::changed)?;
                    let device = guard.device(&self.snapshot)?;
                    if device.active.as_ref().is_some_and(|current| {
                        Some(&current.path) != active.as_ref()
                            || Some(&current.uuid) != guard.active_uuid.as_ref()
                    }) {
                        return Err(Failure::changed());
                    }
                    if let Some(profiles) = forget {
                        operation.phase = Phase::DeleteNext(profiles);
                    } else {
                        return Ok(Some(Value::Null));
                    }
                }
                Phase::DeleteNext(mut profiles) => {
                    let guard = operation.guard.as_ref().ok_or_else(Failure::changed)?;
                    guard.current_active(&self.snapshot)?;
                    while let Some(profile_guard) = profiles.pop_front() {
                        if !self
                            .snapshot
                            .profiles
                            .iter()
                            .any(|profile| profile.uuid == profile_guard.uuid)
                        {
                            continue;
                        }
                        let entry = guard.entry(&self.snapshot, &self.entries)?;
                        let profile = profile_guard.validate(&self.snapshot)?;
                        if !entry
                            .profiles
                            .iter()
                            .any(|index| self.snapshot.profiles[*index].path == profile.path)
                        {
                            return Err(Failure::changed());
                        }
                        let message = call(&self.owner, &profile.path, CONNECTION, "Delete")?;
                        operation.phase = Phase::AwaitDelete(profile_guard, profiles);
                        self.send_operation(bus, key, operation, message)?;
                        return Ok(None);
                    }
                    return Ok(Some(Value::Null));
                }
                Phase::VerifyDelete(profile, profiles) => {
                    if self
                        .snapshot
                        .profiles
                        .iter()
                        .any(|current| current.uuid == profile.uuid)
                    {
                        return Err(Failure::changed());
                    }
                    operation.phase = Phase::DeleteNext(profiles);
                }
                Phase::SecretStart(guard) => {
                    guard.validate(&self.snapshot, &self.entries, &self.busy)?;
                    if guard.security == Security::Open {
                        operation.phase = Phase::SecretFinish(guard, Secret(String::new()));
                        operation.after_refresh = self.refresh_serial + 1;
                        self.mark_dirty(now);
                    } else {
                        let message =
                            call(&self.owner, &guard.profile.path, CONNECTION, "GetSecrets")?
                                .append1(SECURITY);
                        operation.phase = Phase::AwaitSecret(guard);
                        self.send_operation(bus, key, operation, message)?;
                    }
                }
                Phase::SecretFinish(guard, password) => {
                    guard.validate(&self.snapshot, &self.entries, &self.busy)?;
                    return Ok(Some(guard.credentials(&password.0)));
                }
                Phase::Start(Method::ToggleWifi) => {
                    if !self.snapshot.devices.iter().any(|device| device.wireless) {
                        return Err(Failure::new("No Wi-Fi device is available."));
                    }
                    if !self.snapshot.wifi_hardware_enabled {
                        return Err(Failure::new("Wi-Fi is blocked by the hardware switch."));
                    }
                    let enabled = !self.snapshot.wifi_enabled;
                    let message =
                        call(&self.owner, ROOT, "org.freedesktop.DBus.Properties", "Set")?.append3(
                            NM,
                            "WirelessEnabled",
                            Variant(enabled),
                        );
                    operation.phase = Phase::AwaitToggle(enabled);
                    self.send_operation(bus, key, operation, message)?;
                }
                Phase::VerifyToggle(enabled) => {
                    if self.snapshot.wifi_enabled != enabled {
                        return Err(Failure::new("Unable to change the Wi-Fi radio state."));
                    }
                    return Ok(Some(Value::Null));
                }
                phase => {
                    operation.phase = phase;
                }
            }
            Ok(None)
        }
        fn configure_scans(&mut self, bus: &impl Sender, now: Instant) {
            if !self.opened
                || !self.snapshot.wifi_enabled
                || !self.snapshot.wifi_hardware_enabled
                || self.owner.is_empty()
            {
                self.stop_scans();
                return;
            }
            let devices: Vec<_> = self
                .snapshot
                .devices
                .iter()
                .filter(|device| device.wireless)
                .map(|device| (device.path.clone(), device.incarnation))
                .collect();
            self.scans
                .retain(|path, (incarnation, _)| devices.contains(&(path.clone(), *incarnation)));
            self.pending.retain(|_, pending| match &pending.kind {
                PendingKind::Scan(path, incarnation) => {
                    devices.contains(&(path.clone(), *incarnation))
                }
                _ => true,
            });
            for (path, incarnation) in devices {
                let (previous, due) = self.scans.entry(path.clone()).or_insert((incarnation, now));
                if *previous != incarnation {
                    *previous = incarnation;
                    *due = now;
                }
                if *due > now || self.pending.values().any(|pending| matches!(&pending.kind, PendingKind::Scan(current, _) if current == &path)) { continue; }
                *due = now + SCAN_TIME;
                if let Ok(message) = call(&self.owner, &path, WIRELESS, "RequestScan") {
                    let _ = self.send(
                        bus,
                        message.append1(PropMap::new()),
                        PendingKind::Scan(path, incarnation),
                        now + READ_TIME,
                    );
                }
            }
        }
        fn tick(&mut self, bus: &impl Sender, now: Instant) {
            if self.stopped {
                return;
            }
            let expired: Vec<_> = self
                .pending
                .iter()
                .filter(|(_, pending)| pending.deadline <= now)
                .map(|(serial, _)| *serial)
                .collect();
            for serial in expired {
                if let Some(pending) = self.pending.remove(&serial) {
                    match pending.kind {
                        PendingKind::Operation(key) => self.finish(&key, Err(Failure::timeout())),
                        PendingKind::Objects(_) | PendingKind::Settings(..) => {
                            self.refresh_failed(now)
                        }
                        PendingKind::Match => {
                            self.stop();
                            return;
                        }
                        PendingKind::Owner => self.owner_pending = false,
                        PendingKind::Scan(..) => (),
                    }
                }
            }
            let expired: Vec<_> = self
                .operations
                .iter()
                .filter(|(_, operation)| operation.deadline <= now)
                .map(|(key, _)| key.clone())
                .collect();
            for key in expired {
                self.finish(&key, Err(Failure::timeout()));
            }
            if self.owner.is_empty() && !self.owner_pending && self.subscriptions == 0 {
                let keys: Vec<_> = self.operations.keys().cloned().collect();
                for key in keys {
                    self.finish(&key, Err(Failure::unavailable()));
                }
            }
            if !self.dirty && self.refresh.is_none() && !self.snapshot.owner.is_empty() {
                self.configure_scans(bus, now);
                let keys: Vec<_> = self
                    .operations
                    .iter()
                    .filter(|(_, operation)| operation.after_refresh <= self.completed_refresh)
                    .map(|(key, _)| key.clone())
                    .collect();
                for key in keys {
                    let Some(mut operation) = self.operations.remove(&key) else {
                        continue;
                    };
                    let result = self.step(bus, &key, &mut operation, now);
                    self.operations.insert(key.clone(), operation);
                    match result {
                        Ok(Some(result)) => self.finish(&key, Ok(result)),
                        Err(error) => self.finish(&key, Err(error)),
                        _ => (),
                    }
                    if self.dirty {
                        break;
                    }
                }
                if self
                    .operations
                    .values()
                    .any(|operation| operation.after_refresh > self.completed_refresh)
                    && !self.dirty
                {
                    self.mark_dirty(now);
                }
            }
            if self.dirty
                && self.refresh.is_none()
                && now >= self.refresh_at
                && !self.owner.is_empty()
                && self.subscriptions == 0
                && self.start_refresh(bus, now).is_err()
            {
                self.refresh_failed(now);
            }
        }
    }

    struct Nonblocking {
        fd: i32,
        flags: i32,
    }
    impl Nonblocking {
        fn new(fd: i32) -> io::Result<Self> {
            let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
            if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0
            {
                return Err(io::Error::last_os_error());
            }
            Ok(Self { fd, flags })
        }
    }
    impl Drop for Nonblocking {
        fn drop(&mut self) {
            unsafe {
                libc::fcntl(self.fd, libc::F_SETFL, self.flags);
            }
        }
    }
    #[derive(Default)]
    struct Input {
        line: Vec<u8>,
        overlong: bool,
    }
    impl Input {
        fn feed(&mut self, bytes: &[u8], engine: &mut Engine, now: Instant) {
            for byte in bytes {
                if *byte == b'\n' {
                    if self.overlong {
                        engine.reply(Value::Null, Err(Failure::invalid()));
                    } else {
                        engine.enqueue(&self.line, now);
                    }
                    self.line.fill(0);
                    self.line.clear();
                    self.overlong = false;
                } else if !self.overlong {
                    if self.line.len() == REQUEST_LIMIT {
                        self.line.fill(0);
                        self.line.clear();
                        self.overlong = true;
                    } else {
                        self.line.push(*byte);
                    }
                }
                if engine.stopped {
                    break;
                }
            }
        }
        fn read(&mut self, fd: i32, engine: &mut Engine, now: Instant) -> io::Result<bool> {
            let mut bytes = [0u8; 8192];
            for _ in 0..32 {
                let count = unsafe { libc::read(fd, bytes.as_mut_ptr().cast(), bytes.len()) };
                if count == 0 {
                    self.line.fill(0);
                    engine.stop();
                    return Ok(false);
                }
                if count < 0 {
                    let error = io::Error::last_os_error();
                    if error.kind() == io::ErrorKind::WouldBlock {
                        return Ok(true);
                    }
                    if error.kind() == io::ErrorKind::Interrupted {
                        continue;
                    }
                    return Err(error);
                }
                self.feed(&bytes[..count as usize], engine, now);
                bytes.fill(0);
                if engine.stopped {
                    return Ok(false);
                }
            }
            Ok(true)
        }
    }
    #[derive(Default)]
    struct Output {
        replies: VecDeque<Vec<u8>>,
        state: Option<Vec<u8>>,
        current: Option<(Vec<u8>, usize)>,
        bytes: usize,
    }
    impl Output {
        fn push(&mut self, value: Value) -> bool {
            let mut bytes = match serde_json::to_vec(&value) {
                Ok(bytes) => bytes,
                Err(_) => return false,
            };
            bytes.push(b'\n');
            if value["type"] == "state"
                && let Some(mut previous) = self.state.take()
            {
                self.bytes -= previous.len();
                previous.fill(0);
            }
            if self.bytes.saturating_add(bytes.len()) > OUTPUT_LIMIT {
                bytes.fill(0);
                return false;
            }
            self.bytes += bytes.len();
            if value["type"] == "state" {
                self.state = Some(bytes);
            } else {
                self.replies.push_back(bytes);
            }
            true
        }
        fn write(&mut self, fd: i32) -> io::Result<()> {
            let mut budget = 65536;
            while budget > 0 {
                if self.current.is_none() {
                    self.current = self
                        .state
                        .take()
                        .or_else(|| self.replies.pop_front())
                        .map(|bytes| (bytes, 0));
                }
                let Some((bytes, offset)) = &mut self.current else {
                    return Ok(());
                };
                let count = unsafe {
                    libc::write(
                        fd,
                        bytes[*offset..].as_ptr().cast(),
                        (bytes.len() - *offset).min(budget),
                    )
                };
                if count < 0 {
                    let error = io::Error::last_os_error();
                    if error.kind() == io::ErrorKind::WouldBlock {
                        return Ok(());
                    }
                    if error.kind() == io::ErrorKind::Interrupted {
                        continue;
                    }
                    return Err(error);
                }
                if count == 0 {
                    return Err(io::Error::from(io::ErrorKind::WriteZero));
                }
                let count = count as usize;
                *offset += count;
                self.bytes -= count;
                budget -= count;
                if *offset == bytes.len() {
                    bytes.fill(0);
                    self.current = None;
                }
            }
            Ok(())
        }
    }
    impl Drop for Output {
        fn drop(&mut self) {
            if let Some((bytes, _)) = &mut self.current {
                bytes.fill(0);
            }
            if let Some(bytes) = &mut self.state {
                bytes.fill(0);
            }
            for bytes in &mut self.replies {
                bytes.fill(0);
            }
        }
    }
    fn run_loop(
        input_fd: i32,
        output_fd: i32,
        receiver: std::sync::mpsc::Receiver<Option<Channel>>,
    ) -> i32 {
        let _input_flags = match Nonblocking::new(input_fd) {
            Ok(flags) => flags,
            Err(_) => return 1,
        };
        let _output_flags = match Nonblocking::new(output_fd) {
            Ok(flags) => flags,
            Err(_) => return 1,
        };
        let started = Instant::now();
        let mut engine = Engine::new(started);
        let mut input = Input::default();
        let mut output = Output::default();
        let mut channel: Option<Channel> = None;
        let mut connecting = true;
        let mut output_progress = started;
        loop {
            let now = Instant::now();
            // Input is drained before dispatching any replies/continuations. A
            // cancelled request and an EOF can never schedule its next NM call.
            match input.read(input_fd, &mut engine, now) {
                Ok(false) => return 0,
                Err(_) => {
                    engine.stop();
                    return 1;
                }
                _ => (),
            }
            if engine.stopped {
                return 1;
            }
            if connecting {
                match receiver.try_recv() {
                    Ok(connection) => {
                        channel = connection;
                        connecting = false;
                    }
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => connecting = false,
                    _ if now.duration_since(started) >= READ_TIME => connecting = false,
                    _ => (),
                }
                if let Some(channel) = &mut channel {
                    channel.set_watch_enabled(true);
                    if engine.bootstrap(channel, now).is_err() {
                        engine.stop();
                        return 1;
                    }
                }
            }
            let mut bus_backlog = false;
            if let Some(bus) = &channel {
                if bus.read_write(Some(Duration::ZERO)).is_err() {
                    engine.set_owner(String::new(), now);
                    channel = None;
                } else {
                    for index in 0..128 {
                        let Some(message) = bus.pop_message() else {
                            break;
                        };
                        engine.receive(bus, message, now);
                        if engine.stopped {
                            return 1;
                        }
                        bus_backlog = index == 127;
                    }
                    if !bus_backlog {
                        engine.tick(bus, now);
                    }
                }
            } else if !connecting {
                let ids: Vec<_> = engine.operations.keys().cloned().collect();
                for id in ids {
                    engine.finish(&id, Err(Failure::unavailable()));
                }
            }
            if output.bytes == 0 {
                output_progress = now;
            }
            while let Some(value) = engine.output.pop_front() {
                if !output.push(value) {
                    engine.stop();
                    return 1;
                }
            }
            let before_write = output.bytes;
            if output.write(output_fd).is_err() {
                engine.stop();
                return 1;
            }
            if output.bytes < before_write {
                output_progress = now;
            }
            if output.bytes != 0 && now.duration_since(output_progress) >= READ_TIME {
                engine.stop();
                return 1;
            }
            let watch = channel.as_ref().map(Channel::watch);
            let mut fds = [
                libc::pollfd {
                    fd: input_fd,
                    events: libc::POLLIN,
                    revents: 0,
                },
                libc::pollfd {
                    fd: if output.bytes == 0 { -1 } else { output_fd },
                    events: libc::POLLOUT,
                    revents: 0,
                },
                libc::pollfd {
                    fd: watch.map_or(-1, |watch| watch.fd),
                    events: watch.map_or(0, |watch| {
                        (if watch.read { libc::POLLIN } else { 0 })
                            | (if watch.write { libc::POLLOUT } else { 0 })
                    }),
                    revents: 0,
                },
            ];
            if unsafe {
                libc::poll(
                    fds.as_mut_ptr(),
                    fds.len() as libc::nfds_t,
                    if bus_backlog { 0 } else { 50 },
                )
            } < 0
                && io::Error::last_os_error().kind() != io::ErrorKind::Interrupted
            {
                engine.stop();
                return 1;
            }
        }
    }
    pub(super) fn run() -> i32 {
        // Bus authentication is the only blocking libdbus operation. Isolate it
        // from stdin/cancellation, bound startup, and never reconnect/retry calls.
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        if std::thread::Builder::new()
            .name("network-bus-connect".to_owned())
            .spawn(move || {
                let _ = sender.send(Channel::get_private(BusType::System).ok());
            })
            .is_err()
        {
            return 1;
        }
        run_loop(0, 1, receiver)
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::cell::{Cell, RefCell};
        use std::io::{BufRead, BufReader, Read, Write};
        use std::os::fd::AsRawFd;
        use std::os::unix::net::UnixStream;
        use std::process::{Child, Command, Stdio};
        use std::sync::mpsc;
        use std::thread;

        const D: &str = "/org/freedesktop/NetworkManager/Devices/1";
        const P: &str = "/org/freedesktop/NetworkManager/Settings/1";
        const P2: &str = "/org/freedesktop/NetworkManager/Settings/2";
        const A: &str = "/org/freedesktop/NetworkManager/AccessPoint/1";
        const A2: &str = "/org/freedesktop/NetworkManager/AccessPoint/2";
        const C: &str = "/org/freedesktop/NetworkManager/ActiveConnection/1";
        const IP: &str = "/org/freedesktop/NetworkManager/IP4Config/1";
        const UUID: &str = "00000000-0000-4000-8000-000000000001";
        const UUID2: &str = "00000000-0000-4000-8000-000000000002";
        const PASSWORD: &str = "fixture-only-never-log-this-password";

        #[derive(Default)]
        struct FakeBus {
            serial: Cell<u32>,
            messages: RefCell<VecDeque<Message>>,
        }
        impl Sender for FakeBus {
            fn send(&self, mut message: Message) -> Result<u32, ()> {
                let serial = self.serial.get() + 1;
                self.serial.set(serial);
                message.set_serial(serial);
                self.messages.borrow_mut().push_back(message);
                Ok(serial)
            }
        }
        impl FakeBus {
            fn pop(&self) -> Option<Message> {
                self.messages.borrow_mut().pop_front()
            }
        }
        fn member(message: &Message) -> String {
            message
                .member()
                .map(|member| member.to_string())
                .unwrap_or_default()
        }
        fn put<T: RefArg + 'static>(
            objects: &mut Managed,
            path: &str,
            interface: &str,
            name: &str,
            value: T,
        ) {
            objects
                .entry(dbus_path(path).unwrap())
                .or_default()
                .entry(interface.to_owned())
                .or_default()
                .insert(name.to_owned(), Variant(Box::new(value)));
        }
        fn clone_managed(objects: &Managed) -> Managed {
            objects
                .iter()
                .map(|(path, interfaces)| {
                    (
                        path.clone(),
                        interfaces
                            .iter()
                            .map(|(name, properties)| {
                                (
                                    name.clone(),
                                    properties
                                        .iter()
                                        .map(|(key, value)| {
                                            (key.clone(), Variant(value.0.box_clone()))
                                        })
                                        .collect(),
                                )
                            })
                            .collect(),
                    )
                })
                .collect()
        }
        fn saved(uuid: &str, ssid: &[u8], security: Security) -> Settings {
            let mut settings = Settings::new();
            insert(
                &mut settings,
                "connection",
                "id",
                "Duplicate display ID".to_owned(),
            );
            insert(&mut settings, "connection", "uuid", uuid.to_owned());
            insert(&mut settings, "connection", "type", WIFI.to_owned());
            insert(
                &mut settings,
                "connection",
                "interface-name",
                "wlan-test".to_owned(),
            );
            insert(&mut settings, WIFI, "ssid", ssid.to_vec());
            insert(&mut settings, WIFI, "mode", "infrastructure".to_owned());
            insert(&mut settings, WIFI, "hidden", true);
            if security != Security::Open {
                insert(
                    &mut settings,
                    SECURITY,
                    "key-mgmt",
                    match security {
                        Security::Wep => "none",
                        Security::Psk => "wpa-psk",
                        Security::Sae => "sae",
                        Security::Owe => "owe",
                        Security::Enterprise => "wpa-eap",
                        _ => "unsupported",
                    }
                    .to_owned(),
                );
            }
            settings
        }
        fn basic_objects() -> Managed {
            let mut objects = Managed::new();
            put(
                &mut objects,
                ROOT,
                NM,
                "Devices",
                vec![dbus_path(D).unwrap()],
            );
            put(&mut objects, ROOT, NM, "WirelessEnabled", true);
            put(&mut objects, ROOT, NM, "WirelessHardwareEnabled", true);
            put(
                &mut objects,
                SETTINGS_PATH,
                "org.freedesktop.NetworkManager.Settings",
                "Connections",
                vec![dbus_path(P).unwrap()],
            );
            put(&mut objects, D, DEVICE, "Interface", "wlan-test".to_owned());
            put(&mut objects, D, DEVICE, "DeviceType", 2u32);
            put(&mut objects, D, DEVICE, "State", 30u32);
            put(&mut objects, D, DEVICE, "StateReason", (30u32, 0u32));
            put(
                &mut objects,
                D,
                DEVICE,
                "ActiveConnection",
                dbus_path("/").unwrap(),
            );
            put(&mut objects, D, DEVICE, "Ip4Config", dbus_path(IP).unwrap());
            put(
                &mut objects,
                D,
                DEVICE,
                "Ip6Config",
                dbus_path("/").unwrap(),
            );
            put(
                &mut objects,
                D,
                WIRELESS,
                "AccessPoints",
                Vec::<Path<'static>>::new(),
            );
            put(
                &mut objects,
                D,
                WIRELESS,
                "ActiveAccessPoint",
                dbus_path("/").unwrap(),
            );
            put(
                &mut objects,
                D,
                WIRELESS,
                "HwAddress",
                "02:00:00:00:00:01".to_owned(),
            );
            put(
                &mut objects,
                D,
                WIRELESS,
                "PermHwAddress",
                "02:00:00:00:00:01".to_owned(),
            );
            put(&mut objects, D, WIRELESS, "WirelessCapabilities", 0x3fffu32);
            put(&mut objects, D, WIRELESS, "Bitrate", 866700u32);
            put(&mut objects, P, CONNECTION, "VersionId", 7u64);
            put(
                &mut objects,
                IP,
                "org.freedesktop.NetworkManager.IP4Config",
                "Gateway",
                "192.0.2.1".to_owned(),
            );
            let mut address = PropMap::new();
            address.insert(
                "address".to_owned(),
                Variant(Box::new("192.0.2.77".to_owned())),
            );
            address.insert("prefix".to_owned(), Variant(Box::new(24u32)));
            put(
                &mut objects,
                IP,
                "org.freedesktop.NetworkManager.IP4Config",
                "AddressData",
                vec![address],
            );
            let mut dns = PropMap::new();
            dns.insert(
                "address".to_owned(),
                Variant(Box::new("192.0.2.53".to_owned())),
            );
            put(
                &mut objects,
                IP,
                "org.freedesktop.NetworkManager.IP4Config",
                "NameserverData",
                vec![dns],
            );
            objects
        }
        fn add_ap(
            objects: &mut Managed,
            path: &str,
            ssid: &[u8],
            security: Security,
            strength: u8,
        ) {
            put(objects, path, AP_INTERFACE, "Ssid", ssid.to_vec());
            put(objects, path, AP_INTERFACE, "Strength", strength);
            put(
                objects,
                path,
                AP_INTERFACE,
                "HwAddress",
                if path == A {
                    "02:00:00:00:01:01"
                } else {
                    "02:00:00:00:01:02"
                }
                .to_owned(),
            );
            put(objects, path, AP_INTERFACE, "Frequency", 6115u32);
            put(objects, path, AP_INTERFACE, "Mode", 2u32);
            put(
                objects,
                path,
                AP_INTERFACE,
                "Flags",
                if security == Security::Open { 0u32 } else { 1 },
            );
            put(objects, path, AP_INTERFACE, "WpaFlags", 0u32);
            put(
                objects,
                path,
                AP_INTERFACE,
                "RsnFlags",
                match security {
                    Security::Psk => PSK,
                    Security::Sae => SAE | PSK,
                    Security::Enterprise => EAP,
                    Security::Owe => OWE,
                    Security::Unknown => 8,
                    _ => 0,
                },
            );
        }
        fn bind(
            objects: &mut Managed,
            profile: &str,
            uuid: &str,
            ap: &str,
            device_state: u32,
            active_state: u32,
        ) {
            put(objects, C, ACTIVE, "Uuid", uuid.to_owned());
            put(objects, C, ACTIVE, "Id", "Active profile".to_owned());
            put(objects, C, ACTIVE, "Type", WIFI.to_owned());
            put(
                objects,
                C,
                ACTIVE,
                "Connection",
                dbus_path(profile).unwrap(),
            );
            put(objects, C, ACTIVE, "State", active_state);
            put(objects, C, ACTIVE, "Devices", vec![dbus_path(D).unwrap()]);
            put(objects, C, ACTIVE, "Ip4Config", dbus_path(IP).unwrap());
            put(objects, C, ACTIVE, "Ip6Config", dbus_path("/").unwrap());
            put(
                objects,
                D,
                DEVICE,
                "ActiveConnection",
                dbus_path(C).unwrap(),
            );
            put(objects, D, DEVICE, "State", device_state);
            put(
                objects,
                D,
                WIRELESS,
                "ActiveAccessPoint",
                dbus_path(ap).unwrap(),
            );
        }
        struct Fixture {
            now: Instant,
            engine: Engine,
            bus: FakeBus,
            objects: Managed,
            settings: BTreeMap<String, Settings>,
            calls: VecDeque<Message>,
        }
        impl Fixture {
            fn new(security: Security) -> Self {
                let now = Instant::now();
                let mut engine = Engine::new(now);
                engine.set_owner(":1.42".to_owned(), now);
                let mut fixture = Self {
                    now,
                    engine,
                    bus: FakeBus::default(),
                    objects: basic_objects(),
                    settings: BTreeMap::from([(P.to_owned(), saved(UUID, b"fixture", security))]),
                    calls: VecDeque::new(),
                };
                fixture.pump();
                fixture
            }
            fn pump(&mut self) {
                for _ in 0..32 {
                    self.engine.tick(&self.bus, self.now);
                    while let Some(message) = self.bus.pop() {
                        let reply = match member(&message).as_str() {
                            "GetManagedObjects" => Some(
                                message
                                    .method_return()
                                    .append1(clone_managed(&self.objects)),
                            ),
                            "GetSettings" => Some(message.method_return().append1(clone_settings(
                                &self.settings[&message.path().unwrap().to_string()],
                            ))),
                            _ => {
                                self.calls.push_back(message);
                                None
                            }
                        };
                        if let Some(reply) = reply {
                            self.engine.receive(&self.bus, reply, self.now);
                        }
                    }
                }
            }
            fn refresh(&mut self) {
                self.engine.mark_dirty(self.now);
                self.pump();
            }
            fn request(&mut self, id: u32, method: &str, args: Value) {
                self.engine.enqueue(
                    json!({ "id": id, "method": method, "args": args, "expectedStamp": self.engine.last_state["rows"].as_array().and_then(|rows| rows.iter().find(|row| row["key"] == args[0])).map(|row| &row["selectionStamp"]) })
                        .to_string()
                        .as_bytes(),
                    self.now,
                );
                self.pump();
            }
            fn key(&self) -> String {
                self.engine
                    .entries
                    .iter()
                    .find(|entry| entry.ssid == b"fixture")
                    .unwrap()
                    .key
                    .clone()
            }
            fn take_call(&mut self, expected: &str) -> Message {
                let message = self
                    .calls
                    .pop_front()
                    .unwrap_or_else(|| panic!("missing {expected} fixture call"));
                assert_eq!(member(&message), expected);
                message
            }
            fn reply(&mut self, reply: Message) {
                self.engine.receive(&self.bus, reply, self.now);
                self.pump();
            }
            fn result(&self, id: u32) -> Option<&Value> {
                self.engine.output.iter().find(|value| value["id"] == id)
            }
            fn detect(&mut self, security: Security) {
                add_ap(&mut self.objects, A, b"fixture", security, 60);
                put(
                    &mut self.objects,
                    D,
                    WIRELESS,
                    "AccessPoints",
                    vec![dbus_path(A).unwrap()],
                );
                self.refresh();
            }
        }

        #[test]
        fn grouping_preserves_raw_ssid_security_device_and_active_ap() {
            let mut f = Fixture::new(Security::Psk);
            f.detect(Security::Sae);
            assert_eq!(f.engine.entries.len(), 1);
            assert_eq!(f.engine.entries[0].security, Security::Sae);
            assert_eq!(
                f.engine.entries[0].profiles.len(),
                1,
                "PSK transition compatibility"
            );
            let key = f.key();
            add_ap(&mut f.objects, A2, b"fixture", Security::Sae, 95);
            put(
                &mut f.objects,
                D,
                WIRELESS,
                "AccessPoints",
                vec![dbus_path(A).unwrap(), dbus_path(A2).unwrap()],
            );
            bind(&mut f.objects, P, UUID, A, 100, 2);
            f.refresh();
            let row = &f.engine.last_state["rows"][0];
            assert_eq!(row["key"], key);
            assert_eq!(row["strength"], 60);
            assert_eq!(row["connected"], true);
            let details = &f.engine.last_state["details"][&key];
            assert_eq!(details["frequency"], "6115 MHz");
            assert_eq!(details["channel"], "33");
            assert_eq!(details["rate"], "866.7 Mbit/s");
            let raw1 = "/org/freedesktop/NetworkManager/AccessPoint/3";
            let raw2 = "/org/freedesktop/NetworkManager/AccessPoint/4";
            add_ap(&mut f.objects, raw1, &[0xff], Security::Open, 50);
            add_ap(&mut f.objects, raw2, &[0xfe], Security::Open, 50);
            put(
                &mut f.objects,
                D,
                WIRELESS,
                "AccessPoints",
                vec![
                    dbus_path(A).unwrap(),
                    dbus_path(raw1).unwrap(),
                    dbus_path(raw2).unwrap(),
                ],
            );
            f.refresh();
            let raw: Vec<_> = f
                .engine
                .entries
                .iter()
                .filter(|entry| entry.ssid.len() == 1)
                .collect();
            assert_eq!(raw.len(), 2);
            assert_ne!(raw[0].key, raw[1].key);
        }
        #[test]
        fn saved_detection_keeps_stamps_and_distinguishes_null_from_zero() {
            let mut f = Fixture::new(Security::Psk);
            let key = f.key();
            let row = f.engine.last_state["rows"][0].clone();
            assert!(row["strength"].is_null());
            f.detect(Security::Psk);
            put(&mut f.objects, A, AP_INTERFACE, "Strength", 0u8);
            f.refresh();
            let detected = &f.engine.last_state["rows"][0];
            assert_eq!(detected["key"], key);
            assert_eq!(detected["strength"], 0);
            assert_eq!(row["savedInfoStamp"], detected["savedInfoStamp"]);
            insert(
                f.settings.get_mut(P).unwrap(),
                "connection",
                "id",
                "Renamed".to_owned(),
            );
            f.refresh();
            assert_eq!(
                row["profileStamp"],
                f.engine.last_state["rows"][0]["profileStamp"]
            );
            assert_eq!(
                row["savedInfoStamp"],
                f.engine.last_state["rows"][0]["savedInfoStamp"]
            );
            f.engine.incarnations.remove(P);
            f.refresh();
            assert_ne!(
                row["savedInfoStamp"],
                f.engine.last_state["rows"][0]["savedInfoStamp"]
            );
        }
        #[test]
        fn security_labels_cover_beacons_and_saved_protocols() {
            for (wpa, rsn, privacy, expected_type, label) in [
                (0, 0, 0, Security::Open, "Open"),
                (0, 0, 1, Security::Wep, "WEP"),
                (PSK, 0, 1, Security::Psk, "WPA"),
                (PSK, PSK, 1, Security::Psk, "WPA2"),
                (0, PSK | SAE, 1, Security::Sae, "WPA3"),
                (EAP, 0, 1, Security::Enterprise, "WPA Enterprise"),
                (0, EAP, 1, Security::Enterprise, "WPA2 Enterprise"),
                (0, SUITE_B, 1, Security::Enterprise, "WPA3 Enterprise"),
                (0, OWE_TM, 0, Security::Owe, "Enhanced Open (OWE)"),
                (0, 8, 1, Security::Unknown, "Unknown"),
            ] {
                let mut f = Fixture::new(Security::Psk);
                f.settings.clear();
                f.objects.remove(&dbus_path(P).unwrap());
                f.detect(Security::Open);
                put(&mut f.objects, A, AP_INTERFACE, "WpaFlags", wpa);
                put(&mut f.objects, A, AP_INTERFACE, "RsnFlags", rsn);
                put(&mut f.objects, A, AP_INTERFACE, "Flags", privacy);
                f.refresh();
                assert_eq!(f.engine.entries[0].security, expected_type);
                assert_eq!(f.engine.last_state["rows"][0]["security"], label);
            }
            let mut f = Fixture::new(Security::Enterprise);
            insert(
                f.settings.get_mut(P).unwrap(),
                SECURITY,
                "key-mgmt",
                "ieee8021x".to_owned(),
            );
            f.detect(Security::Wep);
            assert_eq!(f.engine.last_state["rows"][0]["security"], "Dynamic WEP");
            insert(
                f.settings.get_mut(P).unwrap(),
                SECURITY,
                "auth-alg",
                "leap".to_owned(),
            );
            f.refresh();
            assert_eq!(f.engine.last_state["rows"][0]["security"], "LEAP");
            let mut profile = Profile::parse(
                P.to_owned(),
                1,
                saved(UUID, b"fixture", Security::Psk),
                &json!({}),
            );
            insert(
                &mut profile.settings,
                SECURITY,
                "proto",
                vec!["rsn".to_owned()],
            );
            profile.value = settings_json(&profile.settings);
            assert_eq!(profile.label(), "WPA2");
        }
        #[test]
        fn compatibility_honors_ssid_bssid_mode_band_protocol_cipher_and_device_binding() {
            let mut f = Fixture::new(Security::Psk);
            f.detect(Security::Sae);
            let ap = f.engine.snapshot.devices[0].aps[0].clone();
            for (section, field, value) in [
                (WIFI, "bssid", json!([2, 0, 0, 0, 99, 99])),
                (WIFI, "ssid", json!([1, 2])),
                (WIFI, "mode", json!("adhoc")),
                (WIFI, "band", json!("bg")),
                (WIFI, "channel", json!(1)),
                (SECURITY, "proto", json!(["wpa"])),
                (SECURITY, "pairwise", json!(["tkip"])),
                (SECURITY, "group", json!(["wep40"])),
            ] {
                let mut profile = Profile::parse(
                    P.to_owned(),
                    1,
                    saved(UUID, b"fixture", Security::Psk),
                    &json!({}),
                );
                profile.value[section][field] = value;
                if field == "ssid" {
                    profile.ssid = vec![1, 2];
                }
                assert!(!ap_compatible(&ap, &profile), "restriction {field}");
            }
            let mut profile = Profile::parse(
                P.to_owned(),
                1,
                saved(UUID, b"fixture", Security::Psk),
                &json!({}),
            );
            profile.value["connection"]["interface-name"] = json!("different-radio");
            assert!(!device_compatible(&f.engine.snapshot.devices[0], &profile));
            profile.value["connection"]["interface-name"] = json!("wlan-test");
            profile.value[WIFI]["mac-address"] = json!([1, 2, 3, 4, 5, 6]);
            assert!(!device_compatible(&f.engine.snapshot.devices[0], &profile));
        }
        #[test]
        fn offline_details_never_leak_another_connections_live_properties() {
            let mut f = Fixture::new(Security::Psk);
            f.detect(Security::Psk);
            let other = "/org/freedesktop/NetworkManager/AccessPoint/9";
            add_ap(&mut f.objects, other, b"other", Security::Psk, 80);
            f.settings
                .insert(P2.to_owned(), saved(UUID2, b"other", Security::Psk));
            put(&mut f.objects, P2, CONNECTION, "VersionId", 1u64);
            bind(&mut f.objects, P2, UUID2, other, 100, 2);
            f.refresh();
            let details = &f.engine.last_state["details"][f.key()];
            assert_eq!(details["identity"], Value::Null);
            assert_eq!(details["profileUuid"], UUID);
            assert_eq!(details.as_object().unwrap().len(), 5);
            for field in [
                "ipv4",
                "ipv6",
                "dns",
                "gateway4",
                "mac",
                "bssid",
                "activePath",
                "frequency",
            ] {
                assert!(details[field].is_null());
            }
            let active = f
                .engine
                .entries
                .iter()
                .find(|entry| entry.ssid == b"other")
                .unwrap();
            assert_eq!(
                active.details(&f.engine.snapshot)["ipv4"],
                json!(["192.0.2.77/24"])
            );
            put(
                &mut f.objects,
                C,
                ACTIVE,
                "Ip4Config",
                dbus_path("/").unwrap(),
            );
            f.refresh();
            let active = f
                .engine
                .entries
                .iter()
                .find(|entry| entry.ssid == b"other")
                .unwrap();
            assert_eq!(active.details(&f.engine.snapshot)["ipv4"], json!([]));
        }
        #[test]
        fn connection_construction_and_password_rules_match_native_shapes() {
            for security in [
                Security::Open,
                Security::Wep,
                Security::Psk,
                Security::Sae,
                Security::Owe,
            ] {
                let mut f = Fixture::new(security);
                f.detect(security);
                let password = if security == Security::Wep {
                    Some("abcde")
                } else if security.password() {
                    Some(PASSWORD)
                } else {
                    None
                };
                let (uuid, settings) =
                    new_connection(&f.engine.snapshot, &f.engine.entries[0], password).unwrap();
                let value = settings_json(&settings);
                assert_eq!(uuid.len(), 36);
                assert_eq!(value["connection"]["uuid"], uuid);
                assert_eq!(value["connection"]["interface-name"], "wlan-test");
                assert_eq!(value["connection"]["autoconnect"], true);
                assert_eq!(value["ipv4"]["method"], "auto");
                assert_eq!(value["ipv6"]["method"], "auto");
                assert!(value[WIFI]["bssid"].is_null());
                assert_eq!(value[WIFI]["ssid"], json!(b"fixture".to_vec()));
                if matches!(security, Security::Sae | Security::Owe) {
                    assert_eq!(value[SECURITY]["pmf"], 3);
                }
            }
            for (security, password, success) in [
                (Security::Wep, "0123456789", true),
                (Security::Wep, "passphrase", true),
                (Security::Psk, "short", false),
                (Security::Sae, "x", true),
                (Security::Sae, PASSWORD, true),
                (Security::Psk, "bad\0secret", false),
            ] {
                let mut settings = saved(UUID, b"fixture", security);
                insert(&mut settings, SECURITY, "wep-tx-keyidx", 2u32);
                assert_eq!(set_password(&mut settings, password).is_ok(), success);
                if success && security == Security::Wep {
                    assert_eq!(settings_json(&settings)[SECURITY]["wep-key2"], password);
                }
            }
            let mut settings = saved(UUID, b"fixture", Security::Sae);
            assert!(set_password(&mut settings, &"x".repeat(100)).is_ok());
            let mut settings = saved(UUID, b"fixture", Security::Psk);
            assert!(set_password(&mut settings, &"a".repeat(64)).is_ok());
            assert!(set_password(&mut settings, &"g".repeat(64)).is_err());
        }
        #[test]
        fn descriptors_are_native_and_ambiguous_credentials_are_not_authorized() {
            let mut f = Fixture::new(Security::Psk);
            let row = &f.engine.last_state["rows"][0];
            assert_eq!(row["canActivate"], true);
            assert_eq!(row["canShare"], true);
            assert_eq!(row["acceptsPassword"], true);
            assert_eq!(row["requiresPassword"], false);
            assert_eq!(row["canDisconnect"], false);
            assert_eq!(row["canForget"], true);
            f.settings
                .insert(P2.to_owned(), saved(UUID2, b"fixture", Security::Psk));
            put(&mut f.objects, P2, CONNECTION, "VersionId", 1u64);
            f.refresh();
            assert_eq!(f.engine.last_state["rows"][0]["acceptsPassword"], false);
            let key = f.key();
            f.request(
                1,
                "activate",
                json!([key, { "profileUuid": UUID2, "password": PASSWORD }]),
            );
            assert!(
                f.result(1).unwrap()["error"]["message"]
                    .as_str()
                    .unwrap()
                    .contains("Multiple")
            );
            assert!(f.calls.is_empty());
            f.settings
                .insert(P2.to_owned(), saved(UUID, b"fixture", Security::Psk));
            f.refresh();
            assert_eq!(f.engine.last_state["rows"][0]["canShare"], false);
        }
        #[test]
        fn saved_secret_guards_allow_revision_rename_and_detection_but_reject_context_changes() {
            let mut f = Fixture::new(Security::Wep);
            let key = f.key();
            let guard = SecretGuard::capture(
                &f.engine.snapshot,
                &f.engine.entries,
                &key,
                None,
                Some(UUID),
                &f.engine.busy,
            )
            .unwrap();
            put(&mut f.objects, P, CONNECTION, "VersionId", 8u64);
            insert(
                f.settings.get_mut(P).unwrap(),
                "connection",
                "id",
                "Renamed".to_owned(),
            );
            f.detect(Security::Wep);
            guard
                .validate(&f.engine.snapshot, &f.engine.entries, &f.engine.busy)
                .unwrap();
            type Change = fn(&mut Fixture);
            let changes: &[Change] = &[
                |f| {
                    f.engine.snapshot.profiles[0].incarnation += 1;
                },
                |f| {
                    f.engine.snapshot.devices[0].incarnation += 1;
                },
                |f| {
                    f.engine.snapshot.devices[0].iface = "replacement".to_owned();
                },
                |f| {
                    f.engine.snapshot.wifi_enabled = false;
                },
                |f| {
                    f.engine.snapshot.wifi_hardware_enabled = false;
                },
                |f| {
                    f.engine.snapshot.profiles[0].uuid = UUID2.to_owned();
                },
                |f| {
                    f.engine.snapshot.profiles[0].value[WIFI]["hidden"] = json!(false);
                },
                |f| {
                    f.engine.snapshot.profiles[0].value[SECURITY]["wep-tx-keyidx"] = json!(2);
                },
                |f| {
                    f.engine.snapshot.profiles[0].value["connection"]["interface-name"] =
                        json!("other");
                },
                |f| {
                    f.engine.snapshot.profiles[0].ssid = b"different".to_vec();
                },
                |f| {
                    f.engine.snapshot.profiles.clear();
                },
            ];
            for change in changes {
                let mut f = Fixture::new(Security::Wep);
                let key = f.key();
                let guard = SecretGuard::capture(
                    &f.engine.snapshot,
                    &f.engine.entries,
                    &key,
                    None,
                    Some(UUID),
                    &f.engine.busy,
                )
                .unwrap();
                change(&mut f);
                assert!(
                    guard
                        .validate(&f.engine.snapshot, &f.engine.entries, &f.engine.busy)
                        .is_err()
                );
            }
        }
        #[test]
        fn secrets_are_explicit_exact_uuid_local_and_guarded_after_reply() {
            let mut f = Fixture::new(Security::Psk);
            f.settings
                .insert(P2.to_owned(), saved(UUID2, b"fixture", Security::Psk));
            put(&mut f.objects, P2, CONNECTION, "VersionId", 1u64);
            f.refresh();
            assert!(f.calls.is_empty());
            let key = f.key();
            f.request(1, "readSavedSecret", json!([key, UUID2]));
            let message = f.take_call("GetSecrets");
            assert_eq!(message.path().unwrap().to_string(), P2);
            assert_eq!(message.read1::<String>().unwrap(), SECURITY);
            put(&mut f.objects, P2, CONNECTION, "VersionId", 2u64);
            let mut secrets = Settings::new();
            insert(&mut secrets, SECURITY, "psk", PASSWORD.to_owned());
            f.reply(message.method_return().append1(secrets));
            assert_eq!(f.result(1).unwrap()["result"]["password"], PASSWORD);
            assert!(!f.engine.last_state.to_string().contains(PASSWORD));
            assert!(!f.engine.last_state.to_string().contains("psk-flags"));
            f.request(2, "readSavedSecret", json!([f.key(), UUID]));
            let message = f.take_call("GetSecrets");
            insert(f.settings.get_mut(P).unwrap(), WIFI, "hidden", false);
            let mut secrets = Settings::new();
            insert(&mut secrets, SECURITY, "psk", PASSWORD.to_owned());
            f.reply(message.method_return().append1(secrets));
            assert!(f.result(2).unwrap().get("error").is_some());
            assert!(!f.result(2).unwrap().to_string().contains(PASSWORD));
            let mut open = Fixture::new(Security::Open);
            open.request(3, "readSavedSecret", json!([open.key(), UUID]));
            assert!(open.calls.is_empty());
            assert_eq!(
                open.result(3).unwrap()["result"],
                json!({ "password": "", "authentication": "nopass", "hidden": true })
            );
        }
        #[test]
        fn connected_reads_pin_complete_identity_and_profile_incarnation() {
            let mut f = Fixture::new(Security::Psk);
            f.detect(Security::Psk);
            bind(&mut f.objects, P, UUID, A, 100, 2);
            f.refresh();
            let key = f.key();
            let identity = f.engine.entries[0].identity(&f.engine.snapshot).unwrap();
            f.request(1, "readSecret", json!([key, identity]));
            let message = f.take_call("GetSecrets");
            put(
                &mut f.objects,
                A,
                AP_INTERFACE,
                "HwAddress",
                "02:00:00:00:99:99".to_owned(),
            );
            let mut secrets = Settings::new();
            insert(&mut secrets, SECURITY, "psk", PASSWORD.to_owned());
            f.reply(message.method_return().append1(secrets));
            assert!(
                f.result(1).unwrap()["error"]["message"]
                    .as_str()
                    .unwrap()
                    .contains("active connection changed")
            );
            assert!(!f.result(1).unwrap().to_string().contains(PASSWORD));
        }
        #[test]
        fn cancellation_timeout_and_eof_never_continue_a_native_request() {
            let mut f = Fixture::new(Security::Psk);
            f.detect(Security::Psk);
            f.request(1, "activate", json!([f.key(), { "password": PASSWORD }]));
            let message = f.take_call("Update2");
            let (settings, flags, args) = message.read3::<Settings, u32, PropMap>().unwrap();
            assert_eq!(flags, 1);
            assert_eq!(args["version-id"].0.as_u64(), Some(7));
            assert_eq!(settings_json(&settings)[SECURITY]["psk"], PASSWORD);
            f.engine
                .enqueue(br#"{"method":"cancel","args":[1]}"#, f.now);
            f.reply(message.method_return().append1(PropMap::new()));
            assert_eq!(f.result(1).unwrap()["error"]["code"], "cancelled");
            assert!(f.calls.is_empty());
            assert!(f.engine.busy.is_empty());
            f.request(2, "readSavedSecret", json!([f.key(), UUID]));
            let message = f.take_call("GetSecrets");
            f.now += REQUEST_TIME;
            f.pump();
            assert_eq!(f.result(2).unwrap()["error"]["code"], "timeout");
            let mut secrets = Settings::new();
            insert(&mut secrets, SECURITY, "psk", PASSWORD.to_owned());
            f.reply(message.method_return().append1(secrets));
            assert!(!f.result(2).unwrap().to_string().contains(PASSWORD));
            f.engine.stop();
            let sent = f.bus.serial.get();
            f.engine.tick(&f.bus, f.now + REQUEST_TIME);
            f.engine
                .enqueue(br#"{"id":9,"method":"toggleWifi","args":[]}"#, f.now);
            assert_eq!(f.bus.serial.get(), sent);
            assert!(f.engine.output.is_empty());
        }
        #[test]
        fn activation_waits_for_exact_binding_and_ignores_previous_failed_state() {
            let mut f = Fixture::new(Security::Psk);
            f.detect(Security::Psk);
            put(&mut f.objects, D, DEVICE, "State", 120u32);
            f.refresh();
            f.request(1, "activate", json!([f.key(), { "profileUuid": UUID }]));
            let message = f.take_call("ActivateConnection");
            let (profile, device, specific) = message
                .read3::<Path<'static>, Path<'static>, Path<'static>>()
                .unwrap();
            assert_eq!(&*profile, P);
            assert_eq!(&*device, D);
            assert_eq!(&*specific, A);
            assert_eq!(message.destination().unwrap().to_string(), ":1.42");
            bind(&mut f.objects, P, UUID, A, 120, 1);
            put(
                &mut f.objects,
                D,
                DEVICE,
                "ActiveConnection",
                dbus_path("/").unwrap(),
            );
            f.reply(message.method_return().append1(dbus_path(C).unwrap()));
            assert!(f.result(1).is_none());
            put(&mut f.objects, C, ACTIVE, "State", 2u32);
            put(&mut f.objects, D, DEVICE, "State", 100u32);
            f.refresh();
            assert!(f.result(1).is_none());
            put(
                &mut f.objects,
                D,
                DEVICE,
                "ActiveConnection",
                dbus_path(C).unwrap(),
            );
            f.refresh();
            assert_eq!(f.result(1).unwrap()["result"], Value::Null);
            assert!(f.engine.busy.is_empty());
        }
        #[test]
        fn activation_failure_authentication_and_uuid_replacement_are_not_success() {
            for replacement in [false, true] {
                let mut f = Fixture::new(Security::Psk);
                f.detect(Security::Psk);
                f.request(1, "activate", json!([f.key(), {}]));
                let message = f.take_call("ActivateConnection");
                bind(
                    &mut f.objects,
                    P,
                    if replacement { UUID2 } else { UUID },
                    A,
                    50,
                    if replacement { 2 } else { 4 },
                );
                f.engine.reasons.insert(C.to_owned(), 9);
                f.reply(message.method_return().append1(dbus_path(C).unwrap()));
                let error = &f.result(1).unwrap()["error"];
                assert_eq!(
                    error["code"],
                    if replacement {
                        "network_error"
                    } else {
                        "authentication"
                    }
                );
                assert!(f.calls.is_empty());
            }
        }
        #[test]
        fn guarded_profile_updates_cannot_activate_a_replacement() {
            let mut f = Fixture::new(Security::Psk);
            f.detect(Security::Psk);
            f.request(1, "activate", json!([f.key(), { "password": PASSWORD }]));
            let message = f.take_call("Update2");
            f.engine.incarnations.remove(P);
            f.reply(message.method_return().append1(PropMap::new()));
            assert!(f.result(1).unwrap().get("error").is_some());
            assert!(f.calls.is_empty());
        }
        #[test]
        fn closing_after_disconnect_or_first_delete_stops_forget() {
            for connected in [false, true] {
                let mut f = Fixture::new(Security::Psk);
                f.detect(Security::Psk);
                f.settings
                    .insert(P2.to_owned(), saved(UUID2, b"fixture", Security::Psk));
                put(&mut f.objects, P2, CONNECTION, "VersionId", 1u64);
                if connected {
                    bind(&mut f.objects, P, UUID, A, 100, 2);
                }
                f.refresh();
                f.request(1, "forget", json!([f.key()]));
                let sent = f.take_call(if connected { "Disconnect" } else { "Delete" });
                f.engine
                    .enqueue(br#"{"method":"setOpened","args":[false]}"#, f.now);
                f.reply(sent.method_return());
                assert_eq!(f.result(1).unwrap()["error"]["code"], "cancelled");
                assert!(f.calls.is_empty());
            }
        }
        #[test]
        fn stale_confirmation_or_authentication_context_never_dispatches() {
            for change in ["replacement", "addition", "authentication"] {
                let mut f = Fixture::new(Security::Psk);
                let stamp = f.engine.last_state["rows"][0]["selectionStamp"].clone();
                match change {
                    "replacement" => {
                        f.engine.incarnations.remove(P);
                    }
                    "addition" => {
                        f.settings
                            .insert(P2.to_owned(), saved(UUID2, b"fixture", Security::Psk));
                        put(&mut f.objects, P2, CONNECTION, "VersionId", 1u64);
                    }
                    _ => insert(f.settings.get_mut(P).unwrap(), SECURITY, "pmf", 3u32),
                }
                f.refresh();
                assert_ne!(stamp, f.engine.last_state["rows"][0]["selectionStamp"]);
                let key = f.key();
                for (method, args) in [
                    ("forget", json!([key])),
                    ("activate", json!([key, {"password": PASSWORD}])),
                    ("readSavedSecret", json!([key, UUID])),
                ] {
                    f.engine.enqueue(
                        json!({"id":1,"method":method,"args":args,"expectedStamp":stamp})
                            .to_string()
                            .as_bytes(),
                        f.now,
                    );
                    assert!(f.engine.operations.is_empty());
                    assert!(f.result(1).unwrap().get("error").is_some());
                    f.pump();
                    assert!(f.calls.is_empty());
                }
            }
        }
        #[test]
        fn forget_deletes_all_original_profiles_sequentially_without_retargeting() {
            let mut f = Fixture::new(Security::Psk);
            f.detect(Security::Psk);
            f.settings
                .insert(P2.to_owned(), saved(UUID2, b"fixture", Security::Psk));
            put(&mut f.objects, P2, CONNECTION, "VersionId", 1u64);
            bind(&mut f.objects, P, UUID, A, 100, 2);
            f.refresh();
            f.request(1, "forget", json!([f.key()]));
            let disconnect = f.take_call("Disconnect");
            put(&mut f.objects, D, DEVICE, "State", 30u32);
            put(
                &mut f.objects,
                D,
                DEVICE,
                "ActiveConnection",
                dbus_path("/").unwrap(),
            );
            f.reply(disconnect.method_return());
            let deletion = f.take_call("Delete");
            assert_eq!(deletion.path().unwrap().to_string(), P);
            f.objects.remove(&dbus_path(P).unwrap());
            f.settings.remove(P);
            f.reply(deletion.method_return());
            let deletion = f.take_call("Delete");
            assert_eq!(deletion.path().unwrap().to_string(), P2);
            f.objects.remove(&dbus_path(P2).unwrap());
            f.settings.remove(P2);
            f.reply(deletion.method_return());
            assert!(f.result(1).unwrap().get("result").is_some());
            assert!(f.calls.is_empty());
            let mut f = Fixture::new(Security::Psk);
            f.request(2, "forget", json!([f.key()]));
            let deletion = f.take_call("Delete");
            f.engine.incarnations.remove(P);
            f.reply(deletion.method_return());
            assert!(f.result(2).unwrap().get("error").is_some());
            assert!(f.calls.is_empty());
        }
        #[test]
        fn scans_are_immediate_periodic_open_only_nonoverlapping_and_radio_guarded() {
            let mut f = Fixture::new(Security::Psk);
            assert!(f.calls.is_empty());
            f.engine
                .enqueue(br#"{"method":"setOpened","args":[true]}"#, f.now);
            f.pump();
            let first = f.take_call("RequestScan");
            f.now += Duration::from_secs(1);
            f.pump();
            assert!(f.calls.is_empty());
            f.reply(first.method_return());
            f.now += Duration::from_secs(13);
            f.pump();
            assert!(f.calls.is_empty());
            f.now += Duration::from_secs(1);
            f.pump();
            let second = f.take_call("RequestScan");
            f.engine
                .enqueue(br#"{"method":"setOpened","args":[false]}"#, f.now);
            f.reply(second.method_return());
            f.now += Duration::from_secs(45);
            f.pump();
            assert!(f.calls.is_empty());
            assert!(f.engine.scans.is_empty());
            put(&mut f.objects, ROOT, NM, "WirelessHardwareEnabled", false);
            f.refresh();
            f.engine
                .enqueue(br#"{"method":"setOpened","args":[true]}"#, f.now);
            f.pump();
            assert!(f.calls.is_empty());
            f.request(1, "toggleWifi", json!([]));
            assert!(
                f.result(1).unwrap()["error"]["message"]
                    .as_str()
                    .unwrap()
                    .contains("hardware")
            );
            assert!(f.calls.is_empty());
        }
        #[test]
        fn raw_errors_and_malformed_requests_are_secret_safe_and_queues_are_bounded() {
            let mut f = Fixture::new(Security::Psk);
            f.request(1, "readSavedSecret", json!([f.key(), UUID]));
            let message = f.take_call("GetSecrets");
            let name = "org.freedesktop.NetworkManager.SecretAgent.NoSecrets".into();
            let body = std::ffi::CString::new(PASSWORD).unwrap();
            f.reply(message.error(&name, &body));
            assert_eq!(f.result(1).unwrap()["error"]["code"], "authentication");
            assert!(!f.result(1).unwrap().to_string().contains(PASSWORD));
            for method in ["spawn", "refresh", "getIdentity", "details", "Delete"] {
                f.request(2, method, json!([PASSWORD]));
                assert_eq!(f.result(2).unwrap()["error"]["code"], "invalid_request");
            }
            let mut input = Input::default();
            input.feed(&vec![b'x'; REQUEST_LIMIT + 100], &mut f.engine, f.now);
            assert!(input.line.len() <= REQUEST_LIMIT);
            input.feed(b"\n", &mut f.engine, f.now);
            assert_eq!(
                f.engine.output.back().unwrap()["error"]["code"],
                "invalid_request"
            );
            f.engine.output.clear();
            for id in 10..10 + MAX_OPERATIONS as u32 + 5 {
                f.engine.enqueue(
                    json!({ "id": id, "method": "readSavedSecret", "args": [f.key(), UUID], "expectedStamp": f.engine.last_state["rows"][0]["selectionStamp"] })
                        .to_string()
                        .as_bytes(),
                    f.now,
                );
            }
            assert_eq!(f.engine.operations.len(), MAX_OPERATIONS);
            let mut output = Output::default();
            assert!(output.push(json!({"type":"state","rows":[1]})));
            assert!(output.push(json!({"type":"state","rows":[2]})));
            assert_eq!(output.replies.len(), 0);
            assert!(output.state.is_some());
            assert!(!output.push(json!({"id":1,"result":"x".repeat(OUTPUT_LIMIT)})));
        }

        #[test]
        fn forgetting_an_offline_profile_does_not_disconnect_an_unrelated_live_network() {
            let mut f = Fixture::new(Security::Psk);
            f.detect(Security::Psk);
            let other = "/org/freedesktop/NetworkManager/AccessPoint/9";
            add_ap(&mut f.objects, other, b"other", Security::Psk, 80);
            f.settings
                .insert(P2.to_owned(), saved(UUID2, b"other", Security::Psk));
            put(&mut f.objects, P2, CONNECTION, "VersionId", 1u64);
            bind(&mut f.objects, P2, UUID2, other, 100, 2);
            f.refresh();
            f.request(1, "forget", json!([f.key()]));
            let deletion = f.take_call("Delete");
            assert_eq!(deletion.path().unwrap().to_string(), P);
            f.objects.remove(&dbus_path(P).unwrap());
            f.settings.remove(P);
            f.reply(deletion.method_return());
            assert!(f.result(1).unwrap().get("result").is_some());
            assert!(f.calls.is_empty());
            let live = f
                .engine
                .entries
                .iter()
                .find(|entry| entry.ssid == b"other")
                .unwrap();
            assert_eq!(
                live.identity(&f.engine.snapshot).unwrap().profile_uuid,
                UUID2
            );
        }
        #[test]
        fn mixed_old_ap_and_new_active_profile_cannot_publish_the_new_networks_ip_details() {
            let mut f = Fixture::new(Security::Psk);
            f.detect(Security::Psk);
            f.settings
                .insert(P2.to_owned(), saved(UUID2, b"other", Security::Psk));
            put(&mut f.objects, P2, CONNECTION, "VersionId", 1u64);
            bind(&mut f.objects, P2, UUID2, A, 100, 2);
            f.refresh();
            let key = f.key();
            let row = f.engine.last_state["rows"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["key"] == key)
                .unwrap();
            assert_eq!(row["connected"], false);
            assert!(f.engine.last_state["identities"][&key].is_null());
            let details = &f.engine.last_state["details"][&key];
            assert_eq!(details["profileUuid"], UUID);
            assert!(details["ipv4"].is_null());
            assert!(details["gateway4"].is_null());
            assert!(details["activePath"].is_null());
            f.request(1, "forget", json!([key]));
            let deletion = f.take_call("Delete");
            assert_eq!(deletion.path().unwrap().to_string(), P);
            assert!(
                f.calls.is_empty(),
                "an unrelated active profile must not be disconnected"
            );
        }
        #[test]
        fn newly_constructed_connection_activation_binds_the_returned_profile_and_uuid() {
            let mut f = Fixture::new(Security::Open);
            f.settings.clear();
            f.objects.remove(&dbus_path(P).unwrap());
            f.detect(Security::Open);
            let key = f.key();
            assert_eq!(f.engine.last_state["rows"][0]["requiresPassword"], false);
            assert!(
                f.engine.last_state["details"]
                    .as_object()
                    .unwrap()
                    .is_empty(),
                "unsaved details are absent, not null"
            );
            f.request(1, "activate", json!([key, {}]));
            let message = f.take_call("AddAndActivateConnection");
            let (settings, device, ap) = message
                .read3::<Settings, Path<'static>, Path<'static>>()
                .unwrap();
            let uuid = text(&settings_json(&settings)["connection"], "uuid");
            assert_eq!(&*device, D);
            assert_eq!(&*ap, A);
            f.settings.insert(P.to_owned(), settings);
            put(&mut f.objects, P, CONNECTION, "VersionId", 1u64);
            bind(&mut f.objects, P, &uuid, A, 40, 1);
            f.reply(
                message
                    .method_return()
                    .append2(dbus_path(P).unwrap(), dbus_path(C).unwrap()),
            );
            assert!(f.result(1).is_none());
            bind(&mut f.objects, P, &uuid, A, 100, 2);
            f.refresh();
            assert!(f.result(1).unwrap().get("result").is_some());
            assert_eq!(f.engine.last_state["identities"][key]["profileUuid"], uuid);
        }
        #[test]
        fn saved_wep_retrieval_uses_only_the_selected_transmit_key() {
            let mut f = Fixture::new(Security::Wep);
            insert(
                f.settings.get_mut(P).unwrap(),
                SECURITY,
                "wep-tx-keyidx",
                2u32,
            );
            f.refresh();
            f.request(1, "readSavedSecret", json!([f.key(), UUID]));
            let message = f.take_call("GetSecrets");
            let mut secrets = Settings::new();
            insert(&mut secrets, SECURITY, "wep-key0", "wrong-key".to_owned());
            insert(&mut secrets, SECURITY, "wep-key2", "abcde".to_owned());
            f.reply(message.method_return().append1(secrets));
            assert_eq!(
                f.result(1).unwrap()["result"],
                json!({ "password":"abcde", "authentication":"WEP", "hidden":true })
            );
            assert!(!f.engine.last_state.to_string().contains("abcde"));
        }
        #[test]
        fn owner_replacement_cancels_without_replaying_a_secret_request() {
            let mut f = Fixture::new(Security::Psk);
            f.request(1, "readSavedSecret", json!([f.key(), UUID]));
            let message = f.take_call("GetSecrets");
            f.engine.set_owner(":1.43".to_owned(), f.now);
            let mut secrets = Settings::new();
            insert(&mut secrets, SECURITY, "psk", PASSWORD.to_owned());
            f.reply(message.method_return().append1(secrets));
            assert_eq!(f.result(1).unwrap()["error"]["code"], "unavailable");
            assert!(f.calls.is_empty());
            assert!(!f.result(1).unwrap().to_string().contains(PASSWORD));
        }
        #[test]
        fn ssid_display_uses_native_encoding_hints_without_losing_raw_identity() {
            assert_eq!(ssid_convert(&[0xe9], "iso-8859-1").unwrap(), "é");
            assert_eq!(ssid_encodings("ru_RU.UTF-8")[0], "koi8-r");
            assert_eq!(ssid_encodings("zh_TW.UTF-8")[0], "big5");
            assert_eq!(ssid_text("Coffee ☕".as_bytes()), "Coffee ☕");
            assert_eq!(ssid_text(b"hidden\0suffix"), "hidden");
        }

        struct PrivateBus {
            child: Child,
            address: String,
        }
        impl PrivateBus {
            fn new() -> Self {
                let mut child = Command::new("dbus-daemon")
                    .args(["--session", "--nofork", "--nopidfile", "--print-address=1"])
                    .stdout(Stdio::piped())
                    .stderr(Stdio::null())
                    .spawn()
                    .expect("private-bus fixture needs dbus-daemon");
                let stdout = child.stdout.take().unwrap();
                let (sender, receiver) = mpsc::sync_channel(1);
                thread::spawn(move || {
                    let mut address = String::new();
                    let _ = BufReader::new(stdout).read_line(&mut address);
                    let _ = sender.send(address);
                });
                let mut bus = Self {
                    child,
                    address: String::new(),
                };
                bus.address = receiver
                    .recv_timeout(Duration::from_secs(2))
                    .expect("private daemon startup deadline")
                    .trim()
                    .to_owned();
                assert!(bus.address.starts_with("unix:"));
                bus
            }
            fn connect(&self) -> Channel {
                let mut channel = Channel::open_private(&self.address).unwrap();
                channel.register().unwrap();
                channel
            }
        }
        impl Drop for PrivateBus {
            fn drop(&mut self) {
                let _ = self.child.kill();
                let _ = self.child.wait();
            }
        }
        fn read_frame(reader: &mut BufReader<UnixStream>) -> Value {
            let mut line = String::new();
            assert!(reader.read_line(&mut line).expect("JSONL read deadline") > 0);
            serde_json::from_str(&line).expect("complete JSONL frame")
        }
        fn read_until(
            reader: &mut BufReader<UnixStream>,
            predicate: impl Fn(&Value) -> bool,
        ) -> Value {
            for _ in 0..64 {
                let frame = read_frame(reader);
                if predicate(&frame) {
                    return frame;
                }
            }
            panic!("fixture frame limit");
        }
        #[test]
        fn private_dbus_jsonl_fixture_stays_responsive_to_cancel_late_reply_and_eof() {
            enum Control {
                Reply,
                Stop,
            }
            let private = PrivateBus::new();
            let exporter = private.connect();
            let reply = exporter
                .send_with_reply_and_block(
                    call(DBUS, DBUS_PATH, DBUS, "RequestName")
                        .unwrap()
                        .append2(NM, 0u32),
                    READ_TIME,
                )
                .unwrap();
            assert_eq!(reply.read1::<u32>().unwrap(), 1);
            let (control_tx, control_rx) = mpsc::channel();
            let (observed_tx, observed_rx) = mpsc::channel();
            let exporter_thread = thread::spawn(move || {
                let objects = basic_objects();
                let settings = saved(UUID, b"fixture", Security::Psk);
                let mut held = Vec::<Message>::new();
                let mut methods = Vec::new();
                let started = Instant::now();
                while started.elapsed() < Duration::from_secs(12) {
                    while let Ok(control) = control_rx.try_recv() {
                        match control {
                            Control::Stop => return methods,
                            Control::Reply => {
                                for message in held.drain(..) {
                                    let mut secrets = Settings::new();
                                    insert(&mut secrets, SECURITY, "psk", PASSWORD.to_owned());
                                    exporter
                                        .send(message.method_return().append1(secrets))
                                        .unwrap();
                                }
                            }
                        }
                    }
                    exporter
                        .read_write(Some(Duration::from_millis(10)))
                        .unwrap();
                    for _ in 0..64 {
                        let Some(message) = exporter.pop_message() else {
                            break;
                        };
                        if message.msg_type() != MessageType::MethodCall {
                            continue;
                        }
                        let method = member(&message);
                        methods.push(method.clone());
                        let response = match method.as_str() {
                            "GetManagedObjects" => {
                                Some(message.method_return().append1(clone_managed(&objects)))
                            }
                            "GetSettings" => {
                                Some(message.method_return().append1(clone_settings(&settings)))
                            }
                            "GetSecrets" => {
                                held.push(message);
                                observed_tx.send("GetSecrets").unwrap();
                                None
                            }
                            _ => {
                                panic!("unexpected private fixture mutation {method}");
                            }
                        };
                        if let Some(response) = response {
                            exporter.send(response).unwrap();
                        }
                    }
                }
                panic!("private exporter fixture deadline");
            });
            let (mut writer, service_input) = UnixStream::pair().unwrap();
            let (service_output, reader) = UnixStream::pair().unwrap();
            reader
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut reader = BufReader::new(reader);
            let (connect_tx, connect_rx) = mpsc::channel();
            connect_tx.send(Some(private.connect())).unwrap();
            let (done_tx, done_rx) = mpsc::channel();
            let runtime_thread = thread::spawn(move || {
                let status = run_loop(
                    service_input.as_raw_fd(),
                    service_output.as_raw_fd(),
                    connect_rx,
                );
                done_tx.send(status).unwrap();
            });
            let state = read_until(&mut reader, |frame| {
                frame["type"] == "state" && frame["available"] == true
            });
            let row = &state["rows"][0];
            let key = row["key"].as_str().unwrap().to_owned();
            assert_eq!(row["canShare"], true);
            assert!(row["selectionStamp"].is_string());
            writer
                .write_all(
                    json!({ "id": 1, "method": "readSavedSecret", "args": [key, UUID], "expectedStamp": row["selectionStamp"] })
                        .to_string()
                        .as_bytes(),
                )
                .unwrap();
            writer.write_all(b"\n").unwrap();
            assert_eq!(
                observed_rx.recv_timeout(Duration::from_secs(2)).unwrap(),
                "GetSecrets"
            );
            let cancelled_at = Instant::now();
            writer
                .write_all(b"{\"method\":\"cancel\",\"args\":[1]}\n")
                .unwrap();
            let cancellation = read_until(&mut reader, |frame| frame["id"] == 1);
            assert_eq!(cancellation["error"]["code"], "cancelled");
            assert!(cancelled_at.elapsed() < Duration::from_secs(1));
            control_tx.send(Control::Reply).unwrap();
            writer
                .write_all(b"{\"id\":20,\"method\":\"invalid\",\"args\":[]}\n")
                .unwrap();
            let fence = read_until(&mut reader, |frame| {
                assert!(!frame.to_string().contains(PASSWORD));
                frame["id"] == 20
            });
            assert_eq!(fence["error"]["code"], "invalid_request");
            writer
                .write_all(
                    json!({ "id": 2, "method": "readSavedSecret", "args": [key, UUID], "expectedStamp": row["selectionStamp"] })
                        .to_string()
                        .as_bytes(),
                )
                .unwrap();
            writer.write_all(b"\n").unwrap();
            assert_eq!(
                observed_rx.recv_timeout(Duration::from_secs(2)).unwrap(),
                "GetSecrets"
            );
            drop(writer);
            assert_eq!(
                done_rx
                    .recv_timeout(Duration::from_secs(1))
                    .expect("EOF must not wait on GetSecrets"),
                0
            );
            runtime_thread.join().unwrap();
            let mut remainder = String::new();
            reader.read_to_string(&mut remainder).unwrap();
            assert!(!remainder.contains(PASSWORD));
            control_tx.send(Control::Stop).unwrap();
            let methods = exporter_thread.join().unwrap();
            assert_eq!(
                methods
                    .iter()
                    .filter(|method| *method == "GetSecrets")
                    .count(),
                2
            );
            assert!(methods.iter().all(|method| matches!(
                method.as_str(),
                "GetManagedObjects" | "GetSettings" | "GetSecrets"
            )));
        }
        #[test]
        fn pending_bus_authentication_does_not_block_eof() {
            let (writer, service_input) = UnixStream::pair().unwrap();
            let (service_output, _reader) = UnixStream::pair().unwrap();
            let (_connect_tx, connect_rx) = mpsc::channel();
            let (done_tx, done_rx) = mpsc::channel();
            let worker = thread::spawn(move || {
                done_tx
                    .send(run_loop(
                        service_input.as_raw_fd(),
                        service_output.as_raw_fd(),
                        connect_rx,
                    ))
                    .unwrap();
            });
            drop(writer);
            assert_eq!(done_rx.recv_timeout(Duration::from_secs(1)).unwrap(), 0);
            worker.join().unwrap();
        }
    }
}

mod dashboard_backend {
    use serde::{Deserialize, Serialize};
    use serde_json::{Value, json};
    use std::{
        env, fs,
        io::{self, BufRead, Read, Write},
        os::unix::fs::{DirBuilderExt, OpenOptionsExt},
        path::{Path, PathBuf},
        process::{Child, Command, Stdio},
        sync::mpsc,
        time::{Duration, Instant},
    };
    const FRAME: usize = 128 * 1024;
    fn limited(path: &Path, limit: usize) -> io::Result<Vec<u8>> {
        let mut bytes = Vec::new();
        let f = fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(path)?;
        let meta = f.metadata()?;
        if !meta.is_file() || meta.len() > limit as u64 {
            return Err(io::ErrorKind::InvalidData.into());
        }
        f.take(limit as u64 + 1).read_to_end(&mut bytes)?;
        if bytes.len() > limit {
            return Err(io::ErrorKind::InvalidData.into());
        }
        Ok(bytes)
    }
    fn save(path: &Path, bytes: &[u8]) -> io::Result<()> {
        let parent = path.parent().ok_or(io::ErrorKind::InvalidInput)?;
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(parent)?;
        let tmp = parent.join(format!(".dashboard-{}.tmp", std::process::id()));
        let mut f = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(&tmp)?;
        let result = f
            .write_all(bytes)
            .and_then(|()| f.sync_all())
            .and_then(|()| fs::rename(&tmp, path));
        if result.is_err() {
            let _ = fs::remove_file(tmp);
        }
        result
    }
    fn home() -> PathBuf {
        env::var_os("XDG_STATE_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(env::var_os("HOME").unwrap_or_default()).join(".local/state")
            })
    }
    #[cfg(test)]
    thread_local! {static EVENTS: std::cell::RefCell<Vec<Value>>=const{std::cell::RefCell::new(Vec::new())};}
    #[cfg(test)]
    fn emit(v: Value) -> io::Result<()> {
        EVENTS.with(|e| e.borrow_mut().push(v));
        Ok(())
    }
    #[cfg(not(test))]
    fn emit(v: Value) -> io::Result<()> {
        writeln!(io::stdout().lock(), "{v}")
    }
    #[derive(Deserialize)]
    #[serde(tag = "op", rename_all = "camelCase", deny_unknown_fields)]
    enum Request {
        Notify { command: Notice },
        Active { active: bool, generation: u64 },
        Timeout { minutes: u16 },
    }
    fn cpu(s: &str) -> Option<(u64, u64)> {
        let mut it = s.lines().next()?.split_whitespace();
        if it.next()? != "cpu" {
            return None;
        }
        let v = it
            .map(str::parse::<u64>)
            .collect::<Result<Vec<_>, _>>()
            .ok()?;
        let idle = v.get(3)?.checked_add(*v.get(4).unwrap_or(&0))?;
        Some((
            v.iter().take(8).try_fold(0u64, |a, b| a.checked_add(*b))?,
            idle,
        ))
    }
    fn percent(prev: Option<(u64, u64)>, next: Option<(u64, u64)>) -> Option<f64> {
        let (t, i) = prev?;
        let (n, j) = next?;
        let dt = n.checked_sub(t)?;
        let busy = dt.checked_sub(j.checked_sub(i)?)?;
        (dt > 0).then(|| (100. * busy as f64 / dt as f64).round())
    }
    fn memory(s: &str) -> Option<[f64; 3]> {
        let get = |key: &str| {
            s.lines().find_map(|l| {
                let mut f = l.split_whitespace();
                if f.next()? != key {
                    return None;
                }
                let n = f.next()?.parse::<u64>().ok()?;
                (f.next() == Some("kB") && f.next().is_none()).then_some(n)
            })
        };
        let t = get("MemTotal:")?;
        if t == 0 {
            return None;
        }
        let a = get("MemAvailable:").or_else(|| {
            get("MemFree:")?
                .checked_add(get("Buffers:").unwrap_or(0))?
                .checked_add(get("Cached:").unwrap_or(0))
        })?;
        let u = t.saturating_sub(a) as f64;
        Some([
            (100. * u / t as f64).round(),
            u / 1048576.,
            t as f64 / 1048576.,
        ])
    }
    fn disk() -> Option<[f64; 3]> {
        let mut s = std::mem::MaybeUninit::uninit(); // statvfs initializes the structure on success.
        if unsafe { libc::statvfs(c"/".as_ptr(), s.as_mut_ptr()) } != 0 {
            return None;
        }
        let s = unsafe { s.assume_init() };
        disk_usage(s.f_blocks, s.f_bfree, s.f_bavail, s.f_frsize)
    }
    fn disk_usage(blocks: u64, free: u64, avail: u64, size: u64) -> Option<[f64; 3]> {
        let used = blocks.checked_sub(free)? as f64;
        let avail = used + avail as f64;
        let scale = size as f64 / 1073741824.;
        (blocks > 0 && avail > 0. && size > 0).then(|| {
            [
                (100. * used / avail).ceil(),
                used * scale,
                blocks as f64 * scale,
            ]
        })
    }
    fn temperature(s: &str) -> Option<f64> {
        let s = s.trim();
        if !s.bytes().all(|b| b.is_ascii_digit() || b == b'.') {
            return None;
        }
        let t = s.parse::<f64>().ok()?;
        (t.is_finite() && t > 0.).then(|| if t > 1000. { t / 1000. } else { t })
    }
    fn sample(prev: &mut Option<(u64, u64)>, d: [f64; 3]) -> Value {
        let text = |p: &str| {
            limited(Path::new(p), 1024 * 1024)
                .ok()
                .and_then(|b| String::from_utf8(b).ok())
                .unwrap_or_default()
        };
        let n = cpu(&text("/proc/stat"));
        let c = percent(*prev, n);
        *prev = n;
        let m = memory(&text("/proc/meminfo")).unwrap_or([-1.; 3]);
        let temp = temperature(&text("/sys/class/thermal/thermal_zone0/temp"));
        json!({"event":"metrics","data":{"cpuPercent":c.unwrap_or(-1.),"memoryPercent":m[0],"memoryUsedGiB":m[1],"memoryTotalGiB":m[2],"diskPercent":d[0],"diskUsedGiB":d[1],"diskTotalGiB":d[2],"cpuTemperatureC":temp.unwrap_or(-1.)}})
    }
    struct Sampler {
        active: bool,
        generation: u64,
        next: Instant,
        disk_due: Instant,
        disk: [f64; 3],
        previous: Option<(u64, u64)>,
    }
    impl Sampler {
        fn new(now: Instant) -> Self {
            Self {
                active: false,
                generation: 0,
                next: now,
                disk_due: now,
                disk: [-1.; 3],
                previous: None,
            }
        }
        fn activate(&mut self, active: bool, generation: u64, now: Instant) {
            self.generation = generation;
            if self.active != active {
                self.active = active;
                self.next = now;
                self.disk_due = now;
                self.previous = None;
            }
        }
        fn tick(
            &mut self,
            now: Instant,
            read_disk: impl FnOnce() -> Option<[f64; 3]>,
            read_metrics: impl FnOnce(&mut Option<(u64, u64)>, [f64; 3]) -> Value,
        ) -> Option<Value> {
            if !self.active || now < self.next {
                return None;
            }
            if now >= self.disk_due {
                self.disk = read_disk().unwrap_or([-1.; 3]);
                self.disk_due = now + Duration::from_secs(30);
            }
            let mut event = read_metrics(&mut self.previous, self.disk);
            event["generation"] = json!(self.generation);
            self.next = now + Duration::from_millis(2500);
            Some(event)
        }
        fn wait(&self, now: Instant) -> Duration {
            if self.active {
                self.next.saturating_duration_since(now)
            } else {
                Duration::from_secs(3600)
            }
        }
    }
    struct Watch(Child);
    impl Drop for Watch {
        fn drop(&mut self) {
            if self.0.try_wait().ok().flatten().is_some() {
                return;
            }
            // Unreaped child PID cannot be reused. Prefer graceful compositor disconnect.
            unsafe {
                libc::kill(self.0.id() as libc::pid_t, libc::SIGTERM);
            }
            for _ in 0..20 {
                if self.0.try_wait().ok().flatten().is_some() {
                    return;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    fn valid(m: u16) -> bool {
        matches!(m, 0 | 10 | 30 | 60 | 120 | 180 | 360)
    }
    struct Idle {
        path: PathBuf,
        watcher: std::ffi::OsString,
        systemctl: String,
        child: Option<Watch>,
        minutes: u16,
        saved_minutes: u16,
    }
    impl Idle {
        fn command(&self, m: u16) -> Command {
            let ctl = self.systemctl.replace('\'', "'\\''");
            let mut cmd = Command::new(&self.watcher);
            cmd.args([
                "-w",
                "-C",
                "/dev/null",
                "timeout",
                &(u32::from(m) * 60).to_string(),
                &format!("exec '{ctl}' suspend"),
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null());
            // If the backend dies without unwinding, do not orphan the idle watcher.
            let parent = std::process::id();
            unsafe {
                std::os::unix::process::CommandExt::pre_exec(&mut cmd, move || {
                    if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM) != 0 {
                        return Err(io::Error::last_os_error());
                    }
                    if libc::getppid() as u32 != parent {
                        return Err(io::ErrorKind::Interrupted.into());
                    }
                    Ok(())
                });
            }
            cmd
        }
        fn start(&self, m: u16) -> io::Result<Option<Watch>> {
            if !valid(m) {
                return Err(io::ErrorKind::InvalidInput.into());
            }
            if m == 0 {
                return Ok(None);
            }
            // swayidle has no readiness handshake. Catch immediate startup failures
            // while the previous watcher and its saved selection are still intact.
            let mut candidate = Watch(self.command(m).spawn()?);
            let deadline = Instant::now() + Duration::from_millis(150);
            loop {
                if candidate.0.try_wait()?.is_some() {
                    return Err(io::Error::other("idle watcher exited during startup"));
                }
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    return Ok(Some(candidate));
                }
                std::thread::sleep(remaining.min(Duration::from_millis(10)));
            }
        }
        fn restore(path: PathBuf, watcher: std::ffi::OsString, systemctl: String) -> Self {
            let mut idle = Self {
                path,
                watcher,
                systemctl,
                child: None,
                minutes: 0,
                saved_minutes: 0,
            };
            let m = limited(&idle.path, 1024)
                .ok()
                .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
                .filter(|v| v["version"] == 1)
                .and_then(|v| u16::try_from(v["minutes"].as_u64()?).ok())
                .unwrap_or(0);
            idle.saved_minutes = if valid(m) { m } else { 0 };
            if let Ok(child) = idle.start(m) {
                idle.child = child;
                idle.minutes = m
            }
            idle
        }
        fn set(&mut self, m: u16) -> io::Result<bool> {
            if m == self.minutes && m == self.saved_minutes {
                return Ok(false);
            }
            let candidate = self.start(m)?;
            save(
                &self.path,
                json!({"version":1,"minutes":m}).to_string().as_bytes(),
            )?;
            self.child = candidate;
            self.saved_minutes = m;
            self.minutes = m;
            Ok(true)
        }
        fn exited(&mut self) -> io::Result<bool> {
            if let Some(w) = self.child.as_mut()
                && w.0.try_wait()?.is_some()
            {
                self.child = None;
                self.minutes = 0;
                return Ok(true);
            }
            Ok(false)
        }
    }
    fn serve() -> io::Result<()> {
        let (tx, rx) = mpsc::sync_channel(8);
        let closed = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let eof = closed.clone();
        std::thread::spawn(move || {
            let mut input = io::stdin().lock();
            loop {
                let mut b = Vec::new();
                if input
                    .by_ref()
                    .take(FRAME as u64 + 1)
                    .read_until(b'\n', &mut b)
                    .is_err()
                    || b.len() > FRAME
                    || b.last() != Some(&b'\n')
                {
                    break;
                }
                if tx.send(serde_json::from_slice::<Request>(&b)).is_err() {
                    break;
                }
            }
            eof.store(true, std::sync::atomic::Ordering::Release);
        });
        let path = home().join("dashboard/screen-timeout.json");
        let mut idle = Idle::restore(
            path,
            env::var_os("SWAYIDLE").unwrap_or_else(|| "swayidle".into()),
            env::var("SYSTEMCTL").unwrap_or_else(|_| "systemctl".into()),
        );
        let report = |minutes| emit(json!({"event":"timeout","minutes":minutes}));
        report(idle.minutes)?;
        let mut center = Center::default();
        let mut sampler = Sampler::new(Instant::now());
        loop {
            if closed.load(std::sync::atomic::Ordering::Acquire) {
                break;
            }
            let now = Instant::now();
            if let Some(event) = sampler.tick(now, disk, sample) {
                emit(event)?;
            }
            if idle.exited()? {
                report(0)?
            }
            let now = Instant::now();
            let mut delay = sampler.wait(now);
            if idle.child.is_some() {
                delay = delay.min(Duration::from_secs(1))
            }
            let delay = center.tick(now, delay)?;
            match rx.recv_timeout(delay) {
                _ if closed.load(std::sync::atomic::Ordering::Acquire) => break,
                Ok(Ok(Request::Notify { command })) => center.handle(command)?,
                Ok(Ok(Request::Active {
                    active: a,
                    generation: g,
                })) => {
                    sampler.activate(a, g, Instant::now());
                }
                Ok(Ok(Request::Timeout { minutes })) => match idle.set(minutes) {
                    Ok(true) => report(minutes)?,
                    Ok(false) => (),
                    Err(_) => emit(json!({"event":"error","code":"timeout"}))?,
                },
                Ok(Err(_)) => emit(json!({"event":"error","code":"request"}))?,
                Err(mpsc::RecvTimeoutError::Timeout) => (),
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
        Ok(())
    }
    pub(super) fn run() -> i32 {
        if serve().is_ok() { 0 } else { 1 }
    }
    #[derive(Serialize, Deserialize)]
    struct NativeAction {
        identifier: String,
        text: String,
    }
    #[derive(Serialize, Deserialize)]
    struct Snapshot {
        app: String,
        summary: String,
        body: String,
        icon: String,
        urgency: u8,
        transient: bool,
        timeout: i32,
        actions: Vec<NativeAction>,
    }
    #[derive(Deserialize)]
    #[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
    enum Notice {
        Init {
            dnd: bool,
            user: bool,
        },
        Received {
            native: u32,
            time: u64,
            token: String,
            row: Snapshot,
            restore: bool,
        },
        Resolved {
            token: String,
            reason: u32,
        },
        Dnd {
            value: bool,
        },
        SeedAck {
            value: bool,
        },
        Toggle,
        Hover {
            id: String,
            token: String,
            value: bool,
        },
        Dismiss {
            id: String,
            token: String,
        },
        Action {
            id: String,
            token: String,
            identifier: String,
        },
        Clear {
            #[serde(default)]
            instances: Vec<Instance>,
        },
    }
    #[derive(Deserialize)]
    struct Instance {
        native: u32,
        token: String,
    }
    struct Live {
        native: u32,
        token: String,
        row: Value,
        hover: bool,
        due: Option<Instant>,
    }
    impl Live {
        fn restart(&mut self) {
            let ms = self.row["timeout"].as_i64().unwrap_or(-1);
            self.due = (!self.hover && ms != 0 && self.row["urgency"] != 2).then(|| {
                Instant::now() + Duration::from_millis(if ms < 0 { 8000 } else { ms as u64 })
            });
        }
        fn operation(&self, op: &str, argument: &str) -> io::Result<()> {
            emit(
                json!({"event":"native","operation":op,"native":self.native,"token":self.token,"argument":argument}),
            )
        }
    }
    #[derive(Default)]
    struct Center {
        history: Vec<Value>,
        popups: Vec<Value>,
        live: Vec<Live>,
        path: PathBuf,
        dnd: bool,
        saved: Vec<u8>,
        pending_seed: Option<bool>,
    }
    fn archived(mut v: Value) -> Value {
        v["closed"] = json!(true);
        v["actions"] = json!([]);
        if v["icon"]
            .as_str()
            .is_some_and(|s| s.starts_with("image://"))
        {
            v["icon"] = json!("")
        }
        v.as_object_mut().unwrap().remove("token");
        v
    }
    fn normalize(v: &Value) -> Option<Value> {
        let id = v.get("id")?.as_str()?;
        if id.is_empty() || v["transient"] == true {
            return None;
        }
        Some(archived(
            json!({"id":id,"app":v["app"].as_str()?,"summary":v["summary"].as_str()?,"body":v["body"].as_str()?,"icon":v["icon"].as_str().unwrap_or(""),"urgency":v["urgency"].as_u64().filter(|n|*n<=2).unwrap_or(1),"transient":false,"timeout":v["timeout"].as_i64().filter(|n|(-1..=2147483647).contains(n)).unwrap_or(-1)}),
        ))
    }
    impl Center {
        fn restore(
            &mut self,
            path: PathBuf,
            legacy: &Path,
            dnd: bool,
            user: bool,
        ) -> io::Result<()> {
            self.path = path;
            self.dnd = dnd;
            let mut first = false;
            let bytes = limited(&self.path, 8 * 1024 * 1024).or_else(|e| {
                if e.kind() != io::ErrorKind::NotFound {
                    return Err(e);
                }
                first = true;
                limited(legacy, 8 * 1024 * 1024)
            });
            if let Some(v) = bytes
                .ok()
                .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
                .filter(|v| v["version"] == 1)
                && let Some(rows) = v["history"].as_array()
            {
                for row in rows.iter().filter_map(normalize) {
                    if !self.history.iter().any(|r| r["id"] == row["id"]) {
                        self.history.push(row)
                    }
                    if self.history.len() == 200 {
                        break;
                    }
                }
                if !user {
                    self.pending_seed = if first {
                        Some(v["dnd"] == true)
                    } else {
                        v["pendingSeed"].as_bool()
                    };
                    if let Some(value) = self.pending_seed {
                        self.dnd = value;
                        emit(json!({"event":"seed","value":value}))?;
                    }
                }
            }
            Ok(())
        }
        fn publish(&mut self) -> io::Result<()> {
            let mut state = json!({"version":1,"dnd":self.dnd,"history":self.history.iter().cloned().map(archived).collect::<Vec<_>>()});
            if let Some(value) = self.pending_seed {
                state["pendingSeed"] = json!(value);
            }
            let archive = serde_json::to_vec(&state)?;
            if archive != self.saved {
                if archive.len() <= 8 * 1024 * 1024 && save(&self.path, &archive).is_ok() {
                    self.saved = archive
                } else {
                    emit(json!({"event":"error","code":"archive"}))?
                }
            }
            emit(json!({"event":"notifications","history":self.history,"popups":self.popups}))
        }
        fn tick(&mut self, now: Instant, mut delay: Duration) -> io::Result<Duration> {
            for l in &mut self.live {
                if let Some(t) = l.due {
                    if t <= now {
                        l.operation("expire", "")?;
                        l.due = None
                    } else {
                        delay = delay.min(t - now)
                    }
                }
            }
            Ok(delay)
        }
        fn handle(&mut self, cmd: Notice) -> io::Result<()> {
            match cmd {
                Notice::Init { dnd, user } if self.path.as_os_str().is_empty() => {
                    let path = env::var_os("NOTIFICATION_HISTORY")
                        .map(PathBuf::from)
                        .unwrap_or_else(|| home().join("dashboard/notifications.json"));
                    let legacy = env::var_os("LEGACY_NOTIFICATION_HISTORY")
                        .map(PathBuf::from)
                        .unwrap_or_else(|| home().join("quickshell/notifications.json"));
                    self.restore(path, &legacy, dnd, user)?;
                }
                _ if self.path.as_os_str().is_empty() => return Ok(()),
                Notice::Init { .. } => return Ok(()),
                Notice::Received {
                    native,
                    time,
                    token,
                    row,
                    restore,
                } => {
                    if token.len() > 64 || token.is_empty() {
                        return Ok(());
                    }
                    let mut row = serde_json::to_value(row)?;
                    row["closed"] = json!(false);
                    let prev = self
                        .live
                        .iter()
                        .position(|l| l.native == native)
                        .map(|i| self.live.remove(i));
                    let mut l = Live {
                        native,
                        token: token.clone(),
                        row: Value::Null,
                        hover: prev.as_ref().is_some_and(|p| p.hover),
                        due: None,
                    };
                    if restore {
                        let prefix = format!("astal:{native}:");
                        let found = self.history.iter_mut().find(|r| {
                            let key = r["id"].as_str().unwrap_or("");
                            if !key
                                .strip_prefix(&prefix)
                                .and_then(|s| s.split(':').next()?.parse::<u64>().ok())
                                .is_some_and(|t| t <= time)
                            {
                                return false;
                            }
                            row["id"] = json!(key);
                            archived(row.clone()) == archived((*r).clone())
                        });
                        let Some(r) = found else {
                            return l.operation("expire", "");
                        };
                        if row["icon"]
                            .as_str()
                            .is_some_and(|s| s.starts_with("image://"))
                        {
                            row["icon"] = json!("");
                        }
                        row["token"] = json!(token);
                        *r = row.clone();
                    } else {
                        row["id"] = prev
                            .map(|l| l.row["id"].clone())
                            .unwrap_or_else(|| json!(format!("astal:{native}:{time}:{token}")));
                        row["token"] = json!(token);
                        for (rows, admit, max) in [
                            (&mut self.history, row["transient"] != true, 200),
                            (&mut self.popups, !self.dnd, 5),
                        ] {
                            rows.retain(|r| r["id"] != row["id"]);
                            if admit {
                                rows.insert(0, row.clone());
                                rows.truncate(max)
                            }
                        }
                    }
                    l.row = row;
                    l.restart();
                    self.live.push(l)
                }
                Notice::Resolved { token, reason } => {
                    let Some(i) = self.live.iter().position(|l| l.token == token) else {
                        return Ok(());
                    };
                    let id = self.live.remove(i).row["id"].clone();
                    self.popups.retain(|r| r["id"] != id);
                    if reason == 1 {
                        if let Some(r) = self.history.iter_mut().find(|r| r["id"] == id) {
                            *r = archived(r.clone())
                        }
                    } else {
                        self.history.retain(|r| r["id"] != id)
                    }
                }
                Notice::SeedAck { .. } if self.pending_seed.is_none() => return Ok(()),
                Notice::Dnd { value } | Notice::SeedAck { value } => {
                    if matches!(cmd, Notice::SeedAck { .. }) {
                        self.pending_seed = None;
                    }
                    self.dnd = value;
                    if value {
                        for l in &mut self.live {
                            if self.popups.iter().any(|r| r["token"] == l.token) {
                                l.hover = false;
                                l.restart()
                            }
                        }
                        self.popups.clear()
                    }
                }
                Notice::Toggle => return emit(json!({"event":"toggle"})),
                Notice::Hover { id, token, value } => {
                    if let Some(l) = self
                        .live
                        .iter_mut()
                        .find(|l| l.row["id"] == id && l.token == token)
                        && l.hover != value
                        && (!value || self.popups.iter().any(|r| r["id"] == id))
                    {
                        l.hover = value;
                        l.restart()
                    }
                    return Ok(());
                }
                Notice::Action {
                    id,
                    token,
                    identifier,
                } => {
                    if let Some(l) = self
                        .live
                        .iter()
                        .find(|l| l.row["id"] == id && l.token == token)
                        && l.row["actions"]
                            .as_array()
                            .is_some_and(|a| a.iter().any(|a| a["identifier"] == identifier))
                    {
                        l.operation("invoke", &identifier)?
                    }
                    return Ok(());
                }
                Notice::Dismiss { id, token } => {
                    if let Some(i) = self.live.iter().position(|l| l.row["id"] == id) {
                        if self.live[i].token != token {
                            return Ok(());
                        }
                        self.live.remove(i).operation("dismiss", "")?
                    } else if !token.is_empty() {
                        // A delayed click on a resolved instance is not a click on its archive row.
                        return Ok(());
                    }
                    self.history.retain(|r| r["id"] != id);
                    self.popups.retain(|r| r["id"] != id)
                }
                Notice::Clear { instances } => {
                    // Include native objects whose notified signal has not yet reached the adapter.
                    for i in instances {
                        if !self
                            .live
                            .iter()
                            .any(|l| l.token == i.token && l.native == i.native)
                        {
                            emit(
                                json!({"event":"native","operation":"dismiss","native":i.native,"token":i.token,"argument":""}),
                            )?;
                        }
                    }
                    for l in self.live.drain(..) {
                        l.operation("dismiss", "")?
                    }
                    self.history.clear();
                    self.popups.clear()
                }
            }
            // The byte budget also caps large archives with fewer than 200 rows.
            let mut bytes = 128;
            self.history.retain(|r| {
                bytes += r.to_string().len() + 32;
                bytes <= 8 * 1024 * 1024
            });
            let mut i = 0;
            while i < self.live.len() {
                let l = &mut self.live[i];
                let visible = self.popups.iter().any(|r| r["token"] == l.token);
                if !visible && !self.history.iter().any(|r| r["token"] == l.token) {
                    self.live.remove(i).operation("expire", "")?;
                    continue;
                }
                if !visible && l.hover {
                    l.hover = false;
                    l.restart()
                }
                i += 1
            }
            self.publish()
        }
    }
    #[cfg(test)]
    mod tests {
        use super::*;
        use std::os::unix::fs::PermissionsExt;
        struct Temp(PathBuf);
        impl Temp {
            fn new() -> Self {
                static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
                let path = env::temp_dir().join(format!(
                    "dashboard-test-{}-{}",
                    std::process::id(),
                    SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                ));
                fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
                Self(path)
            }
        }
        impl Drop for Temp {
            fn drop(&mut self) {
                fs::remove_dir_all(&self.0).unwrap()
            }
        }
        fn events() -> Vec<Value> {
            EVENTS.with(|e| e.take())
        }
        fn cmd(center: &mut Center, value: Value) -> Vec<Value> {
            center
                .handle(serde_json::from_value(value).unwrap())
                .unwrap();
            events()
        }
        fn received(id: u32, token: &str, over: Value, restore: bool) -> Value {
            let mut row = json!({"id":"","app":"Test","summary":"Summary","body":"<plain text>","icon":"image://live","urgency":1,"transient":false,"timeout":-1,"actions":[{"identifier":"open","text":"Open"}],"closed":false});
            row.as_object_mut()
                .unwrap()
                .extend(over.as_object().unwrap().clone());
            json!({"kind":"received","native":id,"token":token,"time":100,"row":row,"restore":restore})
        }
        fn center(t: &Temp) -> Center {
            let mut c = Center::default();
            c.restore(t.0.join("new"), &t.0.join("old"), false, false)
                .unwrap();
            c
        }
        #[test]
        fn sampler_only_reads_visible_and_resets_baseline() {
            let now = Instant::now();
            let mut s = Sampler::new(now);
            let reads = std::cell::Cell::new(0);
            let disk = || {
                reads.set(reads.get() + 1);
                Some([5.; 3])
            };
            let sample = |prev: &mut Option<(u64, u64)>, d: [f64; 3]| {
                let fresh = prev.is_none();
                *prev = Some((100, 80));
                json!({"fresh":fresh,"disk":d})
            };
            assert!(s.tick(now, || panic!(), |_, _| panic!()).is_none());
            s.activate(true, 1, now);
            assert_eq!(s.tick(now, disk, sample).unwrap()["fresh"], true);
            assert!(
                s.tick(
                    now + Duration::from_millis(2499),
                    || panic!(),
                    |_, _| panic!()
                )
                .is_none()
            );
            for n in 1..12 {
                let e = s
                    .tick(now + Duration::from_millis(2500 * n), || panic!(), sample)
                    .unwrap();
                assert_eq!(e["fresh"], false);
                assert_eq!(e["disk"], json!([5., 5., 5.]));
            }
            assert_eq!(
                s.tick(now + Duration::from_secs(30), || None, sample)
                    .unwrap()["disk"],
                json!([-1., -1., -1.])
            );
            s.activate(false, 2, now);
            assert!(
                s.tick(now + Duration::from_secs(100), || panic!(), |_, _| panic!())
                    .is_none()
            );
            s.activate(true, 3, now + Duration::from_secs(101));
            let e = s
                .tick(now + Duration::from_secs(101), disk, sample)
                .unwrap();
            assert_eq!(e["fresh"], true);
            assert_eq!(e["generation"], 3);
            assert_eq!(reads.get(), 2);
        }
        #[test]
        fn telemetry_parsers() {
            let before = cpu("cpu 100 20 30 400 50 6 7 8 20 5\ncpu0 1 2 3 4");
            assert_eq!(before, Some((621, 450)));
            let after = cpu("cpu 140 20 40 440 60 6 7 8 60 5");
            assert_eq!(percent(before, after), Some(50.));
            for (p, n) in [
                (None, after),
                (before, before),
                (after, before),
                (before, None),
                (Some((10, 5)), Some((20, 16))),
                (Some((10, 5)), Some((20, 4))),
            ] {
                assert!(percent(p, n).is_none())
            }
            for s in [
                "",
                "cpu0 1 2 3 4",
                "cpu 1 2 3",
                "cpu 1 -2 3 4",
                "cpu 1 NaN 3 4",
                "cpu 1 2 3 4x",
                "cpu malformed\ncpu 1 2 3 4",
            ] {
                assert!(cpu(s).is_none())
            }
            assert_eq!(cpu("cpu 1 2 3 4\r\ncpu0 malformed"), Some((10, 4)));
            assert_eq!(
                memory("MemTotal: 2097152 kB\nMemAvailable: 524288 kB\nMemFree: 0 kB"),
                Some([75., 1.5, 2.])
            );
            for (m, p) in [
                ("MemAvailable: 0 kB\nMemFree: 80 kB", 100.),
                ("MemFree: 10 kB\nBuffers: 10 kB\nCached: 30 kB", 50.),
                ("MemAvailable: 200 kB", 0.),
            ] {
                assert_eq!(memory(&format!("MemTotal: 100 kB\n{m}")).unwrap()[0], p)
            }
            for s in [
                "",
                "MemTotal: 0 kB\nMemFree: 2 kB",
                "MemTotal: 100 kB",
                "MemAvailable: 100 kB",
                "MemTotal: nonsense",
            ] {
                assert!(memory(s).is_none())
            }
            assert_eq!(disk_usage(4, 3, 2, 1 << 30), Some([34., 1., 4.]));
            assert_eq!(disk_usage(100, 100, 95, 4096).unwrap()[0], 0.);
            assert!(disk_usage(0, 0, 0, 4096).is_none());
            assert_eq!(temperature("45678\n"), Some(45.678));
            assert_eq!(temperature(" 52.5 "), Some(52.5));
            for s in ["", "0", "-1", "NaN", "45C", "40000\n50000"] {
                assert!(temperature(s).is_none())
            }
        }
        #[test]
        fn idle_transaction_restore_exit_and_cleanup() {
            let t = Temp::new();
            let path = t.0.join("nested/screen-timeout.json");
            let exe = t.0.join("fake-watcher");
            fs::write(&exe, "#!/bin/sh\nexec sleep 60\n").unwrap();
            fs::set_permissions(&exe, fs::Permissions::from_mode(0o700)).unwrap();
            let restore = || {
                Idle::restore(
                    path.clone(),
                    exe.clone().into(),
                    "/test/system'ctl with spaces".into(),
                )
            };
            let mut idle = restore();
            assert!(!idle.set(0).unwrap());
            assert!(!path.exists());
            for m in [10, 30, 60, 120, 180, 360] {
                let old = idle.child.as_ref().map(|w| w.0.id());
                assert!(idle.set(m).unwrap());
                assert_eq!(idle.minutes, m);
                let pid = idle.child.as_ref().unwrap().0.id();
                assert!(!idle.set(m).unwrap());
                assert_eq!(idle.child.as_ref().unwrap().0.id(), pid);
                if let Some(old) = old {
                    assert_eq!(unsafe { libc::kill(old as i32, 0) }, -1)
                }
                assert_eq!(
                    idle.command(m).get_args().collect::<Vec<_>>(),
                    [
                        "-w",
                        "-C",
                        "/dev/null",
                        "timeout",
                        &(u32::from(m) * 60).to_string(),
                        "exec '/test/system'\\''ctl with spaces' suspend"
                    ]
                );
                assert_eq!(
                    serde_json::from_slice::<Value>(&fs::read(&path).unwrap()).unwrap(),
                    json!({"version":1,"minutes":m})
                );
                assert_eq!(
                    fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                    0o600
                )
            }
            let pid = idle.child.as_ref().unwrap().0.id();
            for m in [1, 5, 20, 61, 361] {
                assert!(idle.set(m).is_err())
            }
            idle.watcher = t.0.join("missing").into();
            assert!(idle.set(30).is_err());
            assert_eq!(idle.child.as_ref().unwrap().0.id(), pid);
            let saved = fs::read(&path).unwrap();
            idle.watcher = "false".into();
            assert!(idle.set(30).is_err());
            assert_eq!(idle.child.as_mut().unwrap().0.try_wait().unwrap(), None);
            assert_eq!(idle.child.as_ref().unwrap().0.id(), pid);
            assert_eq!(idle.minutes, 360);
            assert_eq!(fs::read(&path).unwrap(), saved);
            idle.watcher = exe.clone().into();
            idle.path = path.join("bad");
            for m in [30, 0] {
                assert!(idle.set(m).is_err());
                assert_eq!(idle.child.as_ref().unwrap().0.id(), pid);
                assert_eq!(idle.minutes, 360)
            }
            idle.path = path.clone();
            drop(idle);
            assert_eq!(unsafe { libc::kill(pid as i32, 0) }, -1);
            let mut idle = restore();
            assert_eq!(idle.minutes, 360);
            idle.child.as_mut().unwrap().0.kill().unwrap();
            idle.child.as_mut().unwrap().0.wait().unwrap();
            assert!(idle.exited().unwrap());
            assert_eq!(idle.minutes, 0);
            assert_eq!(restore().minutes, 360);
            assert!(idle.set(0).unwrap());
            assert_eq!(
                serde_json::from_slice::<Value>(&fs::read(&path).unwrap()).unwrap()["minutes"],
                0
            );
            assert!(!idle.set(0).unwrap());
            assert!(idle.child.is_none());
            assert_eq!(restore().minutes, 0);
        }
        #[test]
        fn idle_malformed_oversize_boundary_and_failed_restore() {
            let t = Temp::new();
            let p = t.0.join("state");
            let restore = |exe: &str| Idle::restore(p.clone(), exe.into(), "ignored".into());
            let valid = "{\"version\":1,\"minutes\":10}";
            for s in [
                "broken",
                "{}",
                "{\"version\":2,\"minutes\":10}",
                "{\"version\":1,\"minutes\":11}",
                &format!("{valid:1025}"),
            ] {
                fs::write(&p, s).unwrap();
                assert_eq!(restore("true").minutes, 0);
                assert_eq!(fs::read_to_string(&p).unwrap(), s)
            }
            let s = format!("{valid:1024}");
            fs::write(&p, &s).unwrap();
            let exe = t.0.join("watcher");
            fs::write(&exe, "#!/bin/sh\nexec sleep 60\n").unwrap();
            fs::set_permissions(&exe, fs::Permissions::from_mode(0o700)).unwrap();
            assert_eq!(restore(exe.to_str().unwrap()).minutes, 10);
            assert_eq!(restore("/missing/dashboard-test").minutes, 0);
            assert_eq!(fs::read_to_string(p).unwrap(), s)
        }
        #[test]
        fn notification_lifetime_hover_and_close() {
            let t = Temp::new();
            let mut c = center(&t);
            let start = Instant::now();
            cmd(&mut c, received(1, "one", json!({}), false));
            let id = c.history[0]["id"].clone();
            let now = Instant::now();
            assert!(
                (start + Duration::from_secs(8)..=now + Duration::from_secs(8))
                    .contains(&c.live[0].due.unwrap())
            );
            cmd(
                &mut c,
                json!({"kind":"hover","id":id,"token":"one","value":true}),
            );
            assert!(c.live[0].due.is_none());
            cmd(&mut c, received(1, "two", json!({"timeout":600}), false));
            assert_eq!(c.history[0]["id"], id);
            assert!(c.live[0].hover);
            assert_eq!(c.history.len(), 1);
            for token in ["one", "two"] {
                let e = cmd(
                    &mut c,
                    json!({"kind":"action","id":id,"token":token,"identifier":"open"}),
                );
                assert_eq!(e.len(), usize::from(token == "two"));
                if token == "two" {
                    assert_eq!(e[0]["operation"], "invoke")
                }
            }
            assert!(
                cmd(
                    &mut c,
                    json!({"kind":"action","id":id,"token":"two","identifier":"missing"})
                )
                .is_empty()
            );
            cmd(&mut c, json!({"kind":"dismiss","id":id,"token":"one"}));
            assert_eq!(c.history.len(), 1);
            let now = Instant::now();
            cmd(
                &mut c,
                json!({"kind":"hover","id":id,"token":"two","value":false}),
            );
            let due = c.live[0].due.unwrap();
            assert!(due >= now + Duration::from_millis(600));
            c.tick(due - Duration::from_nanos(1), Duration::MAX)
                .unwrap();
            assert!(events().is_empty());
            c.tick(due, Duration::MAX).unwrap();
            assert_eq!(events()[0]["token"], "two");
            cmd(&mut c, json!({"kind":"resolved","token":"one","reason":3}));
            assert!(!c.live.is_empty());
            cmd(&mut c, json!({"kind":"resolved","token":"two","reason":1}));
            assert!(c.popups.is_empty());
            assert_eq!(c.history[0]["closed"], true);
            assert_eq!(c.history[0]["actions"], json!([]));
            assert_eq!(c.history[0]["icon"], "");
            assert!(
                cmd(
                    &mut c,
                    json!({"kind":"action","id":id,"token":"two","identifier":"open"})
                )
                .is_empty()
            );
            cmd(&mut c, json!({"kind":"dismiss","id":id,"token":""}));
            assert!(c.history.is_empty());
            for (i, row) in [json!({"timeout":0}), json!({"urgency":2,"timeout":50})]
                .into_iter()
                .enumerate()
            {
                cmd(&mut c, received(2, "three", row, false));
                assert!(c.live[0].due.is_none());
                cmd(
                    &mut c,
                    json!({"kind":"resolved","token":"three","reason":2+i}),
                );
                assert!(c.history.is_empty())
            }
        }
        #[test]
        fn pending_seed_survives_interruption_but_never_overrides_user_preference() {
            for user in [false, true] {
                let t = Temp::new();
                let legacy = t.0.join("old");
                fs::write(&legacy, br#"{"version":1,"dnd":true,"history":[]}"#).unwrap();
                let mut c = center(&t);
                let path = c.path.clone();
                assert_eq!(events()[0]["value"], true);
                cmd(&mut c, json!({"kind":"dnd","value":false}));
                let saved = || serde_json::from_slice::<Value>(&fs::read(&path).unwrap()).unwrap();
                assert_eq!(saved()["pendingSeed"], true);
                drop(c);
                fs::remove_file(&legacy).unwrap();
                let mut c = Center::default();
                c.restore(path.clone(), &legacy, false, user).unwrap();
                if user {
                    assert!(events().is_empty());
                    assert!(!c.dnd);
                    c.publish().unwrap();
                    events();
                } else {
                    assert_eq!(events(), vec![json!({"event":"seed","value":true})]);
                    cmd(&mut c, json!({"kind":"seedAck","value":true}));
                }
                assert!(saved().get("pendingSeed").is_none());
                assert_eq!(saved()["dnd"], !user);
                let _c = center(&t);
                assert!(events().is_empty());
            }
        }
        #[test]
        fn migration_dnd_admission_and_reattachment() {
            let t = Temp::new();
            let legacy = t.0.join("old");
            let row = json!({"id":"legacy","app":"app","summary":"s","body":"b","icon":"image://old","timeout":-8,"urgency":5});
            fs::write(&legacy,json!({"version":1,"dnd":true,"history":[null,{},row,row,{"id":"t","app":"a","summary":"s","body":"b","transient":true}]}).to_string()).unwrap();
            let mut explicit = Center::default();
            explicit
                .restore(t.0.join("explicit"), &legacy, false, true)
                .unwrap();
            assert!(!explicit.dnd);
            assert!(events().is_empty());
            let mut c = center(&t);
            assert_eq!(events()[0]["event"], "seed");
            assert!(c.dnd);
            assert_eq!(c.history.len(), 1);
            assert_eq!(c.history[0]["timeout"], -1);
            assert_eq!(c.history[0]["urgency"], 1);
            assert_eq!(c.history[0]["icon"], "");
            let e = cmd(
                &mut c,
                received(4, "quiet", json!({"transient":true}), false),
            );
            assert_eq!(e[0]["operation"], "expire");
            assert!(c.popups.is_empty());
            cmd(&mut c, json!({"kind":"seedAck","value":true}));
            cmd(&mut c, json!({"kind":"dnd","value":false}));
            cmd(&mut c, received(2, "first", json!({}), false));
            cmd(&mut c, received(1, "next", json!({}), false));
            cmd(
                &mut c,
                received(2, "replacement", json!({"summary":"updated"}), false),
            );
            let ids = c
                .history
                .iter()
                .map(|v| v["id"].clone())
                .collect::<Vec<_>>();
            let mut c = center(&t);
            assert!(!c.dnd);
            assert!(events().is_empty());
            for (id, token, over) in [
                (1, "reattach1", json!({})),
                (2, "reattach2", json!({"summary":"updated"})),
            ] {
                cmd(&mut c, received(id, token, over, true));
            }
            assert_eq!(c.live.len(), 2);
            assert!(c.popups.is_empty());
            assert_eq!(
                c.history
                    .iter()
                    .map(|v| v["id"].clone())
                    .collect::<Vec<_>>(),
                ids
            );
            assert_eq!(c.history[0]["actions"][0]["identifier"], "open");
            cmd(
                &mut c,
                received(3, "transient", json!({"transient":true}), false),
            );
            let e = cmd(&mut c, json!({"kind":"dnd","value":true}));
            assert_eq!(e[0]["token"], "transient");
            assert!(c.popups.is_empty());
            assert_eq!(
                serde_json::from_slice::<Value>(&fs::read(&c.path).unwrap()).unwrap()["dnd"],
                true
            );
            let e = cmd(&mut c, json!({"kind":"clear"}));
            assert_eq!(e.iter().filter(|v| v["operation"] == "dismiss").count(), 2);
            let mut c = center(&t);
            assert!(c.history.is_empty());
            assert_eq!(
                cmd(&mut c, received(1, "stale", json!({}), true))[0]["operation"],
                "expire"
            );
            fs::write(&c.path, "broken").unwrap();
            assert!(center(&t).history.is_empty())
        }
        #[test]
        fn bounded_order_pruning_and_hover_eviction() {
            let t = Temp::new();
            let mut c = center(&t);
            cmd(&mut c, received(1, "1", json!({}), false));
            let id = c.history[0]["id"].clone();
            cmd(
                &mut c,
                json!({"kind":"hover","id":id,"token":"1","value":true}),
            );
            for i in 2..=201 {
                let e = cmd(&mut c, received(i, &i.to_string(), json!({}), false));
                if i == 6 {
                    assert!(!c.live[0].hover);
                    assert!(c.live[0].due.is_some())
                }
                if i == 201 {
                    assert_eq!(e[0]["operation"], "expire");
                    assert_eq!(e[0]["token"], "1")
                }
            }
            assert_eq!(c.history.len(), 200);
            assert_eq!(c.popups.len(), 5);
            assert_eq!(c.live.len(), 200);
            assert_eq!(c.history[0]["token"], "201");
            assert_eq!(c.popups[4]["token"], "197");
            assert_eq!(center(&t).history.len(), 200)
        }
    }
}

mod desktop_runtime {
    use std::{
        env,
        ffi::{OsStr, OsString},
        fs,
        io::{self, Read},
        os::{
            fd::{AsRawFd, FromRawFd, OwnedFd},
            unix::{
                ffi::OsStrExt,
                fs::{MetadataExt, OpenOptionsExt},
                process::CommandExt,
            },
        },
        path::Path,
        process::{Child, Command, Stdio},
        thread,
        time::{Duration, Instant},
    };
    type Result<T> = io::Result<T>;
    fn bad() -> io::Error {
        io::Error::other("invalid runtime arguments or response")
    }
    struct Managed(Child);
    impl Drop for Managed {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    fn ready(fd: i32, deadline: Option<Instant>) -> Result<bool> {
        loop {
            let timeout = match deadline {
                None => -1,
                Some(d) => {
                    let left = d.saturating_duration_since(Instant::now());
                    if left.is_zero() {
                        return Ok(false);
                    }
                    left.as_millis().saturating_add(1).min(i32::MAX as u128) as i32
                }
            };
            let mut p = libc::pollfd {
                fd,
                events: libc::POLLIN,
                revents: 0,
            };
            let n = unsafe { libc::poll(&mut p, 1, timeout) };
            if n > 0 {
                return Ok(true);
            }
            if n < 0 {
                let e = io::Error::last_os_error();
                if e.kind() != io::ErrorKind::Interrupted {
                    return Err(e);
                }
            }
        }
    }
    fn pidfd(pid: i32) -> Result<OwnedFd> {
        let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0u32) };
        if fd < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(unsafe { OwnedFd::from_raw_fd(fd as i32) })
        }
    }
    fn bounded(cmd: &mut Command, limit: Duration) -> Result<Vec<u8>> {
        let deadline = Some(Instant::now() + limit);
        let mut child = Managed(
            cmd.stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()?,
        );
        let mut pipe = child.0.stdout.take().unwrap();
        let mut bytes = Vec::new();
        let mut buf = [0; 4096];
        let timeout = || io::Error::from(io::ErrorKind::TimedOut);
        loop {
            if !ready(pipe.as_raw_fd(), deadline)? {
                return Err(timeout());
            }
            let n = pipe.read(&mut buf)?;
            if n == 0 {
                break;
            }
            if bytes.len() + n > 65536 {
                return Err(bad());
            }
            bytes.extend_from_slice(&buf[..n]);
        }
        if !ready(pidfd(child.0.id() as i32)?.as_raw_fd(), deadline)? {
            return Err(timeout());
        }
        if !child.0.wait()?.success() {
            return Err(io::Error::other("subcommand failed"));
        }
        Ok(bytes)
    }
    fn name(bytes: &[u8]) -> Result<Option<String>> {
        let val: serde_json::Value = serde_json::from_slice(bytes)?;
        if val.is_null() {
            return Ok(None);
        }
        let v = val.get("name").ok_or_else(bad)?;
        if v.is_null() {
            return Ok(None);
        }
        let s = v.as_str().filter(|s| valid(s)).ok_or_else(bad)?;
        Ok(Some(s.into()))
    }
    fn valid(s: &str) -> bool {
        !s.trim().is_empty() && s.len() <= 255 && !s.chars().any(char::is_control)
    }
    fn toggle(tool: &OsStr) -> Result<()> {
        input_toggle(|args| bounded(Command::new(tool).args(args), Duration::from_secs(2)))
    }
    fn input_toggle(mut run: impl FnMut(&[&str]) -> Result<Vec<u8>>) -> Result<()> {
        let bytes = run(&["-n"])?;
        let s = std::str::from_utf8(&bytes)
            .map_err(|_| bad())?
            .trim_end_matches('\n');
        if !valid(s) {
            return Err(bad());
        }
        run(&[
            "-s",
            if s == "pinyin" {
                "keyboard-gb"
            } else {
                "pinyin"
            },
        ])?;
        Ok(())
    }
    fn same_parent(before: i32, after: i32) -> bool {
        before > 1 && before == after
    }
    fn parent_exec(program: &OsStr, args: &[OsString]) -> Result<()> {
        let before = unsafe { libc::getppid() };
        if unsafe { libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL, 0, 0, 0) } < 0 {
            return Err(io::Error::last_os_error());
        }
        if !same_parent(before, unsafe { libc::getppid() }) {
            return Ok(());
        }
        Err(Command::new(program).args(args).exec())
    }
    fn resync(dir: &Path) -> Result<bool> {
        for e in fs::read_dir(dir)? {
            if e?.file_name().as_bytes().ends_with(b".lst") {
                return Ok(false);
            }
        }
        Ok(true)
    }
    fn parse_pid(s: &str) -> Option<i32> {
        let s = s.trim();
        s.bytes()
            .all(|b| b.is_ascii_digit())
            .then(|| s.parse::<i32>().ok())
            .flatten()
            .filter(|p| *p > 1)
    }
    fn signal(path: &Path, python: &OsStr, script: &OsStr) -> Result<()> {
        let uid = unsafe { libc::geteuid() };
        let f = fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK | libc::O_NOFOLLOW)
            .open(path)?;
        let m = f.metadata()?;
        if !m.is_file() || m.uid() != uid || m.len() > 32 {
            return Err(bad());
        }
        let mut s = String::new();
        f.take(33).read_to_string(&mut s)?;
        let pid = parse_pid(&s).ok_or_else(bad)?;
        let fd = pidfd(pid)?;
        let proc = Path::new("/proc").join(pid.to_string());
        if fs::metadata(&proc)?.uid() != uid
            || fs::read_link(proc.join("exe"))? != fs::canonicalize(python)?
        {
            return Err(bad());
        }
        let cmd = fs::read(proc.join("cmdline"))?;
        if cmd.split(|b| *b == 0).nth(1) != Some(script.as_bytes()) {
            return Err(bad());
        }
        if unsafe {
            libc::syscall(
                libc::SYS_pidfd_send_signal,
                fd.as_raw_fd(),
                libc::SIGUSR1,
                std::ptr::null::<libc::siginfo_t>(),
                0u32,
            )
        } < 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
    fn watch(delay: Duration, ctl: &OsStr, unit: &OsStr, cmd: &mut Command) -> Result<()> {
        let mut child = Managed(cmd.stdout(Stdio::piped()).spawn()?);
        let mut pipe = child.0.stdout.take().unwrap();
        debounce(&mut pipe, delay, || {
            bounded(
                Command::new(ctl).args([
                    OsStr::new("--user"),
                    OsStr::new("start"),
                    OsStr::new("--no-block"),
                    unit,
                ]),
                Duration::from_secs(5),
            )
            .map(drop)
        })?;
        if !child.0.wait()?.success() {
            return Err(io::Error::other("watcher failed"));
        }
        Ok(())
    }
    fn debounce(
        pipe: &mut (impl Read + AsRawFd),
        delay: Duration,
        mut start: impl FnMut() -> Result<()>,
    ) -> Result<()> {
        let mut deadline = None;
        let mut buf = [0; 8192];
        loop {
            if !ready(pipe.as_raw_fd(), deadline)? {
                start()?;
                deadline = None;
                continue;
            }
            let n = pipe.read(&mut buf)?;
            if n == 0 {
                return Ok(());
            }
            if buf[..n].contains(&b'\n') {
                deadline = Some(Instant::now() + delay)
            }
        }
    }
    fn run(args: &[OsString]) -> Result<()> {
        let (cmd, a) = args.split_first().ok_or_else(bad)?;
        match (cmd.to_str(), a) {
            (Some("focused-output"), []) => {
                let data = bounded(
                    Command::new(env::var_os("NIRI").unwrap_or_else(|| "niri".into())).args([
                        "msg",
                        "-j",
                        "focused-output",
                    ]),
                    Duration::from_secs(2),
                )?;
                println!("{}", serde_json::json!({"name":name(&data)?}));
                Ok(())
            }
            (Some("input-toggle"), []) => {
                toggle(OsStr::new("/run/current-system/sw/bin/fcitx5-remote"))
            }
            (Some("with-parent-death"), [p, rest @ ..]) => parent_exec(p, rest),
            (Some("bisync"), [dir, p, rest @ ..]) => {
                let mut cmd = Command::new(p);
                cmd.args(rest);
                if resync(Path::new(dir))? {
                    cmd.arg("--resync");
                }
                Err(cmd.exec())
            }
            (Some("watch"), [secs, ctl, unit, p, rest @ ..]) => {
                let s = secs
                    .to_str()
                    .and_then(|s| s.parse::<u32>().ok())
                    .filter(|s| *s > 0)
                    .ok_or_else(bad)?;
                watch(
                    Duration::from_secs(s.into()),
                    ctl,
                    unit,
                    Command::new(p).args(rest),
                )
            }
            (Some("ycal-toggle"), [ctl, python, script, popup]) => {
                let home = env::var_os("HOME").ok_or_else(bad)?;
                let path = Path::new(&home).join(".cache/waybar-ycal/popup.pid");
                let send = || signal(&path, python, script).is_ok();
                if send() {
                    return Ok(());
                }
                if bounded(
                    Command::new(ctl).args(["--user", "start", "waybar-ycal.service"]),
                    Duration::from_secs(5),
                )
                .is_ok()
                {
                    for _ in 0..5 {
                        thread::sleep(Duration::from_millis(200));
                        if send() {
                            return Ok(());
                        }
                    }
                }
                Command::new(popup).spawn()?;
                Ok(())
            }
            _ => Err(bad()),
        }
    }
    pub(super) fn status(args: &[OsString]) -> i32 {
        match run(args) {
            Ok(()) => 0,
            Err(e) => {
                if matches!(args, [command] if command == "focused-output") {
                    println!("{{\"name\":null}}");
                }
                eprintln!("panel-launcher runtime: {e}");
                1
            }
        }
    }
    #[cfg(test)]
    mod tests {
        use super::*;
        use std::{io::Write, os::unix::net::UnixStream};
        #[test]
        fn parsing() {
            assert!(run(&[]).is_err());
            assert!(run(&["input-toggle".into(), "extra".into()]).is_err());
            assert_eq!(
                name(br#"{"name":"DP-1","other":1}"#).unwrap().as_deref(),
                Some("DP-1")
            );
            assert!(name(br#"{"name":123}"#).is_err());
            assert!(name(b"null").unwrap().is_none());
        }
        #[test]
        fn input_switches_only_after_valid_query() {
            for (current, next) in [
                ("pinyin\n", "keyboard-gb"),
                ("keyboard-gb\n", "pinyin"),
                ("other", "pinyin"),
            ] {
                let mut calls = Vec::new();
                input_toggle(|args| {
                    calls.push(args.join(" "));
                    Ok(current.as_bytes().to_vec())
                })
                .unwrap();
                assert_eq!(calls, ["-n".to_string(), format!("-s {next}")]);
            }
            for response in [Ok(vec![]), Ok(vec![255]), Err(bad())] {
                let mut once = Some(response);
                assert!(
                    input_toggle(|args| {
                        assert_eq!(args, ["-n"]);
                        once.take().unwrap()
                    })
                    .is_err()
                );
            }
        }
        #[test]
        fn identity_checks() {
            for s in ["0", "-2", "1", "+42", "999999999999", "123x"] {
                assert!(parse_pid(s).is_none())
            }
            assert_eq!(parse_pid("42\n"), Some(42));
            assert!(same_parent(4, 4));
            assert!(!same_parent(1, 1));
            assert!(!same_parent(42, 43));
        }
        #[test]
        fn state_listing() {
            let dir = env::temp_dir().join(format!("panel-state-{}", std::process::id()));
            fs::create_dir(&dir).unwrap();
            assert!(resync(&dir).unwrap());
            fs::write(dir.join("old.lst"), []).unwrap();
            assert!(!resync(&dir).unwrap());
            fs::remove_dir_all(&dir).unwrap();
            assert!(resync(&dir).is_err());
        }
        #[test]
        fn coalesces_and_cancels() {
            let (mut rx, mut tx) = UnixStream::pair().unwrap();
            tx.write_all(b"a\nb\n").unwrap();
            let mut calls = 0;
            debounce(&mut rx, Duration::from_millis(1), || {
                calls += 1;
                tx.shutdown(std::net::Shutdown::Write)?;
                Ok(())
            })
            .unwrap();
            assert_eq!(calls, 1);
            let (mut rx, mut tx) = UnixStream::pair().unwrap();
            tx.write_all(b"pending\n").unwrap();
            drop(tx);
            debounce(&mut rx, Duration::from_secs(3), || panic!("cancelled")).unwrap();
        }
        #[test]
        fn bounded_failure() {
            assert!(bounded(Command::new("sleep").arg("10"), Duration::from_millis(10)).is_err());
            assert!(toggle(OsStr::new("false")).is_err());
        }
    }
}
