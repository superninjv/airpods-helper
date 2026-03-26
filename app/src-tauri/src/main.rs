// Prevents additional console window on Windows in release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod aap;
mod daemon;
mod models;
mod state;

use std::sync::Arc;
use tauri::{
    menu::{MenuBuilder, MenuItemBuilder, SubmenuBuilder},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager,
};
use tokio::sync::Mutex;

use state::{AirPodsState, CommandSender, DaemonCommand, SharedState};

/// Tauri command: get current AirPods status for the frontend
#[tauri::command]
async fn get_status(state: tauri::State<'_, SharedState>) -> Result<AirPodsState, String> {
    Ok(state.current())
}

/// Tauri command: set ANC mode
#[tauri::command]
async fn set_anc_mode(
    mode: String,
    state: tauri::State<'_, SharedState>,
    cmd_sender: tauri::State<'_, CommandSender>,
) -> Result<(), String> {
    let anc_mode = aap::AncMode::from_str(&mode).ok_or("invalid ANC mode")?;
    send_command(&cmd_sender, DaemonCommand::SetAncMode(anc_mode)).await?;
    state.update(|s| s.anc_mode = mode);
    Ok(())
}

/// Tauri command: set conversational awareness
#[tauri::command]
async fn set_conversational_awareness(
    enabled: bool,
    cmd_sender: tauri::State<'_, CommandSender>,
) -> Result<(), String> {
    send_command(
        &cmd_sender,
        DaemonCommand::SetConversationalAwareness(enabled),
    )
    .await
}

/// Tauri command: set adaptive noise level
#[tauri::command]
async fn set_adaptive_noise_level(
    level: u8,
    cmd_sender: tauri::State<'_, CommandSender>,
) -> Result<(), String> {
    send_command(&cmd_sender, DaemonCommand::SetAdaptiveNoiseLevel(level)).await
}

/// Tauri command: set one-bud ANC
#[tauri::command]
async fn set_one_bud_anc(
    enabled: bool,
    cmd_sender: tauri::State<'_, CommandSender>,
) -> Result<(), String> {
    send_command(&cmd_sender, DaemonCommand::SetOneBudAnc(enabled)).await
}

/// Tauri command: set volume swipe
#[tauri::command]
async fn set_volume_swipe(
    enabled: bool,
    cmd_sender: tauri::State<'_, CommandSender>,
) -> Result<(), String> {
    send_command(&cmd_sender, DaemonCommand::SetVolumeSwipe(enabled)).await
}

/// Tauri command: set EQ preset (stored locally, actual PipeWire EQ is separate)
#[tauri::command]
async fn set_eq_preset(
    preset: String,
    state: tauri::State<'_, SharedState>,
) -> Result<(), String> {
    state.update(|s| s.eq_preset = preset);
    Ok(())
}

/// Tauri command: set auto-reconnect preference
#[tauri::command]
async fn set_auto_reconnect(
    enabled: bool,
    state: tauri::State<'_, SharedState>,
) -> Result<(), String> {
    state.update(|s| s.auto_reconnect = enabled);
    Ok(())
}

/// Tauri command: set start-on-login preference
#[tauri::command]
async fn set_start_on_login(
    enabled: bool,
    state: tauri::State<'_, SharedState>,
) -> Result<(), String> {
    state.update(|s| s.start_on_login = enabled);
    Ok(())
}

/// Tauri command: disconnect from AirPods
#[tauri::command]
async fn disconnect(cmd_sender: tauri::State<'_, CommandSender>) -> Result<(), String> {
    send_command(&cmd_sender, DaemonCommand::Disconnect).await
}

/// Send a command to the daemon's L2CAP write loop
async fn send_command(cmd_sender: &CommandSender, cmd: DaemonCommand) -> Result<(), String> {
    let sender = cmd_sender.lock().await;
    if let Some(tx) = sender.as_ref() {
        tx.send(cmd)
            .await
            .map_err(|e| format!("failed to send command: {e}"))
    } else {
        Err("not connected".to_string())
    }
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "airpods_app=info".parse().unwrap()),
        )
        .init();

    let shared_state = state::create_shared_state();
    let cmd_sender: CommandSender = Arc::new(Mutex::new(None));

    let daemon_state = shared_state.clone();
    let daemon_cmd_sender = cmd_sender.clone();

    tauri::Builder::default()
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_notification::init())
        .manage(shared_state.clone())
        .manage(cmd_sender.clone())
        .invoke_handler(tauri::generate_handler![
            get_status,
            set_anc_mode,
            set_conversational_awareness,
            set_adaptive_noise_level,
            set_one_bud_anc,
            set_volume_swipe,
            set_eq_preset,
            set_auto_reconnect,
            set_start_on_login,
            disconnect,
        ])
        .setup(move |app| {
            // Build tray menu
            let anc_off = MenuItemBuilder::with_id("anc_off", "Off").build(app)?;
            let anc_noise = MenuItemBuilder::with_id("anc_noise", "Noise Cancellation").build(app)?;
            let anc_transparency =
                MenuItemBuilder::with_id("anc_transparency", "Transparency").build(app)?;
            let anc_adaptive =
                MenuItemBuilder::with_id("anc_adaptive", "Adaptive").build(app)?;

            let anc_submenu = SubmenuBuilder::with_id(app, "anc", "ANC Mode")
                .items(&[&anc_off, &anc_noise, &anc_transparency, &anc_adaptive])
                .build()?;

            let disconnect_item =
                MenuItemBuilder::with_id("disconnect", "Disconnect").build(app)?;
            let quit_item = MenuItemBuilder::with_id("quit", "Quit").build(app)?;

            let menu = MenuBuilder::new(app)
                .items(&[&anc_submenu, &disconnect_item, &quit_item])
                .build()?;

            let tray_state = shared_state.clone();
            let tray_cmd_sender = cmd_sender.clone();

            let _tray = TrayIconBuilder::new()
                .tooltip("AirPods: Disconnected")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(move |app, event| {
                    let cmd_sender = tray_cmd_sender.clone();
                    match event.id().as_ref() {
                        "anc_off" => {
                            let cs = cmd_sender.clone();
                            tauri::async_runtime::spawn(async move {
                                let _ = send_command(
                                    &cs,
                                    DaemonCommand::SetAncMode(aap::AncMode::Off),
                                )
                                .await;
                            });
                        }
                        "anc_noise" => {
                            let cs = cmd_sender.clone();
                            tauri::async_runtime::spawn(async move {
                                let _ = send_command(
                                    &cs,
                                    DaemonCommand::SetAncMode(aap::AncMode::NoiseCancellation),
                                )
                                .await;
                            });
                        }
                        "anc_transparency" => {
                            let cs = cmd_sender.clone();
                            tauri::async_runtime::spawn(async move {
                                let _ = send_command(
                                    &cs,
                                    DaemonCommand::SetAncMode(aap::AncMode::Transparency),
                                )
                                .await;
                            });
                        }
                        "anc_adaptive" => {
                            let cs = cmd_sender.clone();
                            tauri::async_runtime::spawn(async move {
                                let _ = send_command(
                                    &cs,
                                    DaemonCommand::SetAncMode(aap::AncMode::Adaptive),
                                )
                                .await;
                            });
                        }
                        "disconnect" => {
                            let cs = cmd_sender.clone();
                            tauri::async_runtime::spawn(async move {
                                let _ =
                                    send_command(&cs, DaemonCommand::Disconnect).await;
                            });
                        }
                        "quit" => {
                            app.exit(0);
                        }
                        _ => {}
                    }
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        if let Some(window) = app.get_webview_window("main") {
                            if window.is_visible().unwrap_or(false) {
                                let _ = window.hide();
                            } else {
                                let _ = window.show();
                                let _ = window.unminimize();
                                let _ = window.set_focus();
                            }
                        }
                    }
                })
                .build(app)?;

            // Start daemon in background
            tauri::async_runtime::spawn(async move {
                daemon::run(daemon_state, daemon_cmd_sender).await;
            });

            // Start tray tooltip updater
            let tooltip_state = tray_state.clone();
            let app_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                    let s = tooltip_state.current();
                    let tooltip = if s.connected {
                        let name = if s.model_name.is_empty() {
                            "AirPods".to_string()
                        } else {
                            s.model_name.clone()
                        };
                        if s.battery_left >= 0 && s.battery_right >= 0 {
                            format!("{name} -- L: {}% R: {}%", s.battery_left, s.battery_right)
                        } else {
                            format!("{name} -- Connected")
                        }
                    } else {
                        "AirPods: Disconnected".to_string()
                    };
                    if let Some(tray) = app_handle.tray_by_id("main") {
                        let _ = tray.set_tooltip(Some(&tooltip));
                    }
                }
            });

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
