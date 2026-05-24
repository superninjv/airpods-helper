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

/// Pair (and trust) an AirPods device by MAC address. Registers a transient
/// NoInputNoOutput just-works agent for the duration of the attempt, starts
/// discovery if the device hasn't been seen yet, then performs the BlueZ Pair
/// followed by SetTrusted(true) so the AirPods auto-reconnect on case-open.
///
/// Returns an error if the device doesn't appear within 20 seconds — usually
/// means the AirPods aren't in pairing mode (case open, status light blinking
/// white). Pair calls itself can also fail if the AAP-side accepts a different
/// pairing flavor (Magic Pairing), but standard just-works covers the supported
/// AirPods we target.
pub async fn pair_and_trust(address: Address) -> bluer::Result<()> {
    use std::time::Duration;

    let session = Session::new().await?;

    // Transient just-works agent — dropped at end of scope, unregisters.
    let agent = bluer::agent::Agent::default();
    let _agent_handle = session.register_agent(agent).await?;

    let adapter = session.default_adapter().await?;
    adapter.set_powered(true).await?;
    let _ = adapter.set_pairable(true).await;

    // Start discovery if the device isn't already known. Holding the stream
    // alive keeps discovery active; dropping it stops discovery.
    let mut _discovery_stream = None;
    let known = adapter.device_addresses().await.unwrap_or_default();
    if !known.contains(&address) {
        info!("device {address} unknown, starting discovery");
        _discovery_stream = Some(adapter.discover_devices().await?);

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
                return Err(bluer::Error {
                    kind: bluer::ErrorKind::NotFound,
                    message: format!(
                        "device {address} not seen within 20s — make sure AirPods are in pairing mode (case open, status light flashing white)"
                    ),
                });
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    let device = adapter.device(address)?;
    info!("pairing {address}");
    device.pair().await?;
    device.set_trusted(true).await?;
    info!("paired and trusted {address}");
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

/// One candidate from a quick-pair LE scan.
#[derive(Debug, Clone)]
pub struct QuickPairCandidate {
    pub address: Address,
    pub name: String,
    pub model_hint: String,
    pub rssi: i16,
    /// Heuristic — true if the AirPods look like they're in pairing mode
    /// (Apple Continuity status nibble indicates case open + buds inside).
    pub in_pair_mode: bool,
}

/// Map known Apple AirPods BLE product IDs (Continuity type 0x07, bytes 2-3
/// little-endian) to display names. Returned as &'static for cheap cloning.
fn continuity_model_name(model_le: u16) -> Option<&'static str> {
    // Stored as the BE-readable value (e.g. 0x200E for AirPods 1, since the
    // wire bytes are 0x0E 0x20). Match against the LE-decoded u16 here.
    Some(match model_le {
        0x0220 => "AirPods 1",
        0x0F20 => "AirPods 2",
        0x1320 => "AirPods Pro",
        0x1420 => "AirPods Max",
        0x1B20 => "AirPods Pro 2 (Lightning)",
        0x2420 => "AirPods Pro 2 (USB-C)",
        0x2024 => "AirPods 4 ANC",
        0x2424 => "AirPods Pro 3",
        0x2020 => "AirPods 3",
        0x1F20 => "AirPods 4",
        _ => return None,
    })
}

/// Parse Apple manufacturer data (vendor ID 0x004C) and pull out the AirPods
/// proximity-pairing record if present. Returns (model_hint, in_pair_mode).
fn parse_apple_proximity(payload: &[u8]) -> Option<(String, bool)> {
    // Layout for proximity record:
    //   [0]=0x07 (type), [1]=length (usually 0x19), [2..]=record bytes
    // Inside the record:
    //   [2..4] = model ID (little-endian)
    //   [4]    = status byte — lower nibble describes case lid + buds
    let mut i = 0;
    while i + 1 < payload.len() {
        let ty = payload[i];
        let len = payload[i + 1] as usize;
        let end = i + 2 + len;
        if end > payload.len() {
            return None;
        }
        if ty == 0x07 && len >= 5 {
            // Bytes are at payload[i+2 .. end]
            let rec = &payload[i + 2..end];
            let model_le = u16::from_le_bytes([rec[0], rec[1]]);
            let status = rec[2];
            let name = continuity_model_name(model_le)
                .map(|s| s.to_string())
                .unwrap_or_else(|| format!("AirPods (model 0x{model_le:04X})"));
            // Lower nibble: 0 case closed, ≥4 case open with buds inside.
            // The exact bit pattern varies by firmware; this is a heuristic.
            let in_pair_mode = (status & 0x0F) >= 4;
            return Some((name, in_pair_mode));
        }
        i = end;
    }
    None
}

/// Run an LE scan for `duration` seconds and return any nearby AirPods that
/// broadcast Apple Continuity proximity-pairing records. Already-paired
/// devices are filtered out (they're not pair candidates).
pub async fn quick_pair_scan(duration_secs: u32) -> bluer::Result<Vec<QuickPairCandidate>> {
    use std::collections::HashMap;
    use std::time::Duration;

    let session = Session::new().await?;
    let adapter = session.default_adapter().await?;
    adapter.set_powered(true).await?;

    let _discovery = adapter.discover_devices().await?;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(duration_secs as u64);

    let mut candidates: HashMap<Address, QuickPairCandidate> = HashMap::new();

    while tokio::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(500)).await;
        // Pull current device list and inspect manufacturer data on each one.
        let addrs = adapter.device_addresses().await.unwrap_or_default();
        for addr in addrs {
            if candidates.contains_key(&addr) {
                continue;
            }
            let Ok(device) = adapter.device(addr) else {
                continue;
            };
            if device.is_paired().await.unwrap_or(false) {
                continue;
            }
            let Ok(Some(mfd)) = device.manufacturer_data().await else {
                continue;
            };
            let Some(payload) = mfd.get(&crate::aap::APPLE_COMPANY_ID) else {
                continue;
            };
            if let Some((model_hint, in_pair_mode)) = parse_apple_proximity(payload) {
                let name = device
                    .name()
                    .await
                    .ok()
                    .flatten()
                    .unwrap_or_else(|| "AirPods".to_string());
                let rssi = device.rssi().await.ok().flatten().unwrap_or(0);
                candidates.insert(
                    addr,
                    QuickPairCandidate {
                        address: addr,
                        name,
                        model_hint,
                        rssi,
                        in_pair_mode,
                    },
                );
            }
        }
    }

    let mut out: Vec<_> = candidates.into_values().collect();
    // Sort: in_pair_mode first, then strongest RSSI first.
    out.sort_by(|a, b| {
        b.in_pair_mode
            .cmp(&a.in_pair_mode)
            .then(b.rssi.cmp(&a.rssi))
    });
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
