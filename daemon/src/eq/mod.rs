//! Parametric EQ for the AirPods output.
//!
//! [`EqManager`] owns the selected preset and the connected device, picks an
//! audio backend, and runs the backend's supervisor task while both are
//! present. Status is published into [`SharedState`] so D-Bus clients see
//! `EqStatus` / `EqError` / `EqBackend` change live.

pub mod dsp;
mod pipewire;
pub mod preset;
mod pulse;

use bluer::Address;
use std::process::Stdio;
use std::sync::Arc;
use tokio::process::Command;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tracing::{info, warn};

use crate::config::EqBackendChoice;
use crate::state::SharedState;
pub use preset::{EqBand, EqPreset, FilterType, PresetError, Source};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    /// PipeWire + WirePlumber ≥ 0.5 smart filter.
    PipeWire,
    /// PipeWire with older WirePlumber: default-sink redirection.
    PipeWireLegacy,
    PulseAudio,
    None,
}

impl Backend {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PipeWire => "pipewire",
            Self::PipeWireLegacy => "pipewire-legacy",
            Self::PulseAudio => "pulseaudio",
            Self::None => "none",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Off,
    Active,
    Waiting,
    Error,
    Unsupported,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Active => "active",
            Self::Waiting => "waiting",
            Self::Error => "error",
            Self::Unsupported => "unsupported",
        }
    }
}

/// Run a helper binary and return stdout, or an error including stderr.
pub(crate) async fn run_cmd(bin: &str, args: &[&str]) -> anyhow::Result<String> {
    let out = Command::new(bin)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .await
        .map_err(|e| anyhow::anyhow!("{bin}: {e}"))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        anyhow::bail!("{bin} {}: {}", args.join(" "), stderr.trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Handle backends use to publish their status.
#[derive(Clone)]
pub struct StatusSink {
    state: SharedState,
}

impl StatusSink {
    fn set(&self, status: Status, error: String) {
        self.state.update_if_changed(|s| {
            let changed = s.eq_status != status.as_str() || s.eq_error != error;
            s.eq_status = status.as_str().to_string();
            s.eq_error = error;
            changed
        });
    }
    pub fn active(&self) {
        self.set(Status::Active, String::new());
    }
    pub fn waiting(&self) {
        self.set(Status::Waiting, String::new());
    }
    pub fn error(&self, msg: String) {
        self.set(Status::Error, msg);
    }
}

enum Restore {
    PipeWire(Arc<Mutex<pipewire::Restore>>),
    Pulse(Arc<Mutex<pulse::Restore>>),
}

pub struct EqManager {
    state: SharedState,
    status: StatusSink,
    choice: EqBackendChoice,
    preset: Option<EqPreset>,
    device: Option<Address>,
    task: Option<(JoinHandle<()>, Restore)>,
}

impl EqManager {
    pub async fn new(state: SharedState, choice: EqBackendChoice) -> Self {
        pipewire::remove_legacy_dropin().await;
        if pulse::detect().await {
            pulse::cleanup_stale().await;
        }
        let mgr = Self {
            status: StatusSink {
                state: state.clone(),
            },
            state,
            choice,
            preset: None,
            device: None,
            task: None,
        };
        let backend = mgr.detect().await;
        mgr.state
            .update(|s| s.eq_backend = backend.as_str().to_string());
        mgr.publish_idle(backend);
        mgr
    }

    async fn detect(&self) -> Backend {
        let pw = || async {
            match pipewire::detect().await {
                Some(true) => Backend::PipeWire,
                Some(false) => Backend::PipeWireLegacy,
                None => Backend::None,
            }
        };
        let pa = || async {
            if pulse::detect().await {
                Backend::PulseAudio
            } else {
                Backend::None
            }
        };
        match self.choice {
            EqBackendChoice::Pipewire => pw().await,
            EqBackendChoice::Pulseaudio => pa().await,
            EqBackendChoice::Auto => match pw().await {
                Backend::None => pa().await,
                found => found,
            },
        }
    }

    /// Status to show when nothing is running.
    fn publish_idle(&self, backend: Backend) {
        let (status, error) = match (&self.preset, backend) {
            (None, _) => (Status::Off, String::new()),
            (Some(_), Backend::None) => (Status::Unsupported, unsupported_reason(self.choice)),
            (Some(_), _) => (Status::Waiting, String::new()),
        };
        self.status.set(status, error);
    }

    /// Select a preset (or `None` to turn EQ off) and apply it if the AirPods
    /// are connected.
    pub async fn select(&mut self, preset: Option<EqPreset>) {
        self.preset = preset;
        self.restart().await;
    }

    /// The AirPods connected (`Some`) or went away (`None`).
    pub async fn set_device(&mut self, device: Option<Address>) {
        if self.device == device && self.task.is_some() {
            return;
        }
        self.device = device;
        self.restart().await;
    }

    pub fn preset_id(&self) -> Option<&str> {
        self.preset.as_ref().map(|p| p.id.as_str())
    }

    async fn restart(&mut self) {
        self.stop_task().await;
        let backend = self.detect().await;
        self.state.update_if_changed(|s| {
            let changed = s.eq_backend != backend.as_str();
            s.eq_backend = backend.as_str().to_string();
            changed
        });

        let (Some(preset), Some(address)) = (self.preset.clone(), self.device) else {
            self.publish_idle(backend);
            return;
        };
        if backend == Backend::None {
            self.publish_idle(backend);
            return;
        }
        if preset.is_flat() {
            info!("EQ preset '{}' is flat; no filter needed", preset.id);
            self.status.active();
            return;
        }

        info!(
            "starting EQ '{}' for {address} via {}",
            preset.id,
            backend.as_str()
        );
        let peak = dsp::peak_gain_db(&preset, 48_000.0);
        if peak > 0.5 {
            warn!(
                "EQ preset '{}' boosts up to {peak:+.1} dB; loud tracks may clip. \
                 Lower its preamp to about {:.1} dB to avoid that.",
                preset.id,
                preset.preamp - peak
            );
        }
        let status = self.status.clone();
        self.task = Some(match backend {
            Backend::PipeWire | Backend::PipeWireLegacy => {
                let restore = Arc::new(Mutex::new(pipewire::Restore::default()));
                let smart = backend == Backend::PipeWire;
                let handle = tokio::spawn(pipewire::supervise(
                    preset,
                    address,
                    smart,
                    status,
                    restore.clone(),
                ));
                (handle, Restore::PipeWire(restore))
            }
            Backend::PulseAudio => {
                let restore = Arc::new(Mutex::new(pulse::Restore::default()));
                let handle =
                    tokio::spawn(pulse::supervise(preset, address, status, restore.clone()));
                (handle, Restore::Pulse(restore))
            }
            Backend::None => unreachable!(),
        });
    }

    async fn stop_task(&mut self) {
        let Some((handle, restore)) = self.task.take() else {
            return;
        };
        handle.abort();
        // Wait for the abort so child processes (kill_on_drop) are gone
        // before we restore routing.
        let _ = handle.await;
        match restore {
            Restore::PipeWire(r) => std::mem::take(&mut *r.lock().await).run().await,
            Restore::Pulse(r) => std::mem::take(&mut *r.lock().await).run().await,
        }
    }

    /// Stop everything and undo routing changes (daemon shutdown).
    pub async fn shutdown(&mut self) {
        self.stop_task().await;
        if self.preset.is_some() {
            warn!("EQ stopped for shutdown");
        }
    }
}

fn unsupported_reason(choice: EqBackendChoice) -> String {
    match choice {
        EqBackendChoice::Pipewire => "PipeWire isn't running (or the `pipewire` binary is missing), and eq.backend is set to \"pipewire\"".into(),
        EqBackendChoice::Pulseaudio => "PulseAudio isn't reachable, or pactl/pacat/parec are missing (install pulseaudio-utils)".into(),
        EqBackendChoice::Auto => "No supported audio server found: EQ needs PipeWire, or PulseAudio with pactl/pacat/parec".into(),
    }
}

#[cfg(test)]
mod tests {
    //! End-to-end tests against the desktop's real audio server, using a null
    //! sink that impersonates an AirPods A2DP sink. They never play audio:
    //! routing is checked structurally (links, streams, default sink), and
    //! the DSP itself is covered by the offline tests in `dsp.rs`.
    //! Run with `cargo test -p airpods-daemon -- --ignored --test-threads=1`.
    use super::*;
    use crate::state::create_shared_state;
    use std::time::Duration;

    const FAKE_MAC: &str = "AA:BB:CC:DD:EE:01";
    const FAKE_SINK: &str = "bluez_output.AA_BB_CC_DD_EE_01.1";

    struct FakePods(String);
    impl FakePods {
        async fn new() -> Self {
            let props = format!(
                "sink_properties='api.bluez5.address=\"{FAKE_MAC}\" device.api=\"bluez5\" device.description=\"FakePods\"'"
            );
            let idx = run_cmd(
                "pactl",
                &[
                    "load-module",
                    "module-null-sink",
                    &format!("sink_name={FAKE_SINK}"),
                    &props,
                ],
            )
            .await
            .expect("load fake sink");
            tokio::time::sleep(Duration::from_millis(500)).await;
            Self(idx.trim().to_string())
        }
        async fn remove(self) {
            let _ = run_cmd("pactl", &["unload-module", &self.0]).await;
        }
    }

    async fn wait_for(mut check: impl AsyncFnMut() -> bool) -> bool {
        for _ in 0..40 {
            if check().await {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        false
    }

    fn bass_boost() -> EqPreset {
        EqPreset::load("bass-boost").unwrap().0
    }

    async fn short_list(kind: &str) -> String {
        run_cmd("pactl", &["list", "short", kind])
            .await
            .unwrap_or_default()
    }

    fn index_of(list: &str, name: &str) -> Option<String> {
        list.lines()
            .map(|l| l.split('\t').collect::<Vec<_>>())
            .find(|c| c.get(1) == Some(&name))
            .map(|c| c[0].to_string())
    }

    #[tokio::test]
    #[ignore]
    async fn live_pipewire_smart_filter_end_to_end() {
        let fake = FakePods::new().await;
        let state = create_shared_state();
        let mut eq = EqManager::new(state.clone(), EqBackendChoice::Pipewire).await;
        assert_eq!(state.current().eq_backend, "pipewire");
        let default_before = run_cmd("pactl", &["get-default-sink"]).await.unwrap();

        eq.select(Some(bass_boost())).await;
        assert_eq!(state.current().eq_status, "waiting", "no device yet");
        eq.set_device(Some(FAKE_MAC.parse().unwrap())).await;
        let st = state.clone();
        assert!(
            wait_for(async || st.current().eq_status == "active").await,
            "{:?}",
            state.current()
        );

        // WirePlumber must place the filter in front of the "AirPods".
        let want = format!("{FAKE_SINK}:playback_FL");
        let linked = wait_for(async || {
            let links = run_cmd("pw-link", &["-l"]).await.unwrap_or_default();
            links.contains("airpods_eq_output:output_FL") && links.contains(&want)
        })
        .await;
        let default_during = run_cmd("pactl", &["get-default-sink"]).await.unwrap();

        eq.shutdown().await;
        tokio::time::sleep(Duration::from_millis(500)).await;
        let nodes_after = run_cmd("pw-dump", &[]).await.unwrap();
        fake.remove().await;

        assert!(linked, "EQ output never linked to the AirPods sink");
        assert_eq!(
            default_before, default_during,
            "smart filter must not touch the default sink"
        );
        assert!(
            !nodes_after.contains("airpods_eq_sink"),
            "filter node left behind"
        );
    }

    #[tokio::test]
    #[ignore]
    async fn live_pulseaudio_backend_end_to_end() {
        let fake = FakePods::new().await;
        let state = create_shared_state();
        let mut eq = EqManager::new(state.clone(), EqBackendChoice::Pulseaudio).await;
        assert_eq!(state.current().eq_backend, "pulseaudio");
        let default_before = run_cmd("pactl", &["get-default-sink"]).await.unwrap();

        eq.select(Some(bass_boost())).await;
        eq.set_device(Some(FAKE_MAC.parse().unwrap())).await;
        let st = state.clone();
        assert!(
            wait_for(async || st.current().eq_status == "active").await,
            "{:?}",
            state.current()
        );

        // Pipeline: a capture stream on the EQ sink's monitor, and a playback
        // stream on the fake AirPods.
        let routed = wait_for(async || {
            let sinks = short_list("sinks").await;
            let sources = short_list("sources").await;
            let (Some(fake_idx), Some(mon_idx)) = (
                index_of(&sinks, FAKE_SINK),
                index_of(&sources, &format!("{}.monitor", pulse::SINK_NAME)),
            ) else {
                return false;
            };
            let inputs = short_list("sink-inputs").await;
            let outputs = short_list("source-outputs").await;
            inputs
                .lines()
                .any(|l| l.split('\t').nth(1) == Some(fake_idx.as_str()))
                && outputs
                    .lines()
                    .any(|l| l.split('\t').nth(1) == Some(mon_idx.as_str()))
        })
        .await;
        let default_during = run_cmd("pactl", &["get-default-sink"]).await.unwrap();

        eq.shutdown().await;
        tokio::time::sleep(Duration::from_millis(500)).await;
        let default_after = run_cmd("pactl", &["get-default-sink"]).await.unwrap();
        let modules_after = short_list("modules").await;
        fake.remove().await;

        assert!(
            routed,
            "EQ pipeline streams not routed monitor → fake AirPods"
        );
        assert_eq!(default_during.trim(), pulse::SINK_NAME);
        assert_eq!(default_before, default_after, "default sink not restored");
        assert!(
            !modules_after.contains("sink_name=airpods_eq "),
            "null sink left loaded"
        );
    }
}
