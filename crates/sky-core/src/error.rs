//! Error types and the translation of technical failures into clear,
//! actionable messages for ordinary users.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub type Result<T, E = CoreError> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("rclone returned an error ({status}) for {path}: {message}")]
    Rclone {
        path: String,
        status: u16,
        message: String,
    },
    #[error("could not reach the rclone process: {0}")]
    RcloneUnavailable(String),
    #[error("rclone process failed: {0}")]
    Process(String),
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("secret storage error: {0}")]
    Secrets(String),
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    #[error("serialisation error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("invalid input: {0}")]
    Invalid(String),
    #[error("refused for safety: {0}")]
    Safety(String),
    #[error("ipc error: {0}")]
    Ipc(String),
    /// A rule violation with a stable `rule` code and parameters, so UIs can
    /// show a translated message. `message` is the English fallback.
    #[error("{message}")]
    Rule {
        rule: &'static str,
        safety: bool,
        message: String,
        params: BTreeMap<String, String>,
    },
}

impl CoreError {
    /// An input problem the user can fix (e.g. "folder doesn't exist").
    pub fn rule(rule: &'static str, message: impl Into<String>) -> Self {
        CoreError::Rule {
            rule,
            safety: false,
            message: message.into(),
            params: BTreeMap::new(),
        }
    }

    /// Refused because it could endanger the user's files.
    pub fn safety(rule: &'static str, message: impl Into<String>) -> Self {
        CoreError::Rule {
            rule,
            safety: true,
            message: message.into(),
            params: BTreeMap::new(),
        }
    }

    /// Attach a parameter used by translated messages.
    pub fn with(mut self, key: &str, value: impl std::fmt::Display) -> Self {
        if let CoreError::Rule { params, .. } = &mut self {
            params.insert(key.to_string(), value.to_string());
        }
        self
    }
}

/// What the UI should suggest to the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UserAction {
    Retry,
    SignInAgain,
    CheckConnection,
    FreeUpSpace,
    CheckPermissions,
    ContactSupport,
    None,
}

/// A user-presentable error. `technical` is shown behind a "Details" toggle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserError {
    pub code: String,
    pub title: String,
    pub message: String,
    pub action: UserAction,
    pub technical: String,
    /// Stable code of the violated rule (with `invalid` / `safety`), used by
    /// UIs to show a translated message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<String, String>,
}

impl UserError {
    fn new(code: &str, title: &str, message: &str, action: UserAction, technical: String) -> Self {
        Self {
            code: code.into(),
            title: title.into(),
            message: message.into(),
            action,
            technical,
            rule: None,
            params: BTreeMap::new(),
        }
    }
}

impl CoreError {
    /// Translate this error into something an ordinary person can act on.
    pub fn to_user(&self) -> UserError {
        let technical = self.to_string();
        if let CoreError::Rule {
            rule,
            safety,
            message,
            params,
        } = self
        {
            let (code, title) = if *safety {
                ("safety", "Stopped to protect your files")
            } else {
                ("invalid", "Something isn't right")
            };
            return UserError {
                rule: Some(rule.to_string()),
                params: params.clone(),
                ..UserError::new(code, title, message, UserAction::None, technical)
            };
        }
        let lower = technical.to_lowercase();
        let has = |needles: &[&str]| needles.iter().any(|n| lower.contains(n));

        if has(&[
            "invalid_grant",
            "token expired",
            "couldn't fetch token",
            "unauthorized",
            "401",
            "oauth2: cannot fetch token",
        ]) {
            return UserError::new(
                "auth_expired",
                "Please sign in again",
                "Your Telia Sky session has expired or was revoked. Sign in again to resume syncing — your files are safe.",
                UserAction::SignInAgain,
                technical,
            );
        }
        if has(&["quota", "insufficient storage", "no space left", "507"]) {
            return UserError::new(
                "storage_full",
                "Not enough space",
                "There isn't enough free space to finish this transfer. Free up space or upgrade your storage plan.",
                UserAction::FreeUpSpace,
                technical,
            );
        }
        if has(&[
            "no such host",
            "connection refused",
            "network is unreachable",
            "i/o timeout",
            "timeout",
            "tls handshake",
            "dial tcp",
        ]) && !matches!(self, CoreError::RcloneUnavailable(_))
        {
            return UserError::new(
                "offline",
                "You appear to be offline",
                "Sky Linux couldn't reach the cloud. Transfers will resume automatically when your connection is back.",
                UserAction::CheckConnection,
                technical,
            );
        }
        if has(&[
            "permission denied",
            "access denied",
            "operation not permitted",
        ]) {
            return UserError::new(
                "permission_denied",
                "Permission denied",
                "Sky Linux isn't allowed to read or write this folder. Check the folder's permissions and try again.",
                UserAction::CheckPermissions,
                technical,
            );
        }
        if has(&["429", "too many requests", "rate limit"]) {
            return UserError::new(
                "rate_limited",
                "The service is busy",
                "Telia Sky asked us to slow down. Sky Linux will retry automatically in a moment.",
                UserAction::Retry,
                technical,
            );
        }
        match self {
            CoreError::NotFound(_) => UserError::new(
                "not_found",
                "Item not found",
                "The file or folder no longer exists. It may have been moved or deleted on another device.",
                UserAction::None,
                technical,
            ),
            CoreError::Safety(msg) => UserError::new(
                "safety",
                "Stopped to protect your files",
                msg,
                UserAction::None,
                technical,
            ),
            CoreError::Invalid(msg) => UserError::new(
                "invalid",
                "Something isn't right",
                msg,
                UserAction::None,
                technical,
            ),
            CoreError::RcloneUnavailable(_) | CoreError::Process(_) | CoreError::Ipc(_) => {
                UserError::new(
                    "engine_unavailable",
                    "The background service isn't responding",
                    "Sky Linux is restarting its background service. If this keeps happening, restart the app.",
                    UserAction::Retry,
                    technical,
                )
            }
            CoreError::Secrets(_) => UserError::new(
                "keyring",
                "Couldn't access your keyring",
                "Sky Linux stores your sign-in securely in the system keyring. Make sure it is unlocked and try again.",
                UserAction::Retry,
                technical,
            ),
            _ => UserError::new(
                "unexpected",
                "Something went wrong",
                "An unexpected problem occurred. Try again; if it persists, open Settings → Diagnostics.",
                UserAction::ContactSupport,
                technical,
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rc(msg: &str) -> CoreError {
        CoreError::Rclone {
            path: "operations/list".into(),
            status: 500,
            message: msg.into(),
        }
    }

    #[test]
    fn translates_token_errors() {
        let e = rc("couldn't fetch token: invalid_grant: Token is not active");
        assert_eq!(e.to_user().code, "auth_expired");
        assert_eq!(e.to_user().action, UserAction::SignInAgain);
    }

    #[test]
    fn translates_network_errors() {
        let e = rc("dial tcp: lookup jfs.jottacloud.com: no such host");
        assert_eq!(e.to_user().code, "offline");
    }

    #[test]
    fn translates_quota_errors() {
        assert_eq!(
            rc("HTTP 507 insufficient storage").to_user().code,
            "storage_full"
        );
    }

    #[test]
    fn engine_unavailable_is_not_reported_as_offline() {
        let e = CoreError::RcloneUnavailable("connection refused".into());
        assert_eq!(e.to_user().code, "engine_unavailable");
    }

    #[test]
    fn rules_carry_code_and_params() {
        let u = CoreError::safety("system_folder", "/etc is a system folder")
            .with("path", "/etc")
            .to_user();
        assert_eq!(u.code, "safety");
        assert_eq!(u.rule.as_deref(), Some("system_folder"));
        assert_eq!(u.params["path"], "/etc");
        let json = serde_json::to_value(&u).unwrap();
        assert_eq!(json["params"]["path"], "/etc");
        assert!(serde_json::to_value(rc("x").to_user())
            .unwrap()
            .get("rule")
            .is_none());
    }

    #[test]
    fn keeps_technical_details() {
        let e = rc("boom");
        assert!(e.to_user().technical.contains("boom"));
    }
}
