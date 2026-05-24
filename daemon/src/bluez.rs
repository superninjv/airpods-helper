use bluer::{AdapterEvent, Address, Device, Session};
use futures::StreamExt;
use std::collections::HashSet;
use tokio::sync::mpsc;
use tracing::{info, warn};

/// Events from the BlueZ monitor
#[derive(Debug)]
pub enum BlueZEvent {
    AirPodsConnected(Address),
    AirPodsDisconnected(Address),
}

/// Monitor BlueZ D-Bus for AirPods connect/disconnect events.
/// Retries connecting to BlueZ with backoff if it's not ready yet (boot race).
pub async fn monitor(tx: mpsc::Sender<BlueZEvent>) -> bluer::Result<()> {
    let (_session, adapter) = {
        let mut delay = std::time::Duration::from_secs(1);
        let max_delay = std::time::Duration::from_secs(30);
        loop {
            match Session::new().await {
                Ok(session) => match session.default_adapter().await {
                    Ok(adapter) => break (session, adapter),
                    Err(e) => {
                        warn!("BlueZ adapter not ready, retrying in {}s: {e}", delay.as_secs());
                    }
                },
                Err(e) => {
                    warn!("BlueZ session not ready, retrying in {}s: {e}", delay.as_secs());
                }
            }
            tokio::time::sleep(delay).await;
            delay = (delay * 2).min(max_delay);
        }
    };
    info!("monitoring BlueZ adapter: {}", adapter.name());

    let mut known_connected: HashSet<Address> = HashSet::new();

    // Check already-connected devices on startup
    let addrs = adapter.device_addresses().await?;
    for addr in addrs {
        if let Ok(device) = adapter.device(addr)
            && device.is_connected().await.unwrap_or(false) && is_airpods(&device).await {
                info!("found already-connected AirPods: {addr}");
                known_connected.insert(addr);
                let _ = tx.send(BlueZEvent::AirPodsConnected(addr)).await;
            }
    }

    // Watch for device events (DeviceAdded fires on property changes too with discover_devices_with_changes)
    let mut events = adapter.discover_devices_with_changes().await?;

    while let Some(event) = events.next().await {
        match event {
            AdapterEvent::DeviceAdded(addr) => {
                if let Ok(device) = adapter.device(addr) {
                    let connected = device.is_connected().await.unwrap_or(false);
                    let was_known = known_connected.contains(&addr);

                    if connected && !was_known && is_airpods(&device).await {
                        info!("AirPods connected: {addr}");
                        known_connected.insert(addr);
                        let _ = tx.send(BlueZEvent::AirPodsConnected(addr)).await;
                    } else if !connected && was_known {
                        info!("AirPods disconnected: {addr}");
                        known_connected.remove(&addr);
                        let _ = tx.send(BlueZEvent::AirPodsDisconnected(addr)).await;
                    }
                }
            }
            AdapterEvent::DeviceRemoved(addr) => {
                if known_connected.remove(&addr) {
                    info!("AirPods removed: {addr}");
                    let _ = tx.send(BlueZEvent::AirPodsDisconnected(addr)).await;
                }
            }
            _ => {}
        }
    }

    Ok(())
}

/// Trigger a BlueZ-level connect to a device by address
pub async fn connect_device(address: Address) -> bluer::Result<()> {
    let session = Session::new().await?;
    let adapter = session.default_adapter().await?;
    let device = adapter.device(address)?;
    device.connect().await?;
    Ok(())
}

/// Trigger a BlueZ-level disconnect for a device by address
pub async fn disconnect_device(address: Address) -> bluer::Result<()> {
    let session = Session::new().await?;
    let adapter = session.default_adapter().await?;
    let device = adapter.device(address)?;
    device.disconnect().await?;
    Ok(())
}

/// List paired AirPods (paired + AAP-capable), with their display names.
/// Returns (address, name) tuples.
pub async fn list_paired_airpods() -> bluer::Result<Vec<(Address, String)>> {
    let session = Session::new().await?;
    let adapter = session.default_adapter().await?;
    let mut out = Vec::new();
    for addr in adapter.device_addresses().await? {
        if let Ok(device) = adapter.device(addr) {
            let paired = device.is_paired().await.unwrap_or(false);
            if paired && is_airpods(&device).await {
                let name = device
                    .name()
                    .await
                    .ok()
                    .flatten()
                    .unwrap_or_else(|| "AirPods".to_string());
                out.push((addr, name));
            }
        }
    }
    Ok(out)
}

/// Look up which paired AirPods (if any) is currently connected.
pub async fn currently_connected_airpods() -> bluer::Result<Option<Address>> {
    let session = Session::new().await?;
    let adapter = session.default_adapter().await?;
    for addr in adapter.device_addresses().await? {
        if let Ok(device) = adapter.device(addr)
            && device.is_connected().await.unwrap_or(false)
            && is_airpods(&device).await
        {
            return Ok(Some(addr));
        }
    }
    Ok(None)
}

/// Check if a BlueZ device is AirPods
async fn is_airpods(device: &Device) -> bool {
    // Check by service UUID
    if let Ok(Some(uuids)) = device.uuids().await {
        for uuid in &uuids {
            if uuid.to_string() == crate::aap::AIRPODS_SERVICE_UUID {
                return true;
            }
        }
    }

    // Fallback: check device name
    if let Ok(Some(name)) = device.name().await
        && name.contains("AirPods") {
            return true;
        }

    false
}
