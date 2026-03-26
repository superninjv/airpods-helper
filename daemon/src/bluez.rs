use bluer::{AdapterEvent, Address, Device, Session};
use futures::StreamExt;
use std::collections::HashSet;
use tokio::sync::mpsc;
use tracing::info;

/// Events from the BlueZ monitor
#[derive(Debug)]
pub enum BlueZEvent {
    AirPodsConnected(Address),
    AirPodsDisconnected(Address),
}

/// Monitor BlueZ D-Bus for AirPods connect/disconnect events
pub async fn monitor(tx: mpsc::Sender<BlueZEvent>) -> bluer::Result<()> {
    let session = Session::new().await?;
    let adapter = session.default_adapter().await?;
    info!("monitoring BlueZ adapter: {}", adapter.name());

    let mut known_connected: HashSet<Address> = HashSet::new();

    // Check already-connected devices on startup
    let addrs = adapter.device_addresses().await?;
    for addr in addrs {
        if let Ok(device) = adapter.device(addr) {
            if device.is_connected().await.unwrap_or(false) && is_airpods(&device).await {
                info!("found already-connected AirPods: {addr}");
                known_connected.insert(addr);
                let _ = tx.send(BlueZEvent::AirPodsConnected(addr)).await;
            }
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
    if let Ok(Some(name)) = device.name().await {
        if name.contains("AirPods") {
            return true;
        }
    }

    false
}
