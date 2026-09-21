//! Generic local-file preview generation shared by launchers.

use std::collections::hash_map::DefaultHasher;
use std::env;
use std::ffi::{OsStr, OsString};
use std::fs::{self, File};
use std::hash::{Hash, Hasher};
use std::io::{self, Read};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::UNIX_EPOCH;

use crate::panel_client::PanelContent;

const MAX_TEXT_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PreviewKind {
    Text,
    Image,
    Pdf,
    Video,
    Unsupported,
}

#[derive(Clone)]
pub struct FilePreviewer {
    cache_namespace: String,
    file: OsString,
    pdftoppm: OsString,
    ffmpegthumbnailer: OsString,
}

impl FilePreviewer {
    pub fn new(
        cache_namespace: impl Into<String>,
        file: impl Into<OsString>,
        pdftoppm: impl Into<OsString>,
        ffmpegthumbnailer: impl Into<OsString>,
    ) -> Self {
        Self {
            cache_namespace: cache_namespace.into(),
            file: file.into(),
            pdftoppm: pdftoppm.into(),
            ffmpegthumbnailer: ffmpegthumbnailer.into(),
        }
    }

    pub fn preview(&self, path: &Path) -> io::Result<PanelContent> {
        let mime = self.mime_type(path)?;
        match preview_kind(&mime) {
            PreviewKind::Text => Ok(PanelContent::ReadOnlyText(read_text(path)?)),
            PreviewKind::Image => Ok(PanelContent::Image(path.to_path_buf())),
            PreviewKind::Pdf => Ok(PanelContent::Image(self.cached_pdf(path)?)),
            PreviewKind::Video => Ok(PanelContent::Image(self.cached_video(path)?)),
            PreviewKind::Unsupported => Ok(PanelContent::ReadOnlyText(format!(
                "Preview is not available for {mime}.\n\n{}",
                path.display()
            ))),
        }
    }

    fn mime_type(&self, path: &Path) -> io::Result<String> {
        let output = Command::new(&self.file)
            .args([
                OsStr::new("--brief"),
                OsStr::new("--mime-type"),
                OsStr::new("--"),
            ])
            .arg(path)
            .output()?;
        if !output.status.success() {
            return Err(io::Error::other(format!(
                "file exited with {}",
                output.status
            )));
        }
        String::from_utf8(output.stdout)
            .map(|value| value.trim().to_owned())
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
    }

    fn cached_pdf(&self, path: &Path) -> io::Result<PathBuf> {
        let target = self.cache_path(path, "pdf", "png")?;
        if !target.exists() {
            render_pdf_thumbnail(path, &target, 1200, &self.pdftoppm)?;
        }
        Ok(target)
    }

    fn cached_video(&self, path: &Path) -> io::Result<PathBuf> {
        let target = self.cache_path(path, "video", "png")?;
        if target.exists() {
            return Ok(target);
        }
        let temporary = temporary_path(&target);
        let status = Command::new(&self.ffmpegthumbnailer)
            .arg("-i")
            .arg(path)
            .arg("-o")
            .arg(&temporary)
            .args(["-s", "1200", "-t", "10%", "-q", "8"])
            .status()?;
        if !status.success() || !temporary.is_file() {
            let _ = fs::remove_file(&temporary);
            return Err(io::Error::other(format!(
                "ffmpegthumbnailer exited with {status}"
            )));
        }
        fs::rename(&temporary, &target)?;
        Ok(target)
    }

    fn cache_path(&self, path: &Path, category: &str, extension: &str) -> io::Result<PathBuf> {
        let root = env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
            .ok_or_else(|| io::Error::other("neither XDG_CACHE_HOME nor HOME is set"))?
            .join(&self.cache_namespace)
            .join(category);
        fs::create_dir_all(&root)?;
        let metadata = fs::metadata(path)?;
        let modified = metadata
            .modified()?
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        let mut hasher = DefaultHasher::new();
        path.as_os_str().as_bytes().hash(&mut hasher);
        metadata.len().hash(&mut hasher);
        modified.as_secs().hash(&mut hasher);
        modified.subsec_nanos().hash(&mut hasher);
        Ok(root.join(format!("{:016x}.{extension}", hasher.finish())))
    }
}

pub fn preview_kind(mime: &str) -> PreviewKind {
    if mime == "application/pdf" {
        PreviewKind::Pdf
    } else if mime.starts_with("image/") {
        PreviewKind::Image
    } else if mime.starts_with("video/") {
        PreviewKind::Video
    } else if mime.starts_with("text/")
        || matches!(
            mime,
            "application/json"
                | "application/ld+json"
                | "application/javascript"
                | "application/xml"
                | "application/x-httpd-php"
                | "application/x-shellscript"
                | "application/x-yaml"
        )
    {
        PreviewKind::Text
    } else {
        PreviewKind::Unsupported
    }
}

fn read_text(path: &Path) -> io::Result<String> {
    let mut bytes = Vec::new();
    File::open(path)?
        .take(MAX_TEXT_BYTES + 1)
        .read_to_end(&mut bytes)?;
    let clipped = bytes.len() as u64 > MAX_TEXT_BYTES;
    if clipped {
        bytes.truncate(MAX_TEXT_BYTES as usize);
        while std::str::from_utf8(&bytes).is_err() && !bytes.is_empty() {
            bytes.pop();
        }
    }
    let mut text = String::from_utf8(bytes)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    if clipped {
        text.push_str("\n\n… preview truncated at 2 MiB …\n");
    }
    Ok(text)
}

pub fn render_pdf_thumbnail(
    input: &Path,
    output: &Path,
    size: u32,
    pdftoppm: &OsStr,
) -> io::Result<()> {
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }
    let prefix = pdf_render_prefix(output);
    let rendered = prefix.with_extension("png");
    let _ = fs::remove_file(&rendered);
    let conversion = Command::new(pdftoppm)
        .args(["-png", "-f", "1", "-singlefile", "-scale-to"])
        .arg(size.to_string())
        .arg("--")
        .arg(input)
        .arg(&prefix)
        .output()?;
    if !conversion.status.success() || !rendered.is_file() {
        let _ = fs::remove_file(&rendered);
        let stderr = String::from_utf8_lossy(&conversion.stderr);
        let stderr = stderr.trim();
        let reason = if conversion.status.success() {
            format!(
                "pdftoppm succeeded but did not create {}",
                rendered.display()
            )
        } else {
            format!("pdftoppm exited with {}", conversion.status)
        };
        return Err(io::Error::other(if stderr.is_empty() {
            reason
        } else {
            format!("{reason}: {stderr}")
        }));
    }
    fs::rename(rendered, output)
}

fn pdf_render_prefix(output: &Path) -> PathBuf {
    output.with_file_name(format!(".rofi-preview-shared-pdf-{}", std::process::id()))
}

fn temporary_path(target: &Path) -> PathBuf {
    let name = target
        .file_name()
        .map(|name| name.to_string_lossy())
        .unwrap_or_default();
    target.with_file_name(format!(".{name}.{}.tmp.png", std::process::id()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_requested_preview_families_are_supported() {
        assert_eq!(preview_kind("text/plain"), PreviewKind::Text);
        assert_eq!(preview_kind("application/json"), PreviewKind::Text);
        assert_eq!(preview_kind("image/webp"), PreviewKind::Image);
        assert_eq!(preview_kind("application/pdf"), PreviewKind::Pdf);
        assert_eq!(preview_kind("video/mp4"), PreviewKind::Video);
        assert_eq!(preview_kind("audio/mpeg"), PreviewKind::Unsupported);
    }
}
