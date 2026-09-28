//! Windows backend: a client of the standalone `airpods-windows daemon`
//! (see `windows/`), which serves a small HTTP API on 127.0.0.1:7654.
//!
//! The HTTP API only covers status + the basic controls, so device management,
//! EQ and daemon settings report "not supported on Windows yet". The method set
//! mirrors `linux::Client` so `main.rs` is platform-agnostic.

use std::sync::Arc;
use std::time::Duration;

use crate::models;
use crate::status::{
    DaemonState, EqPresetDetail, EqPresetInfo, PairedDevice, QuickPairCandidate, SharedStore,
};

const API_BASE: &str = "http://127.0.0.1:7654";
const UNSUPPORTED: &str = "Not supported on Windows yet.";

pub struct Client {
    store: SharedStore,
    http: reqwest::Client,
}

impl Client {
    pub fn new(store: SharedStore) -> Arc<Self> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap_or_default();
        Arc::new(Self { store, http })
    }

    pub fn start(self: &Arc<Self>) {
        let this = self.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                this.poll().await;
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        });
    }

    async fn poll(&self) {
        let resp = match self.http.get(format!("{API_BASE}/status")).send().await {
            Ok(r) => r,
            Err(_) => {
                self.store.update(|s| {
                    s.clear_daemon_fields();
                    s.daemon = DaemonState::Absent;
                    s.daemon_error = "Could not reach airpods-windows on 127.0.0.1:7654.".into();
                });
                return;
            }
        };
        let Ok(json) = resp.json::<serde_json::Value>().await else {
            return;
        };
        let b = |k: &str, d: bool| json[k].as_bool().unwrap_or(d);
        let i = |k: &str| json[k].as_i64().unwrap_or(-1).clamp(-1, 100) as i32;
        let st = |k: &str| json[k].as_str().unwrap_or_default().to_string();
        self.store.update(|s| {
            s.daemon = DaemonState::Running;
            s.daemon_error.clear();
            s.connected = b("connected", false);
            if !s.connected {
                s.clear_daemon_fields();
                s.daemon = DaemonState::Running;
                return;
            }
            s.battery_left = i("battery_left");
            s.battery_right = i("battery_right");
            s.battery_case = i("battery_case");
            s.charging_left = b("charging_left", false);
            s.charging_right = b("charging_right", false);
            s.charging_case = b("charging_case", false);
            s.anc_mode = json["anc_mode"].as_str().unwrap_or("off").to_string();
            s.ear_left = b("ear_left", false);
            s.ear_right = b("ear_right", false);
            s.conversational_awareness = b("conversational_awareness", false);
            s.adaptive_noise_level =
                json["adaptive_noise_level"].as_u64().unwrap_or(50).min(100) as u8;
            s.one_bud_anc = b("one_bud_anc", true);
            s.volume_swipe = b("volume_swipe", true);
            s.model = st("model");
            s.features = models::model_features(&s.model)
                .into_iter()
                .map(str::to_string)
                .collect();
            s.model_name = match st("model_name") {
                n if !n.is_empty() => n,
                _ => models::model_display_name(&s.model).to_string(),
            };
            s.firmware = st("firmware");
        });
    }

    async fn post(&self, path: &str, body: serde_json::Value) -> Result<(), String> {
        let resp = self
            .http
            .post(format!("{API_BASE}{path}"))
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("airpods-windows: {e}"))?;
        if resp.status().is_success() {
            Ok(())
        } else {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            Err(if text.is_empty() {
                status.to_string()
            } else {
                text
            })
        }
    }

    pub async fn retry(&self) -> Result<(), String> {
        self.poll().await;
        let s = self.store.current();
        if s.daemon == DaemonState::Running {
            Ok(())
        } else {
            Err(s.daemon_error)
        }
    }

    pub async fn set_anc_mode(&self, mode: String) -> Result<(), String> {
        self.post("/anc", serde_json::json!({ "mode": mode })).await
    }

    pub async fn set_adaptive_noise_level(&self, level: u8) -> Result<(), String> {
        self.post("/noise", serde_json::json!({ "level": level.min(100) }))
            .await
    }

    pub async fn set_conversational_awareness(&self, enabled: bool) -> Result<(), String> {
        self.post("/ca", serde_json::json!({ "enabled": enabled }))
            .await
    }

    pub async fn set_one_bud_anc(&self, enabled: bool) -> Result<(), String> {
        self.post("/one-bud-anc", serde_json::json!({ "enabled": enabled }))
            .await
    }

    pub async fn set_volume_swipe(&self, enabled: bool) -> Result<(), String> {
        self.post("/volume-swipe", serde_json::json!({ "enabled": enabled }))
            .await
    }

    pub async fn set_mic_mode(&self, _mode: String) -> Result<(), String> {
        Err(UNSUPPORTED.into())
    }

    pub async fn set_setting(&self, _key: &str, _value: serde_json::Value) -> Result<(), String> {
        Err(UNSUPPORTED.into())
    }

    pub async fn list_paired(&self) -> Result<Vec<PairedDevice>, String> {
        Ok(Vec::new())
    }

    pub async fn connect(&self, _address: String) -> Result<(), String> {
        Err(UNSUPPORTED.into())
    }

    pub async fn disconnect(&self) -> Result<(), String> {
        Err(UNSUPPORTED.into())
    }

    pub async fn reconnect(&self) -> Result<(), String> {
        Err(UNSUPPORTED.into())
    }

    pub async fn pair(&self, _address: String) -> Result<(), String> {
        Err(UNSUPPORTED.into())
    }

    pub async fn quick_pair_scan(&self, _seconds: u32) -> Result<Vec<QuickPairCandidate>, String> {
        Err(UNSUPPORTED.into())
    }

    pub async fn refresh_presets(&self) -> Result<Vec<EqPresetInfo>, String> {
        Ok(Vec::new())
    }

    pub async fn get_eq_preset(&self, _id: String) -> Result<EqPresetDetail, String> {
        Err(UNSUPPORTED.into())
    }

    pub async fn set_eq_preset(&self, _id: String) -> Result<(), String> {
        Err(UNSUPPORTED.into())
    }

    pub async fn save_eq_preset(&self, _preset: EqPresetDetail) -> Result<(), String> {
        Err(UNSUPPORTED.into())
    }

    pub async fn delete_eq_preset(&self, _id: String) -> Result<(), String> {
        Err(UNSUPPORTED.into())
    }
}
