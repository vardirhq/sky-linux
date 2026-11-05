//! Upload-only backups.
//!
//! A backup job copies a local folder to the cloud using `rclone copy`
//! semantics: new and changed files are uploaded, **nothing is ever deleted
//! remotely**, even if files disappear locally (e.g. an unmounted drive or an
//! accidental `rm -rf`). Jobs run on filesystem changes (inotify via
//! `notify`, debounced), on an interval, or manually.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use notify::{RecursiveMode, Watcher};
use tokio::sync::{mpsc, Mutex};
use tokio::task::JoinHandle;
use uuid::Uuid;

use crate::db::Database;
use crate::error::{CoreError, Result};
use crate::events::{Event, EventBus};
use crate::models::*;
use crate::transfers::{TransferManager, TransferSpec};

/// Default exclusions: caches, editor temp files, and things that are
/// rebuilt or useless off-machine.
pub const DEFAULT_EXCLUDES: &[&str] = &[
    ".cache/**",
    ".Trash-*/**",
    "node_modules/**",
    "*.tmp",
    "*.part",
    "*.crdownload",
    "*.swp",
    "~$*",
    ".~lock.*#",
];

/// Folders that must never be backed up as a whole.
const FORBIDDEN_ROOTS: &[&str] = &[
    "/", "/bin", "/boot", "/dev", "/etc", "/lib", "/lib64", "/proc", "/root", "/run", "/sbin",
    "/sys", "/tmp", "/usr", "/var",
];

/// Validate a job definition before it is stored.
pub fn validate(local: &Path, remote_path: &str) -> Result<PathBuf> {
    if !local.is_absolute() {
        return Err(CoreError::rule(
            "relative_path",
            "Choose a folder using its full path.",
        ));
    }
    let canon = local.canonicalize().map_err(|_| {
        CoreError::rule(
            "folder_missing",
            format!("The folder {} doesn't exist.", local.display()),
        )
        .with("path", local.display())
    })?;
    if !canon.is_dir() {
        return Err(CoreError::rule(
            "not_a_folder",
            format!("{} is not a folder.", local.display()),
        )
        .with("path", local.display()));
    }
    if FORBIDDEN_ROOTS.iter().any(|r| canon == Path::new(r)) {
        return Err(CoreError::safety(
            "system_folder",
            format!(
                "{} is a system folder. Choose a folder with your own files, such as Documents or Pictures.",
                canon.display()
            ),
        )
        .with("path", canon.display()));
    }
    let remote = remote_path.trim_matches('/');
    if remote.is_empty() {
        return Err(CoreError::rule(
            "remote_missing",
            "Choose a cloud folder to back up into.",
        ));
    }
    if remote.split('/').any(|seg| seg == ".." || seg == ".") {
        return Err(CoreError::rule(
            "remote_dots",
            "Cloud folder names can't contain '.' or '..'.",
        ));
    }
    Ok(canon)
}

#[derive(Clone)]
pub struct BackupManager {
    db: Database,
    bus: EventBus,
    transfers: TransferManager,
    tasks: Arc<Mutex<HashMap<Uuid, JoinHandle<()>>>>,
    running: Arc<Mutex<HashMap<Uuid, Arc<Mutex<()>>>>>,
}

impl BackupManager {
    pub fn new(db: Database, bus: EventBus, transfers: TransferManager) -> Self {
        Self {
            db,
            bus,
            transfers,
            tasks: Default::default(),
            running: Default::default(),
        }
    }

    pub fn list(&self) -> Result<Vec<BackupJob>> {
        self.db.backup_jobs()
    }

    pub async fn create(&self, new: NewBackupJob) -> Result<BackupJob> {
        let account = self.db.account(new.account_id)?;
        let local = validate(&new.local_path, &new.remote_path)?;
        crate::areas::check_writable_target(&account, &new.remote_path)?;
        let name = new
            .name
            .filter(|n| !n.trim().is_empty())
            .unwrap_or_else(|| {
                local
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "Backup".into())
            });
        let mut exclude: Vec<String> = DEFAULT_EXCLUDES.iter().map(|s| s.to_string()).collect();
        exclude.extend(new.exclude);
        exclude.dedup();
        let job = BackupJob {
            id: Uuid::new_v4(),
            account_id: new.account_id,
            name,
            local_path: local,
            remote_path: new.remote_path.trim_matches('/').to_string(),
            schedule: new.schedule,
            enabled: true,
            exclude,
            last_run: None,
            last_success: None,
            last_error: None,
            created_at: Utc::now(),
        };
        self.db.upsert_backup_job(&job)?;
        self.bus.publish(Event::BackupJobChanged(job.clone()));
        self.schedule(job.clone()).await;
        Ok(job)
    }

    pub async fn set_enabled(&self, id: Uuid, enabled: bool) -> Result<BackupJob> {
        let mut job = self.db.backup_job(id)?;
        job.enabled = enabled;
        self.db.upsert_backup_job(&job)?;
        self.bus.publish(Event::BackupJobChanged(job.clone()));
        self.schedule(job.clone()).await;
        Ok(job)
    }

    /// Remove the job definition. Files already in the cloud are kept.
    pub async fn delete(&self, id: Uuid) -> Result<()> {
        if let Some(h) = self.tasks.lock().await.remove(&id) {
            h.abort();
        }
        self.db.delete_backup_job(id)?;
        self.bus.publish(Event::BackupJobRemoved { id });
        Ok(())
    }

    /// Start schedulers for all stored jobs.
    pub async fn start_all(&self) -> Result<()> {
        for job in self.db.backup_jobs()? {
            self.schedule(job).await;
        }
        Ok(())
    }

    async fn schedule(&self, job: BackupJob) {
        let mut tasks = self.tasks.lock().await;
        if let Some(h) = tasks.remove(&job.id) {
            h.abort();
        }
        if !job.enabled {
            return;
        }
        let me = self.clone();
        tasks.insert(job.id, tokio::spawn(async move { me.scheduler(job).await }));
    }

    async fn scheduler(self, job: BackupJob) {
        match job.schedule {
            Schedule::Manual => {
                // Catch up once on start: changes may have happened while we were off.
                let _ = self.run_now(job.id).await;
            }
            Schedule::Interval { minutes } => {
                let mut tick = tokio::time::interval(Duration::from_secs(minutes.max(5) * 60));
                loop {
                    tick.tick().await; // first tick fires immediately = catch-up
                    let _ = self.run_now(job.id).await;
                }
            }
            Schedule::OnChange {
                debounce_seconds,
                fallback_hours,
            } => {
                // Watch first, then catch up, so no change slips through the gap.
                let (tx, mut rx) = mpsc::channel::<()>(1);
                let watcher =
                    notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
                        if let Ok(ev) = res {
                            if !ev.kind.is_access() {
                                let _ = tx.try_send(());
                            }
                        }
                    })
                    .and_then(|mut w| {
                        w.watch(&job.local_path, RecursiveMode::Recursive)
                            .map(|_| w)
                    });
                let watcher = match watcher {
                    Ok(w) => Some(w),
                    Err(e) => {
                        // Usually fs.inotify.max_user_watches exhausted: fall
                        // back to polling.
                        tracing::warn!(job = %job.id, "file watching unavailable ({e}); falling back to hourly backups");
                        None
                    }
                };
                let fallback = if watcher.is_some() {
                    Duration::from_secs(fallback_hours.max(1) * 3600)
                } else {
                    Duration::from_secs(3600)
                };
                let debounce = Duration::from_secs(debounce_seconds.max(1));
                let _ = self.run_now(job.id).await;
                loop {
                    tokio::select! {
                        got = rx.recv() => {
                            if got.is_none() { std::future::pending::<()>().await; }
                            // Wait until the folder has been quiet for `debounce`.
                            while let Ok(Some(())) = tokio::time::timeout(debounce, rx.recv()).await {}
                        }
                        _ = tokio::time::sleep(fallback) => {}
                    }
                    let _ = self.run_now(job.id).await;
                }
            }
        }
    }

    /// Run a backup now and wait for it. Runs of the same job are
    /// serialised, so a trigger during a run produces one more (cheap) pass
    /// that picks up whatever changed meanwhile.
    pub async fn run_now(&self, id: Uuid) -> Result<BackupJob> {
        let lock = self.running.lock().await.entry(id).or_default().clone();
        let _guard = lock.lock().await;
        self.run_inner(id).await
    }

    async fn run_inner(&self, id: Uuid) -> Result<BackupJob> {
        let mut job = self.db.backup_job(id)?;
        let account = self.db.account(job.account_id)?;
        job.last_run = Some(Utc::now());

        if !job.local_path.is_dir() {
            // Removable drive unplugged or folder moved: skip, don't fail
            // loudly and — crucially — don't treat it as "everything deleted".
            let already_missing = job
                .last_error
                .as_ref()
                .is_some_and(|e| e.rule.as_deref() == Some("folder_unavailable"));
            job.last_error = Some(
                CoreError::rule(
                    "folder_unavailable",
                    format!(
                        "The folder {} isn't available right now. The backup will continue when it's back.",
                        job.local_path.display()
                    ),
                )
                .with("path", job.local_path.display())
                .to_user(),
            );
            self.db.upsert_backup_job(&job)?;
            let ev = Event::BackupFinished(job.clone());
            // Into the history once, when the folder goes missing, not on
            // every scheduled run while a drive is unplugged.
            if !already_missing {
                self.db.record_event(&ev)?;
            }
            self.bus.publish(ev);
            return Ok(job);
        }

        self.bus.publish(Event::BackupStarted { job_id: id });
        let spec = TransferSpec::UploadDir {
            local: job.local_path.clone(),
            remote_dir: job.remote_path.clone(),
            exclude: job.exclude.clone(),
        };
        match self
            .transfers
            .run_to_completion(&account, spec, Some(id))
            .await
        {
            Ok(t) if t.state == TransferState::Completed => {
                job.last_success = job.last_run;
                job.last_error = None;
            }
            Ok(t) => job.last_error = t.error,
            Err(e) => job.last_error = Some(e.to_user()),
        }
        self.db.upsert_backup_job(&job)?;
        let ev = Event::BackupFinished(job.clone());
        self.db.record_event(&ev)?;
        self.bus.publish(ev);
        Ok(job)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_system_folders() {
        for p in ["/", "/etc"] {
            let e = validate(Path::new(p), "Backup").unwrap_err().to_user();
            assert_eq!(e.code, "safety");
            assert_eq!(e.rule.as_deref(), Some("system_folder"));
            assert_eq!(e.params["path"], p);
        }
    }

    #[test]
    fn rejects_bad_input() {
        let dir = tempfile::tempdir().unwrap();
        assert!(validate(Path::new("relative/path"), "Backup").is_err());
        assert!(validate(&dir.path().join("missing"), "Backup").is_err());
        assert!(validate(dir.path(), "").is_err());
        assert!(validate(dir.path(), "Backup/../x").is_err());
    }

    #[test]
    fn accepts_user_folder() {
        let dir = tempfile::tempdir().unwrap();
        let p = validate(dir.path(), "/Backup/Pictures/").unwrap();
        assert_eq!(p, dir.path().canonicalize().unwrap());
    }
}
