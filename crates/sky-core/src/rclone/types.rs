//! Wire types for the subset of the rclone RC API that Sky Core uses.
//! Field names follow rclone's JSON exactly.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::models::{CloudEntry, EntryKind, LiveStats, StorageUsage};

#[derive(Debug, Clone, Deserialize)]
pub struct VersionInfo {
    pub version: String,
    #[serde(default)]
    pub os: String,
    #[serde(default)]
    pub arch: String,
}

/// One item of `operations/list`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ListItem {
    pub path: String,
    pub name: String,
    #[serde(default)]
    pub size: i64,
    #[serde(default)]
    pub mime_type: Option<String>,
    #[serde(default)]
    pub mod_time: Option<DateTime<Utc>>,
    #[serde(default)]
    pub is_dir: bool,
}

impl From<ListItem> for CloudEntry {
    fn from(i: ListItem) -> Self {
        CloudEntry {
            kind: if i.is_dir {
                EntryKind::Folder
            } else {
                EntryKind::File
            },
            // rclone reports -1 for unknown sizes (typically directories).
            size: if i.is_dir {
                None
            } else {
                u64::try_from(i.size).ok()
            },
            name: i.name,
            path: i.path,
            modified: i.mod_time,
            mime_type: i.mime_type.filter(|m| m != "inode/directory"),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ListResponse {
    #[serde(default)]
    pub list: Vec<ListItem>,
}

/// `operations/about`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct AboutResponse {
    pub total: Option<u64>,
    pub used: Option<u64>,
    pub free: Option<u64>,
    pub trashed: Option<u64>,
}

impl From<AboutResponse> for StorageUsage {
    fn from(a: AboutResponse) -> Self {
        StorageUsage {
            used: a.used.unwrap_or(0),
            total: a.total,
            free: a.free,
            trashed: a.trashed,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct JobStarted {
    pub jobid: i64,
}

/// `job/status`.
#[derive(Debug, Clone, Deserialize)]
pub struct JobStatus {
    pub id: i64,
    pub finished: bool,
    pub success: bool,
    #[serde(default)]
    pub error: String,
    #[serde(default)]
    pub duration: f64,
    #[serde(default)]
    pub group: String,
    /// The method's result once finished (for bisync: its log `output`).
    #[serde(default)]
    pub output: serde_json::Value,
}

/// Options for `sync/bisync`, using rclone's RC parameter names.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BisyncOpts {
    pub path1: String,
    pub path2: String,
    pub workdir: String,
    pub resync: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resync_mode: Option<String>,
    pub check_access: bool,
    pub check_filename: String,
    /// Percentage. Must always be sent: over RC the default is 0%, which
    /// aborts on any deletion (verified with rclone v1.75.2).
    pub max_delete: u8,
    pub force: bool,
    pub resilient: bool,
    pub recover: bool,
    pub conflict_resolve: String,
    pub conflict_loser: String,
    pub conflict_suffix: String,
    pub create_empty_src_dirs: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backup_dir1: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filters_file: Option<String>,
    pub max_lock: String,
    pub dry_run: bool,
    /// Global options for this call only (rclone's `_config`), e.g.
    /// `{"SuffixKeepExtension": true}`.
    #[serde(rename = "_config", skip_serializing_if = "Option::is_none")]
    pub config: Option<serde_json::Value>,
}

/// `core/stats`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoreStats {
    #[serde(default)]
    pub bytes: u64,
    #[serde(default)]
    pub total_bytes: u64,
    #[serde(default)]
    pub speed: f64,
    #[serde(default)]
    pub eta: Option<f64>,
    #[serde(default)]
    pub transfers: u64,
    #[serde(default)]
    pub total_transfers: u64,
    #[serde(default)]
    pub errors: u64,
    #[serde(default)]
    pub last_error: Option<String>,
    #[serde(default)]
    pub transferring: Vec<TransferringItem>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct TransferringItem {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub size: i64,
    #[serde(default)]
    pub bytes: u64,
    #[serde(default)]
    pub speed: f64,
    #[serde(default)]
    pub eta: Option<f64>,
    #[serde(default)]
    pub percentage: u8,
}

impl From<&CoreStats> for LiveStats {
    fn from(s: &CoreStats) -> Self {
        LiveStats {
            bytes: s.bytes,
            total_bytes: s.total_bytes,
            speed: s.speed,
            eta_seconds: s
                .eta
                .filter(|e| e.is_finite() && *e >= 0.0)
                .map(|e| e as u64),
            transfers: s.transfers,
            total_transfers: s.total_transfers,
            errors: s.errors,
        }
    }
}

/// Error body rclone returns on non-200 responses.
#[derive(Debug, Clone, Deserialize)]
pub struct RcErrorBody {
    pub error: String,
    #[serde(default)]
    pub path: String,
}

/// Rclone filter options passed as `_filter`.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct FilterOpt {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub exclude_rule: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_item_maps_directories() {
        let json = r#"{"list":[
            {"Path":"Photos","Name":"Photos","Size":-1,"MimeType":"inode/directory","ModTime":"2025-01-22T21:18:00Z","IsDir":true},
            {"Path":"Norway.jpg","Name":"Norway.jpg","Size":9332736,"MimeType":"image/jpeg","ModTime":"2025-01-21T19:55:00Z","IsDir":false}
        ]}"#;
        let r: ListResponse = serde_json::from_str(json).unwrap();
        let entries: Vec<CloudEntry> = r.list.into_iter().map(Into::into).collect();
        assert_eq!(entries[0].kind, EntryKind::Folder);
        assert_eq!(entries[0].size, None);
        assert_eq!(entries[0].mime_type, None);
        assert_eq!(entries[1].size, Some(9_332_736));
        assert_eq!(entries[1].mime_type.as_deref(), Some("image/jpeg"));
    }

    #[test]
    fn stats_ignore_missing_eta() {
        let s: CoreStats =
            serde_json::from_str(r#"{"bytes":10,"totalBytes":100,"speed":5.0,"eta":null}"#)
                .unwrap();
        let live = LiveStats::from(&s);
        assert_eq!(live.eta_seconds, None);
        assert_eq!(live.total_bytes, 100);
    }
}
