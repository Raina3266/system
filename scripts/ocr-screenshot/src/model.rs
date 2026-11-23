use std::env;
use std::ffi::OsString;
use std::fs::Permissions;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use tempfile::{Builder, TempDir};

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
    directory: TempDir,
}

impl Workspace {
    pub(crate) fn create() -> Result<Self, String> {
        Builder::new()
            .prefix(&format!("ocr-screenshot-{}-", std::process::id()))
            .permissions(Permissions::from_mode(0o700))
            .tempdir()
            .map(|directory| Self { directory })
            .map_err(|error| format!("could not create temporary directory: {error}"))
    }

    pub(crate) fn image_path(&self) -> PathBuf {
        self.directory.path().join("capture.png")
    }

    pub(crate) fn output_prefix(&self) -> PathBuf {
        self.directory.path().join("output")
    }

    pub(crate) fn output_path(&self) -> PathBuf {
        self.directory.path().join("output.txt")
    }
}

pub(crate) fn ensure_success(name: &str, status: std::process::ExitStatus) -> Result<(), String> {
    if status.success() {
        Ok(())
    } else {
        Err(format!("{name} failed with {status}"))
    }
}
