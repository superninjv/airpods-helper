use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DeviceConfig {
    /// Preferred AirPods MAC. When set, other AirPods are ignored.
    pub address: Option<String>,
    /// Device name override (informational)
    pub name: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EqBackendChoice {
    #[default]
    Auto,
    Pipewire,
    Pulseaudio,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EqConfig {
    /// Selected EQ preset id; empty string = EQ off
    #[serde(default)]
    pub active_preset: String,
    /// Apply the preset automatically whenever the AirPods connect
    #[serde(default = "default_true")]
    pub auto_load: bool,
    /// Audio backend: auto, pipewire, pulseaudio
    #[serde(default)]
    pub backend: EqBackendChoice,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EarDetectionConfig {
    /// Pause media when a bud is removed
    #[serde(default = "default_true")]
    pub pause_media: bool,
    /// Resume media when the bud goes back in
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

fn default_true() -> bool {
    true
}

fn default_max_retries() -> u32 {
    3
}

impl Default for EqConfig {
    fn default() -> Self {
        Self {
            active_preset: String::new(),
            auto_load: true,
            backend: EqBackendChoice::Auto,
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

impl Config {
    /// Load config from default path (~/.config/airpods-helper/config.toml)
    pub fn load() -> Self {
        let path = config_path();
        match std::fs::read_to_string(&path) {
            Ok(content) => toml::from_str(&content).unwrap_or_else(|e| {
                tracing::warn!(
                    "failed to parse config at {}: {e}; using defaults",
                    path.display()
                );
                Config::default()
            }),
            Err(_) => {
                tracing::info!("no config found at {}, using defaults", path.display());
                Config::default()
            }
        }
    }

    /// Preferred device as a parsed address (invalid values are ignored).
    pub fn preferred_device(&self) -> Option<bluer::Address> {
        self.device.address.as_deref()?.trim().parse().ok()
    }

    /// Write the settings the daemon manages back to config.toml, keeping the
    /// user's comments, ordering and any unknown keys intact.
    pub fn save(&self) -> std::io::Result<()> {
        use toml_edit::{DocumentMut, Item, Table, value};

        let path = config_path();
        let existing = std::fs::read_to_string(&path).unwrap_or_default();
        let mut doc: DocumentMut = existing
            .parse()
            .map_err(|e| std::io::Error::other(format!("{} doesn't parse: {e}", path.display())))?;

        fn table<'a>(doc: &'a mut DocumentMut, key: &str) -> &'a mut Table {
            if !doc.contains_table(key) {
                doc.insert(key, Item::Table(Table::new()));
            }
            doc[key].as_table_mut().expect("just ensured table")
        }

        let device = table(&mut doc, "device");
        match &self.device.address {
            Some(addr) if !addr.is_empty() => {
                device["address"] = value(addr.as_str());
            }
            _ => {
                device.remove("address");
            }
        }
        let eq = table(&mut doc, "eq");
        eq["active_preset"] = value(self.eq.active_preset.as_str());
        eq["auto_load"] = value(self.eq.auto_load);
        let ear = table(&mut doc, "ear_detection");
        ear["pause_media"] = value(self.ear_detection.pause_media);
        ear["resume_media"] = value(self.ear_detection.resume_media);
        let rc = table(&mut doc, "reconnect");
        rc["auto_reconnect"] = value(self.reconnect.auto_reconnect);

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = path.with_extension("toml.tmp");
        std::fs::write(&tmp, doc.to_string())?;
        std::fs::rename(&tmp, &path)
    }
}

/// Live, shared config. Writers must call [`update_config`] so changes persist.
pub type SharedConfig = Arc<RwLock<Config>>;

pub fn shared(config: Config) -> SharedConfig {
    Arc::new(RwLock::new(config))
}

/// Read a value out of the shared config.
pub fn read<T>(config: &SharedConfig, f: impl FnOnce(&Config) -> T) -> T {
    f(&config.read().unwrap_or_else(|e| e.into_inner()))
}

/// Mutate the shared config and persist it.
///
/// The file is re-read first, so hand edits made while the daemon runs are
/// picked up rather than overwritten. If it doesn't parse, nothing is
/// written: replacing a file with a typo in it would lose the user's config.
pub fn update_config(config: &SharedConfig, f: impl FnOnce(&mut Config)) -> std::io::Result<()> {
    let on_disk = match std::fs::read_to_string(config_path()) {
        Ok(text) => Some(toml::from_str::<Config>(&text).map_err(|e| {
            std::io::Error::other(format!(
                "{} has an error, fix it first: {}",
                config_path().display(),
                e.message()
            ))
        })?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e),
    };
    let snapshot = {
        let mut guard = config.write().unwrap_or_else(|e| e.into_inner());
        if let Some(fresh) = on_disk {
            *guard = fresh;
        }
        f(&mut guard);
        guard.clone()
    };
    snapshot.save()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_configs_still_parse() {
        let old =
            "[device]\n# address = \"AA\"\n[eq]\nactive_preset = \"flat\"\nauto_load = true\n";
        let c: Config = toml::from_str(old).unwrap();
        assert_eq!(c.eq.active_preset, "flat");
        assert_eq!(c.eq.backend, EqBackendChoice::Auto);
        assert!(c.ear_detection.pause_media);
    }

    #[test]
    fn save_preserves_comments_and_unknown_keys() {
        let dir = std::env::temp_dir().join(format!("aph-cfg-test-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("airpods-helper")).unwrap();
        let path = dir.join("airpods-helper/config.toml");
        std::fs::write(&path, "# my notes\n[eq]\nactive_preset = \"flat\" # keep me\nbackend = \"pipewire\"\n[custom]\nx = 1\n").unwrap();
        // SAFETY: tests in this module don't read XDG_CONFIG_HOME concurrently.
        unsafe { std::env::set_var("XDG_CONFIG_HOME", &dir) };
        let mut c = Config::load();
        assert_eq!(c.eq.backend, EqBackendChoice::Pipewire);
        c.eq.active_preset = "bass-boost".into();
        c.device.address = Some("AA:BB:CC:DD:EE:FF".into());
        c.save().unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        unsafe { std::env::remove_var("XDG_CONFIG_HOME") };
        std::fs::remove_dir_all(&dir).ok();
        assert!(text.contains("# my notes"), "{text}");
        assert!(text.contains("active_preset = \"bass-boost\""), "{text}");
        assert!(text.contains("backend = \"pipewire\""), "{text}");
        assert!(text.contains("[custom]"), "{text}");
        assert!(text.contains("address = \"AA:BB:CC:DD:EE:FF\""), "{text}");
    }
}
