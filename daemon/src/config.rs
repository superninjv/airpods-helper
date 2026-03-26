use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub device: DeviceConfig,
    #[serde(default)]
    pub eq: EqConfig,
    #[serde(default)]
    pub ear_detection: EarDetectionConfig,
    #[serde(default)]
    pub reconnect: ReconnectConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceConfig {
    /// Bluetooth address (auto-detected if not set)
    pub address: Option<String>,
    /// Device name override
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EqConfig {
    /// Active EQ preset name
    #[serde(default = "default_eq_preset")]
    pub active_preset: String,
    /// Auto-load EQ on connect
    #[serde(default = "default_true")]
    pub auto_load: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EarDetectionConfig {
    /// Pause media when both buds removed
    #[serde(default = "default_true")]
    pub pause_media: bool,
    /// Resume media when a bud is reinserted
    #[serde(default = "default_true")]
    pub resume_media: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReconnectConfig {
    /// Auto-reconnect on disconnect
    #[serde(default = "default_true")]
    pub auto_reconnect: bool,
    /// Maximum retry attempts
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,
}

fn default_eq_preset() -> String {
    "flat".to_string()
}

fn default_true() -> bool {
    true
}

fn default_max_retries() -> u32 {
    3
}

impl Default for DeviceConfig {
    fn default() -> Self {
        Self {
            address: None,
            name: None,
        }
    }
}

impl Default for EqConfig {
    fn default() -> Self {
        Self {
            active_preset: default_eq_preset(),
            auto_load: true,
        }
    }
}

impl Default for EarDetectionConfig {
    fn default() -> Self {
        Self {
            pause_media: true,
            resume_media: true,
        }
    }
}

impl Default for ReconnectConfig {
    fn default() -> Self {
        Self {
            auto_reconnect: true,
            max_retries: default_max_retries(),
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            device: DeviceConfig::default(),
            eq: EqConfig::default(),
            ear_detection: EarDetectionConfig::default(),
            reconnect: ReconnectConfig::default(),
        }
    }
}

impl Config {
    /// Load config from default path (~/.config/airpods-helper/config.toml)
    pub fn load() -> Self {
        let path = config_path();
        match std::fs::read_to_string(&path) {
            Ok(content) => toml::from_str(&content).unwrap_or_else(|e| {
                tracing::warn!("failed to parse config at {}: {e}", path.display());
                Config::default()
            }),
            Err(_) => {
                tracing::info!("no config found at {}, using defaults", path.display());
                Config::default()
            }
        }
    }

    /// Save config to default path
    pub fn save(&self) -> std::io::Result<()> {
        let path = config_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content =
            toml::to_string_pretty(self).map_err(|e| std::io::Error::other(e.to_string()))?;
        std::fs::write(path, content)
    }
}

fn config_path() -> PathBuf {
    dirs_config_path().join("config.toml")
}

pub fn dirs_config_path() -> PathBuf {
    let base = std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
            PathBuf::from(home).join(".config")
        });
    base.join("airpods-helper")
}

pub fn eq_presets_dir() -> PathBuf {
    dirs_config_path().join("eq")
}
