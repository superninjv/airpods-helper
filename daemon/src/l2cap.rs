use bluer::l2cap::{SeqPacket, Socket, SocketAddr};
use bluer::Address;
use std::io;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use crate::aap;
use crate::aap::parser::{self, AapEvent, AudioSource};
use crate::models;
use crate::state::SharedState;

/// Commands that can be sent to the AirPods over L2CAP
#[derive(Debug)]
pub enum L2capCommand {
    SetAncMode(aap::AncMode),
    SetConversationalAwareness(bool),
    SetAdaptiveNoiseLevel(u8),
    SetOneBudAnc(bool),
    #[allow(dead_code)] // wired in match arm, constructed by future CLI disconnect command
    Disconnect,
}

/// Connect to AirPods via L2CAP and run the read/write loop
pub async fn run(
    address: Address,
    state: SharedState,
    mut cmd_rx: mpsc::Receiver<L2capCommand>,
    event_tx: mpsc::Sender<AapEvent>,
) -> io::Result<()> {
    info!(
        "connecting to AirPods at {} on PSM 0x{:04X}",
        address, aap::AAP_PSM
    );

    // Retry L2CAP connect — the channel may not be ready immediately after BT connect
    let mut seq: Option<SeqPacket> = None;
    for attempt in 1..=5 {
        let socket = Socket::new_seq_packet()?;
        let addr = SocketAddr::new(address, bluer::AddressType::BrEdr, aap::AAP_PSM.into());
        match socket.connect(addr).await {
            Ok(s) => {
                seq = Some(s);
                break;
            }
            Err(e) => {
                warn!("L2CAP connect attempt {attempt}/5 failed: {e}");
                if attempt < 5 {
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                } else {
                    return Err(e);
                }
            }
        }
    }
    let seq = seq.unwrap();

    info!("L2CAP connected, performing handshake");

    // Brief delay for L2CAP transport to become ready
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    // Handshake sequence
    seq.send(&aap::commands::HANDSHAKE).await?;
    debug!("sent handshake");

    let mut buf = vec![0u8; 1024];
    let n = seq.recv(&mut buf).await?;
    match parser::parse(&buf[..n]) {
        Ok(AapEvent::HandshakeAck) => debug!("handshake ACK received"),
        Ok(other) => warn!("unexpected response to handshake: {other:?}"),
        Err(e) => warn!("failed to parse handshake response: {e}"),
    }

    seq.send(&aap::commands::SET_FEATURES).await?;
    debug!("sent feature enable");

    let n = seq.recv(&mut buf).await?;
    match parser::parse(&buf[..n]) {
        Ok(AapEvent::FeaturesAck) => debug!("features ACK received"),
        Ok(other) => warn!("unexpected response to features: {other:?}"),
        Err(e) => warn!("failed to parse features response: {e}"),
    }

    seq.send(&aap::commands::SUBSCRIBE_NOTIFICATIONS).await?;
    debug!("sent notification subscribe");

    // Enable all listening modes (Off + Noise + Transparency + Adaptive)
    // Some AirPods have "Off" disabled in their iPhone config, which causes
    // the Off command to be rejected with an error tone.
    seq.send(&aap::commands::ENABLE_ALL_LISTENING_MODES).await?;
    debug!("sent enable all listening modes");

    state.update(|s| s.connected = true);
    info!("handshake complete, entering main loop");

    // Main read/write loop
    loop {
        tokio::select! {
            // Read from AirPods
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
                            Ok(event) => {
                                apply_event(&state, &event);
                                let _ = event_tx.send(event).await;
                            }
                            Err(e) => {
                                debug!("parse error (non-fatal): {e}");
                            }
                        }
                    }
                    Err(e) => {
                        error!("L2CAP recv error: {e}");
                        break;
                    }
                }
            }
            // Write commands to AirPods
            cmd = cmd_rx.recv() => {
                match cmd {
                    Some(L2capCommand::SetAncMode(mode)) => {
                        // AirPods Pro 2 may have "Off" removed from the listening
                        // mode rotation (via iPhone settings synced through iCloud).
                        // The firmware rejects the Off command if it's not in the
                        // allowed set. Re-send the listening mode config before
                        // switching to Off to ensure it's permitted.
                        if mode == aap::AncMode::Off {
                            debug!("re-enabling Off in listening mode rotation before switching");
                            if let Err(e) = seq.send(&aap::commands::ENABLE_ALL_LISTENING_MODES).await {
                                error!("failed to send listening mode config: {e}");
                            }
                            // Small delay to let the firmware process the config
                            // before we send the actual mode change
                            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                        }
                        let pkt = aap::commands::set_anc_mode(mode);
                        debug!("sending ANC mode {:?}: {:02X?}", mode, pkt);
                        if let Err(e) = seq.send(&pkt).await {
                            error!("failed to send ANC command: {e}");
                        }
                    }
                    Some(L2capCommand::SetConversationalAwareness(enabled)) => {
                        let pkt = aap::commands::set_conversational_awareness(enabled);
                        if let Err(e) = seq.send(&pkt).await {
                            error!("failed to send CA command: {e}");
                        }
                    }
                    Some(L2capCommand::SetAdaptiveNoiseLevel(level)) => {
                        let pkt = aap::commands::set_adaptive_noise_level(level);
                        if let Err(e) = seq.send(&pkt).await {
                            error!("failed to send adaptive noise level command: {e}");
                        }
                    }
                    Some(L2capCommand::SetOneBudAnc(enabled)) => {
                        let pkt = aap::commands::set_one_bud_anc(enabled);
                        if let Err(e) = seq.send(&pkt).await {
                            error!("failed to send one-bud ANC command: {e}");
                        }
                    }
                    Some(L2capCommand::Disconnect) | None => {
                        info!("disconnect requested");
                        break;
                    }
                }
            }
        }
    }

    state.reset();
    info!("L2CAP disconnected");
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
                    // Only update case battery if it's a real reading (case is open/charging)
                    // When case closes, AirPods report 0% — preserve the last known value
                    if case.level > 0 || case.charging {
                        s.battery_case = case.level as i32;
                        s.charging_case = case.charging;
                    }
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
            use crate::aap::parser::CaActivity;
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
