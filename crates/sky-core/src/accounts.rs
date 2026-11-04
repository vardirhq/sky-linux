//! Account manager: connecting and disconnecting cloud accounts.
//!
//! Tokens are owned by rclone (inside Sky Linux's private, encrypted
//! `rclone.conf`); rclone refreshes them transparently. Sky Core only stores
//! non-secret account metadata in SQLite.

use std::sync::Arc;

use chrono::Utc;
use uuid::Uuid;

use crate::db::Database;
use crate::error::{CoreError, Result};
use crate::events::{Event, EventBus};
use crate::models::Account;
use crate::provider::Provider;
use crate::rclone::{extract_auth_url, jottacloud_answerer, remote_fs, RcloneSupervisor};

pub const RCLONE_BACKEND: &str = "jottacloud";

#[derive(Clone)]
pub struct AccountManager {
    db: Database,
    bus: EventBus,
    rclone: Arc<RcloneSupervisor>,
    connecting: Arc<tokio::sync::Mutex<()>>,
}

impl AccountManager {
    pub fn new(db: Database, bus: EventBus, rclone: Arc<RcloneSupervisor>) -> Self {
        Self {
            db,
            bus,
            rclone,
            connecting: Default::default(),
        }
    }

    pub fn list(&self) -> Result<Vec<Account>> {
        self.db.accounts()
    }

    pub fn get(&self, id: Uuid) -> Result<Account> {
        self.db.account(id)
    }

    /// The account to use when the caller doesn't specify one.
    pub fn default_account(&self) -> Result<Account> {
        self.db
            .accounts()?
            .into_iter()
            .next()
            .ok_or_else(|| CoreError::rule("no_account", "No cloud account is connected yet."))
    }

    /// Connect a provider via browser sign-in. Blocks until the user has
    /// completed (or abandoned) sign-in; publishes [`Event::AuthUrl`] so the
    /// UI can offer the link if the browser didn't open.
    pub async fn connect(&self, provider: Provider) -> Result<Account> {
        let _guard = self.connecting.try_lock().map_err(|_| {
            CoreError::rule("sign_in_in_progress", "A sign-in is already in progress.")
        })?;
        if let Some(existing) = self.db.account_for_provider(provider)? {
            return Ok(existing);
        }
        let client = self.rclone.client();
        let remote = provider.remote_name();
        if client.list_remotes().await?.contains(&remote) {
            // Leftover from an abandoned sign-in.
            client.delete_remote(&remote).await?;
        }

        let mut logs = self.rclone.logs();
        let bus = self.bus.clone();
        let forward = tokio::spawn(async move {
            while let Ok(l) = logs.recv().await {
                if let Some(url) = extract_auth_url(&l.line) {
                    bus.publish(Event::AuthUrl { url });
                }
            }
        });
        let answerer = jottacloud_answerer(provider.rclone_whitelabel());
        let result = client
            .configure_remote(&remote, RCLONE_BACKEND, &answerer)
            .await;
        forward.abort();
        if let Err(e) = result {
            let _ = client.delete_remote(&remote).await;
            return Err(e);
        }

        // Prove the token works before declaring success.
        if let Err(e) = client.about(&remote_fs(&remote, "")).await {
            let _ = client.delete_remote(&remote).await;
            return Err(e);
        }

        let account = Account {
            id: Uuid::new_v4(),
            provider,
            remote,
            display_name: None,
            email: None,
            connected_at: Utc::now(),
        };
        self.db.insert_account(&account)?;
        let ev = Event::AccountConnected(account.clone());
        self.db.record_event(&ev)?;
        self.bus.publish(ev);
        Ok(account)
    }

    /// Register an already-configured rclone remote (advanced users, tests).
    pub async fn adopt_remote(&self, provider: Provider, remote: &str) -> Result<Account> {
        let client = self.rclone.client();
        if !client.list_remotes().await?.iter().any(|r| r == remote) {
            return Err(CoreError::NotFound(format!("rclone remote {remote}")));
        }
        client
            .about(&remote_fs(remote, ""))
            .await
            .or_else(|e| match e {
                // Some backends (e.g. local) don't support `about`.
                CoreError::Rclone { ref message, .. } if message.contains("doesn't support") => {
                    Ok(Default::default())
                }
                e => Err(e),
            })?;
        let account = Account {
            id: Uuid::new_v4(),
            provider,
            remote: remote.to_string(),
            display_name: None,
            email: None,
            connected_at: Utc::now(),
        };
        self.db.insert_account(&account)?;
        self.bus.publish(Event::AccountConnected(account.clone()));
        Ok(account)
    }

    /// Sign out: removes the token and Sky Linux's record of the account and
    /// its backup jobs. **No local or cloud files are deleted.**
    pub async fn disconnect(&self, id: Uuid) -> Result<()> {
        let account = self.db.account(id)?;
        let client = self.rclone.client();
        if client.list_remotes().await?.contains(&account.remote) {
            client.delete_remote(&account.remote).await?;
        }
        self.db.delete_account(id)?;
        let ev = Event::AccountRemoved { id };
        self.db.record_event(&ev)?;
        self.bus.publish(ev);
        Ok(())
    }
}
