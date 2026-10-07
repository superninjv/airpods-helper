//! Frontend-facing state model.
//!
//! The app holds no AirPods state of its own: `Status` is a mirror of the
//! daemon's properties (plus a few app-local bits such as "start on login")
//! kept up to date by the D-Bus client in `linux.rs` and pushed to the webview as a
//! `status` event whenever it changes.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::watch;

/// Whether the daemon side is reachable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DaemonState {
    /// Still trying to reach (or D-Bus-activate) the daemon.
    Starting,
    /// Daemon is on the bus / HTTP API answered.
    Running,
    /// Daemon is not running and could not be started.
    Absent,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EqPresetInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub user_editable: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EqBand {
    #[serde(rename = "type")]
    pub kind: String,
    pub freq: f64,
    pub q: f64,
    pub gain: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EqPresetDetail {
    pub id: String,
    pub name: String,
    pub description: String,
    pub preamp: f64,
    pub bands: Vec<EqBand>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PairedDevice {
    pub address: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct QuickPairCandidate {
    pub address: String,
    pub name: String,
    pub model: String,
    pub rssi: i16,
    pub in_pair_mode: bool,
}

/// Everything the UI renders. Field names are the JSON keys used by `main.js`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Status {
    pub daemon: DaemonState,
    /// Human-readable reason the daemon is unreachable (empty otherwise).
    pub daemon_error: String,
    /// The daemon answered but does not expose API v2 properties.
    pub daemon_outdated: bool,
    pub version: String,

    pub connected: bool,
    pub address: String,
    pub model: String,
    pub model_name: String,
    pub firmware: String,
    pub features: Vec<String>,

    pub battery_left: i32,
    pub battery_right: i32,
    pub battery_case: i32,
    pub charging_left: bool,
    pub charging_right: bool,
    pub charging_case: bool,
    pub ear_left: bool,
    pub ear_right: bool,

    pub anc_mode: String,
    pub adaptive_noise_level: u8,
    pub conversational_awareness: bool,
    pub conversational_activity_state: String,
    pub one_bud_anc: bool,
    pub volume_swipe: bool,
    pub mic_mode: String,

    pub eq_preset: String,
    pub eq_status: String,
    pub eq_error: String,
    pub eq_backend: String,
    pub eq_presets: Vec<EqPresetInfo>,

    // Microphone source (off | unavailable | idle | starting | streaming | error).
    pub mic_status: String,
    pub mic_error: String,

    // Daemon settings (read-write properties).
    pub pause_on_removal: bool,
    pub resume_on_insert: bool,
    pub auto_reconnect: bool,
    pub preferred_device: String,
    pub eq_auto_load: bool,
    pub mic_source: bool,

    // App-local.
    pub start_on_login: bool,
}

impl Default for Status {
    fn default() -> Self {
        Self {
            daemon: DaemonState::Starting,
            daemon_error: String::new(),
            daemon_outdated: false,
            version: String::new(),
            connected: false,
            address: String::new(),
            model: String::new(),
            model_name: String::new(),
            firmware: String::new(),
            features: Vec::new(),
            battery_left: -1,
            battery_right: -1,
            battery_case: -1,
            charging_left: false,
            charging_right: false,
            charging_case: false,
            ear_left: false,
            ear_right: false,
            anc_mode: "off".into(),
            adaptive_noise_level: 50,
            conversational_awareness: false,
            conversational_activity_state: "normal".into(),
            one_bud_anc: false,
            volume_swipe: false,
            mic_mode: "auto".into(),
            eq_preset: String::new(),
            eq_status: "off".into(),
            eq_error: String::new(),
            eq_backend: "none".into(),
            eq_presets: Vec::new(),
            mic_status: "off".into(),
            mic_error: String::new(),
            pause_on_removal: true,
            resume_on_insert: true,
            auto_reconnect: true,
            preferred_device: String::new(),
            eq_auto_load: true,
            mic_source: true,
            start_on_login: false,
        }
    }
}

impl Status {
    pub fn has(&self, feature: &str) -> bool {
        self.features.iter().any(|f| f == feature)
    }

    /// Wipe everything the daemon owns, keeping app-local fields.
    pub fn clear_daemon_fields(&mut self) {
        let keep_login = self.start_on_login;
        *self = Status {
            start_on_login: keep_login,
            ..Status::default()
        };
    }

    /// Display name for tray/tooltip.
    pub fn display_name(&self) -> &str {
        if !self.model_name.is_empty() {
            &self.model_name
        } else {
            "AirPods"
        }
    }

    /// One-line battery summary, e.g. `L 80% · R 75% · Case 40%`.
    pub fn battery_summary(&self) -> String {
        fn pct(v: i32, charging: bool) -> String {
            if v < 0 {
                "—".into()
            } else if charging {
                format!("{v}% ⚡")
            } else {
                format!("{v}%")
            }
        }
        if self.has("headphones") {
            // Over-ear: single battery mirrored into Left/Right.
            let level = self.battery_left.max(self.battery_right);
            return pct(level, self.charging_left || self.charging_right);
        }
        let mut parts = Vec::new();
        if self.battery_left >= 0 {
            parts.push(format!("L {}", pct(self.battery_left, self.charging_left)));
        }
        if self.battery_right >= 0 {
            parts.push(format!(
                "R {}",
                pct(self.battery_right, self.charging_right)
            ));
        }
        if self.battery_case >= 0 {
            parts.push(format!(
                "Case {}",
                pct(self.battery_case, self.charging_case)
            ));
        }
        parts.join(" · ")
    }

    pub fn tooltip(&self) -> String {
        match self.daemon {
            DaemonState::Absent => "AirPods Helper — daemon not running".into(),
            DaemonState::Starting => "AirPods Helper".into(),
            DaemonState::Running if !self.connected => "AirPods Helper — disconnected".into(),
            DaemonState::Running => {
                let battery = self.battery_summary();
                if battery.is_empty() {
                    format!("{} — connected", self.display_name())
                } else {
                    format!("{} — {battery}", self.display_name())
                }
            }
        }
    }
}

/// Shared, observable status. Writers call `update`; the event pump and the
/// tray subscribe to changes.
pub struct Store {
    tx: watch::Sender<Status>,
}

pub type SharedStore = Arc<Store>;

impl Store {
    pub fn new() -> SharedStore {
        let (tx, _rx) = watch::channel(Status::default());
        Arc::new(Self { tx })
    }

    /// Apply a mutation; subscribers are only woken if something changed.
    pub fn update(&self, f: impl FnOnce(&mut Status)) {
        self.tx.send_if_modified(|s| {
            let before = s.clone();
            f(s);
            *s != before
        });
    }

    pub fn current(&self) -> Status {
        self.tx.borrow().clone()
    }

    pub fn subscribe(&self) -> watch::Receiver<Status> {
        self.tx.subscribe()
    }
}
