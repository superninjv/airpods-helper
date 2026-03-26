mod aap;
mod bluez;
mod config;
mod dbus;
mod eq;
mod l2cap;
mod models;
mod mpris;
mod state;

use std::sync::Arc;
use bluer::Address;
use tokio::sync::{mpsc, Mutex};
use tracing::{error, info, warn};

use crate::aap::parser::AapEvent;
use crate::bluez::BlueZEvent;
use crate::config::Config;
use crate::dbus::SharedCmdTx;
use crate::eq::EqManager;
use crate::state::create_shared_state;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("airpods_daemon=info".parse().unwrap()),
        )
        .init();

    info!("airpods-daemon starting");

    let config = Config::load();
    let state = create_shared_state();

    // Shared L2CAP command sender (swapped per session)
    let cmd_tx: SharedCmdTx = Arc::new(Mutex::new(None));

    // Channel for AAP events from L2CAP reader
    let (event_tx, mut event_rx) = mpsc::channel::<AapEvent>(64);

    // Channel for BlueZ events
    let (bluez_tx, mut bluez_rx) = mpsc::channel::<BlueZEvent>(16);

    // Channel for D-Bus reconnect requests
    let (reconnect_tx, mut reconnect_rx) = mpsc::channel::<()>(4);

    // Channel for EQ commands from D-Bus
    let (eq_tx, mut eq_rx) = mpsc::channel::<eq::EqCommand>(8);

    // EQ manager
    let mut eq_manager = EqManager::new();

    // Start D-Bus service
    let connection = dbus::serve(state.clone(), cmd_tx.clone(), reconnect_tx, eq_tx).await?;
    info!("D-Bus service ready");

    // Start MPRIS ear detection watcher
    let mpris_state = state.clone();
    tokio::spawn(async move {
        let rx = mpris_state.subscribe();
        mpris::watch_ear_detection(rx).await;
    });

    // Start BlueZ monitor
    tokio::spawn(async move {
        if let Err(e) = bluez::monitor(bluez_tx).await {
            error!("BlueZ monitor error: {e}");
        }
    });

    // Main event loop: handle BlueZ connect/disconnect and AAP events
    let mut l2cap_handle: Option<tokio::task::JoinHandle<()>> = None;
    let mut reconnect_handle: Option<tokio::task::JoinHandle<()>> = None;
    let mut last_address: Option<Address> = None;

    info!("waiting for AirPods connection...");

    loop {
        tokio::select! {
            // BlueZ events
            Some(event) = bluez_rx.recv() => {
                match event {
                    BlueZEvent::AirPodsConnected(addr) => {
                        info!("AirPods detected at {addr}, establishing AAP connection");

                        // Cancel any pending reconnect task
                        if let Some(handle) = reconnect_handle.take() {
                            handle.abort();
                            info!("cancelled pending reconnect task");
                        }

                        // Store last known address
                        last_address = Some(addr);

                        // Abort any existing L2CAP connection
                        if let Some(handle) = l2cap_handle.take() {
                            handle.abort();
                        }

                        let state_clone = state.clone();
                        let event_tx_clone = event_tx.clone();
                        let cmd_tx_clone = cmd_tx.clone();
                        let (session_tx, session_rx) = mpsc::channel(32);

                        // Store the new session's sender so D-Bus can reach it
                        *cmd_tx.lock().await = Some(session_tx);

                        l2cap_handle = Some(tokio::spawn(async move {
                            match l2cap::run(addr, state_clone, session_rx, event_tx_clone).await {
                                Ok(()) => info!("L2CAP session ended cleanly"),
                                Err(e) => error!("L2CAP session error: {e}"),
                            }
                            // Clear the sender when session ends
                            *cmd_tx_clone.lock().await = None;
                        }));
                    }
                    BlueZEvent::AirPodsDisconnected(addr) => {
                        info!("AirPods disconnected: {addr}");
                        if let Some(handle) = l2cap_handle.take() {
                            handle.abort();
                        }
                        *cmd_tx.lock().await = None;
                        eq_manager.stop().await;
                        state.reset();
                        dbus::emit_device_disconnected(&connection).await;
                        dbus::emit_properties_changed(&connection, &["Connected", "EqPreset"]).await;

                        // Store last known address
                        last_address = Some(addr);

                        // Auto-reconnect if enabled
                        if config.reconnect.auto_reconnect {
                            // Cancel any existing reconnect task
                            if let Some(handle) = reconnect_handle.take() {
                                handle.abort();
                            }

                            let max_retries = config.reconnect.max_retries;
                            reconnect_handle = Some(tokio::spawn(async move {
                                reconnect_with_backoff(addr, max_retries).await;
                            }));
                        }
                    }
                }
            }

            // AAP events from L2CAP reader
            Some(event) = event_rx.recv() => {
                match &event {
                    AapEvent::Battery(_) => {
                        dbus::emit_properties_changed(&connection, &[
                            "BatteryLeft", "BatteryRight", "BatteryCase",
                            "ChargingLeft", "ChargingRight", "ChargingCase",
                        ]).await;
                    }
                    AapEvent::AncMode(_) => {
                        dbus::emit_properties_changed(&connection, &["AncMode"]).await;
                    }
                    AapEvent::EarDetection(_ed) => {
                        let s = state.current();
                        dbus::emit_properties_changed(&connection, &["EarLeft", "EarRight"]).await;
                        dbus::emit_ear_detection_changed(&connection, s.ear_left, s.ear_right).await;
                    }
                    AapEvent::ConversationalAwareness(_) => {
                        dbus::emit_properties_changed(&connection, &["ConversationalAwareness"]).await;
                    }
                    AapEvent::ConversationalActivity(_) => {
                        dbus::emit_properties_changed(&connection, &["ConversationalActivityState"]).await;
                    }
                    AapEvent::AdaptiveNoiseLevel(_) => {
                        dbus::emit_properties_changed(&connection, &["AdaptiveNoiseLevel"]).await;
                    }
                    AapEvent::OneBudAnc(_) => {
                        dbus::emit_properties_changed(&connection, &["OneBudAnc"]).await;
                    }
                    AapEvent::VolumeSwipe(_) => {
                        dbus::emit_properties_changed(&connection, &["VolumeSwipe"]).await;
                    }
                    AapEvent::AdaptiveVolume(_) => {
                        dbus::emit_properties_changed(&connection, &["AdaptiveVolume"]).await;
                    }
                    AapEvent::ChimeVolume(_) => {
                        dbus::emit_properties_changed(&connection, &["ChimeVolume"]).await;
                    }
                    AapEvent::AudioSource(_) => {
                        dbus::emit_properties_changed(&connection, &["AudioSource"]).await;
                    }
                    AapEvent::DeviceInfo(info) => {
                        dbus::emit_properties_changed(&connection, &["Model", "ModelName", "Firmware"]).await;
                        dbus::emit_device_connected(&connection, &info.model).await;
                        dbus::emit_properties_changed(&connection, &["Connected"]).await;

                        // Auto-load EQ preset on connect
                        if config.eq.auto_load {
                            let preset_name = &config.eq.active_preset;
                            if let Some(preset) = eq::EqPreset::load(preset_name) {
                                if let Err(e) = eq_manager.apply(&preset).await {
                                    error!("failed to auto-load EQ preset '{preset_name}': {e}");
                                } else {
                                    let name = preset.name.clone();
                                    state.update(|s| s.eq_preset = name);
                                    dbus::emit_properties_changed(&connection, &["EqPreset"]).await;
                                }
                            } else {
                                warn!("configured EQ preset '{preset_name}' not found");
                            }
                        }
                    }
                    AapEvent::Disconnected => {
                        eq_manager.stop().await;
                        state.reset();
                        dbus::emit_device_disconnected(&connection).await;
                        dbus::emit_properties_changed(&connection, &["Connected", "EqPreset"]).await;
                    }
                    _ => {}
                }
            }

            // EQ commands from D-Bus
            Some(cmd) = eq_rx.recv() => {
                match cmd {
                    eq::EqCommand::Apply(preset_name) => {
                        if let Some(preset) = eq::EqPreset::load(&preset_name) {
                            if let Err(e) = eq_manager.apply(&preset).await {
                                error!("failed to apply EQ preset '{preset_name}': {e}");
                            } else {
                                let name = preset.name.clone();
                                state.update(|s| s.eq_preset = name);
                            }
                            dbus::emit_properties_changed(&connection, &["EqPreset"]).await;
                        } else {
                            warn!("EQ preset '{preset_name}' not found");
                        }
                    }
                    eq::EqCommand::Disable => {
                        eq_manager.stop().await;
                        state.update(|s| s.eq_preset.clear());
                        dbus::emit_properties_changed(&connection, &["EqPreset"]).await;
                    }
                }
            }

            // D-Bus manual reconnect request
            Some(()) = reconnect_rx.recv() => {
                if let Some(addr) = last_address {
                    info!("manual reconnect requested for {addr}");

                    // Cancel any existing reconnect task
                    if let Some(handle) = reconnect_handle.take() {
                        handle.abort();
                    }

                    let max_retries = config.reconnect.max_retries;
                    reconnect_handle = Some(tokio::spawn(async move {
                        reconnect_with_backoff(addr, max_retries).await;
                    }));
                } else {
                    warn!("reconnect requested but no known device address");
                }
            }

            // Graceful shutdown
            _ = tokio::signal::ctrl_c() => {
                info!("shutting down");
                eq_manager.stop().await;
                if let Some(handle) = reconnect_handle.take() {
                    handle.abort();
                }
                if let Some(handle) = l2cap_handle.take() {
                    handle.abort();
                }
                break;
            }
        }
    }

    Ok(())
}

/// Attempt to reconnect to AirPods with exponential backoff
async fn reconnect_with_backoff(address: Address, max_retries: u32) {
    let mut delay = std::time::Duration::from_secs(2);

    for attempt in 1..=max_retries {
        info!("reconnect attempt {}/{}", attempt, max_retries);
        tokio::time::sleep(delay).await;

        match bluez::connect_device(address).await {
            Ok(()) => {
                info!("reconnect succeeded on attempt {}", attempt);
                return;
            }
            Err(e) => {
                warn!("reconnect attempt {} failed: {e}", attempt);
                delay *= 2; // exponential backoff: 2s, 4s, 8s, ...
            }
        }
    }

    warn!(
        "all {} reconnect attempts exhausted for {address}",
        max_retries
    );
}
