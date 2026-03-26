use serde::Deserialize;
use std::path::PathBuf;
use std::process::Stdio;
use tokio::process::Command;
use tracing::{info, warn};

use crate::config;

/// Commands sent from D-Bus to the EQ manager in the main event loop
#[derive(Debug)]
pub enum EqCommand {
    /// Apply an EQ preset by name
    Apply(String),
    /// Disable EQ (stop the filter chain)
    Disable,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EqPreset {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub preamp: f64,
    #[serde(default)]
    pub bands: Vec<EqBand>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EqBand {
    #[serde(rename = "type")]
    pub filter_type: String,
    pub freq: f64,
    pub q: f64,
    pub gain: f64,
}

impl EqPreset {
    /// Load an EQ preset from the config directory by name
    pub fn load(name: &str) -> Option<Self> {
        let path = config::eq_presets_dir().join(format!("{name}.toml"));
        let content = std::fs::read_to_string(&path)
            .inspect_err(|e| warn!("failed to read preset {name}: {e}"))
            .ok()?;
        toml::from_str(&content)
            .inspect_err(|e| warn!("failed to parse preset {name}: {e}"))
            .ok()
    }

    /// List all available preset names (without .toml extension)
    pub fn list_available() -> Vec<String> {
        let dir = config::eq_presets_dir();
        let Ok(entries) = std::fs::read_dir(&dir) else {
            warn!("cannot read EQ presets dir: {}", dir.display());
            return Vec::new();
        };
        let mut names: Vec<String> = entries
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let path = e.path();
                if path.extension().and_then(|s| s.to_str()) == Some("toml") {
                    path.file_stem()
                        .and_then(|s| s.to_str())
                        .map(|s| s.to_string())
                } else {
                    None
                }
            })
            .collect();
        names.sort();
        names
    }

    /// Returns true if this preset is effectively a no-op (no bands and no preamp)
    pub fn is_flat(&self) -> bool {
        self.bands.is_empty() && self.preamp.abs() < 0.001
    }
}

/// PipeWire config drop-in path for the EQ filter chain
fn dropin_path() -> PathBuf {
    let base = std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
            PathBuf::from(home).join(".config")
        });
    base.join("pipewire/pipewire.conf.d/99-airpods-eq.conf")
}

/// Manages the PipeWire EQ filter chain via config drop-in
pub struct EqManager {
    active_preset: Option<String>,
}

impl EqManager {
    pub fn new() -> Self {
        Self {
            active_preset: None,
        }
    }

    /// Apply an EQ preset by writing a PipeWire config drop-in and reloading
    pub async fn apply(&mut self, preset: &EqPreset) -> anyhow::Result<()> {
        self.stop().await;

        if preset.is_flat() {
            info!("EQ preset '{}' is flat, no filter chain needed", preset.name);
            self.active_preset = Some(preset.name.clone());
            return Ok(());
        }

        let path = dropin_path();
        let config_content = generate_filter_config(preset);

        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::write(&path, &config_content).await?;

        info!("wrote EQ config to {}", path.display());

        // Tell PipeWire to reload by restarting it
        // This picks up the new drop-in config
        let _ = Command::new("systemctl")
            .args(["--user", "restart", "pipewire.service"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await;

        self.active_preset = Some(preset.name.clone());
        info!("EQ applied: {}", preset.name);
        Ok(())
    }

    /// Stop the EQ filter chain by removing the config drop-in and reloading
    pub async fn stop(&mut self) {
        let path = dropin_path();
        if path.exists() {
            if let Err(e) = tokio::fs::remove_file(&path).await {
                warn!("failed to remove EQ config: {e}");
            }

            let _ = Command::new("systemctl")
                .args(["--user", "restart", "pipewire.service"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .await;

            info!("EQ filter chain stopped");
        }
        self.active_preset = None;
    }

    /// Get the currently active preset name
    pub fn active_preset(&self) -> Option<&str> {
        self.active_preset.as_deref()
    }
}

/// Generate PipeWire filter-chain config for parametric EQ
fn generate_filter_config(preset: &EqPreset) -> String {
    let mut nodes = Vec::new();
    let mut node_names = Vec::new();

    // Add preamp node if non-zero
    if preset.preamp.abs() > 0.001 {
        let name = "preamp";
        nodes.push(format!(
            "          {{ type = builtin label = bq_highshelf name = {name} \
             control = {{ \"Freq\" = 0.0 \"Q\" = 1.0 \"Gain\" = {:.1} }} }}",
            preset.preamp
        ));
        node_names.push(name.to_string());
    }

    // Add EQ band nodes
    for (i, band) in preset.bands.iter().enumerate() {
        let label = match band.filter_type.as_str() {
            "lowshelf" => "bq_lowshelf",
            "highshelf" => "bq_highshelf",
            _ => "bq_peaking",
        };
        let name = format!("eq{i}");
        nodes.push(format!(
            "          {{ type = builtin label = {label} name = {name} \
             control = {{ \"Freq\" = {:.1} \"Q\" = {:.2} \"Gain\" = {:.1} }} }}",
            band.freq, band.q, band.gain
        ));
        node_names.push(name);
    }

    // Build links chain
    let mut links = Vec::new();
    for i in 0..node_names.len().saturating_sub(1) {
        links.push(format!(
            "          {{ output = \"{}:Out\" input = \"{}:In\" }}",
            node_names[i], node_names[i + 1]
        ));
    }

    let nodes_str = nodes.join("\n");
    let links_str = if links.is_empty() {
        String::new()
    } else {
        format!(
            "\n        links = [\n{}\n        ]",
            links.join("\n")
        )
    };

    format!(
        r#"# Auto-generated by airpods-daemon EQ — preset: {name}
context.modules = [
  {{ name = libpipewire-module-filter-chain
    args = {{
      node.description = "AirPods EQ ({name})"
      media.name = "AirPods EQ"
      filter.graph = {{
        nodes = [
{nodes_str}
        ]{links_str}
      }}
      capture.props = {{
        node.name = "airpods_eq_capture"
        media.class = "Audio/Sink"
        audio.position = [FL FR]
      }}
      playback.props = {{
        node.name = "airpods_eq_playback"
      }}
    }}
  }}
]
"#,
        name = preset.name,
    )
}
