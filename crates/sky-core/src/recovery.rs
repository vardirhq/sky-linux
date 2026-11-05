//! The recovery folder of a two-way sync pair: files that sync removed or
//! overwrote on this computer (rclone bisync's `backupDir1`). Listing it and
//! moving files back into the synced folder, never overwriting anything.

use std::path::{Component, Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::error::{CoreError, Result};
use crate::transfers::non_conflicting;

/// At most this many files are listed (newest first).
pub const MAX_LISTED: usize = 5000;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveredFile {
    /// Path relative to the synced folder, with `/` separators.
    pub path: String,
    pub size: u64,
    /// The file's own modification time (rclone keeps it when moving).
    pub modified: Option<DateTime<Utc>>,
}

/// Every file in `dir`, newest first. A missing folder is an empty list.
pub fn list(dir: &Path) -> Result<Vec<RecoveredFile>> {
    let mut out = Vec::new();
    if dir.is_dir() {
        walk(dir, dir, &mut out)?;
    }
    out.sort_by(|a, b| b.modified.cmp(&a.modified).then(a.path.cmp(&b.path)));
    out.truncate(MAX_LISTED);
    Ok(out)
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<RecoveredFile>) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        // symlink_metadata: never follow links out of the recovery folder.
        let meta = entry.path().symlink_metadata()?;
        if meta.is_dir() {
            walk(root, &entry.path(), out)?;
        } else if meta.is_file() {
            let rel = entry.path();
            let rel = rel.strip_prefix(root).unwrap_or(&rel);
            out.push(RecoveredFile {
                path: rel.to_string_lossy().replace('\\', "/"),
                size: meta.len(),
                modified: meta.modified().ok().map(DateTime::<Utc>::from),
            });
        }
    }
    Ok(())
}

/// A relative path made only of normal components (no `..`, no root).
fn safe_relative(path: &str) -> Result<PathBuf> {
    let p = Path::new(path);
    if path.is_empty() || !p.components().all(|c| matches!(c, Component::Normal(_))) {
        return Err(
            CoreError::rule("invalid_path", format!("'{path}' is not a valid path."))
                .with("path", path),
        );
    }
    Ok(p.to_path_buf())
}

/// Move `rel` from `recovery` back to the same place under `local`. If a file
/// with that name exists there, the restored one becomes `name (1).ext`.
/// Returns where the file now is.
pub fn restore(recovery: &Path, local: &Path, rel: &str) -> Result<PathBuf> {
    let rel = safe_relative(rel)?;
    let src = recovery.join(&rel);
    let meta = src
        .symlink_metadata()
        .map_err(|_| CoreError::NotFound(rel.display().to_string()))?;
    if !meta.is_file() {
        return Err(CoreError::NotFound(rel.display().to_string()));
    }
    let dir = match rel.parent() {
        Some(p) => local.join(p),
        None => local.to_path_buf(),
    };
    std::fs::create_dir_all(&dir)?;
    let name = rel
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let target = non_conflicting(&dir, &name);
    if std::fs::rename(&src, &target).is_err() {
        // Different filesystems: copy, then remove the recovered copy.
        std::fs::copy(&src, &target)?;
        std::fs::remove_file(&src)?;
    }
    remove_empty_parents(recovery, src.parent());
    Ok(target)
}

/// Tidy up folders that only held restored files, up to the recovery root.
fn remove_empty_parents(root: &Path, mut dir: Option<&Path>) {
    while let Some(d) = dir {
        if d == root || !d.starts_with(root) || std::fs::remove_dir(d).is_err() {
            return;
        }
        dir = d.parent();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let t = tempfile::tempdir().unwrap();
        let rec = t.path().join("recovery");
        let local = t.path().join("Telia Sky");
        std::fs::create_dir_all(rec.join("Docs/2026")).unwrap();
        std::fs::create_dir_all(&local).unwrap();
        std::fs::write(rec.join("Docs/2026/plan.txt"), "old plan").unwrap();
        std::fs::write(rec.join("photo.jpg"), "jpg").unwrap();
        (t, rec, local)
    }

    #[test]
    fn lists_files_with_relative_paths() {
        let (_t, rec, _) = setup();
        let mut paths: Vec<_> = list(&rec).unwrap().into_iter().map(|f| f.path).collect();
        paths.sort();
        assert_eq!(paths, ["Docs/2026/plan.txt", "photo.jpg"]);
        assert!(list(&rec.join("missing")).unwrap().is_empty());
    }

    #[test]
    fn restores_to_the_original_place_and_tidies_up() {
        let (_t, rec, local) = setup();
        let to = restore(&rec, &local, "Docs/2026/plan.txt").unwrap();
        assert_eq!(to, local.join("Docs/2026/plan.txt"));
        assert_eq!(std::fs::read_to_string(&to).unwrap(), "old plan");
        assert!(!rec.join("Docs").exists(), "empty folders are removed");
        assert!(rec.exists(), "the recovery folder itself stays");
    }

    #[test]
    fn never_overwrites_a_file_in_the_synced_folder() {
        let (_t, rec, local) = setup();
        std::fs::write(local.join("photo.jpg"), "newer").unwrap();
        let to = restore(&rec, &local, "photo.jpg").unwrap();
        assert_eq!(to, local.join("photo (1).jpg"));
        assert_eq!(
            std::fs::read_to_string(local.join("photo.jpg")).unwrap(),
            "newer"
        );
    }

    #[test]
    fn refuses_paths_outside_the_recovery_folder() {
        let (t, rec, local) = setup();
        std::fs::write(t.path().join("secret"), "x").unwrap();
        for bad in ["../secret", "/etc/passwd", "", "Docs/../../secret"] {
            assert!(restore(&rec, &local, bad).is_err(), "{bad}");
        }
        assert!(t.path().join("secret").exists());
        assert!(
            restore(&rec, &local, "Docs").is_err(),
            "folders aren't files"
        );
    }

    #[cfg(unix)]
    #[test]
    fn does_not_follow_symlinks() {
        let (t, rec, local) = setup();
        std::fs::write(t.path().join("outside.txt"), "x").unwrap();
        std::os::unix::fs::symlink(t.path().join("outside.txt"), rec.join("link.txt")).unwrap();
        assert!(!list(&rec).unwrap().iter().any(|f| f.path == "link.txt"));
        assert!(restore(&rec, &local, "link.txt").is_err());
        assert!(t.path().join("outside.txt").exists());
    }
}
