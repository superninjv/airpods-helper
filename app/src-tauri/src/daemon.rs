use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use crate::aap;
use crate::aap::parser::{self, AapEvent, AudioSource};
use crate::models;
use crate::state::{CommandSender, DaemonCommand, SharedState};

/// Start the embedded daemon: monitor BlueZ for AirPods, connect via L2CAP,
/// and run the AAP protocol loop.
pub async fn run(state: SharedState, cmd_sender: CommandSender) {
    info!("daemon starting");

    loop {
        if let Err(e) = run_once(state.clone(), cmd_sender.clone()).await {
            error!("daemon error: {e}");
        }

        state.reset();

        let auto_reconnect = state.current().auto_reconnect;
        if !auto_reconnect {
            info!("auto-reconnect disabled, daemon stopping");
            break;
        }

        info!("will attempt reconnect in 5 seconds");
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    }
}

/// Platform-specific implementation: single connection cycle
#[cfg(target_os = "linux")]
async fn run_once(state: SharedState, cmd_sender: CommandSender) -> Result<(), String> {
    use bluer::Session;

    info!("waiting for AirPods connection via BlueZ");

    // Find AirPods
    let session = Session::new().await.map_err(|e| format!("BlueZ session: {e}"))?;
    let adapter = session
        .default_adapter()
        .await
        .map_err(|e| format!("BlueZ adapter: {e}"))?;
    info!("using BlueZ adapter: {}", adapter.name());

    let address = find_airpods(&adapter).await?;
    info!("found AirPods at {address}");

    // Connect L2CAP
    let seq = connect_l2cap(address).await?;
    info!("L2CAP connected, performing handshake");

    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    // Handshake
    seq.send(&aap::commands::HANDSHAKE)
        .await
        .map_err(|e| format!("handshake send: {e}"))?;

    let mut buf = vec![0u8; 1024];
    let n = seq
        .recv(&mut buf)
        .await
        .map_err(|e| format!("handshake recv: {e}"))?;
    match parser::parse(&buf[..n]) {
        Ok(AapEvent::HandshakeAck) => debug!("handshake ACK received"),
        Ok(other) => warn!("unexpected handshake response: {other:?}"),
        Err(e) => warn!("handshake parse error: {e}"),
    }

    seq.send(&aap::commands::SET_FEATURES)
        .await
        .map_err(|e| format!("features send: {e}"))?;

    let n = seq
        .recv(&mut buf)
        .await
        .map_err(|e| format!("features recv: {e}"))?;
    match parser::parse(&buf[..n]) {
        Ok(AapEvent::FeaturesAck) => debug!("features ACK received"),
        Ok(other) => warn!("unexpected features response: {other:?}"),
        Err(e) => warn!("features parse error: {e}"),
    }

    seq.send(&aap::commands::SUBSCRIBE_NOTIFICATIONS)
        .await
        .map_err(|e| format!("subscribe send: {e}"))?;

    // Enable all listening modes (Off + Noise + Transparency + Adaptive)
    seq.send(&aap::commands::ENABLE_ALL_LISTENING_MODES)
        .await
        .map_err(|e| format!("listening modes send: {e}"))?;
    debug!("enabled all listening modes");

    state.update(|s| s.connected = true);
    info!("handshake complete, entering main loop");

    // Create command channel
    let (cmd_tx, mut cmd_rx) = mpsc::channel::<DaemonCommand>(32);
    {
        let mut sender = cmd_sender.lock().await;
        *sender = Some(cmd_tx);
    }

    // Main read/write loop
    loop {
        tokio::select! {
            result = seq.recv(&mut buf) => {
                match result {
                    Ok(0) => {
                        info!("L2CAP connection closed by remote");
                        break;
                    }
                    Ok(n) => {
                        match parser::parse(&buf[..n]) {
                            Ok(AapEvent::Disconnected) => {
                                info!("AirPods sent disconnect packet");
                                break;
                            }
                            Ok(event) => apply_event(&state, &event),
                            Err(e) => debug!("parse error (non-fatal): {e}"),
                        }
                    }
                    Err(e) => {
                        error!("L2CAP recv error: {e}");
                        break;
                    }
                }
            }
            cmd = cmd_rx.recv() => {
                match cmd {
                    Some(DaemonCommand::SetAncMode(mode)) => {
                        let pkt = aap::commands::set_anc_mode(mode);
                        if let Err(e) = seq.send(&pkt).await {
                            error!("failed to send ANC command: {e}");
                        }
                    }
                    Some(DaemonCommand::SetConversationalAwareness(enabled)) => {
                        let pkt = aap::commands::set_conversational_awareness(enabled);
                        if let Err(e) = seq.send(&pkt).await {
                            error!("failed to send CA command: {e}");
                        }
                    }
                    Some(DaemonCommand::SetAdaptiveNoiseLevel(level)) => {
                        let pkt = aap::commands::set_adaptive_noise_level(level);
                        if let Err(e) = seq.send(&pkt).await {
                            error!("failed to send adaptive noise level command: {e}");
                        }
                    }
                    Some(DaemonCommand::SetOneBudAnc(enabled)) => {
                        let pkt = aap::commands::set_one_bud_anc(enabled);
                        if let Err(e) = seq.send(&pkt).await {
                            error!("failed to send one-bud ANC command: {e}");
                        }
                    }
                    Some(DaemonCommand::SetVolumeSwipe(enabled)) => {
                        let pkt = aap::commands::set_volume_swipe(enabled);
                        if let Err(e) = seq.send(&pkt).await {
                            error!("failed to send volume swipe command: {e}");
                        }
                    }
                    Some(DaemonCommand::Disconnect) | None => {
                        info!("disconnect requested");
                        break;
                    }
                }
            }
        }
    }

    // Clear command sender on disconnect
    {
        let mut sender = cmd_sender.lock().await;
        *sender = None;
    }

    state.reset();
    info!("L2CAP disconnected");
    Ok(())
}

/// Find AirPods via BlueZ -- check already-connected devices, then wait for connection
#[cfg(target_os = "linux")]
async fn find_airpods(adapter: &bluer::Adapter) -> Result<bluer::Address, String> {
    use bluer::AdapterEvent;
    use futures::StreamExt;

    // Check already-connected devices
    let addrs = adapter
        .device_addresses()
        .await
        .map_err(|e| format!("device addresses: {e}"))?;
    for addr in addrs {
        if let Ok(device) = adapter.device(addr) {
            if device.is_connected().await.unwrap_or(false) && is_airpods(&device).await {
                return Ok(addr);
            }
        }
    }

    // Wait for AirPods to connect
    info!("no AirPods found, waiting for connection...");
    let mut events = adapter
        .discover_devices_with_changes()
        .await
        .map_err(|e| format!("discover: {e}"))?;

    while let Some(event) = events.next().await {
        if let AdapterEvent::DeviceAdded(addr) = event {
            if let Ok(device) = adapter.device(addr) {
                if device.is_connected().await.unwrap_or(false) && is_airpods(&device).await {
                    return Ok(addr);
                }
            }
        }
    }

    Err("BlueZ event stream ended without finding AirPods".to_string())
}

#[cfg(target_os = "linux")]
async fn is_airpods(device: &bluer::Device) -> bool {
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

#[cfg(target_os = "linux")]
async fn connect_l2cap(
    address: bluer::Address,
) -> Result<bluer::l2cap::SeqPacket, String> {
    use bluer::l2cap::{Socket, SocketAddr};

    for attempt in 1..=5 {
        let socket = Socket::new_seq_packet().map_err(|e| format!("socket: {e}"))?;
        let addr = SocketAddr::new(address, bluer::AddressType::BrEdr, aap::AAP_PSM.into());
        match socket.connect(addr).await {
            Ok(s) => return Ok(s),
            Err(e) => {
                warn!("L2CAP connect attempt {attempt}/5 failed: {e}");
                if attempt < 5 {
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                }
            }
        }
    }
    Err("L2CAP connect failed after 5 attempts".to_string())
}

/// Windows stub -- polls the standalone airpods-windows daemon's HTTP API for state.
///
/// The Tauri app on Windows does not embed its own L2CAP connection. Instead, it expects
/// the standalone `airpods-windows daemon` to be running on localhost:7654. This stub
/// polls the daemon's HTTP API and mirrors state into the Tauri shared state so the
/// frontend UI works identically.
///
/// To use the Tauri app on Windows:
///   1. Start the daemon: `airpods-windows daemon`
///   2. Launch the Tauri app -- it will poll the daemon's HTTP API automatically
#[cfg(target_os = "windows")]
async fn run_once(state: SharedState, cmd_sender: CommandSender) -> Result<(), String> {
    use crate::aap::AncMode;

    const API_BASE: &str = "http://127.0.0.1:7654";
    let client = reqwest::Client::new();

    info!("Windows mode: polling airpods-windows daemon at {API_BASE}");
    info!("make sure `airpods-windows daemon` is running");

    // Create command channel -- forward commands to the HTTP API
    let (cmd_tx, mut cmd_rx) = mpsc::channel::<DaemonCommand>(32);
    {
        let mut sender = cmd_sender.lock().await;
        *sender = Some(cmd_tx);
    }

    loop {
        tokio::select! {
            _ = tokio::time::sleep(std::time::Duration::from_secs(1)) => {
                // Poll status from the standalone daemon
                match client.get(format!("{API_BASE}/status")).send().await {
                    Ok(resp) => {
                        if let Ok(json) = resp.json::<serde_json::Value>().await {
                            let connected = json["connected"].as_bool().unwrap_or(false);
                            state.update(|s| {
                                s.connected = connected;
                                if connected {
                                    s.battery_left = json["battery_left"].as_i64().unwrap_or(-1) as i32;
                                    s.battery_right = json["battery_right"].as_i64().unwrap_or(-1) as i32;
                                    s.battery_case = json["battery_case"].as_i64().unwrap_or(-1) as i32;
                                    s.charging_left = json["charging_left"].as_bool().unwrap_or(false);
                                    s.charging_right = json["charging_right"].as_bool().unwrap_or(false);
                                    s.charging_case = json["charging_case"].as_bool().unwrap_or(false);
                                    s.anc_mode = json["anc_mode"].as_str().unwrap_or("off").to_string();
                                    s.ear_left = json["ear_left"].as_bool().unwrap_or(false);
                                    s.ear_right = json["ear_right"].as_bool().unwrap_or(false);
                                    s.conversational_awareness = json["conversational_awareness"].as_bool().unwrap_or(false);
                                    s.adaptive_noise_level = json["adaptive_noise_level"].as_u64().unwrap_or(50) as u8;
                                    s.one_bud_anc = json["one_bud_anc"].as_bool().unwrap_or(true);
                                    s.volume_swipe = json["volume_swipe"].as_bool().unwrap_or(true);
                                    if let Some(model) = json["model"].as_str() {
                                        s.model = model.to_string();
                                    }
                                    if let Some(model_name) = json["model_name"].as_str() {
                                        s.model_name = model_name.to_string();
                                    }
                                    if let Some(firmware) = json["firmware"].as_str() {
                                        s.firmware = firmware.to_string();
                                    }
                                    if let Some(audio_source) = json["audio_source"].as_str() {
                                        s.audio_source = audio_source.to_string();
                                    }
                                }
                            });
                        }
                    }
                    Err(_) => {
                        // Daemon not running -- mark disconnected
                        if state.current().connected {
                            state.update(|s| s.connected = false);
                            warn!("lost connection to airpods-windows daemon");
                        }
                    }
                }
            }
            cmd = cmd_rx.recv() => {
                // Forward commands to the daemon's HTTP API
                match cmd {
                    Some(DaemonCommand::SetAncMode(mode)) => {
                        let _ = client.post(format!("{API_BASE}/anc"))
                            .json(&serde_json::json!({ "mode": mode.as_str() }))
                            .send().await;
                    }
                    Some(DaemonCommand::SetConversationalAwareness(enabled)) => {
                        let _ = client.post(format!("{API_BASE}/ca"))
                            .json(&serde_json::json!({ "enabled": enabled }))
                            .send().await;
                    }
                    Some(DaemonCommand::SetAdaptiveNoiseLevel(level)) => {
                        let _ = client.post(format!("{API_BASE}/noise"))
                            .json(&serde_json::json!({ "level": level }))
                            .send().await;
                    }
                    Some(DaemonCommand::SetOneBudAnc(enabled)) => {
                        let _ = client.post(format!("{API_BASE}/one-bud-anc"))
                            .json(&serde_json::json!({ "enabled": enabled }))
                            .send().await;
                    }
                    Some(DaemonCommand::SetVolumeSwipe(enabled)) => {
                        let _ = client.post(format!("{API_BASE}/volume-swipe"))
                            .json(&serde_json::json!({ "enabled": enabled }))
                            .send().await;
                    }
                    Some(DaemonCommand::Disconnect) | None => {
                        info!("disconnect requested");
                        break;
                    }
                }
            }
        }
    }

    {
        let mut sender = cmd_sender.lock().await;
        *sender = None;
    }
    state.reset();
    Ok(())
}

/// Apply a parsed AAP event to the shared state
fn apply_event(state: &SharedState, event: &AapEvent) {
    match event {
        AapEvent::Battery(b) => {
            state.update(|s| {
                if let Some(left) = &b.left {
                    s.battery_left = left.level as i32;
                    s.charging_left = left.charging;
                }
                if let Some(right) = &b.right {
                    s.battery_right = right.level as i32;
                    s.charging_right = right.charging;
                }
                if let Some(case) = &b.case {
                    if case.level > 0 || case.charging {
                        s.battery_case = case.level as i32;
                        s.charging_case = case.charging;
                    }
                }
            });
        }
        AapEvent::AncMode(mode) => {
            let mode_str = mode.as_str().to_string();
            state.update(|s| s.anc_mode = mode_str);
        }
        AapEvent::EarDetection(ed) => {
            // AAP primary = right bud (controller), secondary = left
            state.update(|s| {
                s.ear_left = ed.secondary.is_in_ear();
                s.ear_right = ed.primary.is_in_ear();
            });
        }
        AapEvent::ConversationalAwareness(enabled) => {
            state.update(|s| s.conversational_awareness = *enabled);
        }
        AapEvent::AdaptiveNoiseLevel(level) => {
            state.update(|s| s.adaptive_noise_level = *level);
        }
        AapEvent::OneBudAnc(enabled) => {
            state.update(|s| s.one_bud_anc = *enabled);
        }
        AapEvent::VolumeSwipe(enabled) => {
            state.update(|s| s.volume_swipe = *enabled);
        }
        AapEvent::AdaptiveVolume(enabled) => {
            state.update(|s| s.adaptive_volume = *enabled);
        }
        AapEvent::ChimeVolume(level) => {
            state.update(|s| s.chime_volume = *level);
        }
        AapEvent::AudioSource(source) => {
            let value = match source {
                AudioSource::None => "none",
                AudioSource::Call => "call",
                AudioSource::Media => "media",
                AudioSource::Unknown(_) => "unknown",
            };
            state.update(|s| s.audio_source = value.to_string());
        }
        AapEvent::DeviceInfo(info) => {
            let display_name = models::model_display_name(&info.model).to_string();
            state.update(|s| {
                s.model = info.model.clone();
                s.model_name = display_name;
                s.firmware = info.firmware.clone();
            });
        }
        _ => {}
    }
}
