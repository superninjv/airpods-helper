use std::sync::Arc;
use tokio::sync::{watch, Mutex};

use crate::aap::AncMode;

/// Shared AirPods state, updated by the daemon and consumed by the frontend
#[derive(Debug, Clone, serde::Serialize)]
pub struct AirPodsState {
    pub connected: bool,
    pub battery_left: i32,
    pub battery_right: i32,
    pub battery_case: i32,
    pub charging_left: bool,
    pub charging_right: bool,
    pub charging_case: bool,
    pub anc_mode: String,
    pub ear_left: bool,
    pub ear_right: bool,
    pub conversational_awareness: bool,
    pub adaptive_noise_level: u8,
    pub one_bud_anc: bool,
    pub volume_swipe: bool,
    pub adaptive_volume: bool,
    pub chime_volume: u8,
    pub audio_source: String,
    pub model: String,
    pub model_name: String,
    pub firmware: String,
    pub eq_preset: String,
    pub auto_reconnect: bool,
    pub start_on_login: bool,
}

impl Default for AirPodsState {
    fn default() -> Self {
        Self {
            connected: false,
            battery_left: -1,
            battery_right: -1,
            battery_case: -1,
            charging_left: false,
            charging_right: false,
            charging_case: false,
            anc_mode: AncMode::Off.as_str().to_string(),
            ear_left: false,
            ear_right: false,
            conversational_awareness: false,
            adaptive_noise_level: 50,
            one_bud_anc: true,
            volume_swipe: true,
            adaptive_volume: false,
            chime_volume: 80,
            audio_source: "none".to_string(),
            model: String::new(),
            model_name: String::new(),
            firmware: String::new(),
            eq_preset: "flat".to_string(),
            auto_reconnect: true,
            start_on_login: false,
        }
    }
}

/// State manager providing watch channels for reactive updates
pub struct StateManager {
    tx: watch::Sender<AirPodsState>,
    rx: watch::Receiver<AirPodsState>,
}

impl StateManager {
    pub fn new() -> Self {
        let (tx, rx) = watch::channel(AirPodsState::default());
        Self { tx, rx }
    }

    /// Update state with a closure, automatically notifying all watchers
    pub fn update<F>(&self, f: F)
    where
        F: FnOnce(&mut AirPodsState),
    {
        self.tx.send_modify(f);
    }

    /// Get current state snapshot
    pub fn current(&self) -> AirPodsState {
        self.rx.borrow().clone()
    }

    /// Reset to disconnected defaults
    pub fn reset(&self) {
        self.tx.send_modify(|state| {
            *state = AirPodsState::default();
        });
    }
}

/// Shared state handle
pub type SharedState = Arc<StateManager>;

/// Command sender for L2CAP write loop
pub type CommandSender = Arc<Mutex<Option<tokio::sync::mpsc::Sender<DaemonCommand>>>>;

/// Commands that can be sent to the daemon's L2CAP write loop
#[derive(Debug)]
pub enum DaemonCommand {
    SetAncMode(AncMode),
    SetConversationalAwareness(bool),
    SetAdaptiveNoiseLevel(u8),
    SetOneBudAnc(bool),
    SetVolumeSwipe(bool),
    Disconnect,
}

pub fn create_shared_state() -> SharedState {
    Arc::new(StateManager::new())
}
