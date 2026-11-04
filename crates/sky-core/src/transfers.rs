//! Transfer manager: starts uploads/downloads as rclone async jobs, tracks
//! progress per job (each transfer gets its own rclone stats group), persists
//! state, and resumes interrupted transfers after a restart.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use tokio::sync::{oneshot, Mutex};
use uuid::Uuid;

use crate::db::Database;
use crate::error::{CoreError, Result};
use crate::events::{Event, EventBus};
use crate::models::*;
use crate::rclone::types::FilterOpt;
use crate::rclone::{remote_fs, RcloneClient};

/// What to copy. Paths are absolute local paths or remote-relative paths.
/// Persisted as JSON so interrupted transfers can be resumed exactly.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum TransferSpec {
    #[serde(rename_all = "camelCase")]
    UploadFile { local: PathBuf, remote_dir: String },
    #[serde(rename_all = "camelCase")]
    UploadDir {
        local: PathBuf,
        remote_dir: String,
        #[serde(default)]
        exclude: Vec<String>,
    },
    /// `target` is resolved (to a non-conflicting name) on first submit.
    #[serde(rename_all = "camelCase")]
    DownloadFile {
        remote: String,
        local_dir: PathBuf,
        #[serde(default)]
        target: Option<PathBuf>,
    },
    #[serde(rename_all = "camelCase")]
    DownloadDir {
        remote: String,
        local_dir: PathBuf,
        #[serde(default)]
        target: Option<PathBuf>,
    },
}

impl TransferSpec {
    fn direction(&self) -> TransferDirection {
        match self {
            Self::UploadFile { .. } | Self::UploadDir { .. } => TransferDirection::Upload,
            _ => TransferDirection::Download,
        }
    }
}

struct Tracked {
    spec: TransferSpec,
    remote: String,
    done: Option<oneshot::Sender<TransferState>>,
}

#[derive(Clone)]
pub struct TransferManager {
    db: Database,
    bus: EventBus,
    rclone: RcloneClient,
    active: Arc<Mutex<HashMap<Uuid, (Transfer, Tracked)>>>,
}

fn file_name(p: &str) -> &str {
    p.trim_end_matches('/').rsplit('/').next().unwrap_or(p)
}

fn parent(p: &str) -> &str {
    let p = p.trim_end_matches('/');
    p.rfind('/').map(|i| &p[..i]).unwrap_or("")
}

fn join_remote(dir: &str, name: &str) -> String {
    let dir = dir.trim_matches('/');
    if dir.is_empty() {
        name.to_string()
    } else {
        format!("{dir}/{name}")
    }
}

/// Pick `name`, `name (1).ext`, `name (2).ext`… so downloads never overwrite
/// an existing local file.
pub fn non_conflicting(dir: &Path, name: &str) -> PathBuf {
    let candidate = dir.join(name);
    if !candidate.exists() {
        return candidate;
    }
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name, ""),
    };
    (1..)
        .map(|n| dir.join(format!("{stem} ({n}){ext}")))
        .find(|p| !p.exists())
        .expect("infinite iterator")
}

impl TransferManager {
    pub fn new(db: Database, bus: EventBus, rclone: RcloneClient) -> Self {
        Self {
            db,
            bus,
            rclone,
            active: Default::default(),
        }
    }

    /// Start a transfer and return immediately.
    pub async fn start(
        &self,
        account: &Account,
        spec: TransferSpec,
        backup_job: Option<Uuid>,
    ) -> Result<Transfer> {
        self.start_inner(account, spec, backup_job, None).await
    }

    /// Start a transfer and wait for it to finish (used by backups).
    pub async fn run_to_completion(
        &self,
        account: &Account,
        spec: TransferSpec,
        backup_job: Option<Uuid>,
    ) -> Result<Transfer> {
        let (tx, rx) = oneshot::channel();
        let t = self
            .start_inner(account, spec, backup_job, Some(tx))
            .await?;
        let _ = rx.await;
        self.db
            .transfers(500)?
            .into_iter()
            .find(|x| x.id == t.id)
            .ok_or_else(|| CoreError::NotFound(format!("transfer {}", t.id)))
    }

    async fn start_inner(
        &self,
        account: &Account,
        mut spec: TransferSpec,
        backup_job: Option<Uuid>,
        done: Option<oneshot::Sender<TransferState>>,
    ) -> Result<Transfer> {
        let id = Uuid::new_v4();
        let (source, destination) = describe(&spec);
        let mut t = Transfer {
            id,
            account_id: account.id,
            direction: spec.direction(),
            source,
            destination,
            state: TransferState::Queued,
            bytes_total: None,
            bytes_done: 0,
            speed: None,
            eta_seconds: None,
            error: None,
            rclone_job: None,
            backup_job,
            started_at: Utc::now(),
            finished_at: None,
        };
        self.submit(&mut t, &account.remote, &mut spec).await?;
        self.db.upsert_transfer(&t)?;
        self.db
            .set_transfer_spec(t.id, &serde_json::to_string(&spec)?)?;
        self.bus.publish(Event::TransferUpdated(t.clone()));
        self.active.lock().await.insert(
            id,
            (
                t.clone(),
                Tracked {
                    spec,
                    remote: account.remote.clone(),
                    done,
                },
            ),
        );
        Ok(t)
    }

    async fn submit(&self, t: &mut Transfer, remote: &str, spec: &mut TransferSpec) -> Result<()> {
        let group = t.id.to_string();
        let job = match spec {
            TransferSpec::UploadFile { local, remote_dir } => {
                let dir = local
                    .parent()
                    .ok_or_else(|| CoreError::Invalid("bad path".into()))?;
                let name = local
                    .file_name()
                    .ok_or_else(|| CoreError::Invalid("bad path".into()))?
                    .to_string_lossy();
                self.rclone
                    .copy_file_async(
                        &dir.to_string_lossy(),
                        &name,
                        &remote_fs(remote, remote_dir),
                        &name,
                        &group,
                    )
                    .await?
            }
            TransferSpec::UploadDir {
                local,
                remote_dir,
                exclude,
            } => {
                self.rclone
                    .copy_dir_async(
                        &local.to_string_lossy(),
                        &remote_fs(remote, remote_dir),
                        &group,
                        &FilterOpt {
                            exclude_rule: exclude.clone(),
                        },
                    )
                    .await?
            }
            TransferSpec::DownloadFile {
                remote: path,
                local_dir,
                target,
            } => {
                std::fs::create_dir_all(&*local_dir)?;
                let target = target
                    .get_or_insert_with(|| non_conflicting(local_dir, file_name(path)))
                    .clone();
                t.destination = target.to_string_lossy().into();
                self.rclone
                    .copy_file_async(
                        &remote_fs(remote, parent(path)),
                        file_name(path),
                        &target.parent().unwrap_or(local_dir).to_string_lossy(),
                        &target.file_name().unwrap().to_string_lossy(),
                        &group,
                    )
                    .await?
            }
            TransferSpec::DownloadDir {
                remote: path,
                local_dir,
                target,
            } => {
                std::fs::create_dir_all(&*local_dir)?;
                let target = target
                    .get_or_insert_with(|| non_conflicting(local_dir, file_name(path)))
                    .clone();
                t.destination = target.to_string_lossy().into();
                self.rclone
                    .copy_dir_async(
                        &remote_fs(remote, path),
                        &target.to_string_lossy(),
                        &group,
                        &FilterOpt::default(),
                    )
                    .await?
            }
        };
        t.rclone_job = Some(job);
        t.state = TransferState::Running;
        Ok(())
    }

    pub async fn active(&self) -> Vec<Transfer> {
        self.active
            .lock()
            .await
            .values()
            .map(|(t, _)| t.clone())
            .collect()
    }

    pub async fn cancel(&self, id: Uuid) -> Result<()> {
        let mut active = self.active.lock().await;
        let (mut t, tracked) = active
            .remove(&id)
            .ok_or_else(|| CoreError::NotFound(format!("active transfer {id}")))?;
        if let Some(job) = t.rclone_job {
            let _ = self.rclone.job_stop(job).await;
        }
        t.state = TransferState::Cancelled;
        t.finished_at = Some(Utc::now());
        self.finish(t, tracked)
    }

    fn finish(&self, t: Transfer, tracked: Tracked) -> Result<()> {
        self.db.upsert_transfer(&t)?;
        let ev = Event::TransferUpdated(t.clone());
        self.db.record_event(&ev)?;
        self.bus.publish(ev);
        if let Some(tx) = tracked.done {
            let _ = tx.send(t.state);
        }
        Ok(())
    }

    /// Resume transfers interrupted by a daemon/rclone restart. rclone copy
    /// is idempotent: already-transferred files are skipped.
    pub async fn resume_interrupted(&self) -> Result<usize> {
        let mut n = 0;
        for mut t in self.db.recover_interrupted_transfers()? {
            let account = match self.db.account(t.account_id) {
                Ok(a) => a,
                Err(_) => continue,
            };
            let Some(mut spec) = self
                .db
                .transfer_spec(t.id)?
                .and_then(|j| serde_json::from_str::<TransferSpec>(&j).ok())
            else {
                continue;
            };
            match self.submit(&mut t, &account.remote, &mut spec).await {
                Ok(()) => {
                    self.db.upsert_transfer(&t)?;
                    self.active.lock().await.insert(
                        t.id,
                        (
                            t.clone(),
                            Tracked {
                                spec,
                                remote: account.remote.clone(),
                                done: None,
                            },
                        ),
                    );
                    n += 1;
                }
                Err(e) => tracing::warn!("could not resume transfer {}: {e}", t.id),
            }
        }
        Ok(n)
    }

    /// One polling pass over active transfers.
    pub async fn poll(&self) -> Result<()> {
        let ids: Vec<Uuid> = self.active.lock().await.keys().copied().collect();
        for id in ids {
            let (job, group) = {
                let active = self.active.lock().await;
                match active.get(&id) {
                    Some((t, _)) => (t.rclone_job, t.id.to_string()),
                    None => continue,
                }
            };
            let Some(job) = job else { continue };
            let status = self.rclone.job_status(job).await;
            let stats = self.rclone.stats(Some(&group)).await.ok();

            let mut active = self.active.lock().await;
            let Some((t, _)) = active.get_mut(&id) else {
                continue;
            };
            if let Some(s) = &stats {
                t.bytes_done = s.bytes;
                t.bytes_total = (s.total_bytes > 0).then_some(s.total_bytes);
                t.speed = Some(s.speed);
                t.eta_seconds = s
                    .eta
                    .filter(|e| e.is_finite() && *e >= 0.0)
                    .map(|e| e as u64);
            }
            match status {
                Ok(st) if st.finished => {
                    t.state = if st.success {
                        TransferState::Completed
                    } else {
                        TransferState::Failed
                    };
                    if !st.success {
                        t.error = Some(
                            CoreError::Rclone {
                                path: "job".into(),
                                status: 500,
                                message: st.error,
                            }
                            .to_user(),
                        );
                    }
                    if st.success {
                        if let Some(total) = t.bytes_total {
                            t.bytes_done = total;
                        }
                    }
                    t.speed = None;
                    t.eta_seconds = None;
                    t.finished_at = Some(Utc::now());
                    let (t, tracked) = active.remove(&id).unwrap();
                    drop(active);
                    let _ = self.rclone.reset_stats(&group).await;
                    if let TransferSpec::UploadFile { remote_dir, .. }
                    | TransferSpec::UploadDir { remote_dir, .. } = &tracked.spec
                    {
                        let _ = self
                            .db
                            .invalidate_cache(t.account_id, remote_dir.trim_matches('/'));
                    }
                    self.finish(t, tracked)?;
                }
                Ok(_) => {
                    let snapshot = t.clone();
                    drop(active);
                    self.bus.publish(Event::TransferUpdated(snapshot));
                }
                Err(CoreError::RcloneUnavailable(_)) => {} // supervisor is restarting it
                Err(e) => {
                    // Job vanished (rclone restarted): resubmit.
                    tracing::warn!("job {job} for transfer {id} lost ({e}); resubmitting");
                    let (mut t, mut tracked) = active.remove(&id).unwrap();
                    drop(active);
                    let remote = tracked.remote.clone();
                    match self.submit(&mut t, &remote, &mut tracked.spec).await {
                        Ok(()) => {
                            self.db.upsert_transfer(&t)?;
                            self.active.lock().await.insert(id, (t, tracked));
                        }
                        Err(e) => {
                            t.state = TransferState::Failed;
                            t.error = Some(e.to_user());
                            t.finished_at = Some(Utc::now());
                            self.finish(t, tracked)?;
                        }
                    }
                }
            }
        }
        if let Ok(s) = self.rclone.stats(None).await {
            self.bus.publish(Event::Stats(LiveStats::from(&s)));
        }
        Ok(())
    }

    /// Poll forever.
    pub async fn run(self) {
        let mut tick = tokio::time::interval(Duration::from_secs(1));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tick.tick().await;
            if let Err(e) = self.poll().await {
                tracing::warn!("transfer poll failed: {e}");
            }
        }
    }
}

fn describe(spec: &TransferSpec) -> (String, String) {
    match spec {
        TransferSpec::UploadFile { local, remote_dir } => (
            local.to_string_lossy().into(),
            join_remote(
                remote_dir,
                &local.file_name().unwrap_or_default().to_string_lossy(),
            ),
        ),
        TransferSpec::UploadDir {
            local, remote_dir, ..
        } => (local.to_string_lossy().into(), remote_dir.clone()),
        TransferSpec::DownloadFile {
            remote, local_dir, ..
        }
        | TransferSpec::DownloadDir {
            remote, local_dir, ..
        } => (
            remote.clone(),
            local_dir.join(file_name(remote)).to_string_lossy().into(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_helpers() {
        assert_eq!(file_name("Photos/2025/a.jpg"), "a.jpg");
        assert_eq!(parent("Photos/2025/a.jpg"), "Photos/2025");
        assert_eq!(parent("a.jpg"), "");
        assert_eq!(join_remote("/Photos/", "a.jpg"), "Photos/a.jpg");
        assert_eq!(join_remote("", "a.jpg"), "a.jpg");
    }

    #[test]
    fn spec_roundtrips_as_json() {
        let spec = TransferSpec::DownloadFile {
            remote: "Photos/a.jpg".into(),
            local_dir: "/home/ola/Downloads".into(),
            target: Some("/home/ola/Downloads/a (1).jpg".into()),
        };
        let json = serde_json::to_string(&spec).unwrap();
        assert!(json.contains("\"kind\":\"downloadFile\""));
        assert_eq!(serde_json::from_str::<TransferSpec>(&json).unwrap(), spec);
    }

    #[test]
    fn downloads_never_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            non_conflicting(dir.path(), "a.txt"),
            dir.path().join("a.txt")
        );
        std::fs::write(dir.path().join("a.txt"), "x").unwrap();
        assert_eq!(
            non_conflicting(dir.path(), "a.txt"),
            dir.path().join("a (1).txt")
        );
        std::fs::write(dir.path().join("a (1).txt"), "x").unwrap();
        assert_eq!(
            non_conflicting(dir.path(), "a.txt"),
            dir.path().join("a (2).txt")
        );
        std::fs::write(dir.path().join(".bashrc"), "x").unwrap();
        assert_eq!(
            non_conflicting(dir.path(), ".bashrc"),
            dir.path().join(".bashrc (1)")
        );
    }
}
