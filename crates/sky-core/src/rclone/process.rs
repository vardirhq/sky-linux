//! Supervision of the private `rclone rcd` process: start, health-check,
//! restart with backoff, and log capture (used to surface OAuth URLs).

use std::collections::VecDeque;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{broadcast, watch, Mutex};

use super::client::{RcCredentials, RcloneClient};
use crate::error::{CoreError, Result};

#[derive(Debug, Clone)]
pub struct SupervisorConfig {
    /// Path to the rclone binary (bundled one in packaged builds).
    pub binary: PathBuf,
    /// rclone.conf used exclusively by Sky Linux.
    pub config_file: PathBuf,
    /// Unix socket the RC server listens on.
    pub socket: PathBuf,
    /// Password for an encrypted rclone.conf, read from the keyring.
    pub config_password: Option<String>,
    /// Extra global flags (e.g. bandwidth limits).
    pub extra_args: Vec<String>,
}

impl SupervisorConfig {
    /// Locate rclone: `$SKY_RCLONE`, next to our executable (bundled), then `$PATH`.
    ///
    /// Packages install a private, tested rclone at
    /// `<prefix>/lib/sky-linux/rclone`. It must win over an `rclone` next to
    /// our binary: in `/usr/bin` that is the distribution's rclone, which
    /// may be another version.
    pub fn find_binary() -> Option<PathBuf> {
        if let Some(p) = std::env::var_os("SKY_RCLONE") {
            return Some(PathBuf::from(p));
        }
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|e| e.parent().map(PathBuf::from));
        let path = std::env::var_os("PATH")
            .map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
            .unwrap_or_default();
        rclone_candidates(exe_dir.as_deref(), &path)
            .into_iter()
            .find(|p| p.is_file())
    }
}

/// Where to look for rclone, best first.
fn rclone_candidates(exe_dir: Option<&std::path::Path>, path: &[PathBuf]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(dir) = exe_dir {
        out.push(dir.join("../lib/sky-linux/rclone")); // deb, rpm, AppImage
        out.push(dir.join("rclone")); // development builds (Tauri sidecar dir)
    }
    out.extend(path.iter().map(|d| d.join("rclone")));
    out
}

#[derive(Debug, Clone)]
pub struct RcloneLogLine {
    pub line: String,
}

/// How many recent rclone log lines are kept for the diagnostics report.
const RECENT_LOG_LINES: usize = 200;

/// Owns the rclone child process and restarts it if it dies.
pub struct RcloneSupervisor {
    cfg: SupervisorConfig,
    creds: RcCredentials,
    child: Arc<Mutex<Option<Child>>>,
    logs: broadcast::Sender<RcloneLogLine>,
    recent: Arc<std::sync::Mutex<VecDeque<String>>>,
    running: watch::Sender<bool>,
    shutdown: watch::Sender<bool>,
}

impl RcloneSupervisor {
    pub fn new(cfg: SupervisorConfig) -> Self {
        let (logs, _) = broadcast::channel(256);
        Self {
            cfg,
            creds: RcCredentials::random(),
            child: Arc::new(Mutex::new(None)),
            logs,
            recent: Arc::new(std::sync::Mutex::new(VecDeque::with_capacity(
                RECENT_LOG_LINES,
            ))),
            running: watch::channel(false).0,
            shutdown: watch::channel(false).0,
        }
    }

    pub fn client(&self) -> RcloneClient {
        RcloneClient::new(&self.cfg.socket, &self.creds)
    }

    pub fn logs(&self) -> broadcast::Receiver<RcloneLogLine> {
        self.logs.subscribe()
    }

    /// The most recent rclone log lines, oldest first.
    pub fn recent_logs(&self) -> Vec<String> {
        self.recent
            .lock()
            .map(|r| r.iter().cloned().collect())
            .unwrap_or_default()
    }

    pub fn running(&self) -> watch::Receiver<bool> {
        self.running.subscribe()
    }

    fn command(&self) -> Command {
        let mut cmd = Command::new(&self.cfg.binary);
        cmd.arg("rcd")
            .arg(format!("--rc-addr=unix://{}", self.cfg.socket.display()))
            .arg(format!("--config={}", self.cfg.config_file.display()))
            // Jobs are polled by Sky Core; keep finished ones around briefly.
            .arg("--rc-job-expire-duration=10m")
            .arg("--log-level=INFO")
            // Never open a browser for the RC web GUI.
            .arg("--rc-web-gui=false")
            .args(&self.cfg.extra_args)
            .env_clear()
            .envs(passthrough_env())
            .env("RCLONE_RC_USER", &self.creds.user)
            .env("RCLONE_RC_PASS", &self.creds.pass)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        if let Some(pw) = &self.cfg.config_password {
            cmd.env("RCLONE_CONFIG_PASS", pw);
        }
        cmd
    }

    /// Make sure rclone.conf is encrypted with the keyring-held password.
    /// rclone then keeps it encrypted on every write (token refreshes too).
    async fn ensure_encrypted_config(&self) -> Result<()> {
        let Some(pw) = &self.cfg.config_password else {
            return Ok(());
        };
        let encrypted = std::fs::read_to_string(&self.cfg.config_file)
            .map(|s| s.starts_with("# Encrypted rclone configuration"))
            .unwrap_or(false);
        if encrypted {
            return Ok(());
        }
        if let Some(dir) = self.cfg.config_file.parent() {
            std::fs::create_dir_all(dir)?;
        }
        // The password travels via the child's environment (readable only by
        // this user), never via argv.
        let out = Command::new(&self.cfg.binary)
            .arg(format!("--config={}", self.cfg.config_file.display()))
            .args([
                "config",
                "encryption",
                "set",
                "--password-command",
                "printenv SKY_RCLONE_NEW_PASS",
            ])
            .env_clear()
            .envs(passthrough_env())
            .env("SKY_RCLONE_NEW_PASS", pw)
            .stdin(Stdio::null())
            .output()
            .await
            .map_err(|e| CoreError::Process(format!("cannot run rclone: {e}")))?;
        if !out.status.success() {
            return Err(CoreError::Process(format!(
                "could not encrypt rclone config: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            )));
        }
        tracing::info!("rclone configuration is now encrypted");
        Ok(())
    }

    /// Spawn rclone and wait until the RC API answers.
    pub async fn start(&self) -> Result<()> {
        self.ensure_encrypted_config().await?;
        if self.cfg.socket.exists() {
            // Stale socket from a crashed run.
            let _ = std::fs::remove_file(&self.cfg.socket);
        }
        let mut child = self.command().spawn().map_err(|e| {
            CoreError::Process(format!("cannot start {}: {e}", self.cfg.binary.display()))
        })?;
        for stream in [
            child
                .stdout
                .take()
                .map(|s| Box::new(s) as Box<dyn tokio::io::AsyncRead + Send + Unpin>),
            child
                .stderr
                .take()
                .map(|s| Box::new(s) as Box<dyn tokio::io::AsyncRead + Send + Unpin>),
        ]
        .into_iter()
        .flatten()
        {
            let tx = self.logs.clone();
            let recent = self.recent.clone();
            tokio::spawn(async move {
                let mut lines = BufReader::new(stream).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    tracing::debug!(target: "rclone", "{line}");
                    if let Ok(mut r) = recent.lock() {
                        if r.len() == RECENT_LOG_LINES {
                            r.pop_front();
                        }
                        r.push_back(line.clone());
                    }
                    let _ = tx.send(RcloneLogLine { line });
                }
            });
        }
        *self.child.lock().await = Some(child);

        let client = self.client();
        for _ in 0..100 {
            if client.version().await.is_ok() {
                self.running.send_replace(true);
                return Ok(());
            }
            if let Some(status) = self
                .child
                .lock()
                .await
                .as_mut()
                .and_then(|c| c.try_wait().ok().flatten())
            {
                return Err(CoreError::Process(format!(
                    "rclone exited during startup: {status}"
                )));
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        Err(CoreError::Process(
            "rclone did not become ready within 10s".into(),
        ))
    }

    /// Start rclone and keep it alive until [`RcloneSupervisor::stop`].
    /// Restarts with exponential backoff (1s → 60s).
    pub async fn run(self: Arc<Self>) {
        let mut backoff = Duration::from_secs(1);
        let mut shutdown = self.shutdown.subscribe();
        loop {
            if *shutdown.borrow() {
                return;
            }
            match self.start().await {
                Ok(()) => {
                    tracing::info!("rclone started");
                    backoff = Duration::from_secs(1);
                    let status = {
                        let mut guard = self.child.lock().await;
                        let child = guard.as_mut().expect("child set by start");
                        tokio::select! {
                            s = child.wait() => Some(s),
                            _ = shutdown.changed() => None,
                        }
                    };
                    self.running.send_replace(false);
                    match status {
                        None => return,
                        Some(s) => tracing::warn!("rclone exited: {s:?}; restarting"),
                    }
                }
                Err(e) => {
                    self.running.send_replace(false);
                    tracing::error!("failed to start rclone: {e}");
                }
            }
            tokio::select! {
                _ = tokio::time::sleep(backoff) => {}
                _ = shutdown.changed() => return,
            }
            backoff = (backoff * 2).min(Duration::from_secs(60));
        }
    }

    /// Ask rclone to quit gracefully, then kill it if needed.
    pub async fn stop(&self) {
        self.shutdown.send_replace(true);
        let _ = self.client().quit().await;
        if let Some(mut child) = self.child.lock().await.take() {
            if tokio::time::timeout(Duration::from_secs(5), child.wait())
                .await
                .is_err()
            {
                let _ = child.kill().await;
            }
        }
        self.running.send_replace(false);
        let _ = std::fs::remove_file(&self.cfg.socket);
    }
}

/// Environment variables rclone legitimately needs (browser opening, locale,
/// proxies). Everything else — including any stray `RCLONE_*` — is dropped
/// so the user's personal rclone setup can't leak into Sky Linux.
fn passthrough_env() -> Vec<(String, String)> {
    const KEEP: &[&str] = &[
        "HOME",
        "PATH",
        "LANG",
        "LC_ALL",
        "TZ",
        "DISPLAY",
        "WAYLAND_DISPLAY",
        "XDG_RUNTIME_DIR",
        "XDG_CONFIG_HOME",
        "XDG_CACHE_HOME",
        "XDG_DATA_DIRS",
        "DBUS_SESSION_BUS_ADDRESS",
        "BROWSER",
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "NO_PROXY",
        "http_proxy",
        "https_proxy",
        "no_proxy",
        "SSL_CERT_FILE",
        "SSL_CERT_DIR",
    ];
    std::env::vars()
        .filter(|(k, _)| KEEP.contains(&k.as_str()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_rclone_wins_over_one_next_to_the_binary() {
        let root = tempfile::tempdir().unwrap();
        let bin = root.path().join("usr/bin");
        let lib = root.path().join("usr/lib/sky-linux");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(&lib).unwrap();
        std::fs::write(bin.join("rclone"), "distro").unwrap();
        std::fs::write(lib.join("rclone"), "bundled").unwrap();
        let found = rclone_candidates(Some(&bin), std::slice::from_ref(&bin))
            .into_iter()
            .find(|p| p.is_file())
            .unwrap();
        assert_eq!(std::fs::read_to_string(found).unwrap(), "bundled");
    }

    #[test]
    fn env_passthrough_drops_rclone_vars() {
        std::env::set_var("RCLONE_CONFIG_PASS_LEAK_TEST", "x");
        assert!(passthrough_env()
            .iter()
            .all(|(k, _)| !k.starts_with("RCLONE_")));
    }
}
