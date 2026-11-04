//! `sky-daemon` — the Sky Linux background service.
//!
//! Runs as a systemd user service (or is spawned by the desktop app), owns
//! the private rclone process and serves the desktop UI over a unix socket.
//! Closing the window never stops backups: only stopping this service does.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use sky_core::ipc::{self, IpcClient, Request};
use sky_core::paths::AppPaths;
use sky_core::rclone::SupervisorConfig;
use sky_core::secrets::{MemorySecretStore, SecretStore};
use sky_core::service::{CoreOptions, SkyCore};

const HELP: &str = "\
sky-daemon — Sky Linux background service

USAGE:
    sky-daemon [run] [OPTIONS]    Run the service in the foreground (default)
    sky-daemon status             Print the status of a running service
    sky-daemon sign-in            Sign in to Telia Sky in the browser (service must be running)
    sky-daemon stop               Ask a running service to exit (via systemd if managed)

OPTIONS:
    --rclone <PATH>       rclone binary to use (default: bundled, then $PATH)
    --no-keyring          Do not use the Secret Service; rclone.conf stays unencrypted
    --local-cloud <DIR>   Developer mode: use a local folder as the cloud (no account needed)
    -h, --help            Show this help
    -V, --version         Show version

ENVIRONMENT:
    SKY_RCLONE            Same as --rclone
    RUST_LOG              Log filter, e.g. sky_core=debug
";

struct Args {
    command: String,
    rclone: Option<PathBuf>,
    no_keyring: bool,
    local_cloud: Option<PathBuf>,
}

fn parse_args() -> Result<Args> {
    let mut args = Args {
        command: "run".into(),
        rclone: None,
        no_keyring: false,
        local_cloud: None,
    };
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "-h" | "--help" => {
                print!("{HELP}");
                std::process::exit(0);
            }
            "-V" | "--version" => {
                println!("sky-daemon {}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            "--rclone" => args.rclone = Some(it.next().context("--rclone needs a path")?.into()),
            "--no-keyring" => args.no_keyring = true,
            "--local-cloud" => {
                args.local_cloud = Some(it.next().context("--local-cloud needs a folder")?.into())
            }
            "run" | "status" | "stop" | "sign-in" => args.command = a,
            other => bail!("unknown argument {other}\n\n{HELP}"),
        }
    }
    Ok(args)
}

fn secret_store(no_keyring: bool) -> (Arc<dyn SecretStore>, bool) {
    if !no_keyring && sky_core::secrets::KeyringSecretStore::available() {
        return (Arc::new(sky_core::secrets::KeyringSecretStore), true);
    }
    if !no_keyring {
        tracing::warn!(
            "no Secret Service found (GNOME Keyring, KWallet, KeePassXC); \
             Sky Linux's rclone config will not be encrypted at rest"
        );
    }
    (Arc::new(MemorySecretStore::default()), false)
}

async fn run(args: Args) -> Result<()> {
    let paths = AppPaths::from_env();
    paths.ensure().context("creating state directories")?;
    let listener = ipc::bind(&paths.daemon_socket())?;

    let rclone_binary = args
        .rclone
        .or_else(SupervisorConfig::find_binary)
        .context("rclone was not found. Reinstall Sky Linux, or pass --rclone /path/to/rclone")?;
    let (secrets, encrypt) = secret_store(args.no_keyring);
    let local_cloud = args.local_cloud.clone();

    tracing::info!(version = env!("CARGO_PKG_VERSION"), rclone = %rclone_binary.display(), "starting Sky daemon");
    let core = SkyCore::start(CoreOptions {
        paths: paths.clone(),
        rclone_binary,
        secrets,
        encrypt_rclone_config: encrypt,
    })
    .await?;
    if let Some(dir) = &local_cloud {
        if core.accounts.list()?.is_empty() {
            // Laid out like a Jottacloud device: one folder per storage area.
            let areas: Vec<String> = sky_core::areas::JOTTA_AREAS
                .iter()
                .map(|a| a.to_string())
                .collect();
            for a in &areas {
                std::fs::create_dir_all(dir.join(a))?;
            }
            let dir = dir.canonicalize()?;
            core.client()
                .create_remote(
                    "local-cloud",
                    "alias",
                    serde_json::json!({ "remote": dir.to_string_lossy() }),
                )
                .await?;
            core.accounts
                .register_areas(
                    sky_core::provider::Provider::TeliaSkyNo,
                    "local-cloud",
                    areas,
                    sky_core::areas::AreaLayout::Subfolders,
                )
                .await?;
            tracing::warn!(dir = %dir.display(), "developer mode: using a local folder as the cloud");
        }
    }
    tracing::info!(socket = %paths.daemon_socket().display(), "listening");

    let server = tokio::spawn(ipc::serve(listener, core.clone()));
    let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = term.recv() => {}
        r = server => { tracing::error!("IPC server stopped: {r:?}"); }
    }
    tracing::info!("shutting down");
    core.shutdown().await;
    let _ = std::fs::remove_file(paths.daemon_socket());
    Ok(())
}

/// Run the same browser sign-in as the desktop app, from a terminal.
async fn sign_in() -> Result<()> {
    let paths = AppPaths::from_env();
    let client = IpcClient::connect(&paths.daemon_socket())
        .await
        .context("the Sky daemon is not running (start it with `sky-daemon run`)")?;
    let mut events = client.events();
    tokio::spawn(async move {
        while let Ok(ev) = events.recv().await {
            if let sky_core::events::Event::AuthUrl { url } = ev {
                println!(
                    "Opening the sign-in page in your browser. If it doesn't open, visit:\n  {url}"
                );
                let _ = std::process::Command::new("xdg-open")
                    .arg(&url)
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn();
            }
        }
    });
    let req = Request::Connect {
        provider: sky_core::provider::Provider::TeliaSkyNo,
    };
    match client.request(&req).await {
        Ok(account) => {
            println!("Signed in: {}", serde_json::to_string_pretty(&account)?);
            Ok(())
        }
        Err(e) => bail!("{}: {}\n({})", e.title, e.message, e.technical),
    }
}

async fn status() -> Result<()> {
    let paths = AppPaths::from_env();
    let client = IpcClient::connect(&paths.daemon_socket())
        .await
        .context("the Sky daemon is not running")?;
    let s = client
        .request(&Request::Status)
        .await
        .map_err(|e| anyhow::anyhow!(e.message))?;
    println!("{}", serde_json::to_string_pretty(&s)?);
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,rclone=info".into()),
        )
        .with_target(false)
        .init();
    let args = parse_args()?;
    match args.command.as_str() {
        "status" => status().await,
        "sign-in" => sign_in().await,
        "stop" => {
            // Under systemd, let systemd stop us so it doesn't restart us.
            let st = std::process::Command::new("systemctl")
                .args(["--user", "stop", "sky-daemon.service"])
                .status();
            if !matches!(st, Ok(s) if s.success()) {
                bail!("could not stop via systemd; send SIGTERM to sky-daemon instead");
            }
            Ok(())
        }
        _ => run(args).await,
    }
}
