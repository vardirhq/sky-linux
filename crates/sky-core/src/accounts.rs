//! Account manager: connecting and disconnecting cloud accounts.
//!
//! Sign-in runs in Sky Core (`crate::oauth`) so the browser lands on Sky
//! Linux's own page. Tokens are then owned by rclone (inside Sky Linux's
//! private, encrypted `rclone.conf`); rclone refreshes them transparently. Sky Core only stores
//! non-secret account metadata in SQLite.

use std::sync::Arc;

use chrono::Utc;
use uuid::Uuid;

use crate::areas::{combine_upstreams, AreaLayout, JOTTA_AREAS};
use crate::db::Database;
use crate::error::{CoreError, Result};
use crate::events::{Event, EventBus};
use crate::models::Account;
use crate::oauth::{OAuthService, PageLanguage, SignIn, REDIRECT_PORT};
use crate::provider::Provider;
use crate::rclone::{remote_fs, JottacloudWithToken, RcloneSupervisor};
use serde_json::json;

pub const RCLONE_BACKEND: &str = "jottacloud";
const COMBINED_SUFFIX: &str = "-all";

fn combined_name(base_remote: &str) -> String {
    format!("{base_remote}{COMBINED_SUFFIX}")
}

/// The signed-in remote an account's combined remote is built on, or the
/// account's own remote when it has no areas.
pub fn base_remote(account: &Account) -> &str {
    if account.areas.is_empty() {
        return &account.remote;
    }
    account
        .remote
        .strip_suffix(COMBINED_SUFFIX)
        .unwrap_or(&account.remote)
}

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
    /// completed (or abandoned) sign-in. Publishes [`Event::AuthUrl`]; the
    /// desktop app opens it in the browser.
    pub async fn connect(&self, provider: Provider) -> Result<Account> {
        self.connect_with(
            provider,
            OAuthService::for_provider(provider),
            REDIRECT_PORT,
        )
        .await
    }

    /// [`Self::connect`] against a given identity server (tests use a local one).
    pub async fn connect_with(
        &self,
        provider: Provider,
        service: OAuthService,
        port: u16,
    ) -> Result<Account> {
        let _guard = self.connecting.try_lock().map_err(|_| {
            CoreError::rule("sign_in_in_progress", "A sign-in is already in progress.")
        })?;
        if let Some(existing) = self.db.account_for_provider(provider)? {
            return Ok(existing);
        }

        // 1. Browser sign-in, ending on Sky Linux's own page.
        let sign_in = SignIn::start(service, port).await?;
        self.bus.publish(Event::AuthUrl {
            url: sign_in.auth_url.clone(),
        });
        let lang = PageLanguage::from_setting(
            self.db
                .get_setting::<String>("language")
                .ok()
                .flatten()
                .as_deref(),
        );
        let token = sign_in.finish(lang, provider.display_name()).await?;

        // 2. Hand the token to rclone, which owns and refreshes it.
        let base = provider.remote_name();
        self.create_remote_from_token(provider, &base, &token)
            .await?;

        // 3. Prove the token works before declaring success.
        let client = self.rclone.client();
        if let Err(e) = client.about(&remote_fs(&base, "")).await {
            let _ = client.delete_remote(&base).await;
            return Err(e);
        }

        // 4. One tree over all storage areas (Archive, Photos, …).
        let areas: Vec<String> = JOTTA_AREAS.iter().map(|a| a.to_string()).collect();
        self.register_areas(provider, &base, areas, AreaLayout::Mountpoints)
            .await
    }

    /// Join the storage areas of `base_remote` into one `combine` remote and
    /// record the account. Also used by tests and the daemon's developer
    /// mode with [`AreaLayout::Subfolders`] on a local folder.
    pub async fn register_areas(
        &self,
        provider: Provider,
        base_remote: &str,
        areas: Vec<String>,
        layout: AreaLayout,
    ) -> Result<Account> {
        let client = self.rclone.client();
        let combined = combined_name(base_remote);
        if client.list_remotes().await?.contains(&combined) {
            client.delete_remote(&combined).await?;
        }
        client
            .create_remote(
                &combined,
                "combine",
                json!({ "upstreams": combine_upstreams(base_remote, &areas, layout) }),
            )
            .await?;
        let account = Account {
            id: Uuid::new_v4(),
            provider,
            remote: combined,
            areas,
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

    /// Rewrite combined remotes created before areas had their own rclone
    /// cache entry (see [`crate::areas`]). Returns how many were repaired.
    pub async fn repair_areas(&self) -> Result<usize> {
        let client = self.rclone.client();
        let mut repaired = 0;
        for account in self.list()? {
            if account.areas.is_empty() {
                continue;
            }
            let cfg = client
                .call("config/get", json!({ "name": account.remote }))
                .await?;
            let Some(upstreams) = cfg.get("upstreams").and_then(|u| u.as_str()) else {
                continue;
            };
            if !crate::areas::upstreams_need_repair(upstreams) {
                continue;
            }
            client
                .call(
                    "config/update",
                    json!({
                        "name": account.remote,
                        "parameters": {
                            "upstreams": combine_upstreams(
                                base_remote(&account),
                                &account.areas,
                                AreaLayout::Mountpoints,
                            )
                        },
                        "opt": { "nonInteractive": true },
                    }),
                )
                .await?;
            tracing::warn!(remote = %account.remote, "repaired storage areas");
            repaired += 1;
        }
        Ok(repaired)
    }

    /// Create the rclone remote for `provider` from a token obtained by
    /// [`SignIn`], without rclone opening any browser page itself.
    pub async fn create_remote_from_token(
        &self,
        provider: Provider,
        remote: &str,
        token: &str,
    ) -> Result<()> {
        let client = self.rclone.client();
        if client.list_remotes().await?.iter().any(|r| r == remote) {
            // Leftover from an abandoned sign-in.
            client.delete_remote(remote).await?;
        }
        let answerer = JottacloudWithToken {
            whitelabel: provider.rclone_whitelabel(),
        };
        let result = client
            .configure_remote_with(remote, RCLONE_BACKEND, json!({ "token": token }), &answerer)
            .await;
        if let Err(e) = result {
            let _ = client.delete_remote(remote).await;
            return Err(e);
        }
        Ok(())
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
            areas: vec![],
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
        let remotes = client.list_remotes().await?;
        // The combined remote and the signed-in remote it is built on.
        let base = (!account.areas.is_empty()).then(|| base_remote(&account).to_owned());
        for r in std::iter::once(account.remote.clone()).chain(base) {
            if remotes.contains(&r) {
                client.delete_remote(&r).await?;
            }
        }
        self.db.delete_account(id)?;
        let ev = Event::AccountRemoved { id };
        self.db.record_event(&ev)?;
        self.bus.publish(ev);
        Ok(())
    }
}
