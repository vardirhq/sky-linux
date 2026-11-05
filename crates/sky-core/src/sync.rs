//! Two-way synchronisation, built on `rclone bisync`.
//!
//! Sync can delete files, so every run is wrapped in safeguards:
//!
//! 1. **The first run is a merge.** `resync` with "newer wins" copies files
//!    in both directions and never deletes anything.
//! 2. **Nothing local is lost.** Local files that sync deletes or overwrites
//!    are moved to a per-pair recovery folder (`backupDir1`). On the cloud
//!    side Jottacloud keeps deleted files in its trash and keeps old versions.
//! 3. **Marker files.** A `.sky-linux-sync` file lives on both sides. If the
//!    local folder disappears (unmounted drive) or is replaced, the marker is
//!    gone and sync stops instead of mirroring "everything was deleted".
//! 4. **Delete limit.** A run that would delete more than `max_delete_percent`
//!    of the files stops and waits for the user to review.
//! 5. **Conflicts keep both versions.** The newer file wins; the other is kept
//!    next to it as `name.conflict1.ext`.
//! 6. **Resilient.** Interrupted runs recover on the next run; a lost bisync
//!    state is rebuilt with another non-destructive merge.
//!
//! Local changes trigger a run via inotify (debounced); cloud changes are
//! picked up by polling every `poll_minutes`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use notify::{RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, Mutex};
use tokio::task::JoinHandle;
use uuid::Uuid;

use crate::backup::DEFAULT_EXCLUDES;
use crate::db::Database;
use crate::error::{CoreError, Result};
use crate::events::{Event, EventBus};
use crate::models::*;
use crate::paths::AppPaths;
use crate::rclone::types::BisyncOpts;
use crate::rclone::{remote_fs, RcloneClient};

/// Marker file kept on both sides of every sync pair.
pub const MARKER: &str = ".sky-linux-sync";
const DEFAULT_MAX_DELETE_PERCENT: u8 = 50;
const DEFAULT_POLL_MINUTES: u32 = 5;
const LOCAL_DEBOUNCE: Duration = Duration::from_secs(3);

/// How the user resolves a pair that needs review.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewAction {
    /// The deletions were intended: apply them once.
    AllowDeletions,
    /// Bring back everything deleted on either side (non-destructive merge).
    RestoreAll,
    /// The folder is back (e.g. the drive was mounted): just try again.
    Retry,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RunMode {
    Normal,
    /// Merge both sides without deleting (first run, restore, lost state).
    Merge,
    /// Apply deletions beyond the limit, once, after the user approved.
    Force,
}

/// A pair's scheduler task and the channel that wakes it.
type Scheduler = (JoinHandle<()>, mpsc::Sender<()>);

struct PairDirs {
    work: PathBuf,
    filters: PathBuf,
}

#[derive(Clone)]
pub struct SyncManager {
    db: Database,
    bus: EventBus,
    rclone: RcloneClient,
    paths: AppPaths,
    tasks: Arc<Mutex<HashMap<Uuid, Scheduler>>>,
    locks: Arc<Mutex<HashMap<Uuid, Arc<Mutex<()>>>>>,
}

fn under(a: &Path, b: &Path) -> bool {
    a == b || a.starts_with(b)
}

fn remote_under(a: &str, b: &str) -> bool {
    a == b || a.starts_with(&format!("{b}/"))
}

impl SyncManager {
    pub fn new(db: Database, bus: EventBus, rclone: RcloneClient, paths: AppPaths) -> Self {
        Self {
            db,
            bus,
            rclone,
            paths,
            tasks: Default::default(),
            locks: Default::default(),
        }
    }

    fn dirs(&self, id: Uuid) -> PairDirs {
        let base = self.paths.data_dir.join("sync").join(id.to_string());
        PairDirs {
            work: base.join("work"),
            filters: base.join("filters.txt"),
        }
    }

    pub fn list(&self) -> Result<Vec<SyncPair>> {
        self.db.sync_pairs()
    }

    /// Check a proposed pair against the safety rules.
    fn validate(&self, new: &NewSyncPair) -> Result<(PathBuf, String)> {
        if !new.local_path.is_absolute() {
            return Err(CoreError::rule(
                "relative_path",
                "Choose a folder using its full path.",
            ));
        }
        if new.create_local && !new.local_path.exists() {
            std::fs::create_dir_all(&new.local_path)?;
        }
        if let (Some(home), Ok(local)) = (
            dirs::home_dir().and_then(|h| h.canonicalize().ok()),
            new.local_path.canonicalize(),
        ) {
            if local == home {
                return Err(CoreError::safety(
                    "home_folder",
                    "Your whole home folder can't be synced. Choose or create a folder inside it, such as “Telia Sky”.",
                ));
            }
        }
        let local = crate::backup::validate(&new.local_path, &new.remote_path)?;
        let remote = new.remote_path.trim_matches('/').to_string();
        for own in [
            &self.paths.data_dir,
            &self.paths.config_dir,
            &self.paths.cache_dir,
        ] {
            let own = own.canonicalize().unwrap_or_else(|_| own.clone());
            if under(&local, &own) || under(&own, &local) {
                return Err(CoreError::safety(
                    "app_folder",
                    "This folder contains Sky Linux's own data and can't be synced.",
                ));
            }
        }
        for p in self.db.sync_pairs()? {
            if under(&local, &p.local_path) || under(&p.local_path, &local) {
                return Err(CoreError::rule(
                    "overlaps_sync",
                    format!(
                        "{} overlaps the synced folder “{}”.",
                        local.display(),
                        p.name
                    ),
                )
                .with("path", local.display())
                .with("name", &p.name));
            }
            if p.account_id == new.account_id
                && (remote_under(&remote, &p.remote_path) || remote_under(&p.remote_path, &remote))
            {
                return Err(CoreError::rule(
                    "overlaps_sync_remote",
                    format!(
                        "The cloud folder {remote} overlaps the synced folder “{}”.",
                        p.name
                    ),
                )
                .with("path", &remote)
                .with("name", &p.name));
            }
        }
        Ok((local, remote))
    }

    pub async fn create(&self, new: NewSyncPair) -> Result<SyncPair> {
        let account = self.db.account(new.account_id)?;
        let (local, remote) = self.validate(&new)?;
        crate::areas::check_writable_target(&account, &remote)?;
        let id = Uuid::new_v4();
        let name = new
            .name
            .clone()
            .filter(|n| !n.trim().is_empty())
            .unwrap_or_else(|| {
                local
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "Sync".into())
            });
        let dirs = self.dirs(id);
        std::fs::create_dir_all(&dirs.work)?;
        let recovery = self
            .paths
            .data_dir
            .join("sync")
            .join(id.to_string())
            .join("recovery");
        std::fs::create_dir_all(&recovery)?;
        // Fixed for the pair's lifetime: bisync requires a resync whenever
        // the filters change.
        let mut filters: String = DEFAULT_EXCLUDES
            .iter()
            .map(|r| format!("- {r}\n"))
            .collect();
        filters.push_str("- .sky-linux-partial*\n");
        std::fs::write(&dirs.filters, filters)?;

        let pair = SyncPair {
            id,
            account_id: new.account_id,
            name,
            local_path: local,
            remote_path: remote,
            enabled: true,
            max_delete_percent: DEFAULT_MAX_DELETE_PERCENT,
            poll_minutes: DEFAULT_POLL_MINUTES,
            state: SyncState::New,
            last_sync: None,
            last_summary: None,
            last_error: None,
            recovery_path: recovery,
            created_at: Utc::now(),
        };
        self.db.upsert_sync_pair(&pair)?;
        self.bus.publish(Event::SyncPairChanged(pair.clone()));
        self.schedule(pair.clone()).await;
        Ok(pair)
    }

    pub async fn set_enabled(&self, id: Uuid, enabled: bool) -> Result<SyncPair> {
        let mut pair = self.db.sync_pair(id)?;
        pair.enabled = enabled;
        self.db.upsert_sync_pair(&pair)?;
        self.bus.publish(Event::SyncPairChanged(pair.clone()));
        self.schedule(pair.clone()).await;
        Ok(pair)
    }

    /// Stop syncing. Files stay where they are on both sides.
    pub async fn delete(&self, id: Uuid) -> Result<()> {
        if let Some((h, _)) = self.tasks.lock().await.remove(&id) {
            h.abort();
        }
        let pair = self.db.sync_pair(id)?;
        self.db.delete_sync_pair(id)?;
        let _ = std::fs::remove_file(pair.local_path.join(MARKER));
        // Keep the recovery folder: it may hold the user's only copy of a file.
        let _ = std::fs::remove_dir_all(self.dirs(id).work);
        self.bus.publish(Event::SyncPairRemoved { id });
        Ok(())
    }

    pub async fn start_all(&self) -> Result<()> {
        for p in self.db.sync_pairs()? {
            self.schedule(p).await;
        }
        Ok(())
    }

    /// Files in a pair's recovery folder, newest first.
    pub fn recovery_files(&self, id: Uuid) -> Result<Vec<crate::recovery::RecoveredFile>> {
        crate::recovery::list(&self.db.sync_pair(id)?.recovery_path)
    }

    /// Move files from the recovery folder back into the synced folder
    /// (never overwriting), then sync so they return to the cloud too.
    pub async fn restore(&self, id: Uuid, paths: Vec<String>) -> Result<usize> {
        let restored = {
            // No sync run may move files while we do.
            let lock = self.locks.lock().await.entry(id).or_default().clone();
            let _guard = lock.lock().await;
            let pair = self.db.sync_pair(id)?;
            if !pair.local_path.is_dir() {
                return Err(CoreError::rule(
                    "folder_unavailable",
                    format!(
                        "The folder {} isn't available right now.",
                        pair.local_path.display()
                    ),
                )
                .with("path", pair.local_path.display()));
            }
            let mut n = 0;
            for p in &paths {
                crate::recovery::restore(&pair.recovery_path, &pair.local_path, p)?;
                n += 1;
            }
            n
        };
        self.nudge(id).await;
        Ok(restored)
    }

    /// Ask a pair's scheduler to sync soon.
    pub async fn nudge(&self, id: Uuid) {
        if let Some((_, tx)) = self.tasks.lock().await.get(&id) {
            let _ = tx.try_send(());
        }
    }

    async fn schedule(&self, pair: SyncPair) {
        let mut tasks = self.tasks.lock().await;
        if let Some((h, _)) = tasks.remove(&pair.id) {
            h.abort();
        }
        if !pair.enabled {
            return;
        }
        let (tx, rx) = mpsc::channel::<()>(1);
        let id = pair.id;
        let me = self.clone();
        let handle = tokio::spawn(async move { me.scheduler(pair, rx).await });
        tasks.insert(id, (handle, tx));
    }

    async fn scheduler(self, pair: SyncPair, mut wake: mpsc::Receiver<()>) {
        let (tx, mut changes) = mpsc::channel::<()>(1);
        let watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if let Ok(ev) = res {
                let ours = ev.paths.iter().all(|p| {
                    p.file_name()
                        .map(|n| n.to_string_lossy().starts_with(".sky-linux"))
                        .unwrap_or(false)
                });
                if !ev.kind.is_access() && !ours {
                    let _ = tx.try_send(());
                }
            }
        })
        .and_then(|mut w| {
            w.watch(&pair.local_path, RecursiveMode::Recursive)
                .map(|_| w)
        });
        let _watcher = match watcher {
            Ok(w) => Some(w),
            Err(e) => {
                tracing::warn!(pair = %pair.id, "file watching unavailable ({e}); relying on polling");
                None
            }
        };
        let poll = Duration::from_secs(u64::from(pair.poll_minutes.max(1)) * 60);

        let _ = self.run_scheduled(pair.id).await;
        loop {
            while changes.try_recv().is_ok() {} // our own writes
            tokio::select! {
                got = changes.recv() => {
                    if got.is_none() { std::future::pending::<()>().await; }
                    while let Ok(Some(())) = tokio::time::timeout(LOCAL_DEBOUNCE, changes.recv()).await {}
                }
                _ = wake.recv() => {}
                _ = tokio::time::sleep(poll) => {}
            }
            let _ = self.run_scheduled(pair.id).await;
        }
    }

    /// Scheduled runs skip pairs that wait for the user.
    async fn run_scheduled(&self, id: Uuid) -> Result<SyncPair> {
        let pair = self.db.sync_pair(id)?;
        if matches!(pair.state, SyncState::NeedsReview { .. }) {
            return Ok(pair);
        }
        let mode = if pair.state == SyncState::New {
            RunMode::Merge
        } else {
            RunMode::Normal
        };
        self.run(id, mode).await
    }

    /// Sync now (unless the pair waits for review).
    pub async fn run_now(&self, id: Uuid) -> Result<SyncPair> {
        self.run_scheduled(id).await
    }

    /// Resolve a pair that stopped for review.
    pub async fn resolve(&self, id: Uuid, action: ReviewAction) -> Result<SyncPair> {
        let pair = self.db.sync_pair(id)?;
        let reason = match pair.state {
            SyncState::NeedsReview { reason } => reason,
            _ => return self.run_now(id).await,
        };
        let mode = match (action, reason) {
            (ReviewAction::AllowDeletions, ReviewReason::TooManyDeletes) => RunMode::Force,
            (ReviewAction::AllowDeletions, _) => {
                return Err(CoreError::safety(
                    "folder_not_confirmed",
                    "Deletions can only be approved when the folder itself is available.",
                ))
            }
            (ReviewAction::RestoreAll, _) => {
                if !pair.local_path.is_dir() {
                    return Err(CoreError::rule(
                        "folder_unavailable",
                        format!(
                            "The folder {} isn't available right now.",
                            pair.local_path.display()
                        ),
                    )
                    .with("path", pair.local_path.display()));
                }
                RunMode::Merge
            }
            (ReviewAction::Retry, _) => RunMode::Normal,
        };
        self.run(id, mode).await
    }

    async fn run(&self, id: Uuid, mode: RunMode) -> Result<SyncPair> {
        let lock = self.locks.lock().await.entry(id).or_default().clone();
        let _guard = lock.lock().await;
        let mut pair = self.db.sync_pair(id)?;
        let account = self.db.account(pair.account_id)?;

        if !pair.local_path.is_dir() {
            return self.needs_review(pair, ReviewReason::FolderUnavailable);
        }
        let marker = pair.local_path.join(MARKER);
        if mode == RunMode::Merge {
            self.place_markers(&pair, &account.remote).await?;
        } else if !marker.is_file() {
            return self.needs_review(pair, ReviewReason::MarkerMissing);
        }

        pair.state = SyncState::Syncing;
        self.db.upsert_sync_pair(&pair)?;
        self.bus.publish(Event::SyncStarted { id });

        let mut outcome = self.bisync(&pair, &account.remote, mode).await;
        if mode == RunMode::Normal {
            if let Err(msg) = &outcome {
                if needs_resync(msg) {
                    // bisync lost its state (crash, changed filters): rebuild
                    // it with a merge, which never deletes.
                    tracing::warn!(pair = %id, "bisync state lost; merging both sides again");
                    self.place_markers(&pair, &account.remote).await?;
                    outcome = self.bisync(&pair, &account.remote, RunMode::Merge).await;
                }
            }
        }

        match outcome {
            Ok(summary) => {
                pair.state = SyncState::Idle;
                pair.last_sync = Some(Utc::now());
                pair.last_summary = Some(summary);
                pair.last_error = None;
            }
            Err(msg) if msg.contains("too many deletes") => {
                return self.needs_review(pair, ReviewReason::TooManyDeletes);
            }
            Err(msg) if is_access_failure(&msg) => {
                return self.needs_review(pair, ReviewReason::MarkerMissing);
            }
            Err(msg) => {
                pair.state = SyncState::Error;
                pair.last_error = Some(
                    CoreError::Rclone {
                        path: "sync/bisync".into(),
                        status: 500,
                        message: msg,
                    }
                    .to_user(),
                );
            }
        }
        self.finish(pair)
    }

    fn needs_review(&self, mut pair: SyncPair, reason: ReviewReason) -> Result<SyncPair> {
        tracing::warn!(pair = %pair.id, ?reason, "sync stopped for review");
        pair.state = SyncState::NeedsReview { reason };
        self.finish(pair)
    }

    fn finish(&self, pair: SyncPair) -> Result<SyncPair> {
        self.db.upsert_sync_pair(&pair)?;
        let ev = Event::SyncFinished(pair.clone());
        let noteworthy = !matches!(pair.state, SyncState::Idle)
            || pair
                .last_summary
                .is_some_and(|s| s != SyncSummary::default());
        if noteworthy {
            self.db.record_event(&ev)?;
        }
        self.bus.publish(ev);
        Ok(pair)
    }

    async fn place_markers(&self, pair: &SyncPair, remote: &str) -> Result<()> {
        let marker = pair.local_path.join(MARKER);
        if !marker.is_file() {
            std::fs::write(
                &marker,
                format!(
                    "This folder is synced by Sky Linux (pair {}). Please don't delete this file.\n",
                    pair.id
                ),
            )?;
        }
        self.rclone
            .mkdir(&remote_fs(remote, ""), &pair.remote_path)
            .await?;
        self.rclone
            .copy_file(
                &pair.local_path.to_string_lossy(),
                MARKER,
                &remote_fs(remote, &pair.remote_path),
                MARKER,
            )
            .await
    }

    /// Run bisync and wait. `Err` carries rclone's error text.
    async fn bisync(
        &self,
        pair: &SyncPair,
        remote: &str,
        mode: RunMode,
    ) -> Result<SyncSummary, String> {
        let dirs = self.dirs(pair.id);
        std::fs::create_dir_all(&dirs.work).map_err(|e| e.to_string())?;
        let opts = BisyncOpts {
            path1: pair.local_path.to_string_lossy().into(),
            path2: remote_fs(remote, &pair.remote_path),
            workdir: dirs.work.to_string_lossy().into(),
            resync: mode == RunMode::Merge,
            resync_mode: (mode == RunMode::Merge).then(|| "newer".into()),
            check_access: true,
            check_filename: MARKER.into(),
            max_delete: pair.max_delete_percent,
            force: mode == RunMode::Force,
            resilient: true,
            recover: true,
            conflict_resolve: "newer".into(),
            conflict_loser: "num".into(),
            conflict_suffix: "conflict".into(),
            create_empty_src_dirs: true,
            backup_dir1: Some(pair.recovery_path.to_string_lossy().into()),
            filters_file: Some(dirs.filters.to_string_lossy().into()),
            max_lock: "2m".into(),
            dry_run: false,
            // `photo.conflict1.jpg`, not `photo.jpg.conflict1`, so the copy
            // still opens in the right program.
            config: Some(serde_json::json!({ "SuffixKeepExtension": true })),
        };
        let group = format!("sync-{}", pair.id);
        let job = self
            .rclone
            .bisync_async(&opts, &group)
            .await
            .map_err(|e| e.to_string())?;
        let status = loop {
            match self.rclone.job_status(job).await {
                Ok(st) if st.finished => break st,
                Ok(_) => {}
                Err(e) => return Err(e.to_string()),
            }
            tokio::time::sleep(Duration::from_millis(300)).await;
        };
        let _ = self.rclone.reset_stats(&group).await;
        if !status.success {
            return Err(status.error);
        }
        let log = status
            .output
            .get("output")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        Ok(parse_summary(log))
    }
}

fn needs_resync(msg: &str) -> bool {
    let m = msg.to_lowercase();
    m.contains("--resync") || m.contains("cannot find prior") || m.contains("must run")
}

fn is_access_failure(msg: &str) -> bool {
    let m = msg.to_lowercase();
    m.contains("check file check failed")
        || m.contains("access health")
        || m.contains("check access")
}

/// Strip ANSI colour codes from rclone log output.
fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            for c in chars.by_ref() {
                if c.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Count what a bisync run did from its log.
pub fn parse_summary(log: &str) -> SyncSummary {
    let mut s = SyncSummary::default();
    for line in strip_ansi(log).lines() {
        let words: Vec<&str> = line.split_whitespace().collect();
        let side = words
            .iter()
            .position(|w| *w == "Path1" || *w == "Path2")
            .map(|i| words[i]);
        if line.contains("New or changed in both paths") {
            s.conflicts += 1;
        } else if line.contains("Queue copy to Path2") && !line.contains(".conflict") {
            s.uploaded += 1;
        } else if line.contains("Queue copy to Path1") && !line.contains(".conflict") {
            s.downloaded += 1;
        } else if line.contains("Queue delete") {
            match side {
                Some("Path1") => s.deleted_local += 1,
                Some("Path2") => s.deleted_remote += 1,
                _ => {}
            }
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bisync_log() {
        // Shape captured from rclone v1.75.2 (colours included).
        let log = "\
2026/11/04 NOTICE: - \u{1b}[34mWARNING\u{1b}[0m  \u{1b}[35mNew or changed in both paths\u{1b}[0m       - \u{1b}[36mone.txt\u{1b}[0m
2026/11/04 NOTICE: - \u{1b}[36mPath1\u{1b}[0m    \u{1b}[35m\u{1b}[32mQueue copy to\u{1b}[0m Path2\u{1b}[0m       - \u{1b}[36m/b/one.txt.conflict1\u{1b}[0m
2026/11/04 NOTICE: - \u{1b}[34mPath2\u{1b}[0m    \u{1b}[35m\u{1b}[32mQueue copy to\u{1b}[0m Path1\u{1b}[0m       - \u{1b}[36m/a/one.txt\u{1b}[0m
2026/11/04 NOTICE: - Path1    Queue copy to Path2       - /b/new.txt
2026/11/04 NOTICE: - Path1    Queue delete              - /a/two.txt
2026/11/04 NOTICE: - Path2    Queue delete              - /b/gone.txt
";
        assert_eq!(
            parse_summary(log),
            SyncSummary {
                uploaded: 1,
                downloaded: 1,
                deleted_local: 1,
                deleted_remote: 1,
                conflicts: 1
            }
        );
    }

    #[test]
    fn classifies_errors() {
        assert!(needs_resync(
            "Bisync critical error: cannot find prior Path1 or Path2 listings"
        ));
        assert!(!needs_resync("too many deletes"));
        assert!(is_access_failure(
            "Bisync critical error: check file check failed"
        ));
    }

    #[test]
    fn remote_overlap() {
        assert!(remote_under("Sky/Photos", "Sky"));
        assert!(!remote_under("Sky2", "Sky"));
    }
}
