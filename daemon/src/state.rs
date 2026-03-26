use std::sync::Arc;
use tokio::sync::watch;

use crate::aap::AncMode;

/// Shared AirPods state, updated by the L2CAP reader and consumed by D-Bus
#[derive(Debug, Clone)]
pub struct AirPodsState {
    pub connected: bool,
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
    pub model: String,
    pub firmware: String,
    pub eq_preset: String,
    pub conversational_activity: String,
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
            anc_mode: AncMode::Off,
            ear_left: false,
            ear_right: false,
            conversational_awareness: false,
            adaptive_noise_level: 50,
            one_bud_anc: true,
            model: String::new(),
            firmware: String::new(),
            eq_preset: String::new(),
            conversational_activity: "normal".to_string(),
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

/// Shared state handle that can be passed between tasks
pub type SharedState = Arc<StateManager>;

pub fn create_shared_state() -> SharedState {
    Arc::new(StateManager::new())
}
