use std::env;
use std::path::Path;
use std::process::Command;

use crate::model::{Tools, ensure_success};

pub(crate) fn desktop_is_gnome(desktop: &str) -> bool {
    desktop.to_ascii_lowercase().contains("gnome")
}

pub(crate) fn capture_screenshot(tools: &Tools, image_path: &Path) -> Result<bool, String> {
    let desktop = env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();

    if desktop_is_gnome(&desktop) {
        let status = Command::new(&tools.gnome_screenshot)
            .arg("--area")
            .arg(format!("--file={}", image_path.display()))
            .status()
            .map_err(|error| format!("could not start gnome-screenshot: {error}"))?;

        if !status.success() {
            return if image_path.is_file() {
                Err(format!("gnome-screenshot failed with {status}"))
            } else {
                Ok(false)
            };
        }
    } else {
        let selection = Command::new(&tools.slurp)
            .output()
            .map_err(|error| format!("could not start slurp: {error}"))?;
        if !selection.status.success() {
            return Ok(false);
        }

        let geometry = String::from_utf8(selection.stdout)
            .map_err(|error| format!("slurp returned invalid UTF-8: {error}"))?;
        let geometry = geometry.trim();
        if geometry.is_empty() {
            return Ok(false);
        }

        let status = Command::new(&tools.grim)
            .arg("-g")
            .arg(geometry)
            .arg(image_path)
            .status()
            .map_err(|error| format!("could not start grim: {error}"))?;
        ensure_success("grim", status)?;
    }

    Ok(image_path.is_file())
}
