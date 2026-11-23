//! Saved sleep policy and transactional swayidle supervision and cleanup.

use std::{
    io,
    path::PathBuf,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

use serde_json::{Value, json};

use crate::storage::{limited, save};

pub(crate) struct Watch(pub(crate) Child);
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
pub(crate) struct Idle {
    pub(crate) path: PathBuf,
    pub(crate) watcher: std::ffi::OsString,
    systemctl: String,
    pub(crate) child: Option<Watch>,
    pub(crate) minutes: u16,
    saved_minutes: u16,
}
impl Idle {
    pub(crate) fn command(&self, m: u16) -> Command {
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
    pub(crate) fn restore(path: PathBuf, watcher: std::ffi::OsString, systemctl: String) -> Self {
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
    pub(crate) fn set(&mut self, m: u16) -> io::Result<bool> {
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
    pub(crate) fn exited(&mut self) -> io::Result<bool> {
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
