//! SQLite persistence: settings, accounts, backup jobs, transfer history,
//! activity history and the cloud metadata cache. Secrets never go here.

mod migrations;

use std::path::Path;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{de::DeserializeOwned, Serialize};
use uuid::Uuid;

use crate::error::{CoreError, Result};
use crate::events::{Event, HistoryEntry};
use crate::models::*;
use crate::provider::Provider;

/// Thread-safe handle to the database. Calls are short; heavy work happens in
/// rclone, so a single connection behind a mutex is sufficient.
#[derive(Clone)]
pub struct Database {
    conn: Arc<Mutex<Connection>>,
}

fn to_json<T: Serialize>(v: &T) -> Result<String> {
    Ok(serde_json::to_string(v)?)
}

fn from_json<T: DeserializeOwned>(s: &str) -> rusqlite::Result<T> {
    serde_json::from_str(s).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
    })
}

fn opt_json<T: DeserializeOwned>(s: Option<String>) -> rusqlite::Result<Option<T>> {
    s.map(|s| from_json(&s)).transpose()
}

impl Database {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let conn = Connection::open(path)?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.pragma_update(None, "busy_timeout", 5000)?;
        migrations::run(&conn)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    fn with<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        let conn = self.conn.lock().expect("database mutex poisoned");
        f(&conn)
    }

    pub fn schema_version(&self) -> Result<i64> {
        self.with(|c| Ok(c.query_row("PRAGMA user_version", [], |r| r.get(0))?))
    }

    // ---- settings ---------------------------------------------------------

    pub fn get_setting<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>> {
        self.with(|c| {
            let v: Option<String> = c
                .query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| {
                    r.get(0)
                })
                .optional()?;
            Ok(opt_json(v)?)
        })
    }

    pub fn set_setting<T: Serialize>(&self, key: &str, value: &T) -> Result<()> {
        let v = to_json(value)?;
        self.with(|c| {
            c.execute(
                "INSERT INTO settings(key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![key, v],
            )?;
            Ok(())
        })
    }

    // ---- accounts ---------------------------------------------------------

    fn account_row(r: &Row) -> rusqlite::Result<Account> {
        Ok(Account {
            id: r.get("id")?,
            provider: from_json(&format!("\"{}\"", r.get::<_, String>("provider")?))?,
            remote: r.get("remote")?,
            display_name: r.get("display_name")?,
            email: r.get("email")?,
            connected_at: r.get("connected_at")?,
        })
    }

    pub fn insert_account(&self, a: &Account) -> Result<()> {
        let provider = to_json(&a.provider)?.trim_matches('"').to_string();
        self.with(|c| {
            c.execute(
                "INSERT INTO accounts(id, provider, remote, display_name, email, connected_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    a.id,
                    provider,
                    a.remote,
                    a.display_name,
                    a.email,
                    a.connected_at
                ],
            )?;
            Ok(())
        })
    }

    pub fn accounts(&self) -> Result<Vec<Account>> {
        self.with(|c| {
            let mut st = c.prepare("SELECT * FROM accounts ORDER BY connected_at")?;
            let rows = st.query_map([], Self::account_row)?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
    }

    pub fn account(&self, id: Uuid) -> Result<Account> {
        self.with(|c| {
            c.query_row(
                "SELECT * FROM accounts WHERE id = ?1",
                [id],
                Self::account_row,
            )
            .optional()?
            .ok_or_else(|| CoreError::NotFound(format!("account {id}")))
        })
    }

    pub fn account_for_provider(&self, p: Provider) -> Result<Option<Account>> {
        let provider = to_json(&p)?.trim_matches('"').to_string();
        self.with(|c| {
            Ok(c.query_row(
                "SELECT * FROM accounts WHERE provider = ?1",
                [provider],
                Self::account_row,
            )
            .optional()?)
        })
    }

    /// Remove an account and its backup jobs. **Local and remote files are
    /// never touched.**
    pub fn delete_account(&self, id: Uuid) -> Result<()> {
        self.with(|c| {
            c.execute("DELETE FROM accounts WHERE id = ?1", [id])?;
            Ok(())
        })
    }

    // ---- backup jobs ------------------------------------------------------

    fn job_row(r: &Row) -> rusqlite::Result<BackupJob> {
        Ok(BackupJob {
            id: r.get("id")?,
            account_id: r.get("account_id")?,
            name: r.get("name")?,
            local_path: r.get::<_, String>("local_path")?.into(),
            remote_path: r.get("remote_path")?,
            schedule: from_json(&r.get::<_, String>("schedule")?)?,
            enabled: r.get("enabled")?,
            exclude: from_json(&r.get::<_, String>("exclude")?)?,
            last_run: r.get("last_run")?,
            last_success: r.get("last_success")?,
            last_error: opt_json(r.get("last_error")?)?,
            created_at: r.get("created_at")?,
        })
    }

    pub fn upsert_backup_job(&self, j: &BackupJob) -> Result<()> {
        let schedule = to_json(&j.schedule)?;
        let exclude = to_json(&j.exclude)?;
        let last_error = j.last_error.as_ref().map(to_json).transpose()?;
        let local = j.local_path.to_string_lossy().to_string();
        self.with(|c| {
            c.execute(
                "INSERT INTO backup_jobs(id, account_id, name, local_path, remote_path, schedule,
                    enabled, exclude, last_run, last_success, last_error, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
                 ON CONFLICT(id) DO UPDATE SET name = excluded.name,
                    local_path = excluded.local_path, remote_path = excluded.remote_path,
                    schedule = excluded.schedule, enabled = excluded.enabled,
                    exclude = excluded.exclude, last_run = excluded.last_run,
                    last_success = excluded.last_success, last_error = excluded.last_error",
                params![
                    j.id,
                    j.account_id,
                    j.name,
                    local,
                    j.remote_path,
                    schedule,
                    j.enabled,
                    exclude,
                    j.last_run,
                    j.last_success,
                    last_error,
                    j.created_at
                ],
            )?;
            Ok(())
        })
    }

    pub fn backup_jobs(&self) -> Result<Vec<BackupJob>> {
        self.with(|c| {
            let mut st = c.prepare("SELECT * FROM backup_jobs ORDER BY created_at")?;
            let rows = st.query_map([], Self::job_row)?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
    }

    pub fn backup_job(&self, id: Uuid) -> Result<BackupJob> {
        self.with(|c| {
            c.query_row(
                "SELECT * FROM backup_jobs WHERE id = ?1",
                [id],
                Self::job_row,
            )
            .optional()?
            .ok_or_else(|| CoreError::NotFound(format!("backup job {id}")))
        })
    }

    pub fn delete_backup_job(&self, id: Uuid) -> Result<()> {
        self.with(|c| {
            c.execute("DELETE FROM backup_jobs WHERE id = ?1", [id])?;
            Ok(())
        })
    }

    // ---- sync pairs -------------------------------------------------------

    fn sync_row(r: &Row) -> rusqlite::Result<SyncPair> {
        Ok(SyncPair {
            id: r.get("id")?,
            account_id: r.get("account_id")?,
            name: r.get("name")?,
            local_path: r.get::<_, String>("local_path")?.into(),
            remote_path: r.get("remote_path")?,
            enabled: r.get("enabled")?,
            max_delete_percent: r.get("max_delete_percent")?,
            poll_minutes: r.get("poll_minutes")?,
            state: from_json(&r.get::<_, String>("state")?)?,
            last_sync: r.get("last_sync")?,
            last_summary: opt_json(r.get("last_summary")?)?,
            last_error: opt_json(r.get("last_error")?)?,
            recovery_path: r.get::<_, String>("recovery_path")?.into(),
            created_at: r.get("created_at")?,
        })
    }

    pub fn upsert_sync_pair(&self, p: &SyncPair) -> Result<()> {
        let state = to_json(&p.state)?;
        let summary = p.last_summary.as_ref().map(to_json).transpose()?;
        let error = p.last_error.as_ref().map(to_json).transpose()?;
        let local = p.local_path.to_string_lossy().to_string();
        let recovery = p.recovery_path.to_string_lossy().to_string();
        self.with(|c| {
            c.execute(
                "INSERT INTO sync_pairs(id, account_id, name, local_path, remote_path, enabled,
                    max_delete_percent, poll_minutes, state, last_sync, last_summary, last_error,
                    recovery_path, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
                 ON CONFLICT(id) DO UPDATE SET name = excluded.name, enabled = excluded.enabled,
                    max_delete_percent = excluded.max_delete_percent,
                    poll_minutes = excluded.poll_minutes, state = excluded.state,
                    last_sync = excluded.last_sync, last_summary = excluded.last_summary,
                    last_error = excluded.last_error",
                params![
                    p.id,
                    p.account_id,
                    p.name,
                    local,
                    p.remote_path,
                    p.enabled,
                    p.max_delete_percent,
                    p.poll_minutes,
                    state,
                    p.last_sync,
                    summary,
                    error,
                    recovery,
                    p.created_at
                ],
            )?;
            Ok(())
        })
    }

    pub fn sync_pairs(&self) -> Result<Vec<SyncPair>> {
        self.with(|c| {
            let mut st = c.prepare("SELECT * FROM sync_pairs ORDER BY created_at")?;
            let rows = st.query_map([], Self::sync_row)?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
    }

    pub fn sync_pair(&self, id: Uuid) -> Result<SyncPair> {
        self.with(|c| {
            c.query_row(
                "SELECT * FROM sync_pairs WHERE id = ?1",
                [id],
                Self::sync_row,
            )
            .optional()?
            .ok_or_else(|| CoreError::NotFound(format!("sync pair {id}")))
        })
    }

    pub fn delete_sync_pair(&self, id: Uuid) -> Result<()> {
        self.with(|c| {
            c.execute("DELETE FROM sync_pairs WHERE id = ?1", [id])?;
            Ok(())
        })
    }

    // ---- transfers --------------------------------------------------------

    fn transfer_row(r: &Row) -> rusqlite::Result<Transfer> {
        let bad = |s: String| {
            rusqlite::Error::FromSqlConversionFailure(
                0,
                rusqlite::types::Type::Text,
                format!("bad enum value {s}").into(),
            )
        };
        let dir: String = r.get("direction")?;
        let state: String = r.get("state")?;
        Ok(Transfer {
            id: r.get("id")?,
            account_id: r.get("account_id")?,
            direction: TransferDirection::parse(&dir).ok_or_else(|| bad(dir))?,
            source: r.get("source")?,
            destination: r.get("destination")?,
            state: TransferState::parse(&state).ok_or_else(|| bad(state))?,
            bytes_total: r.get::<_, Option<i64>>("bytes_total")?.map(|v| v as u64),
            bytes_done: r.get::<_, i64>("bytes_done")? as u64,
            speed: None,
            eta_seconds: None,
            error: opt_json(r.get("error")?)?,
            rclone_job: r.get("rclone_job")?,
            backup_job: r.get("backup_job")?,
            started_at: r.get("started_at")?,
            finished_at: r.get("finished_at")?,
        })
    }

    pub fn upsert_transfer(&self, t: &Transfer) -> Result<()> {
        let error = t.error.as_ref().map(to_json).transpose()?;
        self.with(|c| {
            c.execute(
                "INSERT INTO transfers(id, account_id, direction, source, destination, state,
                    bytes_total, bytes_done, error, rclone_job, backup_job, started_at, finished_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
                 ON CONFLICT(id) DO UPDATE SET state = excluded.state,
                    bytes_total = excluded.bytes_total, bytes_done = excluded.bytes_done,
                    error = excluded.error, rclone_job = excluded.rclone_job,
                    finished_at = excluded.finished_at",
                params![
                    t.id,
                    t.account_id,
                    t.direction.as_str(),
                    t.source,
                    t.destination,
                    t.state.as_str(),
                    t.bytes_total.map(|v| v as i64),
                    t.bytes_done as i64,
                    error,
                    t.rclone_job,
                    t.backup_job,
                    t.started_at,
                    t.finished_at
                ],
            )?;
            Ok(())
        })
    }

    pub fn transfers(&self, limit: u32) -> Result<Vec<Transfer>> {
        self.with(|c| {
            let mut st = c.prepare("SELECT * FROM transfers ORDER BY started_at DESC LIMIT ?1")?;
            let rows = st.query_map([limit], Self::transfer_row)?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
    }

    pub fn set_transfer_spec(&self, id: Uuid, spec: &str) -> Result<()> {
        self.with(|c| {
            c.execute(
                "UPDATE transfers SET spec = ?2 WHERE id = ?1",
                params![id, spec],
            )?;
            Ok(())
        })
    }

    pub fn transfer_spec(&self, id: Uuid) -> Result<Option<String>> {
        self.with(|c| {
            Ok(
                c.query_row("SELECT spec FROM transfers WHERE id = ?1", [id], |r| {
                    r.get(0)
                })
                .optional()?
                .flatten(),
            )
        })
    }

    /// Transfers that were in flight when the daemon last stopped. They are
    /// marked `interrupted` and returned so they can be resumed.
    pub fn recover_interrupted_transfers(&self) -> Result<Vec<Transfer>> {
        self.with(|c| {
            c.execute(
                "UPDATE transfers SET state = 'interrupted', rclone_job = NULL
                 WHERE state IN ('queued', 'running')",
                [],
            )?;
            let mut st = c.prepare(
                "SELECT * FROM transfers WHERE state = 'interrupted' ORDER BY started_at",
            )?;
            let rows = st.query_map([], Self::transfer_row)?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
    }

    // ---- history ----------------------------------------------------------

    pub fn record_event(&self, e: &Event) -> Result<()> {
        let json = to_json(e)?;
        self.with(|c| {
            c.execute(
                "INSERT INTO history(at, kind, payload) VALUES (?1, ?2, ?3)",
                params![Utc::now(), e.kind(), json],
            )?;
            Ok(())
        })
    }

    pub fn history(&self, limit: u32) -> Result<Vec<HistoryEntry>> {
        self.with(|c| {
            let mut st =
                c.prepare("SELECT id, at, payload FROM history ORDER BY id DESC LIMIT ?1")?;
            let rows = st.query_map([limit], |r| {
                Ok(HistoryEntry {
                    id: r.get(0)?,
                    at: r.get::<_, DateTime<Utc>>(1)?,
                    event: from_json(&r.get::<_, String>(2)?)?,
                })
            })?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
    }

    /// Keep the history bounded.
    pub fn prune_history(&self, keep: u32) -> Result<usize> {
        self.with(|c| {
            Ok(c.execute(
                "DELETE FROM history WHERE id NOT IN (SELECT id FROM history ORDER BY id DESC LIMIT ?1)",
                [keep],
            )?)
        })
    }

    // ---- metadata cache ---------------------------------------------------

    /// Replace the cached listing of one remote folder.
    pub fn cache_listing(&self, account: Uuid, folder: &str, entries: &[CloudEntry]) -> Result<()> {
        let json = to_json(&entries)?;
        self.with(|c| {
            c.execute(
                "INSERT INTO metadata_cache(account_id, folder, entries, fetched_at)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(account_id, folder) DO UPDATE SET
                    entries = excluded.entries, fetched_at = excluded.fetched_at",
                params![account, folder, json, Utc::now()],
            )?;
            Ok(())
        })
    }

    pub fn cached_listing(
        &self,
        account: Uuid,
        folder: &str,
    ) -> Result<Option<(Vec<CloudEntry>, DateTime<Utc>)>> {
        self.with(|c| {
            let row: Option<(String, DateTime<Utc>)> = c
                .query_row(
                    "SELECT entries, fetched_at FROM metadata_cache WHERE account_id = ?1 AND folder = ?2",
                    params![account, folder],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?;
            Ok(row
                .map(|(e, at)| from_json(&e).map(|e| (e, at)))
                .transpose()?)
        })
    }

    pub fn invalidate_cache(&self, account: Uuid, folder: &str) -> Result<()> {
        self.with(|c| {
            c.execute(
                "DELETE FROM metadata_cache WHERE account_id = ?1 AND (folder = ?2 OR folder LIKE ?3)",
                params![account, folder, format!("{}/%", folder.trim_end_matches('/'))],
            )?;
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account() -> Account {
        Account {
            id: Uuid::new_v4(),
            provider: Provider::TeliaSkyNo,
            remote: "sky-telia-no".into(),
            display_name: Some("Ola Nordmann".into()),
            email: None,
            connected_at: Utc::now(),
        }
    }

    fn transfer(account_id: Uuid, state: TransferState) -> Transfer {
        Transfer {
            id: Uuid::new_v4(),
            account_id,
            direction: TransferDirection::Upload,
            source: "/home/ola/a.jpg".into(),
            destination: "Photos/a.jpg".into(),
            state,
            bytes_total: Some(100),
            bytes_done: 10,
            speed: None,
            eta_seconds: None,
            error: None,
            rclone_job: Some(4),
            backup_job: None,
            started_at: Utc::now(),
            finished_at: None,
        }
    }

    #[test]
    fn migrates_to_latest() {
        let db = Database::open_in_memory().unwrap();
        assert_eq!(db.schema_version().unwrap(), migrations::LATEST);
    }

    #[test]
    fn migrations_are_idempotent_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("sky.db");
        Database::open(&p).unwrap();
        let db = Database::open(&p).unwrap();
        assert_eq!(db.schema_version().unwrap(), migrations::LATEST);
    }

    #[test]
    fn settings_roundtrip() {
        let db = Database::open_in_memory().unwrap();
        assert_eq!(db.get_setting::<bool>("x").unwrap(), None);
        db.set_setting("x", &true).unwrap();
        db.set_setting("x", &false).unwrap();
        assert_eq!(db.get_setting::<bool>("x").unwrap(), Some(false));
    }

    #[test]
    fn accounts_and_jobs() {
        let db = Database::open_in_memory().unwrap();
        let a = account();
        db.insert_account(&a).unwrap();
        assert_eq!(db.accounts().unwrap(), vec![a.clone()]);
        assert_eq!(
            db.account_for_provider(Provider::TeliaSkyNo).unwrap(),
            Some(a.clone())
        );

        let mut job = BackupJob {
            id: Uuid::new_v4(),
            account_id: a.id,
            name: "Pictures".into(),
            local_path: "/home/ola/Pictures".into(),
            remote_path: "Backup/Pictures".into(),
            schedule: Schedule::default(),
            enabled: true,
            exclude: vec!["*.tmp".into()],
            last_run: None,
            last_success: None,
            last_error: None,
            created_at: Utc::now(),
        };
        db.upsert_backup_job(&job).unwrap();
        job.enabled = false;
        db.upsert_backup_job(&job).unwrap();
        assert_eq!(db.backup_jobs().unwrap(), vec![job.clone()]);

        // Deleting the account cascades to its jobs.
        db.delete_account(a.id).unwrap();
        assert!(db.backup_jobs().unwrap().is_empty());
    }

    #[test]
    fn interrupted_transfers_are_recovered() {
        let db = Database::open_in_memory().unwrap();
        let a = account();
        db.insert_account(&a).unwrap();
        let running = transfer(a.id, TransferState::Running);
        let done = transfer(a.id, TransferState::Completed);
        db.upsert_transfer(&running).unwrap();
        db.upsert_transfer(&done).unwrap();
        let rec = db.recover_interrupted_transfers().unwrap();
        assert_eq!(rec.len(), 1);
        assert_eq!(rec[0].id, running.id);
        assert_eq!(rec[0].state, TransferState::Interrupted);
        assert_eq!(rec[0].rclone_job, None);
    }

    #[test]
    fn history_is_bounded() {
        let db = Database::open_in_memory().unwrap();
        for _ in 0..5 {
            db.record_event(&Event::AccountRemoved { id: Uuid::new_v4() })
                .unwrap();
        }
        db.prune_history(2).unwrap();
        assert_eq!(db.history(10).unwrap().len(), 2);
    }

    #[test]
    fn metadata_cache_invalidation() {
        let db = Database::open_in_memory().unwrap();
        let a = account();
        db.insert_account(&a).unwrap();
        db.cache_listing(a.id, "Photos", &[]).unwrap();
        db.cache_listing(a.id, "Photos/2025", &[]).unwrap();
        db.cache_listing(a.id, "Documents", &[]).unwrap();
        db.invalidate_cache(a.id, "Photos").unwrap();
        assert!(db.cached_listing(a.id, "Photos").unwrap().is_none());
        assert!(db.cached_listing(a.id, "Photos/2025").unwrap().is_none());
        assert!(db.cached_listing(a.id, "Documents").unwrap().is_some());
    }
}
