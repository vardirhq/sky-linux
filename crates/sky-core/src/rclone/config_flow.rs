//! Driver for rclone's non-interactive configuration state machine
//! (`config/create` + `config/update` with `continue`).
//!
//! rclone backends describe their setup as a sequence of questions. For the
//! Jottacloud backend with a white-label such as Telia Sky the sequence
//! (verified against rclone v1.75) is:
//!
//! 1. `config_type` → `traditional` (browser-based OAuth; the "standard"
//!    personal-login-token flow is not offered by Telia Sky)
//! 2. `config_traditional` → `telia_no`
//! 3. `config_refresh_token` ("Token already configured – replace it?") →
//!    `false`: Sky Core ran the browser sign-in itself (`crate::oauth`) and
//!    passed the token in, so rclone never starts its own sign-in page.
//! 4. device / mountpoint selection → defaults (`Jotta` / `Archive`).
//!
//! Sky Core answers the questions it knows and accepts rclone's default for
//! anything new, so minor backend changes don't break onboarding.

use serde::Deserialize;
use serde_json::{json, Value};

use super::client::RcloneClient;
use crate::error::{CoreError, Result};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ConfigOption {
    pub name: String,
    #[serde(default)]
    pub help: String,
    #[serde(default)]
    pub default: Value,
    #[serde(default)]
    pub examples: Option<Vec<ConfigExample>>,
    #[serde(default)]
    pub is_password: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ConfigExample {
    pub value: String,
    #[serde(default)]
    pub help: String,
}

/// One step returned by rclone's config state machine.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ConfigQuestion {
    #[serde(default)]
    pub state: String,
    pub option: Option<ConfigOption>,
    #[serde(default)]
    pub error: String,
    #[serde(default)]
    pub result: String,
}

impl ConfigQuestion {
    pub fn is_done(&self) -> bool {
        self.state.is_empty()
    }

    /// rclone's default answer as a string.
    pub fn default_answer(&self) -> String {
        match self.option.as_ref().map(|o| &o.default) {
            Some(Value::String(s)) => s.clone(),
            Some(Value::Bool(b)) => b.to_string(),
            Some(Value::Number(n)) => n.to_string(),
            _ => String::new(),
        }
    }
}

/// What happened at each step, for logs and diagnostics.
#[derive(Debug, Clone)]
pub struct ConfigStep {
    pub option: String,
    pub answer: String,
}

/// Decides the answer to a configuration question. Returning `None` accepts
/// rclone's default.
pub trait ConfigAnswerer: Send + Sync {
    fn answer(&self, q: &ConfigQuestion) -> Option<String>;

    /// Abort instead of answering (e.g. rclone wants to start its own
    /// browser sign-in although Sky Core already supplied a token).
    fn refuses(&self, _q: &ConfigQuestion) -> bool {
        false
    }
}

impl<F> ConfigAnswerer for F
where
    F: Fn(&ConfigQuestion) -> Option<String> + Send + Sync,
{
    fn answer(&self, q: &ConfigQuestion) -> Option<String> {
        self(q)
    }
}

/// Answers for a Jottacloud white-label when Sky Core already ran the
/// browser sign-in (see `crate::oauth`) and passes the token in.
pub struct JottacloudWithToken {
    pub whitelabel: &'static str,
}

impl ConfigAnswerer for JottacloudWithToken {
    fn answer(&self, q: &ConfigQuestion) -> Option<String> {
        let name = q.option.as_ref()?.name.as_str();
        match name {
            "config_type" => Some("traditional".into()),
            "config_traditional" => Some(self.whitelabel.into()),
            // "Token already configured - replace it?" → keep ours.
            "config_refresh_token" => Some("false".into()),
            n if n.contains("password") => Some(String::new()),
            _ => None,
        }
    }

    fn refuses(&self, q: &ConfigQuestion) -> bool {
        matches!(
            q.option.as_ref().map(|o| o.name.as_str()),
            Some("config_is_local" | "config_token")
        )
    }
}

const MAX_STEPS: usize = 32;

impl RcloneClient {
    /// Create `name` of backend `kind` by walking rclone's config questions.
    pub async fn configure_remote(
        &self,
        name: &str,
        kind: &str,
        answerer: &dyn ConfigAnswerer,
    ) -> Result<Vec<ConfigStep>> {
        self.configure_remote_with(name, kind, json!({}), answerer)
            .await
    }

    /// Like [`Self::configure_remote`], starting from known `parameters`
    /// (e.g. a `token` obtained by Sky Core's own sign-in).
    pub async fn configure_remote_with(
        &self,
        name: &str,
        kind: &str,
        parameters: Value,
        answerer: &dyn ConfigAnswerer,
    ) -> Result<Vec<ConfigStep>> {
        let mut q: ConfigQuestion = serde_json::from_value(
            self.call(
                "config/create",
                json!({ "name": name, "type": kind, "parameters": parameters,
                         "opt": { "nonInteractive": true, "obscure": true } }),
            )
            .await?,
        )?;
        let mut steps = Vec::new();
        while !q.is_done() {
            if steps.len() >= MAX_STEPS {
                return Err(CoreError::Invalid(
                    "rclone configuration did not finish".into(),
                ));
            }
            if !q.error.is_empty() {
                return Err(CoreError::Rclone {
                    path: "config/update".into(),
                    status: 400,
                    message: q.error,
                });
            }
            let option = q
                .option
                .as_ref()
                .map(|o| o.name.clone())
                .unwrap_or_default();
            if answerer.refuses(&q) {
                return Err(CoreError::Invalid(format!(
                    "rclone asked an unexpected question during setup: {option}"
                )));
            }
            let answer = answerer.answer(&q).unwrap_or_else(|| q.default_answer());
            tracing::info!(option, state = %q.state, "answering rclone config question");
            steps.push(ConfigStep {
                option,
                answer: answer.clone(),
            });
            // The OAuth step blocks until the user finishes in the browser.
            q = serde_json::from_value(
                self.call_unbounded(
                    "config/update",
                    json!({ "name": name, "parameters": {},
                             "opt": { "nonInteractive": true, "obscure": true,
                                      "continue": true, "state": q.state, "result": answer } }),
                )
                .await?,
            )?;
        }
        if !q.error.is_empty() {
            return Err(CoreError::Rclone {
                path: "config/update".into(),
                status: 400,
                message: q.error,
            });
        }
        Ok(steps)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(name: &str, default: Value) -> ConfigQuestion {
        serde_json::from_value(json!({
            "State": "s", "Option": { "Name": name, "Default": default }, "Error": "", "Result": ""
        }))
        .unwrap()
    }

    #[test]
    fn token_answers_keep_our_token_and_refuse_rclone_sign_in() {
        let a = JottacloudWithToken {
            whitelabel: "telia_no",
        };
        assert_eq!(
            a.answer(&q("config_type", json!("standard"))).as_deref(),
            Some("traditional")
        );
        assert_eq!(
            a.answer(&q("config_traditional", json!("jottacloud")))
                .as_deref(),
            Some("telia_no")
        );
        assert_eq!(
            a.answer(&q("config_refresh_token", json!(true))).as_deref(),
            Some("false")
        );
        assert_eq!(a.answer(&q("config_non_standard", json!(false))), None);
        assert!(a.refuses(&q("config_is_local", json!(true))));
        assert!(a.refuses(&q("config_token", json!(""))));
        assert!(!a.refuses(&q("config_device", json!("Jotta"))));
    }

    #[test]
    fn defaults_are_stringified() {
        assert_eq!(q("x", json!(true)).default_answer(), "true");
        assert_eq!(q("x", json!("Jotta")).default_answer(), "Jotta");
        assert_eq!(q("x", Value::Null).default_answer(), "");
    }

    #[test]
    fn parses_real_rclone_question() {
        // Captured from `rclone config create probe jottacloud --non-interactive` (v1.75.2).
        let raw = r#"{"State":"auth_type_done","Option":{"Name":"config_type","Help":"Type of authentication.","Default":"standard","Examples":[{"Value":"standard","Help":"Standard"},{"Value":"traditional","Help":"Traditional"}],"IsPassword":false},"Error":"","Result":""}"#;
        let q: ConfigQuestion = serde_json::from_str(raw).unwrap();
        assert!(!q.is_done());
        assert_eq!(q.option.unwrap().examples.unwrap().len(), 2);
    }
}
