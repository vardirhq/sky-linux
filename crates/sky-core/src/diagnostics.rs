//! The diagnostics report users can copy into a bug report or support
//! request. It describes the setup and recent problems, and it is built to
//! be safe to share: no tokens, no passwords, no account names or e-mail
//! addresses, and the home folder is written as `~`.

use std::path::Path;

use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::events::{Event, HistoryEntry};
use crate::models::{Account, BackupJob, SyncPair, SyncState};

/// Everything the report is built from.
pub struct DiagnosticsInput<'a> {
    pub version: &'a str,
    pub rclone_version: Option<&'a str>,
    pub os: Option<String>,
    pub bandwidth_limit: Option<u32>,
    pub accounts: &'a [Account],
    pub backups: &'a [BackupJob],
    pub sync_pairs: &'a [SyncPair],
    pub history: &'a [HistoryEntry],
    pub rclone_log: &'a [String],
    pub home: Option<&'a Path>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostics {
    pub generated_at: DateTime<Utc>,
    pub rclone_running: bool,
    /// Number of problems in the recent history.
    pub recent_problems: usize,
    /// Plain-text report, ready to copy.
    pub report: String,
}

/// How many recent problems and log lines the report includes.
const MAX_PROBLEMS: usize = 20;
const MAX_LOG_LINES: usize = 80;

pub fn build(input: &DiagnosticsInput<'_>) -> Diagnostics {
    let home = |s: &str| redact_home(s, input.home);
    let mut r = String::new();
    let now = Utc::now();
    let line = |r: &mut String, s: String| {
        r.push_str(&s);
        r.push('\n');
    };

    line(&mut r, "# Sky Linux diagnostics".into());
    line(
        &mut r,
        format!("Generated: {}", now.format("%Y-%m-%d %H:%M UTC")),
    );
    line(&mut r, format!("Sky Linux: {}", input.version));
    line(
        &mut r,
        format!("rclone: {}", input.rclone_version.unwrap_or("not running")),
    );
    line(
        &mut r,
        format!("System: {}", input.os.as_deref().unwrap_or("unknown")),
    );
    line(
        &mut r,
        format!(
            "Bandwidth limit: {}",
            match input.bandwidth_limit {
                Some(mb) if mb > 0 => format!("{mb} MB/s"),
                _ => "none".into(),
            }
        ),
    );

    line(&mut r, String::new());
    line(&mut r, format!("## Accounts ({})", input.accounts.len()));
    for a in input.accounts {
        line(
            &mut r,
            format!(
                "- {} • areas: {} • connected {}",
                a.provider.display_name(),
                if a.areas.is_empty() {
                    "-".into()
                } else {
                    a.areas.join(", ")
                },
                a.connected_at.format("%Y-%m-%d")
            ),
        );
    }

    line(&mut r, String::new());
    line(&mut r, format!("## Backups ({})", input.backups.len()));
    for b in input.backups {
        line(
            &mut r,
            format!(
                "- {} → {} • {} • last success: {}{}",
                home(&b.local_path.display().to_string()),
                b.remote_path,
                if b.enabled { "on" } else { "paused" },
                when(b.last_success),
                b.last_error
                    .as_ref()
                    .map(|e| format!(" • last error: {}", e.code))
                    .unwrap_or_default()
            ),
        );
    }

    line(&mut r, String::new());
    line(
        &mut r,
        format!("## Two-way sync ({})", input.sync_pairs.len()),
    );
    for p in input.sync_pairs {
        line(
            &mut r,
            format!(
                "- {} ⇄ {} • {} • {} • last sync: {}{}",
                home(&p.local_path.display().to_string()),
                p.remote_path,
                if p.enabled { "on" } else { "paused" },
                sync_state(&p.state),
                when(p.last_sync),
                p.last_error
                    .as_ref()
                    .map(|e| format!(" • last error: {}", e.code))
                    .unwrap_or_default()
            ),
        );
    }

    let problems: Vec<_> = input
        .history
        .iter()
        .filter_map(|h| problem(&h.event).map(|p| (h.at, p)))
        .take(MAX_PROBLEMS)
        .collect();
    line(&mut r, String::new());
    line(&mut r, format!("## Recent problems ({})", problems.len()));
    for (at, p) in &problems {
        line(
            &mut r,
            format!("- {} {}", at.format("%Y-%m-%d %H:%M"), home(p)),
        );
    }

    let start = input.rclone_log.len().saturating_sub(MAX_LOG_LINES);
    line(&mut r, String::new());
    line(&mut r, "## rclone log (most recent last)".into());
    for l in &input.rclone_log[start..] {
        line(&mut r, home(&redact_log_line(l)));
    }

    Diagnostics {
        generated_at: now,
        rclone_running: input.rclone_version.is_some(),
        recent_problems: problems.len(),
        report: r,
    }
}

fn when(t: Option<DateTime<Utc>>) -> String {
    t.map(|t| t.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_else(|| "never".into())
}

fn sync_state(s: &SyncState) -> String {
    match s {
        SyncState::New => "new".into(),
        SyncState::Idle => "idle".into(),
        SyncState::Syncing => "syncing".into(),
        SyncState::NeedsReview { reason } => format!(
            "needs review ({})",
            serde_json::to_value(reason)
                .ok()
                .and_then(|v| v.as_str().map(String::from))
                .unwrap_or_default()
        ),
        SyncState::Error => "error".into(),
    }
}

/// A one-line description of a history event that went wrong.
fn problem(e: &Event) -> Option<String> {
    let err = |what: &str, e: &crate::UserError| {
        format!(
            "{what}: {} ({}{})",
            e.title,
            e.code,
            e.rule.as_ref().map(|r| format!("/{r}")).unwrap_or_default()
        )
    };
    match e {
        Event::Error(u) => Some(err("error", u)),
        Event::AuthExpired { .. } => Some("sign-in expired".into()),
        Event::TransferUpdated(t) => t.error.as_ref().map(|u| err("transfer", u)),
        Event::BackupFinished(b) => b.last_error.as_ref().map(|u| err("backup", u)),
        Event::SyncFinished(p) => match (&p.state, &p.last_error) {
            (_, Some(u)) => Some(err("sync", u)),
            (SyncState::NeedsReview { .. }, None) => {
                Some(format!("sync: {}", sync_state(&p.state)))
            }
            _ => None,
        },
        _ => None,
    }
}

/// Write the home folder as `~`, so the report doesn't reveal the user name.
fn redact_home(s: &str, home: Option<&Path>) -> String {
    match home.map(|h| h.display().to_string()) {
        Some(h) if h.len() > 1 => s.replace(&h, "~"),
        _ => s.to_string(),
    }
}

/// rclone logs at INFO don't contain tokens, but a line that mentions one
/// is dropped whole rather than trusted. Terminal colour codes (bisync
/// colours its output) are removed.
fn redact_log_line(l: &str) -> String {
    let l = strip_ansi(l);
    let lower = l.to_ascii_lowercase();
    if ["token", "password", "secret", "authorization", "pass="]
        .iter()
        .any(|w| lower.contains(w))
    {
        "[line hidden: it may contain sign-in details]".into()
    } else {
        l
    }
}

/// Remove `ESC [ … letter` sequences.
fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.next() == Some('[') {
                for c in chars.by_ref() {
                    if c.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// `PRETTY_NAME` from os-release, e.g. "Ubuntu 24.04.1 LTS".
pub fn os_name() -> Option<String> {
    let text = std::fs::read_to_string("/etc/os-release")
        .or_else(|_| std::fs::read_to_string("/usr/lib/os-release"))
        .ok()?;
    text.lines()
        .find_map(|l| l.strip_prefix("PRETTY_NAME="))
        .map(|v| v.trim_matches('"').to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Schedule;
    use crate::provider::Provider;
    use std::path::PathBuf;
    use uuid::Uuid;

    fn account() -> Account {
        Account {
            id: Uuid::new_v4(),
            provider: Provider::TeliaSkyNo,
            remote: "sky-telia-no-all".into(),
            areas: vec!["Archive".into(), "Photos".into()],
            display_name: Some("Kari Nordmann".into()),
            email: Some("kari@example.no".into()),
            connected_at: Utc::now(),
        }
    }

    fn backup() -> BackupJob {
        BackupJob {
            id: Uuid::new_v4(),
            account_id: Uuid::new_v4(),
            name: "Pictures".into(),
            local_path: PathBuf::from("/home/kari/Pictures"),
            remote_path: "Archive/Pictures".into(),
            schedule: Schedule::Manual,
            enabled: true,
            exclude: vec![],
            last_run: None,
            last_success: None,
            last_error: None,
            created_at: Utc::now(),
        }
    }

    #[test]
    fn report_hides_personal_details() {
        let accounts = [account()];
        let backups = [backup()];
        let log = vec![
            "2026/11/05 INFO  : Archive/Pictures/a.jpg: Copied (new)".to_string(),
            "DEBUG : refresh token=abc123".to_string(),
            "INFO : wrote /home/kari/.config/x".to_string(),
            "INFO  : \u{1b}[32mBisync successful\u{1b}[0m".to_string(),
        ];
        let d = build(&DiagnosticsInput {
            version: "0.1.0",
            rclone_version: Some("v1.75.2"),
            os: Some("Ubuntu 24.04 LTS".into()),
            bandwidth_limit: Some(5),
            accounts: &accounts,
            backups: &backups,
            sync_pairs: &[],
            history: &[],
            rclone_log: &log,
            home: Some(Path::new("/home/kari")),
        });
        let r = &d.report;
        assert!(r.contains("Telia Sky"), "{r}");
        assert!(r.contains("Bandwidth limit: 5 MB/s"));
        assert!(r.contains("~/Pictures → Archive/Pictures"));
        assert!(r.contains("~/.config/x"));
        assert!(r.contains("INFO  : Bisync successful\n"), "{r}");
        assert!(!r.contains("kari"), "{r}");
        assert!(!r.contains("Kari"), "{r}");
        assert!(!r.contains("abc123"), "{r}");
        assert!(d.rclone_running);
    }

    #[test]
    fn problems_are_listed_newest_first() {
        let mut failed = backup();
        failed.last_error = Some(crate::CoreError::RcloneUnavailable("down".into()).to_user());
        let history = [
            HistoryEntry {
                id: 2,
                at: Utc::now(),
                event: Event::BackupFinished(failed),
            },
            HistoryEntry {
                id: 1,
                at: Utc::now(),
                event: Event::BackupFinished(backup()),
            },
        ];
        let d = build(&DiagnosticsInput {
            version: "0.1.0",
            rclone_version: None,
            os: None,
            bandwidth_limit: None,
            accounts: &[],
            backups: &[],
            sync_pairs: &[],
            history: &history,
            rclone_log: &[],
            home: None,
        });
        assert_eq!(d.recent_problems, 1);
        assert!(d.report.contains("backup: "));
        assert!(d.report.contains("rclone: not running"));
        assert!(!d.rclone_running);
    }
}
