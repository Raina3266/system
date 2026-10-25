//! Isolated, one-time import of the former data directory. No legacy executable
//! or environment variables are consulted; explicit CLIPBOARD_DATA_DIR is isolated.
use super::*;
use std::ffi::CString;
use std::sync::atomic::{AtomicU64, Ordering};

static SERIAL: AtomicU64 = AtomicU64::new(0);

unsafe extern "C" {
    fn renameat2(
        oldfd: i32,
        old: *const std::ffi::c_char,
        newfd: i32,
        new: *const std::ffi::c_char,
        flags: u32,
    ) -> i32;
}

struct Staging(PathBuf);
impl Drop for Staging {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn exists(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error).context("Inspect clipboard migration path"),
    }
}

fn regular_file(path: &Path) -> Result<()> {
    ensure!(
        fs::symlink_metadata(path)?.file_type().is_file(),
        "Migration source is not a regular file"
    );
    Ok(())
}

fn copy_private(source: &Path, destination: &Path) -> Result<()> {
    regular_file(source)?;
    let mut input = fs::File::open(source)?;
    let mut output = OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(destination)?;
    std::io::copy(&mut input, &mut output)?;
    output.sync_all()?;
    Ok(())
}

fn exclusive_lock(path: &Path) -> Result<fs::File> {
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .mode(0o600)
        .open(path)?;
    // SAFETY: file owns the descriptor until the returned File is dropped.
    if unsafe { flock(file.as_raw_fd(), 2) } == -1 {
        return Err(std::io::Error::last_os_error()).context("Lock clipboard migration");
    }
    Ok(file)
}

impl Store {
    pub(super) fn at_data_home(base: &Path) -> Result<Self> {
        let root = base.join("clipboard");
        if exists(&root)? {
            return Ok(Self { root });
        }
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(base)?;
        // All discovering helpers serialize migration AND first directory creation.
        // A monitor therefore never attaches to an empty directory later replaced
        // by the migration, and concurrent capture cannot start a competing store.
        let _migration_lock = exclusive_lock(&base.join(".clipboard-migration.lock"))?;
        if exists(&root)? {
            return Ok(Self { root });
        }
        let legacy = base.join("rofi-clipboard");
        if !exists(&legacy)? {
            fs::DirBuilder::new().mode(0o700).create(&root)?;
            return Ok(Self { root });
        }
        ensure!(
            fs::symlink_metadata(&legacy)?.file_type().is_dir(),
            "Legacy clipboard store is not a directory"
        );
        // A running former writer cannot alter history or cache during the copy.
        // Keep the source untouched, including on all validation/I/O failures.
        let _history_lock = exclusive_lock(&legacy.join("history.lock"))?;
        let history_path = legacy.join("history.json");
        if exists(&history_path)? {
            regular_file(&history_path)?;
            let history: History = serde_json::from_slice(&fs::read(&history_path)?)
                .context("Invalid legacy clipboard history")?;
            ensure!(
                history.version == 1,
                "Unsupported legacy clipboard history version"
            );
            Self {
                root: legacy.clone(),
            }
            .items(&history)?;
        }
        let staging_path = base.join(format!(
            ".clipboard-migration-{}-{}",
            std::process::id(),
            SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        fs::DirBuilder::new().mode(0o700).create(&staging_path)?;
        let staging = Staging(staging_path);
        if exists(&history_path)? {
            copy_private(&history_path, &staging.0.join("history.json"))?;
        }
        let images = legacy.join("images");
        if exists(&images)? {
            ensure!(
                fs::symlink_metadata(&images)?.file_type().is_dir(),
                "Legacy image cache is not a directory"
            );
            fs::DirBuilder::new()
                .mode(0o700)
                .create(staging.0.join("images"))?;
            for entry in fs::read_dir(images)? {
                let entry = entry?;
                copy_private(
                    &entry.path(),
                    &staging.0.join("images").join(entry.file_name()),
                )?;
            }
            fs::File::open(staging.0.join("images"))?.sync_all()?;
        }
        fs::File::open(&staging.0)?.sync_all()?;
        let source = CString::new(staging.0.as_os_str().as_bytes())?;
        let destination = CString::new(root.as_os_str().as_bytes())?;
        // SAFETY: both C strings live through the call. Linux RENAME_NOREPLACE
        // atomically publishes the complete copy without replacing ANY target.
        let result = unsafe { renameat2(-100, source.as_ptr(), -100, destination.as_ptr(), 1) };
        if result == -1 {
            let error = std::io::Error::last_os_error();
            if error.kind() != std::io::ErrorKind::AlreadyExists {
                return Err(error).context("Publish clipboard migration");
            }
        }
        Ok(Self { root })
    }
}
