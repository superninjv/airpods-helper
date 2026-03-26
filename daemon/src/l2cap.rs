use bluer::l2cap::{SeqPacket, Socket, SocketAddr};
use bluer::Address;
use std::io;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use crate::aap;
use crate::aap::parser::{self, AapEvent};
use crate::state::SharedState;

/// Commands that can be sent to the AirPods over L2CAP
#[derive(Debug)]
pub enum L2capCommand {
    SetAncMode(aap::AncMode),
    SetConversationalAwareness(bool),
    SetAdaptiveNoiseLevel(u8),
    SetOneBudAnc(bool),
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
                        let pkt = aap::commands::set_anc_mode(mode);
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
                    s.battery_case = case.level as i32;
                    s.charging_case = case.charging;
                }
            });
        }
        AapEvent::AncMode(mode) => {
            state.update(|s| s.anc_mode = *mode);
        }
        AapEvent::EarDetection(ed) => {
            state.update(|s| {
                s.ear_left = ed.primary.is_in_ear();
                s.ear_right = ed.secondary.is_in_ear();
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
        AapEvent::DeviceInfo(info) => {
            state.update(|s| {
                s.model = info.model.clone();
                s.firmware = info.firmware.clone();
            });
        }
        _ => {}
    }
}
