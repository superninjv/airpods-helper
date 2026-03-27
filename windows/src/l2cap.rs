//! L2CAP connection to AirPods for AAP protocol communication.
//!
//! ## Windows L2CAP strategy
//!
//! Windows supports L2CAP sockets via Winsock with `AF_BTH` + `BTHPROTO_L2CAP`.
//! The `SOCKADDR_BTH` structure's `port` field accepts an L2CAP PSM value.
//! This is a Win32 API, accessible from user mode without a kernel driver.
//!
//! On Linux, the daemon uses BlueZ L2CAP SeqPacket sockets (via the `bluer` crate).
//! On Windows, we use raw Winsock Bluetooth sockets through the `windows` crate.
//!
//! ## Alternative: KMDF L2CAP bridge driver
//!
//! The WinPods project (github.com/changcheng967/WinPods) takes a different approach
//! with a KMDF kernel driver that bridges L2CAP to user mode. Our approach tries
//! Winsock first (simpler, no driver install required) and falls back to documenting
//! the KMDF approach if Winsock L2CAP proves insufficient.

use std::io;
use tokio::sync::mpsc;
#[allow(unused_imports)]
use tracing::{debug, error, info, warn};

use crate::aap;
#[allow(unused_imports)]
use crate::aap::parser::{self, AapEvent, AudioSource, CaActivity};
use crate::state::SharedState;

/// Commands that can be sent to the AirPods over L2CAP
#[derive(Debug)]
#[allow(dead_code)]
pub enum L2capCommand {
    SetAncMode(aap::AncMode),
    SetConversationalAwareness(bool),
    SetAdaptiveNoiseLevel(u8),
    SetOneBudAnc(bool),
    SetVolumeSwipe(bool),
    Disconnect,
}

/// Bluetooth address as 6 bytes (used for Winsock SOCKADDR_BTH)
#[derive(Debug, Clone, Copy)]
pub struct BtAddr(pub [u8; 6]);

impl std::fmt::Display for BtAddr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
            self.0[5], self.0[4], self.0[3], self.0[2], self.0[1], self.0[0]
        )
    }
}

impl BtAddr {
    /// Convert from btleplug BDAddr bytes
    pub fn from_bytes(bytes: [u8; 6]) -> Self {
        Self(bytes)
    }

    /// Convert to u64 for SOCKADDR_BTH.btAddr (little-endian 6-byte address in u64)
    #[cfg(target_os = "windows")]
    pub fn to_u64(&self) -> u64 {
        let b = &self.0;
        (b[5] as u64) << 40
            | (b[4] as u64) << 32
            | (b[3] as u64) << 24
            | (b[2] as u64) << 16
            | (b[1] as u64) << 8
            | (b[0] as u64)
    }
}

// ============================================================================
// Windows implementation: Winsock AF_BTH + BTHPROTO_L2CAP
// ============================================================================

#[cfg(target_os = "windows")]
mod win {
    use super::*;
    use std::mem;
    use windows::Win32::Devices::Bluetooth::{
        AF_BTH, BTHPROTO_L2CAP, SOCKADDR_BTH,
    };
    use windows::Win32::Networking::WinSock::{
        closesocket, connect, recv, send, socket, WSACleanup, WSAStartup, SOCK_STREAM, SOCKET,
        WSADATA,
    };

    /// Initialize Winsock
    fn wsa_init() -> io::Result<()> {
        unsafe {
            let mut wsa_data: WSADATA = mem::zeroed();
            let result = WSAStartup(0x0202, &mut wsa_data);
            if result != 0 {
                return Err(io::Error::from_raw_os_error(result));
            }
        }
        Ok(())
    }

    /// Create an L2CAP Bluetooth socket and connect to the given address + PSM
    fn bt_connect(addr: BtAddr, psm: u16) -> io::Result<SOCKET> {
        unsafe {
            let sock = socket(AF_BTH as i32, SOCK_STREAM.0, BTHPROTO_L2CAP as i32);
            if sock.is_invalid() {
                return Err(io::Error::last_os_error());
            }

            let mut sockaddr: SOCKADDR_BTH = mem::zeroed();
            sockaddr.addressFamily = AF_BTH as u16;
            sockaddr.btAddr = addr.to_u64();
            sockaddr.port = psm as u32;

            let result = connect(
                sock,
                &sockaddr as *const _ as *const _,
                mem::size_of::<SOCKADDR_BTH>() as i32,
            );
            if result != 0 {
                closesocket(sock);
                return Err(io::Error::last_os_error());
            }

            Ok(sock)
        }
    }

    /// Send data over the Bluetooth socket
    fn bt_send(sock: SOCKET, data: &[u8]) -> io::Result<usize> {
        unsafe {
            let n = send(sock, data, 0);
            if n < 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(n as usize)
        }
    }

    /// Receive data from the Bluetooth socket
    fn bt_recv(sock: SOCKET, buf: &mut [u8]) -> io::Result<usize> {
        unsafe {
            let n = recv(sock, buf, 0);
            if n < 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(n as usize)
        }
    }

    /// Run the L2CAP connection loop using Winsock
    pub async fn run_winsock(
        address: BtAddr,
        state: SharedState,
        mut cmd_rx: mpsc::Receiver<L2capCommand>,
        event_tx: mpsc::Sender<AapEvent>,
    ) -> io::Result<()> {
        info!(
            "connecting to AirPods at {} on PSM 0x{:04X} via Winsock",
            address, aap::AAP_PSM
        );

        wsa_init()?;

        // Connect in a blocking task (Winsock connect is synchronous)
        let sock = tokio::task::spawn_blocking(move || {
            let mut last_err = None;
            for attempt in 1..=5 {
                match bt_connect(address, aap::AAP_PSM) {
                    Ok(s) => return Ok(s),
                    Err(e) => {
                        warn!("L2CAP connect attempt {attempt}/5 failed: {e}");
                        last_err = Some(e);
                        std::thread::sleep(std::time::Duration::from_secs(1));
                    }
                }
            }
            Err(last_err.unwrap())
        })
        .await
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))??;

        info!("L2CAP connected via Winsock, performing handshake");
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        // Handshake (blocking sends/recvs wrapped in spawn_blocking)
        let sock_clone = sock;
        tokio::task::spawn_blocking(move || bt_send(sock_clone, &aap::commands::HANDSHAKE))
            .await
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))??;
        debug!("sent handshake");

        let sock_clone = sock;
        let ack = tokio::task::spawn_blocking(move || {
            let mut buf = vec![0u8; 1024];
            let n = bt_recv(sock_clone, &mut buf)?;
            Ok::<_, io::Error>(buf[..n].to_vec())
        })
        .await
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))??;

        match parser::parse(&ack) {
            Ok(AapEvent::HandshakeAck) => debug!("handshake ACK received"),
            Ok(other) => warn!("unexpected response to handshake: {other:?}"),
            Err(e) => warn!("failed to parse handshake response: {e}"),
        }

        let sock_clone = sock;
        tokio::task::spawn_blocking(move || bt_send(sock_clone, &aap::commands::SET_FEATURES))
            .await
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))??;
        debug!("sent feature enable");

        let sock_clone = sock;
        let features_ack = tokio::task::spawn_blocking(move || {
            let mut buf = vec![0u8; 1024];
            let n = bt_recv(sock_clone, &mut buf)?;
            Ok::<_, io::Error>(buf[..n].to_vec())
        })
        .await
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))??;

        match parser::parse(&features_ack) {
            Ok(AapEvent::FeaturesAck) => debug!("features ACK received"),
            Ok(other) => warn!("unexpected response to features: {other:?}"),
            Err(e) => warn!("failed to parse features response: {e}"),
        }

        let sock_clone = sock;
        tokio::task::spawn_blocking(move || {
            bt_send(sock_clone, &aap::commands::SUBSCRIBE_NOTIFICATIONS)
        })
        .await
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))??;
        debug!("sent notification subscribe");

        // Enable all listening modes (Off + Noise + Transparency + Adaptive)
        let sock_clone = sock;
        tokio::task::spawn_blocking(move || {
            bt_send(sock_clone, &aap::commands::ENABLE_ALL_LISTENING_MODES)
        })
        .await
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))??;
        debug!("enabled all listening modes");

        state.update(|s| s.connected = true);
        info!("handshake complete, entering main loop");

        // Main read/write loop
        // We need to poll recv in a blocking thread and commands from the channel
        let (read_tx, mut read_rx) = mpsc::channel::<Vec<u8>>(64);
        let read_sock = sock;
        tokio::task::spawn_blocking(move || {
            let mut buf = vec![0u8; 1024];
            loop {
                match bt_recv(read_sock, &mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        if read_tx.blocking_send(buf[..n].to_vec()).is_err() {
                            break;
                        }
                    }
                    Err(e) => {
                        error!("L2CAP recv error: {e}");
                        break;
                    }
                }
            }
        });

        loop {
            tokio::select! {
                Some(data) = read_rx.recv() => {
                    match parser::parse(&data) {
                        Ok(AapEvent::Disconnected) => {
                            info!("AirPods sent disconnect packet");
                            break;
                        }
                        Ok(event) => {
                            apply_event(&state, &event);
                            let _ = event_tx.send(event).await;
                        }
                        Err(e) => {
                            debug!("parse error (non-fatal): {e}");
                        }
                    }
                }
                cmd = cmd_rx.recv() => {
                    match cmd {
                        Some(L2capCommand::SetAncMode(mode)) => {
                            let pkt = aap::commands::set_anc_mode(mode);
                            let s = sock;
                            let _ = tokio::task::spawn_blocking(move || bt_send(s, &pkt)).await;
                        }
                        Some(L2capCommand::SetConversationalAwareness(enabled)) => {
                            let pkt = aap::commands::set_conversational_awareness(enabled);
                            let s = sock;
                            let _ = tokio::task::spawn_blocking(move || bt_send(s, &pkt)).await;
                        }
                        Some(L2capCommand::SetAdaptiveNoiseLevel(level)) => {
                            let pkt = aap::commands::set_adaptive_noise_level(level);
                            let s = sock;
                            let _ = tokio::task::spawn_blocking(move || bt_send(s, &pkt)).await;
                        }
                        Some(L2capCommand::SetOneBudAnc(enabled)) => {
                            let pkt = aap::commands::set_one_bud_anc(enabled);
                            let s = sock;
                            let _ = tokio::task::spawn_blocking(move || bt_send(s, &pkt)).await;
                        }
                        Some(L2capCommand::SetVolumeSwipe(enabled)) => {
                            let pkt = aap::commands::set_volume_swipe(enabled);
                            let s = sock;
                            let _ = tokio::task::spawn_blocking(move || bt_send(s, &pkt)).await;
                        }
                        Some(L2capCommand::Disconnect) | None => {
                            info!("disconnect requested");
                            break;
                        }
                    }
                }
            }
        }

        // Cleanup
        unsafe {
            closesocket(sock);
            WSACleanup();
        }
        state.reset();
        info!("L2CAP disconnected");
        Ok(())
    }
}

// ============================================================================
// Stub implementation for non-Windows (allows cross-compilation checks)
// ============================================================================

#[cfg(not(target_os = "windows"))]
mod stub {
    use super::*;

    pub async fn run_stub(
        address: BtAddr,
        state: SharedState,
        mut cmd_rx: mpsc::Receiver<L2capCommand>,
        event_tx: mpsc::Sender<AapEvent>,
    ) -> io::Result<()> {
        warn!(
            "L2CAP not available on this platform — Windows Winsock required. \
             Address: {}, PSM: 0x{:04X}",
            address,
            aap::AAP_PSM
        );

        // Consume channels to avoid compiler warnings
        drop(event_tx);
        while cmd_rx.recv().await.is_some() {}

        state.reset();
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "L2CAP requires Windows (AF_BTH + BTHPROTO_L2CAP)",
        ))
    }
}

// ============================================================================
// Public API
// ============================================================================

/// Connect to AirPods via L2CAP and run the read/write loop.
///
/// On Windows: uses Winsock AF_BTH + BTHPROTO_L2CAP.
/// On other platforms: returns an error (used for cross-compilation checks only).
pub async fn run(
    address: BtAddr,
    state: SharedState,
    cmd_rx: mpsc::Receiver<L2capCommand>,
    event_tx: mpsc::Sender<AapEvent>,
) -> io::Result<()> {
    #[cfg(target_os = "windows")]
    {
        win::run_winsock(address, state, cmd_rx, event_tx).await
    }

    #[cfg(not(target_os = "windows"))]
    {
        stub::run_stub(address, state, cmd_rx, event_tx).await
    }
}

/// Map Apple model numbers to human-readable product names.
fn model_display_name(model_number: &str) -> &str {
    match model_number {
        "A1523" | "A1722" => "AirPods 1",
        "A2031" | "A2032" => "AirPods 2",
        "A2564" | "A2565" => "AirPods 3",
        "A3050" | "A3053" | "A3054" | "A3058" => "AirPods 4",
        "A3055" | "A3056" | "A3057" | "A3059" => "AirPods 4 ANC",
        "A2083" | "A2084" | "A2190" => "AirPods Pro",
        "A2698" | "A2699" | "A2700" | "A2931" => "AirPods Pro 2",
        "A2968" | "A3047" | "A3048" | "A3049" => "AirPods Pro 2",
        "A3063" | "A3064" | "A3065" | "A3122" => "AirPods Pro 3",
        "A2096" => "AirPods Max",
        "A3184" => "AirPods Max 2",
        "A1602" | "A1938" => "AirPods Case",
        "A2566" | "A2897" => "AirPods 3 Case",
        _ => model_number,
    }
}

/// Apply a parsed AAP event to the shared state
#[allow(dead_code)]
pub fn apply_event(state: &SharedState, event: &AapEvent) {
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
                if let Some(case) = &b.case
                    && (case.level > 0 || case.charging)
                {
                    s.battery_case = case.level as i32;
                    s.charging_case = case.charging;
                }
            });
        }
        AapEvent::AncMode(mode) => {
            state.update(|s| s.anc_mode = *mode);
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
        AapEvent::ConversationalActivity(activity) => {
            let value = match activity {
                CaActivity::Speaking => "speaking",
                CaActivity::Stopped => "stopped",
                CaActivity::Normal => "normal",
            };
            state.update(|s| s.conversational_activity = value.to_string());
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
            let display_name = model_display_name(&info.model).to_string();
            state.update(|s| {
                s.model = info.model.clone();
                s.model_name = display_name;
                s.firmware = info.firmware.clone();
            });
        }
        _ => {}
    }
}
