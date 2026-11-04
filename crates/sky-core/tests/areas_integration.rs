//! Storage areas end to end: a local folder laid out like a Jottacloud
//! device (Archive/, Photos/, Shared/, Sync/) joined with rclone `combine`,
//! exactly as Sky Core does for a real account. Skipped without rclone.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use sky_core::areas::{AreaLayout, JOTTA_AREAS};
use sky_core::models::*;
use sky_core::paths::AppPaths;
use sky_core::provider::Provider;
use sky_core::rclone::SupervisorConfig;
use sky_core::secrets::MemorySecretStore;
use sky_core::service::{CoreOptions, SkyCore};

struct Env {
    _dir: tempfile::TempDir,
    cloud: PathBuf,
    home: PathBuf,
    core: Arc<SkyCore>,
    account: Account,
}

async fn env() -> Option<Env> {
    let rclone = SupervisorConfig::find_binary()?;
    let dir = tempfile::tempdir().unwrap();
    let cloud = dir.path().join("cloud");
    for a in JOTTA_AREAS {
        std::fs::create_dir_all(cloud.join(a)).unwrap();
    }
    // Like the real account: everything is in Photos.
    write(&cloud.join("Photos/Camera/IMG_0001.jpg"), "jpeg");
    let home = dir.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    let core = SkyCore::start(CoreOptions {
        paths: AppPaths::rooted(dir.path().join("s")),
        rclone_binary: rclone,
        secrets: Arc::new(MemorySecretStore::default()),
        encrypt_rclone_config: false,
    })
    .await
    .unwrap();
    core.client()
        .create_remote(
            "sky-telia-no",
            "alias",
            json!({ "remote": cloud.to_string_lossy() }),
        )
        .await
        .unwrap();
    let areas = JOTTA_AREAS.iter().map(|a| a.to_string()).collect();
    let account = core
        .accounts
        .register_areas(
            Provider::TeliaSkyNo,
            "sky-telia-no",
            areas,
            AreaLayout::Subfolders,
        )
        .await
        .unwrap();
    Some(Env {
        _dir: dir,
        cloud,
        home,
        core,
        account,
    })
}

macro_rules! need_rclone {
    () => {
        match env().await {
            Some(e) => e,
            None => {
                eprintln!("rclone not found; skipping");
                return;
            }
        }
    };
}

fn write(p: &Path, body: &str) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, body).unwrap();
}

fn rule<T: std::fmt::Debug>(r: sky_core::Result<T>) -> String {
    r.unwrap_err()
        .to_user()
        .rule
        .unwrap_or_else(|| "<none>".into())
}

async fn wait_done(core: &SkyCore, id: uuid::Uuid) -> Transfer {
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
    panic!("transfer did not finish");
}

#[tokio::test(flavor = "multi_thread")]
async fn all_areas_appear_as_top_level_folders() {
    let env = need_rclone!();
    assert_eq!(env.account.remote, "sky-telia-no-all");
    let mut names: Vec<_> = env
        .core
        .list(None, "", true)
        .await
        .unwrap()
        .into_iter()
        .map(|e| (e.name, e.kind))
        .collect();
    names.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(
        names,
        JOTTA_AREAS
            .iter()
            .map(|a| (a.to_string(), EntryKind::Folder))
            .collect::<Vec<_>>()
    );
    let photos = env.core.list(None, "Photos/Camera", true).await.unwrap();
    assert_eq!(photos[0].path, "Photos/Camera/IMG_0001.jpg");
    // Search spans all areas.
    assert_eq!(env.core.search(None, "IMG_0001").await.unwrap().len(), 1);
    env.core.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn photos_can_be_downloaded_but_not_changed() {
    let env = need_rclone!();
    let photo = env
        .core
        .list(None, "Photos/Camera", true)
        .await
        .unwrap()
        .remove(0);
    let t = env
        .core
        .download(None, vec![photo.clone()], env.home.clone())
        .await
        .unwrap();
    assert_eq!(
        wait_done(&env.core, t[0].id).await.state,
        TransferState::Completed
    );
    assert_eq!(
        std::fs::read_to_string(env.home.join("IMG_0001.jpg")).unwrap(),
        "jpeg"
    );

    let f = env.home.join("new.txt");
    write(&f, "x");
    assert_eq!(
        rule(
            env.core
                .upload(None, vec![f.clone()], "Photos/Camera")
                .await
        ),
        "area_read_only"
    );
    assert_eq!(
        rule(env.core.delete_file(None, &photo.path).await),
        "area_read_only"
    );
    assert_eq!(
        rule(env.core.mkdir(None, "Shared/x").await),
        "area_read_only"
    );
    assert_eq!(
        rule(
            env.core
                .move_entry(None, &photo.path, "Photos/Camera/renamed.jpg", false)
                .await
        ),
        "area_read_only"
    );
    assert!(env.cloud.join("Photos/Camera/IMG_0001.jpg").exists());
    env.core.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn writable_areas_work_and_areas_themselves_are_protected() {
    let env = need_rclone!();
    let f = env.home.join("report.odt");
    write(&f, "doc");
    let t = env.core.upload(None, vec![f], "Archive").await.unwrap();
    assert_eq!(
        wait_done(&env.core, t[0].id).await.state,
        TransferState::Completed
    );
    assert_eq!(
        std::fs::read_to_string(env.cloud.join("Archive/report.odt")).unwrap(),
        "doc"
    );

    env.core.mkdir(None, "Sync/Notes").await.unwrap();
    assert!(env.cloud.join("Sync/Notes").is_dir());
    env.core
        .move_entry(None, "Archive/report.odt", "Archive/Work/report.odt", false)
        .await
        .unwrap();
    assert!(env.cloud.join("Archive/Work/report.odt").exists());

    assert_eq!(
        rule(env.core.mkdir(None, "Telia Sky").await),
        "unknown_area"
    );
    assert_eq!(
        rule(env.core.move_entry(None, "Archive", "Archive2", true).await),
        "area_root"
    );
    assert_eq!(
        rule(
            env.core
                .move_entry(None, "Archive/Work/report.odt", "Sync/report.odt", false)
                .await
        ),
        "move_between_areas"
    );
    env.core.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn sync_and_backups_target_writable_areas() {
    let env = need_rclone!();
    let local = env.home.join("Telia Sky");
    write(&local.join("plan.txt"), "plan");
    let new = |remote: &str| NewSyncPair {
        account_id: env.account.id,
        name: None,
        local_path: local.clone(),
        remote_path: remote.into(),
        create_local: true,
    };
    assert_eq!(
        rule(env.core.sync.create(new("Photos/Telia Sky")).await),
        "area_read_only"
    );
    assert_eq!(
        rule(env.core.sync.create(new("Telia Sky")).await),
        "unknown_area"
    );

    let pair = env
        .core
        .sync
        .create(new("Archive/Telia Sky"))
        .await
        .unwrap();
    for _ in 0..200 {
        let p = env.core.db.sync_pair(pair.id).unwrap();
        if p.state != SyncState::New && p.state != SyncState::Syncing {
            assert_eq!(p.state, SyncState::Idle, "{:?}", p.last_error);
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(
        std::fs::read_to_string(env.cloud.join("Archive/Telia Sky/plan.txt")).unwrap(),
        "plan"
    );

    let backup = |remote: &str| NewBackupJob {
        account_id: env.account.id,
        name: None,
        local_path: env.home.clone(),
        remote_path: remote.into(),
        schedule: Schedule::Manual,
        exclude: vec![],
    };
    assert_eq!(
        rule(env.core.backups.create(backup("Photos/Backup")).await),
        "area_read_only"
    );
    env.core
        .backups
        .create(backup("Archive/Backup/Home"))
        .await
        .unwrap();
    env.core.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn signing_out_removes_both_remotes() {
    let env = need_rclone!();
    env.core.accounts.disconnect(env.account.id).await.unwrap();
    let remotes = env.core.client().list_remotes().await.unwrap();
    assert!(
        !remotes.iter().any(|r| r.starts_with("sky-telia-no")),
        "{remotes:?}"
    );
    // Files are untouched.
    assert!(env.cloud.join("Photos/Camera/IMG_0001.jpg").exists());
    env.core.shutdown().await;
}
