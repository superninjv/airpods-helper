//! PipeWire EQ backend.
//!
//! The filter chain runs in a child `pipewire -c <conf>` process owned by the
//! daemon (the same way PipeWire's own `filter-chain.conf` is meant to be run),
//! so it lives exactly as long as we want it and dies with the daemon.
//!
//! * **Smart filter** (WirePlumber ≥ 0.5): the EQ sink is declared as a
//!   WirePlumber smart filter targeting the AirPods by Bluetooth address.
//!   WirePlumber transparently inserts it in front of the AirPods for any
//!   stream headed there; the user's default sink is never touched, and it
//!   survives A2DP ⇄ HFP profile switches.
//! * **Legacy** (older WirePlumber): the EQ output is pinned to the AirPods
//!   sink by node name and the EQ sink is made the default output while
//!   active; the previous default is restored when EQ stops.

use bluer::Address;
use std::path::PathBuf;
use std::sync::Arc;
use std::process::Stdio;
use std::time::Duration;
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

use super::preset::{EqPreset, FilterType};
use super::{StatusSink, run_cmd};

pub const SINK_NODE: &str = "airpods_eq_sink";
const OUT_NODE: &str = "airpods_eq_output";

/// Probe for a usable PipeWire. Returns `Some(smart_filters_supported)`.
pub async fn detect() -> Option<bool> {
    // The server must be up and we must be able to spawn our own instance.
    run_cmd("pw-cli", &["info", "0"]).await.ok()?;
    which("pipewire")?;
    let smart = match run_cmd("wireplumber", &["--version"]).await {
        Ok(out) => wireplumber_supports_smart_filters(&out),
        Err(_) => false,
    };
    Some(smart)
}

fn which(bin: &str) -> Option<PathBuf> {
    std::env::var_os("PATH")?
        .to_str()?
        .split(':')
        .map(|dir| PathBuf::from(dir).join(bin))
        .find(|p| p.is_file())
}

/// `wireplumber --version` prints e.g. "Compiled with libwireplumber 0.5.14".
fn wireplumber_supports_smart_filters(version_output: &str) -> bool {
    version_output
        .split_whitespace()
        .filter_map(|tok| {
            let mut parts = tok.split('.').map(|p| p.parse::<u32>().ok());
            Some((parts.next()??, parts.next()??))
        })
        .any(|(major, minor)| (major, minor) >= (0, 5))
}

/// SPA-JSON string literal.
fn spa_str(s: &str) -> String {
    let escaped: String = s
        .chars()
        .filter(|c| !c.is_control())
        .flat_map(|c| match c {
            '"' | '\\' => vec!['\\', c],
            _ => vec![c],
        })
        .collect();
    format!("\"{escaped}\"")
}

fn label(t: FilterType) -> &'static str {
    match t {
        FilterType::Peaking => "bq_peaking",
        FilterType::Lowshelf => "bq_lowshelf",
        FilterType::Highshelf => "bq_highshelf",
        FilterType::Lowpass => "bq_lowpass",
        FilterType::Highpass => "bq_highpass",
        FilterType::Notch => "bq_notch",
    }
}

pub enum Routing<'a> {
    /// WirePlumber smart filter matched on the AirPods' BT address.
    Smart { address: Address },
    /// Explicit link to a sink node by name.
    Target { node_name: &'a str },
}

/// Build a standalone PipeWire client config that hosts the filter chain.
pub fn generate_config(preset: &EqPreset, routing: &Routing) -> String {
    let mut nodes = Vec::new();
    // A high shelf at 0 Hz is PipeWire's idiom for a flat gain stage.
    if preset.preamp.abs() > 0.001 {
        nodes.push((
            "preamp".to_string(),
            "bq_highshelf",
            0.0,
            1.0,
            preset.preamp,
        ));
    }
    for (i, b) in preset.bands.iter().enumerate() {
        nodes.push((
            format!("band{i}"),
            label(b.filter_type),
            b.freq,
            b.q,
            b.gain,
        ));
    }
    // An empty graph isn't valid; a 0 dB gain stage is a clean pass-through.
    if nodes.is_empty() {
        nodes.push(("preamp".to_string(), "bq_highshelf", 0.0, 1.0, 0.0));
    }

    let node_lines: Vec<String> = nodes
        .iter()
        .map(|(name, label, freq, q, gain)| {
            format!(
                "          {{ type = builtin name = {name} label = {label} \
                 control = {{ \"Freq\" = {freq:.2} \"Q\" = {q:.3} \"Gain\" = {gain:.2} }} }}"
            )
        })
        .collect();
    let link_lines: Vec<String> = nodes
        .windows(2)
        .map(|w| {
            format!(
                "          {{ output = \"{}:Out\" input = \"{}:In\" }}",
                w[0].0, w[1].0
            )
        })
        .collect();

    let description = spa_str(&format!("AirPods EQ ({})", preset.name));
    let (capture_extra, playback_extra) = match routing {
        Routing::Smart { address } => (
            format!(
                "\n        filter.smart = true\
                 \n        filter.smart.name = \"airpods-helper-eq\"\
                 \n        filter.smart.target = {{ api.bluez5.address = {} media.class = \"Audio/Sink\" }}",
                spa_str(&address.to_string())
            ),
            String::new(),
        ),
        Routing::Target { node_name } => (
            String::new(),
            format!(
                "\n        target.object = {}\n        node.dont-fallback = true",
                spa_str(node_name)
            ),
        ),
    };

    format!(
        r#"# Generated by airpods-daemon — do not edit; rewritten on every EQ change.
# Preset: {id}
context.properties = {{
  log.level = 0
}}
context.spa-libs = {{
  audio.convert.* = audioconvert/libspa-audioconvert
  support.*       = support/libspa-support
}}
context.modules = [
  {{ name = libpipewire-module-rt args = {{ nice.level = -11 }} flags = [ ifexists nofail ] }}
  {{ name = libpipewire-module-protocol-native }}
  {{ name = libpipewire-module-client-node }}
  {{ name = libpipewire-module-adapter }}
  {{ name = libpipewire-module-filter-chain
    args = {{
      node.description = {description}
      media.name       = {description}
      audio.channels   = 2
      audio.position   = [ FL FR ]
      filter.graph = {{
        nodes = [
{nodes}
        ]
        links = [
{links}
        ]
      }}
      capture.props = {{
        node.name   = "{SINK_NODE}"
        media.class = "Audio/Sink"{capture_extra}
      }}
      playback.props = {{
        node.name    = "{OUT_NODE}"
        node.passive = true{playback_extra}
      }}
    }}
  }}
]
"#,
        id = preset.id,
        nodes = node_lines.join("\n"),
        links = link_lines.join("\n"),
    )
}

fn runtime_dir() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("airpods-helper")
}

fn conf_path() -> PathBuf {
    runtime_dir().join("eq-filter-chain.conf")
}

fn pid_path() -> PathBuf {
    runtime_dir().join("eq-filter-chain.pid")
}

/// Start the filter-chain process. Its stderr is drained into `last_line`
/// continuously (an undrained pipe would eventually block it), and it gets
/// SIGTERM if the daemon dies without cleaning up.
async fn spawn(config: &str, last_line: Arc<std::sync::Mutex<String>>) -> std::io::Result<Child> {
    tokio::fs::create_dir_all(runtime_dir()).await?;
    let path = conf_path();
    tokio::fs::write(&path, config).await?;
    let mut cmd = Command::new("pipewire");
    cmd.arg("-c")
        .arg(&path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    super::die_with_parent(&mut cmd);
    let mut child = cmd.spawn()?;
    if let Some(pid) = child.id() {
        let _ = tokio::fs::write(pid_path(), pid.to_string()).await;
    }
    if let Some(stderr) = child.stderr.take() {
        tokio::spawn(async move {
            use tokio::io::AsyncBufReadExt;
            let mut lines = tokio::io::BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if !line.trim().is_empty() {
                    *last_line.lock().unwrap_or_else(|e| e.into_inner()) = line;
                }
            }
        });
    }
    Ok(child)
}

/// Clean up after a previous run that didn't exit cleanly (crash, SIGKILL):
/// stop an orphaned filter process and un-point the default sink at it.
pub async fn cleanup_stale() {
    if let Ok(pid) = tokio::fs::read_to_string(pid_path()).await
        && let Ok(pid) = pid.trim().parse::<i32>()
    {
        let cmdline = tokio::fs::read(format!("/proc/{pid}/cmdline")).await.unwrap_or_default();
        let ours = String::from_utf8_lossy(&cmdline).contains(&*conf_path().to_string_lossy());
        if ours {
            info!("stopping EQ filter process {pid} left by a previous run");
            // SAFETY: plain kill(2) on a pid we verified runs our config.
            unsafe { libc::kill(pid, libc::SIGTERM) };
        }
    }
    let _ = tokio::fs::remove_file(pid_path()).await;
    if run_cmd("pw-cli", &["info", "0"]).await.is_ok()
        && get_configured_default().await.as_deref() == Some(SINK_NODE)
    {
        info!("clearing a default sink that still points at the EQ from a previous run");
        set_configured_default(None).await;
    }
}

/// Minimal view of `pw-dump` output.
#[derive(serde::Deserialize)]
struct PwObject {
    #[serde(default)]
    info: Option<PwInfo>,
}
#[derive(serde::Deserialize)]
struct PwInfo {
    #[serde(default)]
    props: Option<serde_json::Map<String, serde_json::Value>>,
}

/// Name of the AirPods' PipeWire output node, if it currently exists.
pub async fn find_bluez_sink(address: Address) -> Option<String> {
    let dump = run_cmd("pw-dump", &[]).await.ok()?;
    let objects: Vec<PwObject> = serde_json::from_str(&dump).ok()?;
    let want = address.to_string();
    objects.into_iter().find_map(|o| {
        let props = o.info?.props?;
        let get = |k: &str| props.get(k).and_then(|v| v.as_str());
        (get("media.class") == Some("Audio/Sink")
            && get("api.bluez5.address").is_some_and(|a| a.eq_ignore_ascii_case(&want)))
        .then(|| get("node.name").map(str::to_string))
        .flatten()
    })
}

async fn get_configured_default() -> Option<String> {
    let out = run_cmd(
        "pw-metadata",
        &["-n", "default", "0", "default.configured.audio.sink"],
    )
    .await
    .ok()?;
    let value = out.split("value:'").nth(1)?.split('\'').next()?;
    let json: serde_json::Value = serde_json::from_str(value).ok()?;
    json.get("name")?.as_str().map(str::to_string)
}

async fn set_configured_default(name: Option<&str>) {
    let result = match name {
        Some(name) => {
            let value = serde_json::json!({ "name": name }).to_string();
            run_cmd(
                "pw-metadata",
                &[
                    "-n",
                    "default",
                    "0",
                    "default.configured.audio.sink",
                    &value,
                    "Spa:String:JSON",
                ],
            )
            .await
        }
        None => {
            run_cmd(
                "pw-metadata",
                &["-n", "default", "-d", "0", "default.configured.audio.sink"],
            )
            .await
        }
    };
    if let Err(e) = result {
        warn!("failed to set default sink: {e}");
    }
}

/// What must be undone when a legacy-mode EQ stops.
#[derive(Debug, Default)]
pub struct Restore {
    /// `Some(prev)` once we've redirected the default sink; `prev` is what it
    /// was before (`None` = nothing configured).
    previous_default: Option<Option<String>>,
}

impl Restore {
    /// Point the default output at the EQ, remembering the current one.
    async fn redirect_default(&mut self) {
        if self.previous_default.is_none() {
            // Never remember ourselves as "previous" (e.g. after a crash).
            let prev = get_configured_default().await.filter(|d| d != SINK_NODE);
            self.previous_default = Some(prev);
        }
        set_configured_default(Some(SINK_NODE)).await;
    }

    pub async fn run(self) {
        let _ = tokio::fs::remove_file(pid_path()).await;
        if let Some(prev) = self.previous_default {
            // Only restore if we're still the default — the user may have
            // picked something else in the meantime.
            if get_configured_default().await.as_deref() == Some(SINK_NODE) {
                debug!("restoring default sink to {prev:?}");
                set_configured_default(prev.as_deref()).await;
            }
        }
    }
}

/// Run the EQ until cancelled (the task is aborted). Restarts the filter
/// process with backoff if it dies (e.g. PipeWire restarted).
pub async fn supervise(
    preset: EqPreset,
    address: Address,
    smart: bool,
    status: StatusSink,
    restore: Arc<Mutex<Restore>>,
) {
    const MAX_BACKOFF: Duration = Duration::from_secs(30);
    let mut backoff = Duration::from_secs(1);
    loop {
        // Wait until the AirPods' audio sink exists (A2DP can come up a few
        // seconds after connecting, and goes away during calls).
        let sink = loop {
            match find_bluez_sink(address).await {
                Some(sink) => break sink,
                None => {
                    status.waiting();
                    tokio::time::sleep(Duration::from_secs(2)).await;
                }
            }
        };

        let routing = if smart { Routing::Smart { address } } else { Routing::Target { node_name: &sink } };
        let last_line = Arc::new(std::sync::Mutex::new(String::new()));
        let mut child = match spawn(&generate_config(&preset, &routing), last_line.clone()).await {
            Ok(child) => child,
            Err(e) => {
                status.error(format!("failed to start PipeWire filter chain: {e}"));
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(MAX_BACKOFF);
                continue;
            }
        };
        let started = tokio::time::Instant::now();
        info!(
            "EQ filter chain started (preset '{}', {} mode)",
            preset.id,
            if smart { "smart-filter" } else { "legacy" }
        );
        if !smart {
            // Give the node a moment to register before pointing the default at it.
            tokio::time::sleep(Duration::from_millis(300)).await;
            restore.lock().await.redirect_default().await;
        }

        // Track the AirPods sink while the filter runs. pw-dump isn't free,
        // so poll quickly only while waiting and rarely once active.
        let exit = loop {
            let current = find_bluez_sink(address).await;
            match &current {
                Some(name) if smart || *name == sink => status.active(),
                Some(_) => break None, // legacy: pinned node was replaced — respawn
                None if smart => status.waiting(), // WirePlumber re-links when it's back
                None => break None, // legacy: don't leave the default pointing at a dead end
            }
            let every = Duration::from_secs(if current.is_some() { 30 } else { 3 });
            tokio::select! {
                exit = child.wait() => break Some(exit),
                _ = tokio::time::sleep(every) => {}
            }
        };

        let Some(exit) = exit else {
            // Legacy target went away or changed: tear down and start over.
            drop(child);
            std::mem::take(&mut *restore.lock().await).run().await;
            status.waiting();
            continue;
        };
        if started.elapsed() > Duration::from_secs(60) {
            backoff = Duration::from_secs(1); // it ran fine for a while; this is a fresh failure
        }
        let detail = last_line.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let msg = match exit {
            Ok(code) => format!("PipeWire filter chain exited ({code}) {detail}"),
            Err(e) => format!("PipeWire filter chain failed: {e}"),
        };
        warn!("{msg}; restarting in {}s", backoff.as_secs());
        status.error(msg.trim().to_string());
        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(MAX_BACKOFF);
    }
}

/// Remove the drop-in older versions wrote into PipeWire's own config dir.
/// It loaded an unrouted "AirPods EQ" sink into the main PipeWire process on
/// every restart.
pub async fn remove_legacy_dropin() {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")));
    let Some(base) = base else { return };
    let path = base.join("pipewire/pipewire.conf.d/99-airpods-eq.conf");
    if tokio::fs::remove_file(&path).await.is_ok() {
        info!(
            "removed stale {} left by an older airpods-helper; restart PipeWire \
             (systemctl --user restart pipewire) if an orphan 'AirPods EQ' device is still listed",
            path.display()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eq::preset::EqBand;

    fn preset() -> EqPreset {
        EqPreset {
            id: "test".into(),
            name: "Test \"quoted\"".into(),
            description: String::new(),
            preamp: -3.0,
            bands: vec![
                EqBand {
                    filter_type: FilterType::Lowshelf,
                    freq: 105.0,
                    q: 0.7,
                    gain: 4.0,
                },
                EqBand {
                    filter_type: FilterType::Peaking,
                    freq: 3000.0,
                    q: 2.0,
                    gain: -2.5,
                },
            ],
        }
    }

    #[test]
    fn wireplumber_version_detection() {
        assert!(wireplumber_supports_smart_filters(
            "wireplumber\nCompiled with libwireplumber 0.5.14\nLinked with libwireplumber 0.5.14"
        ));
        assert!(!wireplumber_supports_smart_filters(
            "Compiled with libwireplumber 0.4.17"
        ));
        assert!(!wireplumber_supports_smart_filters("garbage"));
    }

    #[test]
    fn smart_config_targets_address() {
        let addr: Address = "AA:BB:CC:DD:EE:FF".parse().unwrap();
        let conf = generate_config(&preset(), &Routing::Smart { address: addr });
        assert!(conf.contains("filter.smart = true"));
        assert!(conf.contains("api.bluez5.address = \"AA:BB:CC:DD:EE:FF\""));
        assert!(conf.contains("label = bq_lowshelf"));
        assert!(conf.contains("{ output = \"preamp:Out\" input = \"band0:In\" }"));
        assert!(conf.contains("{ output = \"band0:Out\" input = \"band1:In\" }"));
        assert!(conf.contains("AirPods EQ (Test \\\"quoted\\\")"));
        assert!(!conf.contains("target.object"));
    }

    #[test]
    fn legacy_config_pins_target() {
        let conf = generate_config(
            &preset(),
            &Routing::Target {
                node_name: "bluez_output.AA_BB.1",
            },
        );
        assert!(conf.contains("target.object = \"bluez_output.AA_BB.1\""));
        assert!(conf.contains("node.dont-fallback = true"));
        assert!(!conf.contains("filter.smart"));
    }

    /// Spawns a real filter chain against the machine's current default sink.
    /// Run with `cargo test -- --ignored` on a desktop with PipeWire.
    #[tokio::test]
    #[ignore]
    async fn live_filter_chain_links_to_sink() {
        let sink = run_cmd("pactl", &["get-default-sink"])
            .await
            .unwrap()
            .trim()
            .to_string();
        let conf = generate_config(&preset(), &Routing::Target { node_name: &sink });
        let mut child = spawn(&conf, Default::default()).await.unwrap();
        tokio::time::sleep(Duration::from_millis(1500)).await;
        let links = run_cmd("pw-link", &["-l"]).await.unwrap();
        child.kill().await.unwrap();
        assert!(links.contains(&format!("{OUT_NODE}:output_FL")), "{links}");
        assert!(links.contains(&format!("{sink}:playback_FL")), "{links}");
    }
}
