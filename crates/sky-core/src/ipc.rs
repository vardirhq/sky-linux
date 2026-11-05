//! Local IPC between user interfaces and the Sky daemon.
//!
//! Transport: a unix domain socket at `$XDG_RUNTIME_DIR/sky-linux/daemon.sock`
//! inside a 0700 directory; the socket itself is 0600 and every connection's
//! peer UID is checked with `SO_PEERCRED`. Framing: one JSON object per line.
//!
//! ```text
//! → {"id":1,"method":"files.list","params":{"path":"Photos"}}
//! ← {"id":1,"result":[...]}
//! ← {"id":2,"error":{"code":"offline","title":"…","message":"…",...}}
//! ← {"event":{"type":"transferUpdated","data":{...}}}      (unsolicited)
//! ```

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{broadcast, mpsc, oneshot, Mutex};
use uuid::Uuid;

use crate::error::{CoreError, Result, UserError};
use crate::events::Event;
use crate::models::*;
use crate::provider::Provider;
use crate::service::SkyCore;

pub const PROTOCOL_VERSION: u32 = 1;

/// Every operation a UI can ask the daemon to perform.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method", content = "params")]
pub enum Request {
    #[serde(rename = "daemon.status")]
    Status,
    #[serde(rename = "accounts.list")]
    Accounts,
    #[serde(rename = "accounts.providers")]
    Providers,
    #[serde(rename = "accounts.connect")]
    Connect { provider: Provider },
    #[serde(rename = "accounts.disconnect")]
    Disconnect { id: Uuid },
    #[serde(rename = "files.list", rename_all = "camelCase")]
    List {
        #[serde(default)]
        account: Option<Uuid>,
        #[serde(default)]
        path: String,
        #[serde(default)]
        refresh: bool,
    },
    #[serde(rename = "files.search")]
    Search {
        #[serde(default)]
        account: Option<Uuid>,
        query: String,
    },
    #[serde(rename = "files.usage")]
    Usage {
        #[serde(default)]
        account: Option<Uuid>,
    },
    #[serde(rename = "files.mkdir")]
    Mkdir {
        #[serde(default)]
        account: Option<Uuid>,
        path: String,
    },
    #[serde(rename = "files.move", rename_all = "camelCase")]
    Move {
        #[serde(default)]
        account: Option<Uuid>,
        from: String,
        to: String,
        #[serde(default)]
        is_dir: bool,
    },
    #[serde(rename = "files.delete")]
    Delete {
        #[serde(default)]
        account: Option<Uuid>,
        path: String,
    },
    #[serde(rename = "transfers.upload", rename_all = "camelCase")]
    Upload {
        #[serde(default)]
        account: Option<Uuid>,
        local_paths: Vec<PathBuf>,
        remote_dir: String,
    },
    #[serde(rename = "transfers.download", rename_all = "camelCase")]
    Download {
        #[serde(default)]
        account: Option<Uuid>,
        entries: Vec<CloudEntry>,
        local_dir: PathBuf,
    },
    #[serde(rename = "transfers.list")]
    Transfers {
        #[serde(default = "default_limit")]
        limit: u32,
    },
    #[serde(rename = "transfers.cancel")]
    CancelTransfer { id: Uuid },
    #[serde(rename = "backups.list")]
    BackupJobs,
    #[serde(rename = "backups.create")]
    CreateBackup(NewBackupJob),
    #[serde(rename = "backups.setEnabled")]
    SetBackupEnabled { id: Uuid, enabled: bool },
    #[serde(rename = "backups.delete")]
    DeleteBackup { id: Uuid },
    #[serde(rename = "backups.runNow")]
    RunBackup { id: Uuid },
    #[serde(rename = "sync.list")]
    SyncPairs,
    #[serde(rename = "sync.create")]
    CreateSync(NewSyncPair),
    #[serde(rename = "sync.setEnabled")]
    SetSyncEnabled { id: Uuid, enabled: bool },
    #[serde(rename = "sync.delete")]
    DeleteSync { id: Uuid },
    #[serde(rename = "sync.runNow")]
    RunSync { id: Uuid },
    #[serde(rename = "sync.resolve")]
    ResolveSync {
        id: Uuid,
        action: crate::sync::ReviewAction,
    },
    #[serde(rename = "history.list")]
    History {
        #[serde(default = "default_limit")]
        limit: u32,
    },
    #[serde(rename = "daemon.diagnostics")]
    Diagnostics,
    #[serde(rename = "settings.get")]
    GetSetting { key: String },
    #[serde(rename = "settings.set")]
    SetSetting { key: String, value: Value },
}

fn default_limit() -> u32 {
    100
}

#[derive(Debug, Deserialize)]
struct Envelope {
    id: u64,
    #[serde(flatten)]
    request: Value,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ServerMessage {
    Response {
        id: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        result: Option<Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<UserError>,
    },
    Event {
        event: Event,
    },
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProviderInfo {
    id: Provider,
    name: &'static str,
    supported: bool,
}

/// Keys the UI may read/write through `settings.*`.
const UI_SETTINGS: &[&str] = &[
    "onboardingComplete",
    "language",
    "startOnLogin",
    "notifications",
    "theme",
    "bandwidthLimit",
    "downloadDir",
];

/// Execute one request against the core.
pub async fn dispatch(core: &SkyCore, req: Request) -> Result<Value> {
    use serde_json::to_value as v;
    Ok(match req {
        Request::Status => v(core.status().await?)?,
        Request::Accounts => v(core.accounts.list()?)?,
        Request::Providers => v(Provider::ALL
            .iter()
            .map(|p| ProviderInfo {
                id: *p,
                name: p.display_name(),
                supported: p.is_supported(),
            })
            .collect::<Vec<_>>())?,
        Request::Connect { provider } => v(core.accounts.connect(provider).await?)?,
        Request::Disconnect { id } => v(core.accounts.disconnect(id).await?)?,
        Request::List {
            account,
            path,
            refresh,
        } => v(core.list(account, &path, refresh).await?)?,
        Request::Search { account, query } => v(core.search(account, &query).await?)?,
        Request::Usage { account } => v(core.usage(account).await?)?,
        Request::Mkdir { account, path } => v(core.mkdir(account, &path).await?)?,
        Request::Move {
            account,
            from,
            to,
            is_dir,
        } => v(core.move_entry(account, &from, &to, is_dir).await?)?,
        Request::Delete { account, path } => v(core.delete_file(account, &path).await?)?,
        Request::Upload {
            account,
            local_paths,
            remote_dir,
        } => v(core.upload(account, local_paths, &remote_dir).await?)?,
        Request::Download {
            account,
            entries,
            local_dir,
        } => v(core.download(account, entries, local_dir).await?)?,
        Request::Transfers { limit } => v(core.transfers(limit.min(1000)).await?)?,
        Request::CancelTransfer { id } => v(core.transfers.cancel(id).await?)?,
        Request::BackupJobs => v(core.backups.list()?)?,
        Request::CreateBackup(new) => v(core.backups.create(new).await?)?,
        Request::SetBackupEnabled { id, enabled } => {
            v(core.backups.set_enabled(id, enabled).await?)?
        }
        Request::DeleteBackup { id } => v(core.backups.delete(id).await?)?,
        Request::RunBackup { id } => v(core.backups.run_now(id).await?)?,
        Request::SyncPairs => v(core.sync.list()?)?,
        Request::CreateSync(new) => v(core.sync.create(new).await?)?,
        Request::SetSyncEnabled { id, enabled } => v(core.sync.set_enabled(id, enabled).await?)?,
        Request::DeleteSync { id } => v(core.sync.delete(id).await?)?,
        Request::RunSync { id } => v(core.sync.run_now(id).await?)?,
        Request::ResolveSync { id, action } => v(core.sync.resolve(id, action).await?)?,
        Request::History { limit } => v(core.history(limit.min(1000))?)?,
        Request::Diagnostics => v(core.diagnostics().await?)?,
        Request::GetSetting { key } => {
            check_setting(&key)?;
            core.db.get_setting::<Value>(&key)?.unwrap_or(Value::Null)
        }
        Request::SetSetting { key, value } => {
            check_setting(&key)?;
            if key == crate::service::BANDWIDTH_SETTING {
                let limit =
                    match &value {
                        Value::Null => None,
                        v => Some(v.as_u64().and_then(|n| u32::try_from(n).ok()).ok_or_else(
                            || CoreError::Invalid("bandwidth limit must be a whole number".into()),
                        )?),
                    };
                core.set_bandwidth_limit(limit).await?;
            } else {
                core.db.set_setting(&key, &value)?;
            }
            Value::Null
        }
    })
}

fn check_setting(key: &str) -> Result<()> {
    if UI_SETTINGS.contains(&key) {
        Ok(())
    } else {
        Err(CoreError::Invalid(format!("unknown setting {key}")))
    }
}

// ---- server -----------------------------------------------------------------

/// Bind the daemon socket with owner-only permissions.
pub fn bind(path: &Path) -> Result<UnixListener> {
    use std::os::unix::fs::PermissionsExt;
    if path.exists() {
        // Refuse to steal the socket from a live daemon.
        if std::os::unix::net::UnixStream::connect(path).is_ok() {
            return Err(CoreError::Ipc(
                "another Sky daemon is already running".into(),
            ));
        }
        std::fs::remove_file(path)?;
    }
    let listener = UnixListener::bind(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(listener)
}

/// Accept connections until the listener fails.
pub async fn serve(listener: UnixListener, core: Arc<SkyCore>) -> Result<()> {
    // SAFETY: getuid(2) cannot fail.
    let my_uid = unsafe { libc::getuid() };
    loop {
        let (stream, _) = listener.accept().await?;
        match stream.peer_cred() {
            Ok(cred) if cred.uid() == my_uid => {}
            other => {
                tracing::warn!("rejecting IPC connection from {other:?}");
                continue;
            }
        }
        let core = core.clone();
        tokio::spawn(async move {
            if let Err(e) = handle(stream, core).await {
                tracing::debug!("ipc connection closed: {e}");
            }
        });
    }
}

async fn handle(stream: UnixStream, core: Arc<SkyCore>) -> Result<()> {
    let (read, mut write) = stream.into_split();
    let (tx, mut rx) = mpsc::channel::<String>(256);

    // Writer task: responses and events share one ordered stream.
    let writer = tokio::spawn(async move {
        while let Some(line) = rx.recv().await {
            if write.write_all(line.as_bytes()).await.is_err()
                || write.write_all(b"\n").await.is_err()
            {
                break;
            }
        }
    });

    let mut events = core.bus.subscribe();
    let etx = tx.clone();
    let forward = tokio::spawn(async move {
        loop {
            match events.recv().await {
                Ok(event) => {
                    let msg = serde_json::to_string(&ServerMessage::Event { event }).unwrap();
                    if etx.send(msg).await.is_err() {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => break,
            }
        }
    });

    let mut lines = BufReader::new(read).lines();
    while let Some(line) = lines.next_line().await? {
        if line.len() > 1 << 20 {
            break; // absurd request
        }
        let env: Envelope = match serde_json::from_str(&line) {
            Ok(e) => e,
            Err(_) => continue,
        };
        let core = core.clone();
        let tx = tx.clone();
        // Requests run concurrently: a long sign-in must not block listing.
        tokio::spawn(async move {
            let result = match serde_json::from_value::<Request>(env.request) {
                Ok(req) => dispatch(&core, req).await,
                Err(e) => Err(CoreError::Invalid(format!("bad request: {e}"))),
            };
            let msg = match result {
                Ok(v) => ServerMessage::Response {
                    id: env.id,
                    result: Some(v),
                    error: None,
                },
                Err(e) => ServerMessage::Response {
                    id: env.id,
                    result: None,
                    error: Some(e.to_user()),
                },
            };
            let _ = tx.send(serde_json::to_string(&msg).unwrap()).await;
        });
    }
    forward.abort();
    drop(tx);
    let _ = writer.await;
    Ok(())
}

// ---- client -----------------------------------------------------------------

/// Result of a daemon request. The error is boxed: `UserError` is large and
/// the common path is success.
pub type IpcResult = std::result::Result<Value, Box<UserError>>;

type Pending = Arc<Mutex<std::collections::HashMap<u64, oneshot::Sender<IpcResult>>>>;

/// Client used by the desktop app (and tests) to talk to the daemon.
#[derive(Clone)]
pub struct IpcClient {
    next: Arc<AtomicU64>,
    pending: Pending,
    out: mpsc::Sender<String>,
    events: broadcast::Sender<Event>,
}

impl IpcClient {
    pub async fn connect(path: &Path) -> Result<Self> {
        let stream = UnixStream::connect(path)
            .await
            .map_err(|e| CoreError::Ipc(format!("cannot connect to daemon: {e}")))?;
        let (read, mut write) = stream.into_split();
        let (out, mut out_rx) = mpsc::channel::<String>(256);
        let pending: Pending = Default::default();
        let (events, _) = broadcast::channel(1024);

        tokio::spawn(async move {
            while let Some(line) = out_rx.recv().await {
                if write.write_all(line.as_bytes()).await.is_err()
                    || write.write_all(b"\n").await.is_err()
                {
                    break;
                }
            }
        });
        let p = pending.clone();
        let ev = events.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(read).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                match serde_json::from_str::<ServerMessage>(&line) {
                    Ok(ServerMessage::Event { event }) => {
                        let _ = ev.send(event);
                    }
                    Ok(ServerMessage::Response { id, result, error }) => {
                        if let Some(tx) = p.lock().await.remove(&id) {
                            let _ = tx.send(match error {
                                Some(e) => Err(Box::new(e)),
                                None => Ok(result.unwrap_or(Value::Null)),
                            });
                        }
                    }
                    Err(e) => tracing::warn!("bad message from daemon: {e}"),
                }
            }
            // Connection lost: fail everything pending.
            p.lock().await.clear();
        });
        Ok(Self {
            next: Arc::new(AtomicU64::new(1)),
            pending,
            out,
            events,
        })
    }

    pub fn events(&self) -> broadcast::Receiver<Event> {
        self.events.subscribe()
    }

    /// Send a request; errors come back already translated for users.
    pub async fn request(&self, req: &Request) -> IpcResult {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let mut msg =
            serde_json::to_value(req).map_err(|e| Box::new(CoreError::from(e).to_user()))?;
        msg["id"] = Value::from(id);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(id, tx);
        let lost =
            || Box::new(CoreError::Ipc("connection to the daemon was lost".into()).to_user());
        self.out.send(msg.to_string()).await.map_err(|_| lost())?;
        rx.await.map_err(|_| lost())?
    }

    /// Send a raw `{method, params}` request (used by the Tauri bridge).
    pub async fn request_raw(&self, method: &str, params: Value) -> IpcResult {
        let req: Request =
            serde_json::from_value(serde_json::json!({ "method": method, "params": params }))
                .or_else(|_| serde_json::from_value(serde_json::json!({ "method": method })))
                .map_err(|e| {
                    Box::new(CoreError::Invalid(format!("unknown request {method}: {e}")).to_user())
                })?;
        self.request(&req).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_wire_format() {
        let r: Request =
            serde_json::from_str(r#"{"method":"files.list","params":{"path":"Photos"}}"#).unwrap();
        assert!(
            matches!(r, Request::List { ref path, refresh: false, account: None } if path == "Photos")
        );
        let r: Request = serde_json::from_str(r#"{"method":"daemon.status"}"#).unwrap();
        assert!(matches!(r, Request::Status));
        let json = serde_json::to_value(Request::Connect {
            provider: Provider::TeliaSkyNo,
        })
        .unwrap();
        assert_eq!(json["method"], "accounts.connect");
        assert_eq!(json["params"]["provider"], "telia_sky_no");
    }

    #[test]
    fn server_messages_are_distinguishable() {
        let resp = r#"{"id":3,"result":[1,2]}"#;
        assert!(matches!(
            serde_json::from_str::<ServerMessage>(resp).unwrap(),
            ServerMessage::Response { id: 3, .. }
        ));
        let ev = r#"{"event":{"type":"rcloneStatus","data":{"running":true}}}"#;
        assert!(matches!(
            serde_json::from_str::<ServerMessage>(ev).unwrap(),
            ServerMessage::Event { .. }
        ));
    }

    #[test]
    fn settings_are_allowlisted() {
        assert!(check_setting("theme").is_ok());
        assert!(check_setting("rclone-config-password").is_err());
    }
}
