use std::sync::Arc;
use tokio::sync::{watch, Mutex};

use crate::aap::{AncMode, MicMode};

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
    pub features: Vec<String>,
    pub auto_reconnect: bool,
    pub start_on_login: bool,
    /// User pref: pause MPRIS media when a bud is removed.
    pub ear_detection_pause: bool,
    /// User pref: resume MPRIS media when both buds are inserted.
    pub ear_detection_resume: bool,
    /// User pref: preferred AirPods MAC. When set, the embedded daemon
    /// prefers this device over any other paired AirPods during auto-discovery.
    /// Empty string = no preference.
    pub preferred_device: String,
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
            features: Vec::new(),
            auto_reconnect: true,
            start_on_login: false,
            ear_detection_pause: true,
            ear_detection_resume: true,
            preferred_device: String::new(),
        }
    }
}

/// User-controlled subset of state that persists to disk.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct PersistedSettings {
    #[serde(default)]
    pub auto_reconnect: Option<bool>,
    #[serde(default)]
    pub start_on_login: Option<bool>,
    #[serde(default)]
    pub ear_detection_pause: Option<bool>,
    #[serde(default)]
    pub ear_detection_resume: Option<bool>,
    #[serde(default)]
    pub preferred_device: Option<String>,
    #[serde(default)]
    pub eq_preset: Option<String>,
}

impl PersistedSettings {
    pub fn from_state(s: &AirPodsState) -> Self {
        Self {
            auto_reconnect: Some(s.auto_reconnect),
            start_on_login: Some(s.start_on_login),
            ear_detection_pause: Some(s.ear_detection_pause),
            ear_detection_resume: Some(s.ear_detection_resume),
            preferred_device: Some(s.preferred_device.clone()),
            eq_preset: Some(s.eq_preset.clone()),
        }
    }

    pub fn apply_to(self, s: &mut AirPodsState) {
        if let Some(v) = self.auto_reconnect {
            s.auto_reconnect = v;
        }
        if let Some(v) = self.start_on_login {
            s.start_on_login = v;
        }
        if let Some(v) = self.ear_detection_pause {
            s.ear_detection_pause = v;
        }
        if let Some(v) = self.ear_detection_resume {
            s.ear_detection_resume = v;
        }
        if let Some(v) = self.preferred_device {
            s.preferred_device = v;
        }
        if let Some(v) = self.eq_preset {
            s.eq_preset = v;
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
    SetMicMode(MicMode),
    Disconnect,
}

pub fn create_shared_state() -> SharedState {
    Arc::new(StateManager::new())
}
