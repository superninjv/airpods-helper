use bluer::{
    Adapter, AdapterEvent, AdapterProperty, Address, Device, DeviceEvent, DeviceProperty, Session,
    SessionEvent,
};
use futures::StreamExt;
use futures::stream::SelectAll;
use std::collections::HashSet;
use tokio::sync::mpsc;
use tracing::{info, warn};

/// Events from the BlueZ monitor
#[derive(Debug)]
pub enum BlueZEvent {
    AirPodsConnected(Address),
    AirPodsDisconnected(Address),
}

type DeviceEvents = std::pin::Pin<Box<dyn futures::Stream<Item = (Address, DeviceEvent)> + Send>>;

/// Monitor BlueZ for AirPods connect/disconnect events.
///
/// This deliberately does NOT start device discovery: continuous inquiry
/// scanning steals radio time and makes A2DP audio stutter. Instead we watch
/// the `Connected` property of every known device, and pick up devices BlueZ
/// adds later (e.g. after pairing).
///
/// Returns when the adapter goes away so the caller can restart against
/// whatever adapter is current.
pub async fn monitor(tx: mpsc::Sender<BlueZEvent>) -> bluer::Result<()> {
    let (session, adapter) = {
        let mut delay = std::time::Duration::from_secs(1);
        let max_delay = std::time::Duration::from_secs(30);
        loop {
            match Session::new().await {
                Ok(session) => match session.default_adapter().await {
                    Ok(adapter) => break (session, adapter),
                    Err(e) => {
                        warn!(
                            "BlueZ adapter not ready, retrying in {}s: {e}",
                            delay.as_secs()
                        );
                    }
                },
                Err(e) => {
                    warn!(
                        "BlueZ session not ready, retrying in {}s: {e}",
                        delay.as_secs()
                    );
                }
            }
            tokio::time::sleep(delay).await;
            delay = (delay * 2).min(max_delay);
        }
    };
    info!("monitoring BlueZ adapter: {}", adapter.name());

    let mut adapter_events = Box::pin(adapter.events().await?);
    let mut session_events = Box::pin(session.events().await?);
    let mut device_events: SelectAll<DeviceEvents> = SelectAll::new();
    // Keeps SelectAll from completing while no devices are watched.
    device_events.push(Box::pin(futures::stream::pending()));
    let mut watched: HashSet<Address> = HashSet::new();
    let mut connected: HashSet<Address> = HashSet::new();

    async fn watch(
        adapter: &Adapter,
        addr: Address,
        watched: &mut HashSet<Address>,
        streams: &mut SelectAll<DeviceEvents>,
    ) -> Option<Device> {
        let device = adapter.device(addr).ok()?;
        if watched.insert(addr) {
            match device.events().await {
                Ok(ev) => streams.push(Box::pin(ev.map(move |e| (addr, e)))),
                Err(e) => warn!("can't watch device {addr}: {e}"),
            }
        }
        Some(device)
    }

    async fn check(
        device: &Device,
        connected: &mut HashSet<Address>,
        tx: &mpsc::Sender<BlueZEvent>,
    ) {
        let addr = device.address();
        let is_connected = device.is_connected().await.unwrap_or(false);
        if is_connected && !connected.contains(&addr) && is_airpods(device).await {
            info!("AirPods connected: {addr}");
            connected.insert(addr);
            let _ = tx.send(BlueZEvent::AirPodsConnected(addr)).await;
        } else if !is_connected && connected.remove(&addr) {
            info!("AirPods disconnected: {addr}");
            let _ = tx.send(BlueZEvent::AirPodsDisconnected(addr)).await;
        }
    }

    for addr in adapter.device_addresses().await? {
        if let Some(device) = watch(&adapter, addr, &mut watched, &mut device_events).await {
            check(&device, &mut connected, &tx).await;
        }
    }

    loop {
        tokio::select! {
            event = adapter_events.next() => match event {
                Some(AdapterEvent::DeviceAdded(addr)) => {
                    if let Some(device) = watch(&adapter, addr, &mut watched, &mut device_events).await {
                        check(&device, &mut connected, &tx).await;
                    }
                }
                Some(AdapterEvent::DeviceRemoved(addr)) => {
                    // Keep `addr` in `watched`: its event stream stays in the
                    // SelectAll and resumes if the device is re-added, so
                    // re-subscribing would deliver every event twice.
                    if connected.remove(&addr) {
                        info!("AirPods removed: {addr}");
                        let _ = tx.send(BlueZEvent::AirPodsDisconnected(addr)).await;
                    }
                }
                Some(AdapterEvent::PropertyChanged(AdapterProperty::Powered(false))) => {
                    info!("Bluetooth adapter powered off");
                    for addr in connected.drain() {
                        let _ = tx.send(BlueZEvent::AirPodsDisconnected(addr)).await;
                    }
                }
                Some(_) => {}
                None => break,
            },
            Some((addr, DeviceEvent::PropertyChanged(prop))) = device_events.next() => {
                // Connected flips on connect/disconnect; UUIDs can resolve
                // after Connected=true on a first connection.
                if matches!(prop, DeviceProperty::Connected(_) | DeviceProperty::Uuids(_))
                    && let Ok(device) = adapter.device(addr)
                {
                    check(&device, &mut connected, &tx).await;
                }
            }
            event = session_events.next() => match event {
                Some(SessionEvent::AdapterRemoved(name)) if name == adapter.name() => {
                    warn!("Bluetooth adapter {name} removed");
                    break;
                }
                Some(_) => {}
                None => break,
            },
        }
    }

    for addr in connected.drain() {
        let _ = tx.send(BlueZEvent::AirPodsDisconnected(addr)).await;
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

    if let Ok(device) = adapter.device(address)
        && device.is_paired().await.unwrap_or(false)
    {
        info!("{address} is already paired; marking trusted");
        device.set_trusted(true).await?;
        return Ok(());
    }

    // Always scan until the device is actually advertising: a cached entry
    // from an earlier session may be stale, and pairing a device that isn't
    // in range fails with an opaque page timeout.
    {
        info!("scanning for {address}");
        let _discovery = adapter.discover_devices().await?;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
        loop {
            if let Ok(device) = adapter.device(address)
                && device.rssi().await.ok().flatten().is_some()
            {
                break;
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(bluer::Error {
                    kind: bluer::ErrorKind::NotFound,
                    message: format!(
                        "device {address} not seen within 20s — make sure AirPods are in pairing mode (case open, hold the button until the light flashes white)"
                    ),
                });
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        // Discovery is stopped here (dropped) — pairing while inquiry
        // scanning is running is slower and less reliable.
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

/// Map Apple Continuity proximity-pairing product IDs (big-endian, as
/// transmitted) to display names. Table from LibrePods.
fn continuity_model_name(model: u16) -> Option<&'static str> {
    Some(match model {
        0x0220 => "AirPods",
        0x0F20 => "AirPods (2nd gen)",
        0x1320 => "AirPods (3rd gen)",
        0x1920 => "AirPods 4",
        0x1B20 => "AirPods 4 (ANC)",
        0x0E20 => "AirPods Pro",
        0x1420 => "AirPods Pro 2 (Lightning)",
        0x2420 => "AirPods Pro 2 (USB-C)",
        0x0A20 => "AirPods Max",
        0x1F20 => "AirPods Max (USB-C)",
        _ => return None,
    })
}

/// Parse Apple manufacturer data (company 0x004C) and pull out the AirPods
/// proximity-pairing record if present. Returns (model_hint, in_pair_mode).
///
/// Record layout (after the company ID, which BlueZ strips):
/// `[0x07 type][len][pairing flag: 0x00 = pairing mode, 0x01 = paired][model hi][model lo][status]...`
fn parse_apple_proximity(payload: &[u8]) -> Option<(String, bool)> {
    let mut i = 0;
    while i + 1 < payload.len() {
        let ty = payload[i];
        let len = payload[i + 1] as usize;
        let end = i + 2 + len;
        if end > payload.len() {
            return None;
        }
        if ty == 0x07 && len >= 4 {
            let rec = &payload[i + 2..end];
            let in_pair_mode = rec[0] == 0x00;
            let model = u16::from_be_bytes([rec[1], rec[2]]);
            let name = continuity_model_name(model)
                .map(str::to_string)
                .unwrap_or_else(|| format!("AirPods (model 0x{model:04X})"));
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
            let Some(payload) = mfd.get(&aap::APPLE_COMPANY_ID) else {
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

/// Check if a BlueZ device is AirPods
async fn is_airpods(device: &Device) -> bool {
    // Check by service UUID
    if let Ok(Some(uuids)) = device.uuids().await {
        for uuid in &uuids {
            if uuid.to_string() == aap::AIRPODS_SERVICE_UUID {
                return true;
            }
        }
    }

    // Fallback: check device name
    if let Ok(Some(name)) = device.name().await
        && name.contains("AirPods")
    {
        return true;
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proximity_record_decodes_model_and_pair_mode() {
        // type 0x07, len 0x19, paired=0x01, model 0x14 0x20 (Pro 2 Lightning), status...
        let mut rec = vec![
            0x07, 0x19, 0x01, 0x14, 0x20, 0x2B, 0x99, 0x8F, 0x01, 0x00, 0x00,
        ];
        rec.resize(2 + 0x19, 0);
        let (name, pairing) = parse_apple_proximity(&rec).unwrap();
        assert_eq!(name, "AirPods Pro 2 (Lightning)");
        assert!(!pairing);
        rec[2] = 0x00;
        assert!(parse_apple_proximity(&rec).unwrap().1);
    }

    #[test]
    fn proximity_skips_other_records() {
        // A 0x10 (nearby info) record followed by a proximity record.
        let data = [
            0x10, 0x02, 0xAA, 0xBB, 0x07, 0x05, 0x01, 0x24, 0x20, 0x00, 0x00,
        ];
        assert_eq!(
            parse_apple_proximity(&data).unwrap().0,
            "AirPods Pro 2 (USB-C)"
        );
        assert!(parse_apple_proximity(&[0x07, 0x09, 0x01]).is_none());
    }
}
