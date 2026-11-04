//! Forward-only schema migrations tracked with `PRAGMA user_version`.

use rusqlite::Connection;

const MIGRATIONS: &[&str] = &[
    // 1: initial schema
    r#"
    CREATE TABLE settings (
        key   TEXT PRIMARY KEY,
        value TEXT NOT NULL
    );

    CREATE TABLE accounts (
        id           BLOB PRIMARY KEY,
        provider     TEXT NOT NULL UNIQUE,
        remote       TEXT NOT NULL UNIQUE,
        display_name TEXT,
        email        TEXT,
        connected_at TEXT NOT NULL
    );

    CREATE TABLE backup_jobs (
        id           BLOB PRIMARY KEY,
        account_id   BLOB NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
        name         TEXT NOT NULL,
        local_path   TEXT NOT NULL,
        remote_path  TEXT NOT NULL,
        schedule     TEXT NOT NULL,
        enabled      INTEGER NOT NULL DEFAULT 1,
        exclude      TEXT NOT NULL DEFAULT '[]',
        last_run     TEXT,
        last_success TEXT,
        last_error   TEXT,
        created_at   TEXT NOT NULL,
        UNIQUE(account_id, local_path, remote_path)
    );

    CREATE TABLE transfers (
        id          BLOB PRIMARY KEY,
        account_id  BLOB NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
        direction   TEXT NOT NULL,
        source      TEXT NOT NULL,
        destination TEXT NOT NULL,
        state       TEXT NOT NULL,
        bytes_total INTEGER,
        bytes_done  INTEGER NOT NULL DEFAULT 0,
        error       TEXT,
        rclone_job  INTEGER,
        spec        TEXT,
        backup_job  BLOB REFERENCES backup_jobs(id) ON DELETE SET NULL,
        started_at  TEXT NOT NULL,
        finished_at TEXT
    );
    CREATE INDEX transfers_started ON transfers(started_at DESC);
    CREATE INDEX transfers_state ON transfers(state);

    CREATE TABLE history (
        id      INTEGER PRIMARY KEY AUTOINCREMENT,
        at      TEXT NOT NULL,
        kind    TEXT NOT NULL,
        payload TEXT NOT NULL
    );

    CREATE TABLE metadata_cache (
        account_id BLOB NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
        folder     TEXT NOT NULL,
        entries    TEXT NOT NULL,
        fetched_at TEXT NOT NULL,
        PRIMARY KEY (account_id, folder)
    );

    -- Reserved for phase 4 (two-way sync). Kept in the schema from day one so
    -- sync state survives upgrades, but nothing writes to it yet.
    CREATE TABLE sync_pairs (
        id          BLOB PRIMARY KEY,
        account_id  BLOB NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
        local_path  TEXT NOT NULL,
        remote_path TEXT NOT NULL,
        enabled     INTEGER NOT NULL DEFAULT 0,
        last_sync   TEXT,
        state       TEXT
    );
    "#,
    // 2: two-way sync. The reserved table was never written to, so it is
    // replaced rather than altered.
    r#"
    DROP TABLE sync_pairs;
    CREATE TABLE sync_pairs (
        id                 BLOB PRIMARY KEY,
        account_id         BLOB NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
        name               TEXT NOT NULL,
        local_path         TEXT NOT NULL UNIQUE,
        remote_path        TEXT NOT NULL,
        enabled            INTEGER NOT NULL DEFAULT 1,
        max_delete_percent INTEGER NOT NULL DEFAULT 50,
        poll_minutes       INTEGER NOT NULL DEFAULT 5,
        state              TEXT NOT NULL,
        last_sync          TEXT,
        last_summary       TEXT,
        last_error         TEXT,
        recovery_path      TEXT NOT NULL,
        created_at         TEXT NOT NULL,
        UNIQUE(account_id, remote_path)
    );
    "#,
];

#[allow(dead_code)] // used by tests and diagnostics
pub const LATEST: i64 = MIGRATIONS.len() as i64;

pub fn run(conn: &Connection) -> rusqlite::Result<()> {
    let current: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(current as usize) {
        let version = i as i64 + 1;
        tracing::info!(version, "applying database migration");
        let tx = conn.unchecked_transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", version)?;
        tx.commit()?;
    }
    Ok(())
}
