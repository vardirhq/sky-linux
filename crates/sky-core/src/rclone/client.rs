//! Typed client for rclone's remote-control API over a unix socket.

use base64::Engine;
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::header::{AUTHORIZATION, CONTENT_TYPE, HOST};
use hyper::Request;
use hyper_util::rt::TokioIo;
use rand::distr::{Alphanumeric, SampleString};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::net::UnixStream;

use super::types::*;
use crate::error::{CoreError, Result};
use crate::models::{CloudEntry, StorageUsage};

/// Per-launch random credentials for the RC server.
#[derive(Clone)]
pub struct RcCredentials {
    pub user: String,
    pub pass: String,
}

impl RcCredentials {
    pub fn random() -> Self {
        let mut rng = rand::rng();
        Self {
            user: format!("sky-{}", Alphanumeric.sample_string(&mut rng, 8)),
            pass: Alphanumeric.sample_string(&mut rng, 40),
        }
    }

    fn header(&self) -> String {
        let raw = format!("{}:{}", self.user, self.pass);
        format!(
            "Basic {}",
            base64::engine::general_purpose::STANDARD.encode(raw)
        )
    }
}

impl std::fmt::Debug for RcCredentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RcCredentials")
            .field("user", &self.user)
            .field("pass", &"<redacted>")
            .finish()
    }
}

#[derive(Debug, Clone)]
pub struct RcloneClient {
    socket: PathBuf,
    auth: String,
    /// Timeout for short, synchronous calls. Long operations use `_async`.
    timeout: Duration,
}

impl RcloneClient {
    pub fn new(socket: impl Into<PathBuf>, creds: &RcCredentials) -> Self {
        Self {
            socket: socket.into(),
            auth: creds.header(),
            timeout: Duration::from_secs(120),
        }
    }

    pub fn socket(&self) -> &Path {
        &self.socket
    }

    /// Call an RC method and return the raw JSON result.
    pub async fn call(&self, method: &str, params: Value) -> Result<Value> {
        self.call_with_timeout(method, params, Some(self.timeout))
            .await
    }

    /// Call an RC method that may block for a long time (e.g. interactive
    /// OAuth during `config/update`).
    pub async fn call_unbounded(&self, method: &str, params: Value) -> Result<Value> {
        self.call_with_timeout(method, params, None).await
    }

    async fn call_with_timeout(
        &self,
        method: &str,
        params: Value,
        timeout: Option<Duration>,
    ) -> Result<Value> {
        let fut = self.post(method, params);
        let (status, body) = match timeout {
            Some(t) => tokio::time::timeout(t, fut).await.map_err(|_| {
                CoreError::RcloneUnavailable(format!("{method} timed out after {t:?}"))
            })??,
            None => fut.await?,
        };
        if status == 200 {
            return Ok(serde_json::from_slice(&body)?);
        }
        let message = serde_json::from_slice::<RcErrorBody>(&body)
            .map(|b| b.error)
            .unwrap_or_else(|_| String::from_utf8_lossy(&body).trim().to_string());
        if status == 404 && message.contains("object not found")
            || message.contains("directory not found")
        {
            return Err(CoreError::NotFound(message));
        }
        Err(CoreError::Rclone {
            path: method.to_string(),
            status,
            message,
        })
    }

    async fn post(&self, method: &str, params: Value) -> Result<(u16, Bytes)> {
        let unavailable = |e: &dyn std::fmt::Display| CoreError::RcloneUnavailable(e.to_string());
        let stream = UnixStream::connect(&self.socket)
            .await
            .map_err(|e| unavailable(&e))?;
        let (mut sender, conn) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
            .await
            .map_err(|e| unavailable(&e))?;
        tokio::spawn(async move {
            let _ = conn.await;
        });
        let body = serde_json::to_vec(&params)?;
        let req = Request::post(format!("/{}", method.trim_start_matches('/')))
            .header(HOST, "localhost")
            .header(CONTENT_TYPE, "application/json")
            .header(AUTHORIZATION, &self.auth)
            .body(Full::new(Bytes::from(body)))
            .map_err(|e| CoreError::Invalid(e.to_string()))?;
        let res = sender
            .send_request(req)
            .await
            .map_err(|e| unavailable(&e))?;
        let status = res.status().as_u16();
        let bytes = res
            .into_body()
            .collect()
            .await
            .map_err(|e| unavailable(&e))?
            .to_bytes();
        Ok((status, bytes))
    }

    async fn typed<T: DeserializeOwned>(&self, method: &str, params: Value) -> Result<T> {
        Ok(serde_json::from_value(self.call(method, params).await?)?)
    }

    // ---- core -----------------------------------------------------------

    pub async fn version(&self) -> Result<VersionInfo> {
        self.typed("core/version", json!({})).await
    }

    pub async fn stats(&self, group: Option<&str>) -> Result<CoreStats> {
        let params = match group {
            Some(g) => json!({ "group": g }),
            None => json!({}),
        };
        self.typed("core/stats", params).await
    }

    pub async fn reset_stats(&self, group: &str) -> Result<()> {
        self.call("core/stats-delete", json!({ "group": group }))
            .await
            .map(drop)
    }

    /// Limit rclone's total bandwidth. `rate` is rclone syntax ("5M") or "off".
    pub async fn set_bwlimit(&self, rate: &str) -> Result<()> {
        self.call("core/bwlimit", json!({ "rate": rate }))
            .await
            .map(drop)
    }

    pub async fn quit(&self) -> Result<()> {
        self.call("core/quit", json!({})).await.map(drop)
    }

    // ---- jobs -----------------------------------------------------------

    pub async fn job_status(&self, jobid: i64) -> Result<JobStatus> {
        self.typed("job/status", json!({ "jobid": jobid })).await
    }

    pub async fn job_stop(&self, jobid: i64) -> Result<()> {
        self.call("job/stop", json!({ "jobid": jobid }))
            .await
            .map(drop)
    }

    // ---- browsing -------------------------------------------------------

    pub async fn list(&self, fs: &str, path: &str) -> Result<Vec<CloudEntry>> {
        let r: ListResponse = self
            .typed(
                "operations/list",
                json!({ "fs": fs, "remote": path, "opt": { "noModTime": false } }),
            )
            .await?;
        Ok(r.list.into_iter().map(Into::into).collect())
    }

    /// Recursive listing with a name filter (used for cloud search).
    pub async fn search(&self, fs: &str, path: &str, query: &str) -> Result<Vec<CloudEntry>> {
        let pattern = format!("*{}*", escape_glob(query));
        let r: ListResponse = self
            .typed(
                "operations/list",
                json!({
                    "fs": fs, "remote": path,
                    "opt": { "recurse": true },
                    "_filter": { "IncludeRule": [format!("**/{pattern}"), pattern], "IgnoreCase": true }
                }),
            )
            .await?;
        // rclone filters apply to files only; directories are always listed.
        let needle = query.to_lowercase();
        Ok(r.list
            .into_iter()
            .filter(|i| i.name.to_lowercase().contains(&needle))
            .map(Into::into)
            .collect())
    }

    pub async fn about(&self, fs: &str) -> Result<StorageUsage> {
        let r: AboutResponse = self.typed("operations/about", json!({ "fs": fs })).await?;
        Ok(r.into())
    }

    // ---- file management ------------------------------------------------

    pub async fn mkdir(&self, fs: &str, path: &str) -> Result<()> {
        self.call("operations/mkdir", json!({ "fs": fs, "remote": path }))
            .await
            .map(drop)
    }

    /// Delete a single file. Remote deletion on Jottacloud moves the file to
    /// the trash unless `hard_delete` is configured (it is not, by default).
    pub async fn delete_file(&self, fs: &str, path: &str) -> Result<()> {
        self.call("operations/deletefile", json!({ "fs": fs, "remote": path }))
            .await
            .map(drop)
    }

    /// Move or rename a single file.
    pub async fn move_file(&self, src_fs: &str, src: &str, dst_fs: &str, dst: &str) -> Result<()> {
        self.call(
            "operations/movefile",
            json!({ "srcFs": src_fs, "srcRemote": src, "dstFs": dst_fs, "dstRemote": dst }),
        )
        .await
        .map(drop)
    }

    /// Move or rename a directory (async job).
    pub async fn move_dir(&self, src_fs: &str, dst_fs: &str, group: &str) -> Result<i64> {
        let r: JobStarted = self
            .typed(
                "sync/move",
                json!({ "srcFs": src_fs, "dstFs": dst_fs, "deleteEmptySrcDirs": true,
                         "_async": true, "_group": group }),
            )
            .await?;
        Ok(r.jobid)
    }

    /// Copy a single file (async job).
    pub async fn copy_file_async(
        &self,
        src_fs: &str,
        src: &str,
        dst_fs: &str,
        dst: &str,
        group: &str,
    ) -> Result<i64> {
        let r: JobStarted = self
            .typed(
                "operations/copyfile",
                json!({ "srcFs": src_fs, "srcRemote": src, "dstFs": dst_fs, "dstRemote": dst,
                         "_async": true, "_group": group }),
            )
            .await?;
        Ok(r.jobid)
    }

    /// Copy a directory tree (async job). **Never deletes** anything at the
    /// destination: this is `rclone copy`, not `rclone sync`.
    pub async fn copy_dir_async(
        &self,
        src_fs: &str,
        dst_fs: &str,
        group: &str,
        filter: &FilterOpt,
    ) -> Result<i64> {
        let r: JobStarted = self
            .typed(
                "sync/copy",
                json!({ "srcFs": src_fs, "dstFs": dst_fs, "createEmptySrcDirs": true,
                         "_async": true, "_group": group, "_filter": filter }),
            )
            .await?;
        Ok(r.jobid)
    }

    /// Two-way sync (async job). See `crate::sync` for the safety model.
    pub async fn bisync_async(&self, opts: &BisyncOpts, group: &str) -> Result<i64> {
        let mut params = serde_json::to_value(opts)?;
        params["_async"] = Value::Bool(true);
        params["_group"] = Value::String(group.to_string());
        let r: JobStarted = self.typed("sync/bisync", params).await?;
        Ok(r.jobid)
    }

    /// Copy one file synchronously (small files such as sync markers).
    pub async fn copy_file(&self, src_fs: &str, src: &str, dst_fs: &str, dst: &str) -> Result<()> {
        self.call(
            "operations/copyfile",
            json!({ "srcFs": src_fs, "srcRemote": src, "dstFs": dst_fs, "dstRemote": dst }),
        )
        .await
        .map(drop)
    }

    // ---- remotes / configuration ---------------------------------------

    pub async fn list_remotes(&self) -> Result<Vec<String>> {
        let v = self.call("config/listremotes", json!({})).await?;
        Ok(v.get("remotes")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|s| s.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default())
    }

    pub async fn delete_remote(&self, name: &str) -> Result<()> {
        self.call("config/delete", json!({ "name": name }))
            .await
            .map(drop)
    }

    /// Create a remote from known parameters, without interaction.
    pub async fn create_remote(&self, name: &str, kind: &str, params: Value) -> Result<()> {
        self.call(
            "config/create",
            json!({ "name": name, "type": kind, "parameters": params,
                     "opt": { "nonInteractive": true, "obscure": true } }),
        )
        .await
        .map(drop)
    }
}

/// Escape rclone glob metacharacters in user input.
fn escape_glob(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(c, '*' | '?' | '[' | ']' | '{' | '}' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// rclone fs string for a remote and path, e.g. `sky-telia-no:Photos`.
pub fn remote_fs(remote: &str, path: &str) -> String {
    format!("{}:{}", remote, path.trim_start_matches('/'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credentials_are_random_and_redacted() {
        let a = RcCredentials::random();
        let b = RcCredentials::random();
        assert_ne!(a.pass, b.pass);
        assert!(!format!("{a:?}").contains(&a.pass));
    }

    #[test]
    fn escapes_glob() {
        assert_eq!(escape_glob("a*b[1]"), "a\\*b\\[1\\]");
    }

    #[test]
    fn builds_fs_strings() {
        assert_eq!(remote_fs("sky", "/Photos/2025"), "sky:Photos/2025");
        assert_eq!(remote_fs("sky", ""), "sky:");
    }
}
