use std::env;
use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::DirBuilderExt;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug)]
pub(crate) struct Tools {
    pub(crate) gnome_screenshot: OsString,
    pub(crate) grim: OsString,
    pub(crate) slurp: OsString,
    pub(crate) tesseract: OsString,
    pub(crate) wl_copy: OsString,
    pub(crate) notify_send: OsString,
}

impl Tools {
    pub(crate) fn from_environment() -> Self {
        Self {
            gnome_screenshot: executable("OCR_SCREENSHOT_GNOME_SCREENSHOT", "gnome-screenshot"),
            grim: executable("OCR_SCREENSHOT_GRIM", "grim"),
            slurp: executable("OCR_SCREENSHOT_SLURP", "slurp"),
            tesseract: executable("OCR_SCREENSHOT_TESSERACT", "tesseract"),
            wl_copy: executable("OCR_SCREENSHOT_WL_COPY", "wl-copy"),
            notify_send: executable("OCR_SCREENSHOT_NOTIFY_SEND", "notify-send"),
        }
    }
}

fn executable(variable: &str, fallback: &str) -> OsString {
    env::var_os(variable).unwrap_or_else(|| OsString::from(fallback))
}

pub(crate) struct Workspace {
    directory: PathBuf,
}

impl Workspace {
    pub(crate) fn create() -> Result<Self, String> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| format!("could not read the system clock: {error}"))?
            .as_nanos();
        let base = format!("ocr-screenshot-{}-{timestamp}", std::process::id());

        for attempt in 0..100 {
            let suffix = if attempt == 0 {
                String::new()
            } else {
                format!("-{attempt}")
            };
            let directory = env::temp_dir().join(format!("{base}{suffix}"));

            match fs::DirBuilder::new().mode(0o700).create(&directory) {
                Ok(()) => return Ok(Self { directory }),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => {
                    return Err(format!(
                        "could not create temporary directory {}: {error}",
                        directory.display()
                    ));
                }
            }
        }

        Err("could not create a unique temporary directory".to_owned())
    }

    pub(crate) fn image_path(&self) -> PathBuf {
        self.directory.join("capture.png")
    }

    pub(crate) fn output_prefix(&self) -> PathBuf {
        self.directory.join("output")
    }

    pub(crate) fn output_path(&self) -> PathBuf {
        self.directory.join("output.txt")
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}

pub(crate) fn ensure_success(name: &str, status: std::process::ExitStatus) -> Result<(), String> {
    if status.success() {
        Ok(())
    } else {
        Err(format!("{name} failed with {status}"))
    }
}
