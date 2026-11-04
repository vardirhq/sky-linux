//! The rclone adapter.
//!
//! Sky Core runs a private `rclone rcd` process and drives it through rclone's
//! JSON remote-control API. The RC server listens on a **unix socket** inside
//! `$XDG_RUNTIME_DIR/sky-linux` (a 0700 directory) and additionally requires
//! per-launch random credentials, so no other user — and no browser tab — can
//! reach it. Credentials are passed via environment variables, never argv,
//! so they do not show up in `ps`.

mod client;
mod config_flow;
mod process;
pub mod types;

pub use client::{remote_fs, RcCredentials, RcloneClient};
pub use config_flow::{jottacloud_answerer, ConfigAnswerer, ConfigQuestion, ConfigStep};
pub use process::{extract_auth_url, RcloneLogLine, RcloneSupervisor, SupervisorConfig};

/// Minimum rclone version Sky Linux is tested against. Older releases lack
/// some Jottacloud white-labels (including `telia_no`).
pub const MIN_RCLONE_VERSION: (u32, u32) = (1, 66);

/// Parse `"v1.75.2"` / `"1.75.2-beta"` into `(major, minor, patch)`.
pub fn parse_version(v: &str) -> Option<(u32, u32, u32)> {
    let v = v.trim().trim_start_matches('v');
    let core = v.split(['-', '+', ' ']).next()?;
    let mut it = core.split('.').map(|p| p.parse::<u32>().ok());
    Some((it.next()??, it.next()??, it.next().flatten().unwrap_or(0)))
}

pub fn version_supported(v: &str) -> bool {
    parse_version(v)
        .map(|(maj, min, _)| (maj, min) >= MIN_RCLONE_VERSION)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_versions() {
        assert_eq!(parse_version("v1.75.2"), Some((1, 75, 2)));
        assert_eq!(parse_version("1.66.0-beta.1234"), Some((1, 66, 0)));
        assert_eq!(parse_version("v1.70"), Some((1, 70, 0)));
        assert_eq!(parse_version("garbage"), None);
    }

    #[test]
    fn checks_minimum_version() {
        assert!(version_supported("v1.75.2"));
        assert!(!version_supported("v1.53.3"));
    }
}
