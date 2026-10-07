// Prevents additional console window on Windows in release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! AirPods Helper desktop app.
//!
//! The app is a UI only: on Linux it is a client of `airpods-daemon` over
//! D-Bus (`org.costa.AirPods`), on Windows of `airpods-windows daemon` over
//! HTTP. State flows backend → [`status::Store`] → `status` event → webview;
//! actions flow webview → Tauri command → backend.

mod status;
mod tray;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::Client as Backend;

#[cfg(target_os = "windows")]
mod models;
#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
pub use windows::Client as Backend;

use std::sync::Arc;
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager, Runtime, State, WindowEvent};
use tauri_plugin_autostart::ManagerExt;

use status::{
    EqPresetDetail, EqPresetInfo, PairedDevice, QuickPairCandidate, SharedStore, Status, Store,
};

/// Argument passed by the login autostart entry: start hidden in the tray.
const MINIMIZED_ARG: &str = "--minimized";

type B<'a> = State<'a, Arc<Backend>>;

// ── Status ────────────────────────────────────────────────────────────────

#[tauri::command]
fn get_status(store: State<'_, SharedStore>) -> Status {
    store.current()
}

#[tauri::command]
async fn retry_daemon(backend: B<'_>) -> Result<(), String> {
    backend.retry().await
}

// ── Controls ──────────────────────────────────────────────────────────────

#[tauri::command]
async fn set_anc_mode(backend: B<'_>, mode: String) -> Result<(), String> {
    backend.set_anc_mode(mode).await
}

#[tauri::command]
async fn set_adaptive_noise_level(backend: B<'_>, level: u8) -> Result<(), String> {
    backend.set_adaptive_noise_level(level).await
}

#[tauri::command]
async fn set_conversational_awareness(backend: B<'_>, enabled: bool) -> Result<(), String> {
    backend.set_conversational_awareness(enabled).await
}

#[tauri::command]
async fn set_one_bud_anc(backend: B<'_>, enabled: bool) -> Result<(), String> {
    backend.set_one_bud_anc(enabled).await
}

#[tauri::command]
async fn set_volume_swipe(backend: B<'_>, enabled: bool) -> Result<(), String> {
    backend.set_volume_swipe(enabled).await
}

#[tauri::command]
async fn set_mic_mode(backend: B<'_>, mode: String) -> Result<(), String> {
    backend.set_mic_mode(mode).await
}

// ── Settings ──────────────────────────────────────────────────────────────

/// Write one of the daemon's read-write settings properties.
/// `key` is the snake_case status field (`pause_on_removal`, `resume_on_insert`,
/// `auto_reconnect`, `preferred_device`, `eq_auto_load`, `mic_source`).
#[tauri::command]
async fn set_setting(backend: B<'_>, key: String, value: serde_json::Value) -> Result<(), String> {
    backend.set_setting(&key, value).await
}

/// App-local: register/unregister the login autostart entry.
#[tauri::command]
fn set_start_on_login(
    app: AppHandle,
    store: State<'_, SharedStore>,
    enabled: bool,
) -> Result<(), String> {
    let autolaunch = app.autolaunch();
    let result = if enabled {
        autolaunch.enable()
    } else {
        autolaunch.disable()
    };
    let actual = autolaunch.is_enabled().unwrap_or(false);
    store.update(|s| s.start_on_login = actual);
    result.map_err(|e| format!("Could not update the login item: {e}"))
}

// ── Devices ───────────────────────────────────────────────────────────────

#[tauri::command]
async fn list_paired(backend: B<'_>) -> Result<Vec<PairedDevice>, String> {
    backend.list_paired().await
}

#[tauri::command]
async fn connect_device(backend: B<'_>, address: String) -> Result<(), String> {
    backend.connect(address).await
}

#[tauri::command]
async fn disconnect_device(backend: B<'_>) -> Result<(), String> {
    backend.disconnect().await
}

#[tauri::command]
async fn reconnect(backend: B<'_>) -> Result<(), String> {
    backend.reconnect().await
}

#[tauri::command]
async fn pair_device(backend: B<'_>, address: String) -> Result<(), String> {
    backend.pair(address).await
}

#[tauri::command]
async fn quick_pair_scan(backend: B<'_>, seconds: u32) -> Result<Vec<QuickPairCandidate>, String> {
    backend.quick_pair_scan(seconds).await
}

// ── EQ ────────────────────────────────────────────────────────────────────

#[tauri::command]
async fn refresh_eq_presets(backend: B<'_>) -> Result<Vec<EqPresetInfo>, String> {
    backend.refresh_presets().await
}

#[tauri::command]
async fn get_eq_preset(backend: B<'_>, id: String) -> Result<EqPresetDetail, String> {
    backend.get_eq_preset(id).await
}

/// Select a preset by id; an empty id disables EQ.
#[tauri::command]
async fn set_eq_preset(backend: B<'_>, id: String) -> Result<(), String> {
    backend.set_eq_preset(id).await
}

#[tauri::command]
async fn save_eq_preset(backend: B<'_>, preset: EqPresetDetail) -> Result<(), String> {
    backend.save_eq_preset(preset).await
}

#[tauri::command]
async fn delete_eq_preset(backend: B<'_>, id: String) -> Result<(), String> {
    backend.delete_eq_preset(id).await
}

// ── Window helpers ────────────────────────────────────────────────────────

fn show_main_window<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

pub fn toggle_main_window<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window("main") {
        if window.is_visible().unwrap_or(false) {
            let _ = window.hide();
        } else {
            show_main_window(app);
        }
    }
}

/// Forward every status change to the webview and the tray. Bursts of
/// PropertiesChanged (the daemon emits one signal per property) are coalesced.
fn spawn_status_pump<R: Runtime>(app: AppHandle<R>, store: SharedStore) {
    let mut rx = store.subscribe();
    tauri::async_runtime::spawn(async move {
        loop {
            let snapshot = rx.borrow_and_update().clone();
            if let Err(e) = app.emit("status", &snapshot) {
                tracing::warn!("failed to emit status: {e}");
            }
            tray::sync(&app, &snapshot);
            if rx.changed().await.is_err() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
    });
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "airpods_app=info".into()),
        )
        .init();

    let store = Store::new();
    let backend = Backend::new(store.clone());
    let start_hidden = std::env::args().any(|a| a == MINIMIZED_ARG);

    tauri::Builder::default()
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec![MINIMIZED_ARG]),
        ))
        .manage(store.clone())
        .manage(backend.clone())
        .manage(tray::TrayCache::default())
        .invoke_handler(tauri::generate_handler![
            get_status,
            retry_daemon,
            set_anc_mode,
            set_adaptive_noise_level,
            set_conversational_awareness,
            set_one_bud_anc,
            set_volume_swipe,
            set_mic_mode,
            set_setting,
            set_start_on_login,
            list_paired,
            connect_device,
            disconnect_device,
            reconnect,
            pair_device,
            quick_pair_scan,
            refresh_eq_presets,
            get_eq_preset,
            set_eq_preset,
            save_eq_preset,
            delete_eq_preset,
        ])
        .setup(move |app| {
            let enabled = app.autolaunch().is_enabled().unwrap_or(false);
            store.update(|s| s.start_on_login = enabled);

            tray::create(app.handle())?;
            spawn_status_pump(app.handle().clone(), store.clone());
            backend.start();

            if !start_hidden {
                show_main_window(app.handle());
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the window keeps the app in the tray; "Quit" exits.
            if let WindowEvent::CloseRequested { api, .. } = event
                && window.label() == "main"
            {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
