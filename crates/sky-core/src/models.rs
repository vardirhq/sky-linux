//! Typed models shared by Sky Core, the daemon and the desktop UI.
//! Everything here is serialised as camelCase JSON across IPC.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

use crate::provider::Provider;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: Uuid,
    pub provider: Provider,
    /// rclone remote name, e.g. `sky-telia-no`.
    pub remote: String,
    pub display_name: Option<String>,
    pub email: Option<String>,
    pub connected_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EntryKind {
    File,
    Folder,
}

/// A file or folder in the cloud.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudEntry {
    pub name: String,
    /// Path relative to the remote root, using `/`.
    pub path: String,
    pub kind: EntryKind,
    /// Bytes; `None` for folders where the size is unknown.
    pub size: Option<u64>,
    pub modified: Option<DateTime<Utc>>,
    pub mime_type: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageUsage {
    pub used: u64,
    pub total: Option<u64>,
    pub free: Option<u64>,
    pub trashed: Option<u64>,
}

impl StorageUsage {
    pub fn fraction_used(&self) -> Option<f64> {
        self.total
            .filter(|t| *t > 0)
            .map(|t| self.used as f64 / t as f64)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TransferDirection {
    Upload,
    Download,
}

impl TransferDirection {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Upload => "upload",
            Self::Download => "download",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "upload" => Some(Self::Upload),
            "download" => Some(Self::Download),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TransferState {
    Queued,
    Running,
    Completed,
    Failed,
    Cancelled,
    /// Was running when the daemon stopped; will be retried on start.
    Interrupted,
}

impl TransferState {
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Interrupted => "interrupted",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "queued" => Self::Queued,
            "running" => Self::Running,
            "completed" => Self::Completed,
            "failed" => Self::Failed,
            "cancelled" => Self::Cancelled,
            "interrupted" => Self::Interrupted,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Transfer {
    pub id: Uuid,
    pub account_id: Uuid,
    pub direction: TransferDirection,
    /// Local path (upload) or remote path (download).
    pub source: String,
    /// Remote path (upload) or local path (download).
    pub destination: String,
    pub state: TransferState,
    pub bytes_total: Option<u64>,
    pub bytes_done: u64,
    /// Bytes per second.
    pub speed: Option<f64>,
    pub eta_seconds: Option<u64>,
    pub error: Option<crate::UserError>,
    /// rclone async job id while running.
    pub rclone_job: Option<i64>,
    /// Backup job that created this transfer, if any.
    pub backup_job: Option<Uuid>,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum Schedule {
    /// Upload shortly after local changes are detected (plus a safety net
    /// run every `fallback_hours`, in case an inotify event was missed).
    #[serde(rename_all = "camelCase")]
    OnChange {
        debounce_seconds: u64,
        fallback_hours: u64,
    },
    /// Run every N minutes.
    Interval { minutes: u64 },
    /// Only when the user presses "Back up now".
    Manual,
}

impl Default for Schedule {
    fn default() -> Self {
        Schedule::OnChange {
            debounce_seconds: 30,
            fallback_hours: 24,
        }
    }
}

/// An upload-only backup of a local folder. Backups never delete remote files.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupJob {
    pub id: Uuid,
    pub account_id: Uuid,
    pub name: String,
    pub local_path: PathBuf,
    /// Destination relative to the remote root, e.g. `Backup/Pictures`.
    pub remote_path: String,
    pub schedule: Schedule,
    pub enabled: bool,
    pub exclude: Vec<String>,
    pub last_run: Option<DateTime<Utc>>,
    pub last_success: Option<DateTime<Utc>>,
    pub last_error: Option<crate::UserError>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewBackupJob {
    pub account_id: Uuid,
    pub name: Option<String>,
    pub local_path: PathBuf,
    pub remote_path: String,
    #[serde(default)]
    pub schedule: Schedule,
    #[serde(default)]
    pub exclude: Vec<String>,
}

/// Aggregated live transfer statistics (from rclone `core/stats`).
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveStats {
    pub bytes: u64,
    pub total_bytes: u64,
    /// Bytes per second.
    pub speed: f64,
    pub eta_seconds: Option<u64>,
    pub transfers: u64,
    pub total_transfers: u64,
    pub errors: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DaemonStatus {
    pub version: String,
    pub rclone_version: Option<String>,
    pub rclone_running: bool,
    pub accounts: Vec<Account>,
    pub active_transfers: usize,
    pub backup_jobs: usize,
}
