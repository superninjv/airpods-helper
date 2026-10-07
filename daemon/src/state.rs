use std::sync::Arc;
use tokio::sync::watch;

use aap::AncMode;

/// Shared AirPods state, updated by the L2CAP reader and consumed by D-Bus
#[derive(Debug, Clone, PartialEq)]
pub struct AirPodsState {
    pub connected: bool,
    /// MAC of the connected AirPods ("" when disconnected)
    pub address: String,
    pub battery_left: i32,
    pub battery_right: i32,
    pub battery_case: i32,
    pub charging_left: bool,
    pub charging_right: bool,
    pub charging_case: bool,
    pub anc_mode: AncMode,
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
    pub mic_mode: String,
    pub conversational_activity: String,
    pub features: Vec<String>,
    // EQ — owned by the EQ manager, survives device resets
    pub eq_preset: String,
    pub eq_status: String,
    pub eq_error: String,
    pub eq_backend: String,
    // Microphone — owned by the mic supervisor, survives device resets
    pub mic_status: String,
    pub mic_error: String,
}

impl Default for AirPodsState {
    fn default() -> Self {
        Self {
            connected: false,
            address: String::new(),
            battery_left: -1,
            battery_right: -1,
            battery_case: -1,
            charging_left: false,
            charging_right: false,
            charging_case: false,
            anc_mode: AncMode::Off,
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
            mic_mode: "auto".to_string(),
            conversational_activity: "normal".to_string(),
            features: Vec::new(),
            eq_preset: String::new(),
            eq_status: "off".to_string(),
            eq_error: String::new(),
            eq_backend: "none".to_string(),
            mic_status: "off".to_string(),
            mic_error: String::new(),
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

    /// Get a receiver for watching state changes
    pub fn subscribe(&self) -> watch::Receiver<AirPodsState> {
        self.rx.clone()
    }

    /// Update state with a closure, automatically notifying all watchers
    pub fn update<F>(&self, f: F)
    where
        F: FnOnce(&mut AirPodsState),
    {
        self.tx.send_modify(f);
    }

    /// Update state; `f` returns whether anything changed, and watchers are
    /// only notified if it did.
    pub fn update_if_changed<F>(&self, f: F)
    where
        F: FnOnce(&mut AirPodsState) -> bool,
    {
        self.tx.send_if_modified(f);
    }

    /// Get current state snapshot
    pub fn current(&self) -> AirPodsState {
        self.rx.borrow().clone()
    }

    /// Reset device state to disconnected defaults. EQ and mic fields are kept:
    /// their own supervisors publish them, and they follow the session anyway.
    pub fn reset(&self) {
        self.tx.send_if_modified(|state| {
            let fresh = AirPodsState {
                eq_preset: state.eq_preset.clone(),
                eq_status: state.eq_status.clone(),
                eq_error: state.eq_error.clone(),
                eq_backend: state.eq_backend.clone(),
                mic_status: state.mic_status.clone(),
                mic_error: state.mic_error.clone(),
                ..AirPodsState::default()
            };
            let changed = *state != fresh;
            *state = fresh;
            changed
        });
    }
}

/// Shared state handle that can be passed between tasks
pub type SharedState = Arc<StateManager>;

pub fn create_shared_state() -> SharedState {
    Arc::new(StateManager::new())
}
