//! Sky Linux desktop shell.
//!
//! The window is only a view: all work happens in `sky-daemon`. This crate
//! (1) makes sure the daemon is running, (2) forwards UI requests to it over
//! the local unix socket, (3) relays daemon events to the webview, and
//! (4) provides the tray icon. Closing the window hides it; backups continue.

use std::path::PathBuf;
use std::time::Duration;

use serde_json::Value;
use sky_core::ipc::{IpcClient, IpcResult};
use sky_core::paths::AppPaths;
use sky_core::UserError;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, State, WindowEvent};
use tokio::sync::Mutex;

#[derive(Default)]
struct Daemon(Mutex<Option<IpcClient>>);

/// Tray menu items, kept so their labels can follow the UI language.
struct TrayItems {
    open: MenuItem<tauri::Wry>,
    quit: MenuItem<tauri::Wry>,
}

fn tray_labels(locale: &str) -> (&'static str, &'static str) {
    match locale {
        "en" => ("Open Sky Linux", "Quit window (backups keep running)"),
        // Norwegian Bokmål is the default language.
        _ => (
            "Åpne Sky Linux",
            "Avslutt vinduet (sikkerhetskopiene fortsetter)",
        ),
    }
}

#[tauri::command]
fn set_tray_language(items: State<'_, TrayItems>, locale: String) -> Result<(), String> {
    let (open, quit) = tray_labels(&locale);
    items.open.set_text(open).map_err(|e| e.to_string())?;
    items.quit.set_text(quit).map_err(|e| e.to_string())?;
    Ok(())
}

fn daemon_binary() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let c = dir.join("sky-daemon");
            if c.is_file() {
                return c;
            }
        }
    }
    PathBuf::from("sky-daemon")
}

/// Connect to the daemon, starting it (via systemd if installed, otherwise
/// directly) when it isn't running.
async fn connect(app: &AppHandle) -> Result<IpcClient, Box<UserError>> {
    let sock = AppPaths::from_env().daemon_socket();
    if let Ok(c) = IpcClient::connect(&sock).await {
        return Ok(c);
    }
    let via_systemd = std::process::Command::new("systemctl")
        .args(["--user", "start", "sky-daemon.service"])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !via_systemd {
        tracing::info!("starting sky-daemon directly");
        let _ = std::process::Command::new(daemon_binary())
            .stdin(std::process::Stdio::null())
            .spawn();
    }
    for _ in 0..50 {
        tokio::time::sleep(Duration::from_millis(200)).await;
        if let Ok(c) = IpcClient::connect(&sock).await {
            return Ok(c);
        }
    }
    let _ = app;
    Err(Box::new(
        sky_core::CoreError::Ipc("the Sky background service did not start".into()).to_user(),
    ))
}

async fn client(app: &AppHandle, state: &Daemon) -> Result<IpcClient, Box<UserError>> {
    let mut guard = state.0.lock().await;
    if let Some(c) = guard.as_ref() {
        return Ok(c.clone());
    }
    let c = connect(app).await?;
    let mut events = c.events();
    let handle = app.clone();
    tokio::spawn(async move {
        while let Ok(ev) = events.recv().await {
            let _ = handle.emit("sky://event", &ev);
        }
    });
    *guard = Some(c.clone());
    Ok(c)
}

#[tauri::command]
async fn daemon_request(
    app: AppHandle,
    state: State<'_, Daemon>,
    method: String,
    params: Option<Value>,
) -> IpcResult {
    let c = client(&app, &state).await?;
    match c
        .request_raw(&method, params.clone().unwrap_or(Value::Null))
        .await
    {
        Err(e) if e.code == "engine_unavailable" => {
            // Daemon restarted: reconnect once and retry.
            *state.0.lock().await = None;
            let c = client(&app, &state).await?;
            c.request_raw(&method, params.unwrap_or(Value::Null)).await
        }
        r => r,
    }
}

fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

pub fn run() {
    tracing_subscriber::fmt().with_env_filter("info").init();
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            show_main(app)
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--hidden"]),
        ))
        .manage(Daemon::default())
        .invoke_handler(tauri::generate_handler![daemon_request, set_tray_language])
        .setup(|app| {
            let (open_label, quit_label) = tray_labels("nb");
            let open = MenuItem::with_id(app, "open", open_label, true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", quit_label, true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&open, &quit])?;
            TrayIconBuilder::with_id("sky")
                .icon(app.default_window_icon().cloned().expect("icon configured"))
                .tooltip("Sky Linux")
                .menu(&menu)
                .on_menu_event(|app, e| match e.id.as_ref() {
                    "open" => show_main(app),
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, e| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        ..
                    } = e
                    {
                        show_main(tray.app_handle());
                    }
                })
                .build(app)?;
            app.manage(TrayItems { open, quit });
            if std::env::args().any(|a| a == "--hidden") {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.hide();
                }
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the window keeps Sky Linux in the tray.
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running Sky Linux");
}
