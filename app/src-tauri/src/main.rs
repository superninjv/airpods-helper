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

/// Tauri command: set primary microphone bud (auto/right/left)
#[tauri::command]
async fn set_mic_mode(
    mode: String,
    cmd_sender: tauri::State<'_, CommandSender>,
) -> Result<(), String> {
    let mic_mode = aap::MicMode::from_str(&mode).ok_or("invalid mic mode (use auto, right, left)")?;
    send_command(&cmd_sender, DaemonCommand::SetMicMode(mic_mode)).await
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

/// Tauri command: disconnect from AirPods.
///
/// Closes the L2CAP session AND issues a BlueZ-level disconnect so the device
/// is fully detached (otherwise the AirPods can auto-reconnect on case open and
/// the user wouldn't see a real disconnect).
#[tauri::command]
async fn disconnect(cmd_sender: tauri::State<'_, CommandSender>) -> Result<(), String> {
    // Tell the embedded daemon to break out of its L2CAP loop first
    let _ = send_command(&cmd_sender, DaemonCommand::Disconnect).await;
    // Then disconnect at the BlueZ level (Linux); Windows path is a no-op for now
    #[cfg(target_os = "linux")]
    {
        if let Some(addr) = bluez_currently_connected_airpods().await {
            bluez_disconnect(addr).await?;
        }
    }
    Ok(())
}

/// Tauri command: connect to a paired AirPods by MAC address.
/// Once BlueZ reports the device connected, the embedded daemon's loop picks it
/// up automatically and runs the AAP handshake.
#[tauri::command]
async fn connect(address: String) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        let addr: bluer::Address = address
            .parse()
            .map_err(|e| format!("invalid MAC '{address}': {e}"))?;
        bluez_connect(addr).await?;
        Ok(())
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = address;
        Err("connect not supported on this platform yet".to_string())
    }
}

/// Tauri command: list paired AirPods known to BlueZ.
#[tauri::command]
async fn list_paired() -> Result<Vec<PairedDevice>, String> {
    #[cfg(target_os = "linux")]
    {
        bluez_list_paired_airpods().await
    }
    #[cfg(not(target_os = "linux"))]
    {
        Ok(Vec::new())
    }
}

/// Tauri command: pair (and trust) a new AirPods by MAC address.
/// Registers a transient just-works agent for the attempt; AirPods must be in
/// pairing mode (case open, status light flashing white).
#[tauri::command]
async fn pair(address: String) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        let addr: bluer::Address = address
            .parse()
            .map_err(|e| format!("invalid MAC '{address}': {e}"))?;
        bluez_pair_and_trust(addr).await
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = address;
        Err("pair not supported on this platform yet".to_string())
    }
}

#[derive(serde::Serialize)]
struct PairedDevice {
    address: String,
    name: String,
    connected: bool,
}

#[cfg(target_os = "linux")]
async fn bluez_connect(address: bluer::Address) -> Result<(), String> {
    let session = bluer::Session::new()
        .await
        .map_err(|e| format!("BlueZ session: {e}"))?;
    let adapter = session
        .default_adapter()
        .await
        .map_err(|e| format!("BlueZ adapter: {e}"))?;
    let device = adapter
        .device(address)
        .map_err(|e| format!("BlueZ device {address}: {e}"))?;
    device
        .connect()
        .await
        .map_err(|e| format!("BlueZ connect: {e}"))?;
    Ok(())
}

#[cfg(target_os = "linux")]
async fn bluez_disconnect(address: bluer::Address) -> Result<(), String> {
    let session = bluer::Session::new()
        .await
        .map_err(|e| format!("BlueZ session: {e}"))?;
    let adapter = session
        .default_adapter()
        .await
        .map_err(|e| format!("BlueZ adapter: {e}"))?;
    let device = adapter
        .device(address)
        .map_err(|e| format!("BlueZ device {address}: {e}"))?;
    device
        .disconnect()
        .await
        .map_err(|e| format!("BlueZ disconnect: {e}"))?;
    Ok(())
}

#[cfg(target_os = "linux")]
async fn bluez_currently_connected_airpods() -> Option<bluer::Address> {
    let session = bluer::Session::new().await.ok()?;
    let adapter = session.default_adapter().await.ok()?;
    for addr in adapter.device_addresses().await.ok()? {
        if let Ok(device) = adapter.device(addr) {
            if device.is_connected().await.unwrap_or(false) && bluez_is_airpods(&device).await {
                return Some(addr);
            }
        }
    }
    None
}

#[cfg(target_os = "linux")]
async fn bluez_list_paired_airpods() -> Result<Vec<PairedDevice>, String> {
    let session = bluer::Session::new()
        .await
        .map_err(|e| format!("BlueZ session: {e}"))?;
    let adapter = session
        .default_adapter()
        .await
        .map_err(|e| format!("BlueZ adapter: {e}"))?;
    let addrs = adapter
        .device_addresses()
        .await
        .map_err(|e| format!("device addresses: {e}"))?;
    let mut out = Vec::new();
    for addr in addrs {
        if let Ok(device) = adapter.device(addr) {
            let paired = device.is_paired().await.unwrap_or(false);
            if paired && bluez_is_airpods(&device).await {
                let name = device
                    .name()
                    .await
                    .ok()
                    .flatten()
                    .unwrap_or_else(|| "AirPods".to_string());
                let connected = device.is_connected().await.unwrap_or(false);
                out.push(PairedDevice {
                    address: addr.to_string(),
                    name,
                    connected,
                });
            }
        }
    }
    Ok(out)
}

#[cfg(target_os = "linux")]
async fn bluez_pair_and_trust(address: bluer::Address) -> Result<(), String> {
    use std::time::Duration;

    let session = bluer::Session::new()
        .await
        .map_err(|e| format!("BlueZ session: {e}"))?;

    let agent = bluer::agent::Agent::default();
    let _agent_handle = session
        .register_agent(agent)
        .await
        .map_err(|e| format!("register agent: {e}"))?;

    let adapter = session
        .default_adapter()
        .await
        .map_err(|e| format!("BlueZ adapter: {e}"))?;
    adapter
        .set_powered(true)
        .await
        .map_err(|e| format!("power on: {e}"))?;
    let _ = adapter.set_pairable(true).await;

    let mut _discovery_stream = None;
    let known = adapter.device_addresses().await.unwrap_or_default();
    if !known.contains(&address) {
        _discovery_stream = Some(
            adapter
                .discover_devices()
                .await
                .map_err(|e| format!("discover: {e}"))?,
        );
        let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
        loop {
            if adapter
                .device_addresses()
                .await
                .unwrap_or_default()
                .contains(&address)
            {
                break;
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(format!(
                    "device {address} not seen within 20s — make sure the case is open and the status light is flashing white"
                ));
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    let device = adapter
        .device(address)
        .map_err(|e| format!("device: {e}"))?;
    device.pair().await.map_err(|e| format!("pair: {e}"))?;
    device
        .set_trusted(true)
        .await
        .map_err(|e| format!("set trusted: {e}"))?;
    Ok(())
}

#[cfg(target_os = "linux")]
async fn bluez_is_airpods(device: &bluer::Device) -> bool {
    if let Ok(Some(uuids)) = device.uuids().await {
        for uuid in &uuids {
            if uuid.to_string() == aap::AIRPODS_SERVICE_UUID {
                return true;
            }
        }
    }
    if let Ok(Some(name)) = device.name().await {
        if name.contains("AirPods") {
            return true;
        }
    }
    false
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
            set_mic_mode,
            set_eq_preset,
            set_auto_reconnect,
            set_start_on_login,
            disconnect,
            connect,
            list_paired,
            pair,
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
