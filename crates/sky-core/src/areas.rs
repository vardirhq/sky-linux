//! Storage areas of a Jottacloud account.
//!
//! A Jottacloud account's built-in device (`Jotta`) has a fixed set of
//! mountpoints. rclone opens one at a time and defaults to `Archive`, but
//! Telia Sky users' files can live in any of them: on a real account all
//! 35.6 GB were in `Photos` (the phone's photo backup) and `Archive` was
//! empty. Sky Core therefore joins all areas into one rclone `combine`
//! remote, so every path starts with its area: `Photos/2025/IMG_1.jpg`.
//! All areas share the signed-in remote (and its token) through connection
//! strings such as `sky-telia-no,mountpoint=Photos:`.

use crate::error::{CoreError, Result};
use crate::models::Account;

/// Mountpoints of the built-in `Jotta` device. rclone treats this set as
/// fixed ("the mountpoint choice is exclusive", backend/jottacloud v1.75.2),
/// and a real Telia Sky account showed exactly these.
pub const JOTTA_AREAS: &[&str] = &["Archive", "Photos", "Shared", "Sync"];

/// Areas Sky Linux only reads from. `Photos` is the phone backup and photo
/// timeline, and `Shared` holds what others share; writing there could
/// confuse Telia's own apps, so it waits until it has been tested.
pub const READ_ONLY_AREAS: &[&str] = &["Photos", "Shared"];

/// Where a cloud area lives relative to the signed-in rclone remote.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AreaLayout {
    /// Jottacloud mountpoints (`remote,mountpoint=Photos:`).
    Mountpoints,
    /// Plain subfolders (`remote:Photos`); used by tests with a local remote.
    Subfolders,
}

impl AreaLayout {
    /// rclone fs string for one area.
    pub fn upstream(self, base_remote: &str, area: &str) -> String {
        match self {
            AreaLayout::Mountpoints => format!("{base_remote},mountpoint={area}:"),
            AreaLayout::Subfolders => format!("{base_remote}:{area}"),
        }
    }
}

/// The `upstreams` value of an rclone `combine` remote joining `areas`.
pub fn combine_upstreams(base_remote: &str, areas: &[String], layout: AreaLayout) -> String {
    areas
        .iter()
        .map(|a| format!("\"{a}={}\"", layout.upstream(base_remote, a)))
        .collect::<Vec<_>>()
        .join(" ")
}

/// The area a cloud path belongs to, if the account has areas.
pub fn area_of<'a>(account: &Account, path: &'a str) -> Option<&'a str> {
    if account.areas.is_empty() {
        return None;
    }
    path.trim_matches('/')
        .split('/')
        .next()
        .filter(|a| !a.is_empty())
}

/// Refuse writes that would create, change or delete something outside a
/// writable area. Accounts without areas (e.g. an adopted plain remote)
/// accept any path.
pub fn check_writable(account: &Account, path: &str) -> Result<()> {
    if account.areas.is_empty() {
        return Ok(());
    }
    let p = path.trim_matches('/');
    let Some(area) = area_of(account, p) else {
        return Err(CoreError::rule(
            "area_root",
            "Choose a folder inside one of your storage areas, such as Archive.",
        ));
    };
    if !account.areas.iter().any(|a| a == area) {
        return Err(CoreError::rule(
            "unknown_area",
            format!("“{area}” is not one of your storage areas."),
        )
        .with("area", area));
    }
    if p == area {
        // The area itself can't be renamed, moved, deleted or replaced.
        return Err(CoreError::rule(
            "area_root",
            "Choose a folder inside one of your storage areas, such as Archive.",
        ));
    }
    if READ_ONLY_AREAS.contains(&area) {
        return Err(CoreError::rule(
            "area_read_only",
            format!("Sky Linux can't change files in {area} yet. You can still browse and download them."),
        )
        .with("area", area));
    }
    Ok(())
}

/// Like [`check_writable`], but the area folder itself is allowed (e.g. a
/// sync pair or backup that targets all of `Archive`).
pub fn check_writable_target(account: &Account, path: &str) -> Result<()> {
    let p = path.trim_matches('/');
    match area_of(account, p) {
        Some(area) if p == area => check_writable(account, &format!("{area}/x")),
        _ => check_writable(account, p),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::Provider;

    fn account(areas: &[&str]) -> Account {
        Account {
            id: uuid::Uuid::new_v4(),
            provider: Provider::TeliaSkyNo,
            remote: "sky-telia-no-all".into(),
            areas: areas.iter().map(|s| s.to_string()).collect(),
            display_name: None,
            email: None,
            connected_at: chrono::Utc::now(),
        }
    }

    fn rule(r: Result<()>) -> String {
        r.unwrap_err().to_user().rule.unwrap()
    }

    #[test]
    fn builds_combine_upstreams() {
        let areas = vec!["Archive".to_string(), "Photos".to_string()];
        assert_eq!(
            combine_upstreams("sky-telia-no", &areas, AreaLayout::Mountpoints),
            r#""Archive=sky-telia-no,mountpoint=Archive:" "Photos=sky-telia-no,mountpoint=Photos:""#
        );
        assert_eq!(
            combine_upstreams("cloud", &areas, AreaLayout::Subfolders),
            r#""Archive=cloud:Archive" "Photos=cloud:Photos""#
        );
    }

    #[test]
    fn writes_need_a_writable_area() {
        let a = account(JOTTA_AREAS);
        assert!(check_writable(&a, "Archive/Docs/a.txt").is_ok());
        assert!(check_writable(&a, "/Sync/x/").is_ok());
        assert_eq!(
            rule(check_writable(&a, "Photos/2025/IMG_1.jpg")),
            "area_read_only"
        );
        assert_eq!(rule(check_writable(&a, "Shared/x")), "area_read_only");
        assert_eq!(rule(check_writable(&a, "Archive")), "area_root");
        assert_eq!(rule(check_writable(&a, "")), "area_root");
        assert_eq!(rule(check_writable(&a, "Telia Sky")), "unknown_area");
    }

    #[test]
    fn targets_may_be_a_whole_writable_area() {
        let a = account(JOTTA_AREAS);
        assert!(check_writable_target(&a, "Archive").is_ok());
        assert!(check_writable_target(&a, "Archive/Telia Sky").is_ok());
        assert_eq!(rule(check_writable_target(&a, "Photos")), "area_read_only");
        assert_eq!(rule(check_writable_target(&a, "")), "area_root");
    }

    #[test]
    fn plain_accounts_accept_any_path() {
        let a = account(&[]);
        assert!(check_writable(&a, "Photos/x").is_ok());
        assert!(check_writable(&a, "x").is_ok());
        assert_eq!(area_of(&a, "Photos/x"), None);
    }
}
