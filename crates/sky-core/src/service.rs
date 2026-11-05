//! [`SkyCore`]: the façade that wires every component together. The daemon
//! owns exactly one instance.

use std::path::PathBuf;
use std::sync::Arc;

use chrono::{Duration as ChronoDuration, Utc};
use uuid::Uuid;

use crate::accounts::AccountManager;
use crate::areas::{area_of, check_writable, check_writable_target};
use crate::backup::BackupManager;
use crate::db::Database;
use crate::error::{CoreError, Result};
use crate::events::{Event, EventBus, HistoryEntry};
use crate::models::*;
use crate::paths::AppPaths;
use crate::rclone::{remote_fs, RcloneClient, RcloneSupervisor, SupervisorConfig};
use crate::secrets::{SecretStore, RCLONE_CONFIG_PASSWORD};
use crate::sync::SyncManager;
use crate::transfers::{TransferManager, TransferSpec};

/// How long a cached folder listing is served without revalidation.
const CACHE_TTL_SECONDS: i64 = 30;

pub struct CoreOptions {
    pub paths: AppPaths,
    pub rclone_binary: PathBuf,
    pub secrets: Arc<dyn SecretStore>,
    /// Encrypt rclone.conf with a keyring-held password.
    pub encrypt_rclone_config: bool,
}

pub struct SkyCore {
    pub paths: AppPaths,
    pub db: Database,
    pub bus: EventBus,
    pub rclone: Arc<RcloneSupervisor>,
    pub accounts: AccountManager,
    pub transfers: TransferManager,
    pub backups: BackupManager,
    pub sync: SyncManager,
}

impl SkyCore {
    /// Open the database, start rclone and all background loops.
    pub async fn start(opts: CoreOptions) -> Result<Arc<Self>> {
        opts.paths.ensure()?;
        let db = Database::open(&opts.paths.database())?;
        let bus = EventBus::new();

        let config_password = if opts.encrypt_rclone_config {
            Some(opts.secrets.get_or_create(RCLONE_CONFIG_PASSWORD)?)
        } else {
            None
        };
        let rclone = Arc::new(RcloneSupervisor::new(SupervisorConfig {
            binary: opts.rclone_binary,
            config_file: opts.paths.rclone_config(),
            socket: opts.paths.rclone_socket(),
            config_password,
            extra_args: vec![],
        }));
        rclone.start().await?;
        apply_bandwidth(&db, &rclone.client()).await;
        let version = rclone.client().version().await?;
        if !crate::rclone::version_supported(&version.version) {
            tracing::warn!(
                "rclone {} is older than the tested minimum {:?}",
                version.version,
                crate::rclone::MIN_RCLONE_VERSION
            );
        }

        let transfers = TransferManager::new(db.clone(), bus.clone(), rclone.client());
        let backups = BackupManager::new(db.clone(), bus.clone(), transfers.clone());
        let accounts = AccountManager::new(db.clone(), bus.clone(), rclone.clone());
        let sync = SyncManager::new(db.clone(), bus.clone(), rclone.client(), opts.paths.clone());

        let core = Arc::new(Self {
            paths: opts.paths,
            db,
            bus,
            rclone,
            accounts,
            transfers,
            backups,
            sync,
        });
        core.spawn_background();
        let resumed = core.transfers.resume_interrupted().await?;
        if resumed > 0 {
            tracing::info!(resumed, "resumed interrupted transfers");
        }
        core.backups.start_all().await?;
        core.sync.start_all().await?;
        Ok(core)
    }

    fn spawn_background(self: &Arc<Self>) {
        // Supervisor: keeps rclone alive after the initial start. `run`
        // restarts the process when it exits.
        let sup = self.rclone.clone();
        let mut running = sup.running();
        let bus = self.bus.clone();
        let (db, client) = (self.db.clone(), self.client());
        tokio::spawn(async move {
            while running.changed().await.is_ok() {
                let r = *running.borrow();
                if r {
                    // A restarted rclone has forgotten the limit.
                    apply_bandwidth(&db, &client).await;
                }
                bus.publish(Event::RcloneStatus { running: r });
            }
        });
        let sup2 = self.rclone.clone();
        let mut running2 = sup2.running();
        tokio::spawn(async move {
            // Wait until the first instance dies, then hand over to the
            // restart loop.
            while *running2.borrow() {
                if running2.changed().await.is_err() {
                    return;
                }
            }
            sup2.run().await;
        });

        tokio::spawn(self.transfers.clone().run());

        // History persistence + auth-expiry detection.
        let mut rx = self.bus.subscribe();
        let db = self.db.clone();
        tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    // Other history events are recorded by the component
                    // that produced them, inside the same state change.
                    Ok(e @ (Event::AuthExpired { .. } | Event::Error(_))) => {
                        let _ = db.record_event(&e);
                    }
                    Ok(_) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(_) => return,
                }
            }
        });
        let db = self.db.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(std::time::Duration::from_secs(3600));
            loop {
                tick.tick().await;
                let _ = db.prune_history(5000);
            }
        });
    }

    pub fn client(&self) -> RcloneClient {
        self.rclone.client()
    }

    fn account_or_default(&self, id: Option<Uuid>) -> Result<Account> {
        match id {
            Some(id) => self.accounts.get(id),
            None => self.accounts.default_account(),
        }
    }

    /// Inspect an error; if it means the token is dead, tell the UI.
    fn observe<T>(&self, account: &Account, r: Result<T>) -> Result<T> {
        if let Err(e) = &r {
            if e.to_user().code == "auth_expired" {
                self.bus.publish(Event::AuthExpired {
                    account_id: account.id,
                });
            }
        }
        r
    }

    pub async fn status(&self) -> Result<DaemonStatus> {
        let rclone_version = self.client().version().await.ok().map(|v| v.version);
        Ok(DaemonStatus {
            version: env!("CARGO_PKG_VERSION").to_string(),
            rclone_running: rclone_version.is_some(),
            rclone_version,
            accounts: self.accounts.list()?,
            active_transfers: self.transfers.active().await.len(),
            backup_jobs: self.backups.list()?.len(),
            sync_pairs: self.sync.list()?.len(),
        })
    }

    // ---- browsing ---------------------------------------------------------

    pub async fn list(
        &self,
        account: Option<Uuid>,
        path: &str,
        refresh: bool,
    ) -> Result<Vec<CloudEntry>> {
        let account = self.account_or_default(account)?;
        let folder = path.trim_matches('/');
        if !refresh {
            if let Some((entries, at)) = self.db.cached_listing(account.id, folder)? {
                if Utc::now() - at < ChronoDuration::seconds(CACHE_TTL_SECONDS) {
                    return Ok(entries);
                }
            }
        }
        let r = self
            .client()
            .list(&remote_fs(&account.remote, ""), folder)
            .await;
        match self.observe(&account, r) {
            Ok(entries) => {
                self.db.cache_listing(account.id, folder, &entries)?;
                Ok(entries)
            }
            // Offline: serve stale cache rather than an empty window.
            Err(e) if e.to_user().code == "offline" => self
                .db
                .cached_listing(account.id, folder)?
                .map(|(e, _)| e)
                .ok_or(e),
            Err(e) => Err(e),
        }
    }

    pub async fn search(&self, account: Option<Uuid>, query: &str) -> Result<Vec<CloudEntry>> {
        let account = self.account_or_default(account)?;
        if query.trim().len() < 2 {
            return Ok(vec![]);
        }
        let r = self
            .client()
            .search(&remote_fs(&account.remote, ""), "", query.trim())
            .await;
        self.observe(&account, r)
    }

    pub async fn usage(&self, account: Option<Uuid>) -> Result<StorageUsage> {
        let account = self.account_or_default(account)?;
        let r = self.client().about(&remote_fs(&account.remote, "")).await;
        self.observe(&account, r)
    }

    // ---- file management --------------------------------------------------

    pub async fn mkdir(&self, account: Option<Uuid>, path: &str) -> Result<()> {
        let account = self.account_or_default(account)?;
        let path = clean_remote(path)?;
        check_writable(&account, &path)?;
        self.client()
            .mkdir(&remote_fs(&account.remote, ""), &path)
            .await?;
        self.db.invalidate_cache(account.id, parent_of(&path))?;
        Ok(())
    }

    /// Rename or move a file or folder within the same account.
    pub async fn move_entry(
        &self,
        account: Option<Uuid>,
        from: &str,
        to: &str,
        is_dir: bool,
    ) -> Result<()> {
        let account = self.account_or_default(account)?;
        let (from, to) = (clean_remote(from)?, clean_remote(to)?);
        check_writable(&account, &from)?;
        check_writable(&account, &to)?;
        if area_of(&account, &from) != area_of(&account, &to) {
            return Err(CoreError::rule(
                "move_between_areas",
                "Files can't be moved between storage areas. Download and upload them instead.",
            ));
        }
        if to == from || to.starts_with(&format!("{from}/")) {
            return Err(CoreError::rule(
                "move_into_self",
                "A folder can't be moved into itself.",
            ));
        }
        let fs = remote_fs(&account.remote, "");
        let client = self.client();
        if is_dir {
            let job = client
                .move_dir(
                    &remote_fs(&account.remote, &from),
                    &remote_fs(&account.remote, &to),
                    &format!("move-{}", Uuid::new_v4()),
                )
                .await?;
            loop {
                let st = client.job_status(job).await?;
                if st.finished {
                    if !st.success {
                        return Err(CoreError::Rclone {
                            path: "sync/move".into(),
                            status: 500,
                            message: st.error,
                        });
                    }
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            }
        } else {
            client.move_file(&fs, &from, &fs, &to).await?;
        }
        self.db.invalidate_cache(account.id, parent_of(&from))?;
        self.db.invalidate_cache(account.id, parent_of(&to))?;
        Ok(())
    }

    /// Delete a file. Jottacloud keeps deleted files in its trash.
    /// Folder deletion is deliberately not offered until it has a trash-aware
    /// confirmation flow in the UI.
    pub async fn delete_file(&self, account: Option<Uuid>, path: &str) -> Result<()> {
        let account = self.account_or_default(account)?;
        let path = clean_remote(path)?;
        check_writable(&account, &path)?;
        self.client()
            .delete_file(&remote_fs(&account.remote, ""), &path)
            .await?;
        self.db.invalidate_cache(account.id, parent_of(&path))?;
        Ok(())
    }

    // ---- transfers --------------------------------------------------------

    pub async fn upload(
        &self,
        account: Option<Uuid>,
        local: Vec<PathBuf>,
        remote_dir: &str,
    ) -> Result<Vec<Transfer>> {
        let account = self.account_or_default(account)?;
        let remote_dir = remote_dir.trim_matches('/').to_string();
        check_writable_target(&account, &remote_dir)?;
        let mut out = Vec::new();
        for path in local {
            let spec = if path.is_dir() {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                TransferSpec::UploadDir {
                    remote_dir: if remote_dir.is_empty() {
                        name
                    } else {
                        format!("{remote_dir}/{name}")
                    },
                    local: path,
                    exclude: vec![],
                }
            } else if path.is_file() {
                TransferSpec::UploadFile {
                    local: path,
                    remote_dir: remote_dir.clone(),
                }
            } else {
                return Err(CoreError::NotFound(path.display().to_string()));
            };
            out.push(self.transfers.start(&account, spec, None).await?);
        }
        Ok(out)
    }

    pub async fn download(
        &self,
        account: Option<Uuid>,
        entries: Vec<CloudEntry>,
        local_dir: PathBuf,
    ) -> Result<Vec<Transfer>> {
        let account = self.account_or_default(account)?;
        let mut out = Vec::new();
        for e in entries {
            let spec = match e.kind {
                EntryKind::File => TransferSpec::DownloadFile {
                    remote: e.path,
                    local_dir: local_dir.clone(),
                    target: None,
                },
                EntryKind::Folder => TransferSpec::DownloadDir {
                    remote: e.path,
                    local_dir: local_dir.clone(),
                    target: None,
                },
            };
            out.push(self.transfers.start(&account, spec, None).await?);
        }
        Ok(out)
    }

    pub async fn transfers(&self, limit: u32) -> Result<Vec<Transfer>> {
        let mut active = self.transfers.active().await;
        let ids: std::collections::HashSet<_> = active.iter().map(|t| t.id).collect();
        active.extend(
            self.db
                .transfers(limit)?
                .into_iter()
                .filter(|t| !ids.contains(&t.id)),
        );
        active.sort_by_key(|t| std::cmp::Reverse(t.started_at));
        active.truncate(limit as usize);
        Ok(active)
    }

    pub fn history(&self, limit: u32) -> Result<Vec<HistoryEntry>> {
        self.db.history(limit)
    }

    /// Save the bandwidth limit (MB/s, `None` or 0 for no limit) and apply it.
    pub async fn set_bandwidth_limit(&self, mb_per_s: Option<u32>) -> Result<()> {
        if mb_per_s.is_some_and(|v| v > MAX_BANDWIDTH_MB) {
            return Err(CoreError::Invalid(format!(
                "bandwidth limit must be at most {MAX_BANDWIDTH_MB} MB/s"
            )));
        }
        self.db
            .set_setting(BANDWIDTH_SETTING, &mb_per_s.filter(|v| *v > 0))?;
        self.client().set_bwlimit(&bwlimit_rate(mb_per_s)).await
    }

    /// A shareable report for bug reports (see [`crate::diagnostics`]).
    pub async fn diagnostics(&self) -> Result<crate::diagnostics::Diagnostics> {
        let rclone_version = self.client().version().await.ok().map(|v| v.version);
        let home = dirs::home_dir();
        Ok(crate::diagnostics::build(
            &crate::diagnostics::DiagnosticsInput {
                version: env!("CARGO_PKG_VERSION"),
                rclone_version: rclone_version.as_deref(),
                os: crate::diagnostics::os_name(),
                bandwidth_limit: bandwidth_setting(&self.db),
                accounts: &self.accounts.list()?,
                backups: &self.backups.list()?,
                sync_pairs: &self.sync.list()?,
                history: &self.db.history(500)?,
                rclone_log: &self.rclone.recent_logs(),
                home: home.as_deref(),
            },
        ))
    }

    pub async fn shutdown(&self) {
        self.rclone.stop().await;
    }
}

/// Settings key for the bandwidth limit in MB/s (`null` = no limit).
pub const BANDWIDTH_SETTING: &str = "bandwidthLimit";
const MAX_BANDWIDTH_MB: u32 = 10_000;

fn bandwidth_setting(db: &Database) -> Option<u32> {
    db.get_setting::<Option<u32>>(BANDWIDTH_SETTING)
        .ok()
        .flatten()
        .flatten()
        .filter(|v| *v > 0)
}

/// rclone `--bwlimit` syntax; rclone's "M" is MiB/s.
fn bwlimit_rate(mb_per_s: Option<u32>) -> String {
    match mb_per_s {
        Some(v) if v > 0 => format!("{v}M"),
        _ => "off".into(),
    }
}

/// Apply the saved limit to a running rclone. Failures are logged: the next
/// rclone start applies it again.
async fn apply_bandwidth(db: &Database, client: &RcloneClient) {
    let rate = bwlimit_rate(bandwidth_setting(db));
    if let Err(e) = client.set_bwlimit(&rate).await {
        tracing::warn!("could not apply the bandwidth limit {rate}: {e}");
    }
}

fn clean_remote(p: &str) -> Result<String> {
    let p = p.trim_matches('/');
    if p.is_empty() || p.split('/').any(|s| s == ".." || s == "." || s.is_empty()) {
        return Err(
            CoreError::rule("invalid_path", format!("'{p}' is not a valid cloud path."))
                .with("path", p),
        );
    }
    Ok(p.to_string())
}

fn parent_of(p: &str) -> &str {
    p.rfind('/').map(|i| &p[..i]).unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleans_remote_paths() {
        assert_eq!(clean_remote("/Photos/a.jpg").unwrap(), "Photos/a.jpg");
        assert!(clean_remote("").is_err());
        assert!(clean_remote("a/../b").is_err());
        assert!(clean_remote("a//b").is_err());
        assert_eq!(parent_of("a/b/c"), "a/b");
        assert_eq!(parent_of("a"), "");
    }

    #[test]
    fn bandwidth_rates() {
        assert_eq!(bwlimit_rate(None), "off");
        assert_eq!(bwlimit_rate(Some(0)), "off");
        assert_eq!(bwlimit_rate(Some(5)), "5M");
    }
}
