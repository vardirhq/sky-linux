//! XDG-compliant locations for Sky Linux state.

use std::path::PathBuf;

/// Directories used by Sky Core. Construct with [`AppPaths::from_env`] or
/// [`AppPaths::rooted`] (tests, portable installs).
#[derive(Debug, Clone)]
pub struct AppPaths {
    /// `~/.config/sky-linux` — settings, rclone.conf (encrypted).
    pub config_dir: PathBuf,
    /// `~/.local/share/sky-linux` — SQLite database.
    pub data_dir: PathBuf,
    /// `~/.cache/sky-linux` — metadata cache, logs.
    pub cache_dir: PathBuf,
    /// `$XDG_RUNTIME_DIR/sky-linux` — sockets (mode 0700).
    pub runtime_dir: PathBuf,
}

pub const APP_DIR: &str = "sky-linux";

impl AppPaths {
    pub fn from_env() -> Self {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        let runtime = dirs::runtime_dir()
            .unwrap_or_else(|| std::env::temp_dir().join(format!("sky-linux-{}", whoami_uid())));
        Self {
            config_dir: dirs::config_dir()
                .unwrap_or_else(|| home.join(".config"))
                .join(APP_DIR),
            data_dir: dirs::data_dir()
                .unwrap_or_else(|| home.join(".local/share"))
                .join(APP_DIR),
            cache_dir: dirs::cache_dir()
                .unwrap_or_else(|| home.join(".cache"))
                .join(APP_DIR),
            runtime_dir: runtime.join(APP_DIR),
        }
    }

    pub fn rooted(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        Self {
            config_dir: root.join("config"),
            data_dir: root.join("data"),
            cache_dir: root.join("cache"),
            runtime_dir: root.join("run"),
        }
    }

    pub fn database(&self) -> PathBuf {
        self.data_dir.join("sky.db")
    }
    pub fn rclone_config(&self) -> PathBuf {
        self.config_dir.join("rclone.conf")
    }
    pub fn daemon_socket(&self) -> PathBuf {
        self.runtime_dir.join("daemon.sock")
    }
    pub fn rclone_socket(&self) -> PathBuf {
        self.runtime_dir.join("rclone.sock")
    }

    /// Create all directories with owner-only permissions.
    pub fn ensure(&self) -> std::io::Result<()> {
        use std::os::unix::fs::PermissionsExt;
        for d in [
            &self.config_dir,
            &self.data_dir,
            &self.cache_dir,
            &self.runtime_dir,
        ] {
            std::fs::create_dir_all(d)?;
            std::fs::set_permissions(d, std::fs::Permissions::from_mode(0o700))?;
        }
        Ok(())
    }
}

fn whoami_uid() -> u32 {
    // SAFETY: getuid(2) has no preconditions and cannot fail.
    unsafe { libc::getuid() }
}
