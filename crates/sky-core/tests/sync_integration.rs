//! Two-way sync against a real `rclone rcd` (alias remote → temp dir).
//! Every safety rule in `sky_core::sync` has a test here. Skipped when
//! rclone isn't installed.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use sky_core::models::*;
use sky_core::paths::AppPaths;
use sky_core::provider::Provider;
use sky_core::rclone::SupervisorConfig;
use sky_core::secrets::MemorySecretStore;
use sky_core::service::{CoreOptions, SkyCore};
use sky_core::sync::{ReviewAction, MARKER};

struct Env {
    _dir: tempfile::TempDir,
    cloud: PathBuf,
    local: PathBuf,
    core: Arc<SkyCore>,
    account: Account,
}

async fn env() -> Option<Env> {
    SupervisorConfig::find_binary()?;
    let dir = tempfile::tempdir().unwrap();
    let cloud = dir.path().join("cloud");
    let local = dir.path().join("home/Telia Sky");
    std::fs::create_dir_all(&cloud).unwrap();
    let core = SkyCore::start(CoreOptions {
        paths: AppPaths::rooted(dir.path().join("s")),
        rclone_binary: SupervisorConfig::find_binary().unwrap(),
        secrets: Arc::new(MemorySecretStore::default()),
        encrypt_rclone_config: false,
    })
    .await
    .unwrap();
    core.client()
        .create_remote(
            "cloud",
            "alias",
            json!({ "remote": cloud.to_string_lossy() }),
        )
        .await
        .unwrap();
    let account = core
        .accounts
        .adopt_remote(Provider::Jottacloud, "cloud")
        .await
        .unwrap();
    Some(Env {
        _dir: dir,
        cloud,
        local,
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

fn read(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

fn names(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n != MARKER)
        .collect();
    v.sort();
    v
}

impl Env {
    fn remote(&self) -> PathBuf {
        self.cloud.join("Sky Linux")
    }

    /// Create a pair, wait for its first (merge) run, then stop the
    /// background scheduler so the test drives runs deterministically.
    async fn pair(&self, keep_scheduler: bool) -> SyncPair {
        let pair = self
            .core
            .sync
            .create(NewSyncPair {
                account_id: self.account.id,
                name: None,
                local_path: self.local.clone(),
                remote_path: "Sky Linux".into(),
                create_local: true,
            })
            .await
            .unwrap();
        let pair = self
            .wait_state(pair.id, |s| {
                *s != SyncState::New && *s != SyncState::Syncing
            })
            .await;
        assert_eq!(pair.state, SyncState::Idle, "{:?}", pair.last_error);
        if !keep_scheduler {
            self.core.sync.set_enabled(pair.id, false).await.unwrap();
        }
        pair
    }

    async fn wait_state(&self, id: uuid::Uuid, f: impl Fn(&SyncState) -> bool) -> SyncPair {
        for _ in 0..300 {
            let p = self.core.db.sync_pair(id).unwrap();
            if f(&p.state) {
                return p;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        panic!("sync pair did not reach the expected state");
    }

    async fn sync(&self, id: uuid::Uuid) -> SyncPair {
        self.core.sync.run_now(id).await.unwrap()
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn first_sync_merges_both_sides_without_deleting() {
    let env = need_rclone!();
    write(&env.local.join("from-laptop.txt"), "laptop");
    write(&env.local.join("both.txt"), "old local");
    write(&env.remote().join("from-cloud.txt"), "cloud");
    write(&env.remote().join("Photos/fjord.jpg"), "jpeg");
    std::thread::sleep(Duration::from_millis(1100));
    write(&env.local.join("both.txt"), "newer local");

    let pair = env.pair(false).await;
    assert_eq!(
        names(&env.local),
        ["Photos", "both.txt", "from-cloud.txt", "from-laptop.txt"]
    );
    assert_eq!(names(&env.remote()), names(&env.local));
    assert_eq!(read(&env.remote().join("both.txt")), "newer local");
    assert_eq!(read(&env.local.join("Photos/fjord.jpg")), "jpeg");
    // Markers on both sides.
    assert!(env.local.join(MARKER).is_file() && env.remote().join(MARKER).is_file());
    assert!(pair.last_sync.is_some());
    env.core.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn changes_and_deletions_propagate_and_local_deletions_are_recoverable() {
    let env = need_rclone!();
    write(&env.local.join("keep.txt"), "k");
    write(&env.local.join("a.txt"), "a");
    write(&env.local.join("b.txt"), "b");
    write(&env.local.join("c.txt"), "c");
    let pair = env.pair(false).await;

    // New and edited in the cloud → down; new locally → up.
    write(&env.remote().join("new-in-cloud.txt"), "hello from the web");
    write(&env.local.join("new-local.txt"), "hello from linux");
    std::thread::sleep(Duration::from_millis(1100));
    write(&env.remote().join("keep.txt"), "edited in the cloud");
    let p = env.sync(pair.id).await;
    assert_eq!(p.state, SyncState::Idle, "{:?}", p.last_error);
    assert_eq!(
        read(&env.local.join("new-in-cloud.txt")),
        "hello from the web"
    );
    assert_eq!(read(&env.local.join("keep.txt")), "edited in the cloud");
    assert_eq!(
        read(&env.remote().join("new-local.txt")),
        "hello from linux"
    );
    let s = p.last_summary.unwrap();
    assert_eq!((s.uploaded, s.downloaded), (1, 2), "{s:?}");

    // Deleted in the cloud → removed locally, but kept in the recovery folder.
    std::fs::remove_file(env.remote().join("b.txt")).unwrap();
    // Deleted locally → removed in the cloud (Jottacloud keeps it in its trash).
    std::fs::remove_file(env.local.join("c.txt")).unwrap();
    let p = env.sync(pair.id).await;
    assert_eq!(p.state, SyncState::Idle, "{:?}", p.last_error);
    assert!(!env.local.join("b.txt").exists());
    assert_eq!(read(&p.recovery_path.join("b.txt")), "b");
    assert!(!env.remote().join("c.txt").exists());
    let s = p.last_summary.unwrap();
    assert_eq!((s.deleted_local, s.deleted_remote), (1, 1), "{s:?}");
    env.core.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn conflicts_keep_both_versions() {
    let env = need_rclone!();
    write(&env.local.join("plan.txt"), "v1");
    let pair = env.pair(false).await;

    write(&env.local.join("plan.txt"), "edited on linux");
    std::thread::sleep(Duration::from_millis(1100));
    write(&env.remote().join("plan.txt"), "edited on phone (newer)");
    let p = env.sync(pair.id).await;
    assert_eq!(p.state, SyncState::Idle, "{:?}", p.last_error);
    assert_eq!(p.last_summary.unwrap().conflicts, 1);
    for side in [env.local.clone(), env.remote()] {
        assert_eq!(read(&side.join("plan.txt")), "edited on phone (newer)");
        assert_eq!(read(&side.join("plan.txt.conflict1")), "edited on linux");
    }
    env.core.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn mass_deletion_stops_for_review() {
    let env = need_rclone!();
    for i in 0..10 {
        write(&env.local.join(format!("doc{i}.txt")), "x");
    }
    let pair = env.pair(false).await;

    // Delete 8 of 10 files locally: over the 50 % limit.
    for i in 0..8 {
        std::fs::remove_file(env.local.join(format!("doc{i}.txt"))).unwrap();
    }
    let p = env.sync(pair.id).await;
    assert_eq!(
        p.state,
        SyncState::NeedsReview {
            reason: ReviewReason::TooManyDeletes
        }
    );
    assert_eq!(
        names(&env.remote()).len(),
        10,
        "nothing may be deleted in the cloud"
    );
    // Scheduled runs stay paused while waiting for the user.
    assert_eq!(env.sync(pair.id).await.state, p.state);

    // "Restore everything" brings the files back from the cloud.
    let p = env
        .core
        .sync
        .resolve(pair.id, ReviewAction::RestoreAll)
        .await
        .unwrap();
    assert_eq!(p.state, SyncState::Idle, "{:?}", p.last_error);
    assert_eq!(names(&env.local).len(), 10);

    // Deleting again and approving applies the deletions.
    for i in 0..8 {
        std::fs::remove_file(env.local.join(format!("doc{i}.txt"))).unwrap();
    }
    assert!(matches!(
        env.sync(pair.id).await.state,
        SyncState::NeedsReview { .. }
    ));
    let p = env
        .core
        .sync
        .resolve(pair.id, ReviewAction::AllowDeletions)
        .await
        .unwrap();
    assert_eq!(p.state, SyncState::Idle, "{:?}", p.last_error);
    assert_eq!(names(&env.remote()), ["doc8.txt", "doc9.txt"]);
    env.core.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn missing_or_replaced_folder_never_empties_the_cloud() {
    let env = need_rclone!();
    write(&env.local.join("thesis.odt"), "years of work");
    let pair = env.pair(false).await;

    // Unplugged drive / moved folder.
    let moved = env.local.with_file_name("moved");
    std::fs::rename(&env.local, &moved).unwrap();
    let p = env.sync(pair.id).await;
    assert_eq!(
        p.state,
        SyncState::NeedsReview {
            reason: ReviewReason::FolderUnavailable
        }
    );
    assert!(env.remote().join("thesis.odt").exists());

    // An empty folder appears at the same path (e.g. an unmounted mount point).
    std::fs::create_dir_all(&env.local).unwrap();
    let p = env
        .core
        .sync
        .resolve(pair.id, ReviewAction::Retry)
        .await
        .unwrap();
    assert_eq!(
        p.state,
        SyncState::NeedsReview {
            reason: ReviewReason::MarkerMissing
        }
    );
    assert!(env.remote().join("thesis.odt").exists());
    // Approving deletions is refused for this case.
    assert!(env
        .core
        .sync
        .resolve(pair.id, ReviewAction::AllowDeletions)
        .await
        .is_err());

    // The user confirms this is the right folder: merge brings files back.
    let p = env
        .core
        .sync
        .resolve(pair.id, ReviewAction::RestoreAll)
        .await
        .unwrap();
    assert_eq!(p.state, SyncState::Idle, "{:?}", p.last_error);
    assert_eq!(read(&env.local.join("thesis.odt")), "years of work");
    env.core.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn local_changes_sync_automatically() {
    let env = need_rclone!();
    let _pair = env.pair(true).await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    write(&env.local.join("Notes/idea.md"), "# idea");
    let target = env.remote().join("Notes/idea.md");
    for _ in 0..150 {
        if target.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(read(&target), "# idea");
    env.core.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn unsafe_pairs_are_refused() {
    let env = need_rclone!();
    let new = |local: PathBuf, remote: &str| NewSyncPair {
        account_id: env.account.id,
        name: None,
        local_path: local,
        remote_path: remote.into(),
        create_local: true,
    };
    let rule = |e: sky_core::CoreError| e.to_user().rule.unwrap();

    let home = dirs::home_dir().unwrap();
    assert_eq!(
        rule(env.core.sync.create(new(home, "Home")).await.unwrap_err()),
        "home_folder"
    );
    assert_eq!(
        rule(
            env.core
                .sync
                .create(new("/etc".into(), "Etc"))
                .await
                .unwrap_err()
        ),
        "system_folder"
    );
    assert_eq!(
        rule(
            env.core
                .sync
                .create(new(env.local.clone(), ""))
                .await
                .unwrap_err()
        ),
        "remote_missing"
    );

    env.core
        .sync
        .create(new(env.local.clone(), "Sky Linux"))
        .await
        .unwrap();
    assert_eq!(
        rule(
            env.core
                .sync
                .create(new(env.local.join("sub"), "Other"))
                .await
                .unwrap_err()
        ),
        "overlaps_sync"
    );
    let elsewhere = env.local.with_file_name("Elsewhere");
    assert_eq!(
        rule(
            env.core
                .sync
                .create(new(elsewhere, "Sky Linux/Inner"))
                .await
                .unwrap_err()
        ),
        "overlaps_sync_remote"
    );
    env.core.shutdown().await;
}
