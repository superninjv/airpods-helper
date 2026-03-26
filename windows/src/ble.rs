//! BLE scanning for AirPods discovery using btleplug.
//!
//! Scans for devices advertising the AirPods service UUID or Apple company ID 0x004C.
//! Once found, returns the device address for L2CAP connection.

use anyhow::Result;
use btleplug::api::{Central, CentralEvent, Manager as _, Peripheral as _, ScanFilter};
use btleplug::platform::{Adapter, Manager, Peripheral};
use futures::StreamExt;
use std::time::Duration;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::aap;

// Re-export the Uuid type used by btleplug
use btleplug::api::BDAddr;

/// Discovered AirPods device
#[derive(Debug, Clone)]
pub struct DiscoveredDevice {
    /// Bluetooth address
    pub address: BDAddr,
    /// Device name (if available)
    pub name: Option<String>,
    /// The btleplug peripheral handle (needed for GATT, not for L2CAP)
    pub peripheral: Peripheral,
}

/// Get the first available Bluetooth adapter
pub async fn get_adapter() -> Result<Adapter> {
    let manager = Manager::new().await?;
    let adapters = manager.adapters().await?;
    adapters
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("no Bluetooth adapter found"))
}

/// Scan for AirPods devices.
///
/// Returns the first device that matches the AirPods service UUID or has an Apple
/// manufacturer data prefix. Scans for up to `timeout` duration.
pub async fn scan_for_airpods(adapter: &Adapter, timeout: Duration) -> Result<DiscoveredDevice> {
    let airpods_uuid = Uuid::parse_str(aap::AIRPODS_SERVICE_UUID)?;

    info!("scanning for AirPods (timeout: {timeout:?})");

    // Start scanning with a filter for the AirPods service UUID
    let filter = ScanFilter {
        services: vec![airpods_uuid],
    };
    adapter.start_scan(filter).await?;

    let mut events = adapter.events().await?;

    let scan_result = tokio::time::timeout(timeout, async {
        while let Some(event) = events.next().await {
            if let CentralEvent::DeviceDiscovered(id) = event {
                if let Ok(peripheral) = adapter.peripheral(&id).await {
                    if let Some(device) = check_peripheral(&peripheral, airpods_uuid).await {
                        return Some(device);
                    }
                }
            }
        }
        None
    })
    .await;

    adapter.stop_scan().await?;

    match scan_result {
        Ok(Some(device)) => {
            info!("found AirPods: {:?} at {}", device.name, device.address);
            Ok(device)
        }
        Ok(None) => Err(anyhow::anyhow!("scan ended without finding AirPods")),
        Err(_) => {
            // Timeout — check already-known peripherals before giving up
            info!("scan timed out, checking already-known peripherals");
            let peripherals = adapter.peripherals().await?;
            for p in peripherals {
                if let Some(device) = check_peripheral(&p, airpods_uuid).await {
                    info!("found AirPods in known devices: {:?} at {}", device.name, device.address);
                    return Ok(device);
                }
            }
            Err(anyhow::anyhow!(
                "no AirPods found within {timeout:?} scan window"
            ))
        }
    }
}

/// Check if a peripheral is AirPods by service UUID or Apple manufacturer data
async fn check_peripheral(peripheral: &Peripheral, airpods_uuid: Uuid) -> Option<DiscoveredDevice> {
    let props = peripheral.properties().await.ok()??;

    // Check service UUIDs
    if props.services.contains(&airpods_uuid) {
        debug!(
            "matched AirPods by service UUID: {:?} ({})",
            props.local_name, props.address
        );
        return Some(DiscoveredDevice {
            address: props.address,
            name: props.local_name,
            peripheral: peripheral.clone(),
        });
    }

    // Check Apple manufacturer data (company ID 0x004C)
    if props.manufacturer_data.contains_key(&aap::APPLE_COMPANY_ID) {
        // Apple manufacturer data present — check if device name suggests AirPods
        if let Some(ref name) = props.local_name {
            let lower = name.to_lowercase();
            if lower.contains("airpods") || lower.contains("airpod") {
                debug!("matched AirPods by name + Apple mfr data: {name} ({})", props.address);
                return Some(DiscoveredDevice {
                    address: props.address,
                    name: Some(name.clone()),
                    peripheral: peripheral.clone(),
                });
            }
        }
    }

    // Fallback: any device with "AirPods" in its name
    if let Some(ref name) = props.local_name {
        let lower = name.to_lowercase();
        if lower.contains("airpods") {
            warn!("matched device by name only (no UUID/mfr match): {name}");
            return Some(DiscoveredDevice {
                address: props.address,
                name: Some(name.clone()),
                peripheral: peripheral.clone(),
            });
        }
    }

    None
}
