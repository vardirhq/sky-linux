//! Structured events. Every state change in Sky Core is published on the
//! [`EventBus`]; the daemon forwards them to connected UIs and persists the
//! user-visible ones as history.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use uuid::Uuid;

use crate::models::{Account, BackupJob, LiveStats, SyncPair, Transfer};
use crate::UserError;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "type",
    content = "data"
)]
pub enum Event {
    RcloneStatus {
        running: bool,
    },
    /// The browser sign-in page; shown in the UI in case the browser didn't open.
    AuthUrl {
        url: String,
    },
    AccountConnected(Account),
    AccountRemoved {
        id: Uuid,
    },
    AuthExpired {
        account_id: Uuid,
    },
    TransferUpdated(Transfer),
    Stats(LiveStats),
    BackupStarted {
        job_id: Uuid,
    },
    BackupFinished(BackupJob),
    BackupJobChanged(BackupJob),
    BackupJobRemoved {
        id: Uuid,
    },
    SyncStarted {
        id: Uuid,
    },
    SyncFinished(SyncPair),
    SyncPairChanged(SyncPair),
    SyncPairRemoved {
        id: Uuid,
    },
    Error(UserError),
}

impl Event {
    /// Short machine name used for the history table.
    pub fn kind(&self) -> &'static str {
        match self {
            Event::RcloneStatus { .. } => "rclone_status",
            Event::AuthUrl { .. } => "auth_url",
            Event::AccountConnected(_) => "account_connected",
            Event::AccountRemoved { .. } => "account_removed",
            Event::AuthExpired { .. } => "auth_expired",
            Event::TransferUpdated(_) => "transfer_updated",
            Event::Stats(_) => "stats",
            Event::BackupStarted { .. } => "backup_started",
            Event::BackupFinished(_) => "backup_finished",
            Event::BackupJobChanged(_) => "backup_job_changed",
            Event::BackupJobRemoved { .. } => "backup_job_removed",
            Event::SyncStarted { .. } => "sync_started",
            Event::SyncFinished(_) => "sync_finished",
            Event::SyncPairChanged(_) => "sync_pair_changed",
            Event::SyncPairRemoved { .. } => "sync_pair_removed",
            Event::Error(_) => "error",
        }
    }

    /// Whether the event belongs in the user-visible activity history.
    pub fn is_history(&self) -> bool {
        match self {
            Event::AccountConnected(_)
            | Event::AccountRemoved { .. }
            | Event::AuthExpired { .. }
            | Event::BackupFinished(_)
            | Event::SyncFinished(_)
            | Event::Error(_) => true,
            Event::TransferUpdated(t) => t.state.is_terminal(),
            _ => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    pub id: i64,
    pub at: DateTime<Utc>,
    pub event: Event,
}

#[derive(Clone)]
pub struct EventBus {
    tx: broadcast::Sender<Event>,
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

impl EventBus {
    pub fn new() -> Self {
        Self {
            tx: broadcast::channel(1024).0,
        }
    }
    pub fn publish(&self, e: Event) {
        let _ = self.tx.send(e);
    }
    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.tx.subscribe()
    }
}
