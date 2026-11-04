//! # Sky Core
//!
//! The application engine behind Sky Linux. Sky Core owns *behaviour*
//! (accounts, transfers, backups, history, safety rules), while
//! [rclone](https://rclone.org) owns *cloud operations*. User interfaces never
//! talk to rclone directly: they talk to the Sky daemon over [`ipc`], and the
//! daemon talks to rclone through [`rclone::RcloneClient`].
//!
//! ```text
//!  Desktop UI ──IPC──▶ sky-daemon ──▶ Sky Core ──RC API (unix socket)──▶ rclone ──▶ Jottacloud
//! ```

pub mod accounts;
pub mod backup;
pub mod db;
pub mod error;
pub mod events;
pub mod ipc;
pub mod models;
pub mod paths;
pub mod provider;
pub mod rclone;
pub mod secrets;
pub mod service;
pub mod sync;
pub mod transfers;

pub use error::{CoreError, Result, UserError};
pub use service::SkyCore;
