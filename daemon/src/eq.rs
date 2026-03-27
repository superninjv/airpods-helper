use serde::Deserialize;
use std::path::PathBuf;
use std::process::Stdio;
use tokio::process::Command;
use tracing::{debug, error, info, warn};

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
    #[allow(dead_code)] // displayed by CLI eq list command
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
    /// Load an EQ preset by name, checking user dir then system dir
    pub fn load(name: &str) -> Option<Self> {
        let filename = format!("{name}.toml");
        // User presets take priority
        let user_path = config::eq_presets_dir().join(&filename);
        if let Ok(content) = std::fs::read_to_string(&user_path) {
            return toml::from_str(&content)
                .inspect_err(|e| warn!("failed to parse preset {name}: {e}"))
                .ok();
        }
        // Fall back to system-installed presets
        let system_path = PathBuf::from("/usr/share/airpods-helper/eq-presets").join(&filename);
        if let Ok(content) = std::fs::read_to_string(&system_path) {
            return toml::from_str(&content)
                .inspect_err(|e| warn!("failed to parse system preset {name}: {e}"))
                .ok();
        }
        warn!("preset {name} not found in user or system dirs");
        None
    }

    /// List all available preset names from user + system dirs
    pub fn list_available() -> Vec<String> {
        let mut names = std::collections::HashSet::new();
        for dir in [
            config::eq_presets_dir(),
            PathBuf::from("/usr/share/airpods-helper/eq-presets"),
        ] {
            if let Ok(entries) = std::fs::read_dir(&dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().and_then(|s| s.to_str()) == Some("toml") {
                        if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                            names.insert(stem.to_string());
                        }
                    }
                }
            }
        }
        let mut sorted: Vec<String> = names.into_iter().collect();
        sorted.sort();
        sorted
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

/// Manages the PipeWire EQ filter chain via pw-cli module hot-loading.
///
/// Instead of restarting PipeWire (which kills all audio), we:
/// 1. Write the filter-chain config to the drop-in dir (for persistence across PipeWire restarts)
/// 2. Use `pw-cli load-module` to load the filter chain into the running PipeWire instance
/// 3. Use `pw-cli unload-module` to remove it when switching or disabling
pub struct EqManager {
    active_preset: Option<String>,
    /// PipeWire module ID returned by `pw-cli load-module`, used for unloading
    loaded_module_id: Option<u32>,
}

impl EqManager {
    pub fn new() -> Self {
        Self {
            active_preset: None,
            loaded_module_id: None,
        }
    }

    /// Apply an EQ preset by hot-loading a PipeWire filter-chain module.
    /// No PipeWire restart is needed -- audio continues uninterrupted.
    pub async fn apply(&mut self, preset: &EqPreset) -> anyhow::Result<()> {
        // Unload any existing filter chain first
        self.unload_module().await;

        if preset.is_flat() {
            info!("EQ preset '{}' is flat, no filter chain needed", preset.name);
            self.active_preset = Some(preset.name.clone());
            // Remove stale drop-in config since we're not loading anything
            self.remove_dropin().await;
            return Ok(());
        }

        // Write the drop-in config for persistence (survives PipeWire restarts)
        let path = dropin_path();
        let config_content = generate_filter_config(preset);
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::write(&path, &config_content).await?;
        debug!("wrote EQ drop-in config to {}", path.display());

        // Hot-load the module into the running PipeWire instance
        let module_args = generate_module_args(preset);
        match load_filter_module(&module_args).await {
            Ok(module_id) => {
                self.loaded_module_id = Some(module_id);
                self.active_preset = Some(preset.name.clone());
                info!("EQ applied (module {}): {}", module_id, preset.name);
            }
            Err(e) => {
                error!("pw-cli load-module failed: {e}");
                return Err(e);
            }
        }

        Ok(())
    }

    /// Stop the EQ filter chain by unloading the module and removing the drop-in config.
    /// No PipeWire restart is needed.
    pub async fn stop(&mut self) {
        self.unload_module().await;
        self.remove_dropin().await;
        self.active_preset = None;
    }

    /// Unload the currently loaded filter-chain module via pw-cli
    async fn unload_module(&mut self) {
        if let Some(module_id) = self.loaded_module_id.take() {
            let result = Command::new("pw-cli")
                .args(["unload-module", &module_id.to_string()])
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .output()
                .await;

            match result {
                Ok(output) if output.status.success() => {
                    info!("unloaded EQ filter-chain module {module_id}");
                }
                Ok(output) => {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    warn!("pw-cli unload-module {module_id} failed: {stderr}");
                }
                Err(e) => {
                    warn!("failed to run pw-cli unload-module: {e}");
                }
            }
        }
    }

    /// Remove the drop-in config file
    async fn remove_dropin(&self) {
        let path = dropin_path();
        if path.exists() {
            if let Err(e) = tokio::fs::remove_file(&path).await {
                warn!("failed to remove EQ drop-in config: {e}");
            } else {
                debug!("removed EQ drop-in config at {}", path.display());
            }
        }
    }

    /// Get the currently active preset name (used by CLI introspection)
    #[allow(dead_code)]
    pub fn active_preset(&self) -> Option<&str> {
        self.active_preset.as_deref()
    }
}

/// Load a filter-chain module into PipeWire via pw-cli and return the module ID.
async fn load_filter_module(args: &str) -> anyhow::Result<u32> {
    let output = Command::new("pw-cli")
        .args(["load-module", "libpipewire-module-filter-chain", args])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("pw-cli load-module failed: {stderr}");
    }

    // pw-cli load-module prints the module ID on stdout (e.g. "35\n")
    let stdout = String::from_utf8_lossy(&output.stdout);
    let module_id: u32 = stdout
        .trim()
        .parse()
        .map_err(|e| anyhow::anyhow!("failed to parse module ID from pw-cli output '{stdout}': {e}"))?;

    Ok(module_id)
}

/// Generate pw-cli load-module arguments as a single SPA JSON string.
/// This is the inline equivalent of the drop-in config file's args block.
fn generate_module_args(preset: &EqPreset) -> String {
    let mut nodes = Vec::new();
    let mut node_names = Vec::new();

    // Preamp node
    if preset.preamp.abs() > 0.001 {
        let name = "preamp";
        nodes.push(format!(
            "{{ type = builtin label = bq_highshelf name = {name} \
             control = {{ \"Freq\" = 0.0 \"Q\" = 1.0 \"Gain\" = {:.1} }} }}",
            preset.preamp
        ));
        node_names.push(name.to_string());
    }

    // EQ band nodes
    for (i, band) in preset.bands.iter().enumerate() {
        let label = match band.filter_type.as_str() {
            "lowshelf" => "bq_lowshelf",
            "highshelf" => "bq_highshelf",
            _ => "bq_peaking",
        };
        let name = format!("eq{i}");
        nodes.push(format!(
            "{{ type = builtin label = {label} name = {name} \
             control = {{ \"Freq\" = {:.1} \"Q\" = {:.2} \"Gain\" = {:.1} }} }}",
            band.freq, band.q, band.gain
        ));
        node_names.push(name);
    }

    let nodes_str = nodes.join(" ");

    // Build links chain
    let links: Vec<String> = (0..node_names.len().saturating_sub(1))
        .map(|i| {
            format!(
                "{{ output = \"{}:Out\" input = \"{}:In\" }}",
                node_names[i], node_names[i + 1]
            )
        })
        .collect();

    let links_str = if links.is_empty() {
        String::new()
    } else {
        format!(" links = [ {} ]", links.join(" "))
    };

    // Escape preset name for use in SPA properties
    let safe_name = preset.name.replace('"', "'");

    format!(
        "{{ node.description = \"AirPods EQ ({safe_name})\" \
         media.name = \"AirPods EQ\" \
         filter.graph = {{ \
         nodes = [ {nodes_str} ]{links_str} \
         }} \
         capture.props = {{ \
         node.name = \"airpods_eq_capture\" \
         media.class = \"Audio/Sink\" \
         audio.position = [ FL FR ] \
         }} \
         playback.props = {{ \
         node.name = \"airpods_eq_playback\" \
         }} }}"
    )
}

/// Generate PipeWire filter-chain drop-in config for persistence across PipeWire restarts.
/// This file is written to ~/.config/pipewire/pipewire.conf.d/ so the EQ survives
/// a PipeWire restart, but the primary mechanism is pw-cli hot-loading.
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
# This drop-in persists the EQ across PipeWire restarts.
# The daemon also hot-loads this via pw-cli for seamless switching.
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
