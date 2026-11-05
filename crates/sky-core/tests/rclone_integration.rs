//! End-to-end tests against a real `rclone rcd` process. The "cloud" is an
//! rclone `alias` remote pointing at a temporary directory, so these tests
//! need no network or account. They are skipped when rclone isn't installed.
//!
//! Real Telia Sky validation lives in `scripts/feasibility.sh`.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use sky_core::ipc::{self, IpcClient, Request};
use sky_core::models::*;
use sky_core::paths::AppPaths;
use sky_core::provider::Provider;
use sky_core::rclone::SupervisorConfig;
use sky_core::secrets::MemorySecretStore;
use sky_core::service::{CoreOptions, SkyCore};

struct Env {
    _dir: tempfile::TempDir,
    cloud: std::path::PathBuf,
    local: std::path::PathBuf,
    paths: AppPaths,
}

fn env() -> Option<Env> {
    SupervisorConfig::find_binary()?;
    let dir = tempfile::tempdir().unwrap();
    let cloud = dir.path().join("cloud");
    let local = dir.path().join("local");
    std::fs::create_dir_all(&cloud).unwrap();
    std::fs::create_dir_all(&local).unwrap();
    // Short root: unix socket paths are limited to ~108 bytes.
    let paths = AppPaths::rooted(dir.path().join("s"));
    Some(Env {
        cloud,
        local,
        paths,
        _dir: dir,
    })
}

async fn start(env: &Env) -> Arc<SkyCore> {
    SkyCore::start(CoreOptions {
        paths: env.paths.clone(),
        rclone_binary: SupervisorConfig::find_binary().unwrap(),
        secrets: Arc::new(MemorySecretStore::default()),
        encrypt_rclone_config: false,
    })
    .await
    .expect("core starts")
}

async fn connect(core: &SkyCore, env: &Env) -> Account {
    core.client()
        .create_remote(
            "cloud",
            "alias",
            json!({ "remote": env.cloud.to_string_lossy() }),
        )
        .await
        .unwrap();
    core.accounts
        .adopt_remote(Provider::Jottacloud, "cloud")
        .await
        .unwrap()
}

async fn wait_for(core: &SkyCore, id: uuid::Uuid) -> Transfer {
    for _ in 0..200 {
        if let Some(t) = core
            .transfers(50)
            .await
            .unwrap()
            .into_iter()
            .find(|t| t.id == id)
        {
            if t.state.is_terminal() {
                return t;
            }
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("transfer {id} did not finish");
}

fn write(p: &Path, body: &str) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, body).unwrap();
}

macro_rules! need_rclone {
    () => {
        match env() {
            Some(e) => e,
            None => {
                eprintln!("rclone not found; skipping");
                return;
            }
        }
    };
}

#[tokio::test(flavor = "multi_thread")]
async fn browse_upload_download_and_manage() {
    let env = need_rclone!();
    let core = start(&env).await;
    let account = connect(&core, &env).await;

    write(&env.cloud.join("Photos/norway.jpg"), "fjord");
    write(&env.cloud.join("Holiday plan.pdf"), "plan");

    let root = core.list(None, "", true).await.unwrap();
    let names: Vec<_> = root.iter().map(|e| e.name.as_str()).collect();
    assert!(
        names.contains(&"Photos") && names.contains(&"Holiday plan.pdf"),
        "{names:?}"
    );
    assert_eq!(
        root.iter().find(|e| e.name == "Photos").unwrap().kind,
        EntryKind::Folder
    );

    // Upload a file.
    let src = env.local.join("budget.xlsx");
    write(&src, "numbers");
    let t = core
        .upload(Some(account.id), vec![src.clone()], "Documents")
        .await
        .unwrap();
    let t = wait_for(&core, t[0].id).await;
    assert_eq!(t.state, TransferState::Completed, "{:?}", t.error);
    assert_eq!(
        std::fs::read_to_string(env.cloud.join("Documents/budget.xlsx")).unwrap(),
        "numbers"
    );

    // Upload a folder.
    write(&env.local.join("Work/a.txt"), "a");
    write(&env.local.join("Work/sub/b.txt"), "b");
    let t = core
        .upload(None, vec![env.local.join("Work")], "")
        .await
        .unwrap();
    assert_eq!(
        wait_for(&core, t[0].id).await.state,
        TransferState::Completed
    );
    assert!(env.cloud.join("Work/sub/b.txt").exists());

    // Listing reflects the upload (cache invalidated).
    let docs = core.list(None, "Documents", false).await.unwrap();
    assert_eq!(docs.len(), 1);
    assert_eq!(docs[0].size, Some(7));

    // Download never overwrites an existing local file.
    let dl = env.local.join("Downloads");
    write(&dl.join("norway.jpg"), "mine");
    let entry = core.list(None, "Photos", false).await.unwrap().remove(0);
    let t = core.download(None, vec![entry], dl.clone()).await.unwrap();
    let t = wait_for(&core, t[0].id).await;
    assert_eq!(t.state, TransferState::Completed, "{:?}", t.error);
    assert_eq!(
        std::fs::read_to_string(dl.join("norway.jpg")).unwrap(),
        "mine"
    );
    assert_eq!(
        std::fs::read_to_string(dl.join("norway (1).jpg")).unwrap(),
        "fjord"
    );

    // Rename, mkdir, move folder, delete.
    core.move_entry(None, "Holiday plan.pdf", "Documents/Holiday.pdf", false)
        .await
        .unwrap();
    assert!(env.cloud.join("Documents/Holiday.pdf").exists());
    core.mkdir(None, "Archive/2025").await.unwrap();
    assert!(env.cloud.join("Archive/2025").is_dir());
    core.move_entry(None, "Work", "Archive/Work", true)
        .await
        .unwrap();
    assert!(env.cloud.join("Archive/Work/sub/b.txt").exists());
    assert!(core
        .move_entry(None, "Archive", "Archive/x", true)
        .await
        .is_err());
    core.delete_file(None, "Documents/Holiday.pdf")
        .await
        .unwrap();
    assert!(!env.cloud.join("Documents/Holiday.pdf").exists());

    // Search.
    let hits = core.search(None, "budg").await.unwrap();
    assert_eq!(hits.len(), 1, "{hits:?}");

    // Usage works on the local backend.
    assert!(core.usage(None).await.unwrap().total.is_some());

    // Terminal transfers are recorded in history.
    assert!(!core.history(100).unwrap().is_empty());
    core.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn backups_are_upload_only() {
    let env = need_rclone!();
    let core = start(&env).await;
    let account = connect(&core, &env).await;

    let pics = env.local.join("Pictures");
    write(&pics.join("one.jpg"), "1");
    write(&pics.join("two.jpg"), "2");
    write(&pics.join("scratch.tmp"), "ignored by default excludes");

    let job = core
        .backups
        .create(NewBackupJob {
            account_id: account.id,
            name: None,
            local_path: pics.clone(),
            remote_path: "Backup/Pictures".into(),
            schedule: Schedule::Manual,
            exclude: vec![],
        })
        .await
        .unwrap();
    assert_eq!(job.name, "Pictures");

    let job = core.backups.run_now(job.id).await.unwrap();
    assert!(job.last_error.is_none(), "{:?}", job.last_error);
    assert!(job.last_success.is_some());
    assert!(env.cloud.join("Backup/Pictures/two.jpg").exists());
    assert!(!env.cloud.join("Backup/Pictures/scratch.tmp").exists());

    // Deleting locally must NOT delete in the cloud.
    std::fs::remove_file(pics.join("two.jpg")).unwrap();
    write(&pics.join("three.jpg"), "3");
    core.backups.run_now(job.id).await.unwrap();
    assert!(env.cloud.join("Backup/Pictures/two.jpg").exists());
    assert!(env.cloud.join("Backup/Pictures/three.jpg").exists());

    // An unavailable source folder (unplugged drive) is skipped safely.
    std::fs::rename(&pics, env.local.join("moved")).unwrap();
    let job = core.backups.run_now(job.id).await.unwrap();
    assert!(job.last_error.is_some());
    assert!(env.cloud.join("Backup/Pictures/one.jpg").exists());

    // System folders are refused.
    let err = core
        .backups
        .create(NewBackupJob {
            account_id: account.id,
            name: None,
            local_path: "/".into(),
            remote_path: "Backup".into(),
            schedule: Schedule::Manual,
            exclude: vec![],
        })
        .await
        .unwrap_err();
    assert_eq!(err.to_user().code, "safety");
    core.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn on_change_backup_triggers_from_filesystem_events() {
    let env = need_rclone!();
    let core = start(&env).await;
    let account = connect(&core, &env).await;
    let docs = env.local.join("Docs");
    std::fs::create_dir_all(&docs).unwrap();
    core.backups
        .create(NewBackupJob {
            account_id: account.id,
            name: None,
            local_path: docs.clone(),
            remote_path: "Backup/Docs".into(),
            schedule: Schedule::OnChange {
                debounce_seconds: 1,
                fallback_hours: 24,
            },
            exclude: vec![],
        })
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(500)).await;
    write(&docs.join("new.txt"), "hello");
    let target = env.cloud.join("Backup/Docs/new.txt");
    for _ in 0..100 {
        if target.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(target.exists(), "file change did not trigger a backup");
    core.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn interrupted_transfers_resume_after_restart() {
    let env = need_rclone!();
    let core = start(&env).await;
    let account = connect(&core, &env).await;
    let src = env.local.join("big.bin");
    write(&src, "payload");
    // Simulate a crash mid-transfer: record a running transfer and its spec
    // without submitting it to rclone.
    let t = Transfer {
        id: uuid::Uuid::new_v4(),
        account_id: account.id,
        direction: TransferDirection::Upload,
        source: src.to_string_lossy().into(),
        destination: "Resumed/big.bin".into(),
        state: TransferState::Running,
        bytes_total: None,
        bytes_done: 0,
        speed: None,
        eta_seconds: None,
        error: None,
        rclone_job: Some(999),
        backup_job: None,
        started_at: chrono::Utc::now(),
        finished_at: None,
    };
    core.db.upsert_transfer(&t).unwrap();
    core.db
        .set_transfer_spec(
            t.id,
            &json!({ "kind": "uploadFile", "local": src, "remoteDir": "Resumed" }).to_string(),
        )
        .unwrap();
    core.shutdown().await;
    drop(core);

    let core = start(&env).await;
    let done = wait_for(&core, t.id).await;
    assert_eq!(done.state, TransferState::Completed, "{:?}", done.error);
    assert!(env.cloud.join("Resumed/big.bin").exists());
    core.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn ipc_roundtrip_and_events() {
    let env = need_rclone!();
    let core = start(&env).await;
    connect(&core, &env).await;
    write(&env.cloud.join("hello.txt"), "hi");

    let sock = env.paths.daemon_socket();
    let listener = ipc::bind(&sock).unwrap();
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(
        std::fs::metadata(&sock).unwrap().permissions().mode() & 0o777,
        0o600
    );
    tokio::spawn(ipc::serve(listener, core.clone()));
    // A second daemon must not steal the socket.
    assert!(ipc::bind(&sock).is_err());

    let client = IpcClient::connect(&sock).await.unwrap();
    let mut events = client.events();
    let status = client.request(&Request::Status).await.unwrap();
    assert_eq!(status["rcloneRunning"], true);
    assert_eq!(status["accounts"].as_array().unwrap().len(), 1);

    let list = client
        .request_raw("files.list", json!({ "path": "" }))
        .await
        .unwrap();
    assert_eq!(list[0]["name"], "hello.txt");

    // Errors arrive translated.
    let err = client
        .request_raw("files.delete", json!({ "path": "../etc/passwd" }))
        .await
        .unwrap_err();
    assert_eq!(err.code, "invalid");
    let err = client
        .request_raw("settings.get", json!({ "key": "secret" }))
        .await
        .unwrap_err();
    assert_eq!(err.code, "invalid");

    client
        .request_raw("settings.set", json!({ "key": "theme", "value": "dark" }))
        .await
        .unwrap();
    assert_eq!(
        client
            .request_raw("settings.get", json!({ "key": "theme" }))
            .await
            .unwrap(),
        "dark"
    );

    // Events are pushed (stats are published every second).
    let ev = tokio::time::timeout(Duration::from_secs(5), events.recv()).await;
    assert!(ev.is_ok(), "no events received");
    core.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn rclone_config_is_encrypted_at_rest() {
    let env = need_rclone!();
    let secrets = Arc::new(MemorySecretStore::default());
    let opts = || CoreOptions {
        paths: env.paths.clone(),
        rclone_binary: SupervisorConfig::find_binary().unwrap(),
        secrets: secrets.clone(),
        encrypt_rclone_config: true,
    };
    let core = SkyCore::start(opts()).await.unwrap();
    connect(&core, &env).await;
    let conf = std::fs::read_to_string(env.paths.rclone_config()).unwrap();
    assert!(
        conf.starts_with("# Encrypted rclone configuration"),
        "{conf}"
    );
    assert!(!conf.contains(&*env.cloud.to_string_lossy()));
    core.shutdown().await;
    drop(core);
    // Restart with the same keyring: remotes are still readable.
    let core = SkyCore::start(opts()).await.unwrap();
    assert_eq!(
        core.client().list_remotes().await.unwrap(),
        vec!["cloud".to_string()]
    );
    core.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn bandwidth_limit_reaches_rclone_and_survives_a_restart() {
    let Some(env) = env() else { return };
    let core = start(&env).await;
    let rate = |c: Arc<SkyCore>| async move {
        c.client().call("core/bwlimit", json!({})).await.unwrap()["rate"]
            .as_str()
            .unwrap()
            .to_string()
    };
    assert_eq!(rate(core.clone()).await, "off");

    ipc::dispatch(
        &core,
        serde_json::from_value(
            json!({ "method": "settings.set", "params": { "key": "bandwidthLimit", "value": 5 } }),
        )
        .unwrap(),
    )
    .await
    .unwrap();
    assert_eq!(rate(core.clone()).await, "5Mi");

    let bad: Request = serde_json::from_value(
        json!({ "method": "settings.set", "params": { "key": "bandwidthLimit", "value": "fast" } }),
    )
    .unwrap();
    assert!(ipc::dispatch(&core, bad).await.is_err());

    // A fresh start (as after a reboot) applies the saved limit.
    core.shutdown().await;
    drop(core);
    let core = start(&env).await;
    assert_eq!(rate(core.clone()).await, "5Mi");

    ipc::dispatch(
        &core,
        serde_json::from_value(json!({ "method": "settings.set", "params": { "key": "bandwidthLimit", "value": null } }))
            .unwrap(),
    )
    .await
    .unwrap();
    assert_eq!(rate(core.clone()).await, "off");
    core.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn diagnostics_report_over_ipc() {
    let Some(env) = env() else { return };
    let core = start(&env).await;
    connect(&core, &env).await;
    let v = ipc::dispatch(
        &core,
        serde_json::from_value(json!({ "method": "daemon.diagnostics" })).unwrap(),
    )
    .await
    .unwrap();
    assert_eq!(v["rcloneRunning"], true);
    let report = v["report"].as_str().unwrap();
    assert!(report.contains("## Accounts (1)"), "{report}");
    assert!(report.contains("rclone: v"), "{report}");
    core.shutdown().await;
}
