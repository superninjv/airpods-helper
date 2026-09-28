//! PulseAudio EQ backend (for systems without PipeWire).
//!
//! PulseAudio has no built-in parametric EQ we can drive, so we do the DSP
//! ourselves:
//!
//! ```text
//! apps → [null sink "airpods_eq"] → monitor → parec → Rust biquads → pacat → AirPods sink
//! ```
//!
//! The null sink becomes the default output while EQ is active (and streams
//! already on the AirPods are moved onto it); both are undone on stop. This
//! adds roughly 30–50 ms of latency, which is inaudible for music/video but
//! is why the PipeWire backend is preferred whenever it's available.
//! Only `pactl`/`parec`/`pacat` are needed — no libpulse link dependency.

use bluer::Address;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;
use tracing::{debug, info, warn};

use super::dsp::Processor;
use super::preset::EqPreset;
use super::{StatusSink, run_cmd};

pub const SINK_NAME: &str = "airpods_eq";
const RATE: u32 = 48_000;
const CHANNELS: usize = 2;
const FRAME_BYTES: usize = CHANNELS * 4;

pub async fn detect() -> bool {
    run_cmd("pactl", &["info"]).await.is_ok()
        && Command::new("parec")
            .arg("--version")
            .output()
            .await
            .is_ok()
        && Command::new("pacat")
            .arg("--version")
            .output()
            .await
            .is_ok()
}

/// One row of `pactl list short <kind>`.
fn short_rows(out: &str) -> impl Iterator<Item = Vec<&str>> {
    out.lines()
        .map(|l| l.split('\t').collect::<Vec<_>>())
        .filter(|c| c.len() >= 2)
}

/// Bluetooth sinks are named e.g. `bluez_sink.AA_BB_CC_DD_EE_FF.a2dp_sink`
/// (PulseAudio) or `bluez_output.AA_BB_CC_DD_EE_FF.1` (pipewire-pulse).
pub fn match_bluez_sink(list_short_sinks: &str, address: Address) -> Option<(String, String)> {
    let mac = address.to_string().replace(':', "_").to_ascii_uppercase();
    short_rows(list_short_sinks).find_map(|c| {
        let name = c[1];
        (name.starts_with("bluez_") && name.to_ascii_uppercase().contains(&mac))
            .then(|| (c[0].to_string(), name.to_string()))
    })
}

async fn find_bluez_sink(address: Address) -> Option<(String, String)> {
    let out = run_cmd("pactl", &["list", "short", "sinks"]).await.ok()?;
    match_bluez_sink(&out, address)
}

async fn sink_index(name: &str) -> Option<String> {
    let out = run_cmd("pactl", &["list", "short", "sinks"]).await.ok()?;
    short_rows(&out)
        .find(|c| c[1] == name)
        .map(|c| c[0].to_string())
}

async fn default_sink() -> Option<String> {
    if let Ok(out) = run_cmd("pactl", &["get-default-sink"]).await {
        return Some(out.trim().to_string()).filter(|s| !s.is_empty());
    }
    // pactl < 15 has no get-default-sink.
    let info = run_cmd("pactl", &["info"]).await.ok()?;
    info.lines()
        .find_map(|l| l.strip_prefix("Default Sink: "))
        .map(|s| s.trim().to_string())
}

/// Move every stream currently playing to sink `from_idx` over to `to`.
async fn move_streams(from_idx: &str, to: &str) {
    let Ok(out) = run_cmd("pactl", &["list", "short", "sink-inputs"]).await else {
        return;
    };
    for row in short_rows(&out).filter(|c| c[1] == from_idx) {
        if let Err(e) = run_cmd("pactl", &["move-sink-input", row[0], to]).await {
            debug!("could not move sink-input {}: {e}", row[0]);
        }
    }
}

/// Unload null sinks left behind if the daemon was killed mid-EQ.
pub async fn cleanup_stale() {
    let Ok(out) = run_cmd("pactl", &["list", "short", "modules"]).await else {
        return;
    };
    let marker = format!("sink_name={SINK_NAME} ");
    for row in short_rows(&out) {
        if row[1] == "module-null-sink"
            && row
                .get(2)
                .is_some_and(|a| format!("{a} ").contains(&marker))
        {
            info!("unloading stale EQ null sink (module {})", row[0]);
            let _ = run_cmd("pactl", &["unload-module", row[0]]).await;
        }
    }
}

#[derive(Debug, Default)]
pub struct Restore {
    module: Option<String>,
    previous_default: Option<String>,
    bluez_sink: Option<String>,
}

impl Restore {
    pub async fn run(self) {
        let Some(module) = self.module else { return };
        let fallback = self
            .previous_default
            .filter(|d| d != SINK_NAME)
            .or(self.bluez_sink);
        if let Some(target) = fallback {
            if default_sink().await.as_deref() == Some(SINK_NAME) {
                let _ = run_cmd("pactl", &["set-default-sink", &target]).await;
            }
            if let Some(idx) = sink_index(SINK_NAME).await {
                move_streams(&idx, &target).await;
            }
        }
        if let Err(e) = run_cmd("pactl", &["unload-module", &module]).await {
            warn!("failed to unload EQ null sink: {e}");
        }
    }
}

async fn load_null_sink(description: &str) -> anyhow::Result<String> {
    let desc = description.replace(['"', '\''], "");
    let props = format!(
        "sink_properties='device.description=\"{desc}\" device.icon_name=\"audio-headphones\"'"
    );
    let out = run_cmd(
        "pactl",
        &[
            "load-module",
            "module-null-sink",
            &format!("sink_name={SINK_NAME}"),
            &props,
            &format!("rate={RATE}"),
            &format!("channels={CHANNELS}"),
        ],
    )
    .await?;
    Ok(out.trim().to_string())
}

/// Pump audio from the null sink's monitor through the EQ into the AirPods
/// until one side exits.
async fn pump(preset: &EqPreset, bluez_sink: &str) -> anyhow::Result<()> {
    let fmt = [
        "--raw",
        "--format=float32le",
        "--rate=48000",
        "--channels=2",
    ];
    let mut rec = Command::new("parec")
        .args([
            "-d",
            &format!("{SINK_NAME}.monitor"),
            "--latency-msec=20",
            "--client-name=airpods-helper",
            "--stream-name=EQ capture",
        ])
        .args(fmt)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()?;
    let mut play = Command::new("pacat")
        .args([
            "--playback",
            "-d",
            bluez_sink,
            "--latency-msec=30",
            "--client-name=airpods-helper",
            "--stream-name=AirPods EQ",
            // If the AirPods sink vanishes, fail instead of falling back
            // to the default sink (which is our own EQ sink: a loop) or
            // to the speakers. Ignored by plain PulseAudio.
            "--property=node.dont-fallback=true",
            "--property=node.dont-reconnect=true",
        ])
        .args(fmt)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()?;

    let mut input = rec
        .stdout
        .take()
        .ok_or_else(|| anyhow::anyhow!("parec has no stdout"))?;
    let mut output = play
        .stdin
        .take()
        .ok_or_else(|| anyhow::anyhow!("pacat has no stdin"))?;
    let mut processor = Processor::new(preset, RATE as f64, CHANNELS);

    let mut bytes = vec![0u8; 1024 * FRAME_BYTES];
    let mut filled = 0;
    let mut samples: Vec<f32> = Vec::with_capacity(1024 * CHANNELS);
    loop {
        let n = input.read(&mut bytes[filled..]).await?;
        if n == 0 {
            anyhow::bail!("parec stopped");
        }
        filled += n;
        let whole = filled - filled % FRAME_BYTES;
        samples.clear();
        samples.extend(
            bytes[..whole]
                .chunks_exact(4)
                .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])),
        );
        processor.process(&mut samples);
        for (dst, s) in bytes[..whole].chunks_exact_mut(4).zip(&samples) {
            dst.copy_from_slice(&s.to_le_bytes());
        }
        output.write_all(&bytes[..whole]).await?;
        bytes.copy_within(whole..filled, 0);
        filled -= whole;
    }
}

pub async fn supervise(
    preset: EqPreset,
    address: Address,
    status: StatusSink,
    restore: std::sync::Arc<tokio::sync::Mutex<Restore>>,
) {
    let mut backoff = Duration::from_secs(1);
    loop {
        let (bluez_idx, bluez_sink) = loop {
            match find_bluez_sink(address).await {
                Some(found) => break found,
                None => {
                    status.waiting();
                    tokio::time::sleep(Duration::from_secs(2)).await;
                }
            }
        };

        {
            let mut r = restore.lock().await;
            if r.module.is_none() {
                match load_null_sink(&format!("AirPods EQ ({})", preset.name)).await {
                    Ok(module) => {
                        r.previous_default = default_sink().await;
                        r.module = Some(module);
                    }
                    Err(e) => {
                        drop(r);
                        status.error(format!("failed to create EQ sink: {e}"));
                        tokio::time::sleep(backoff).await;
                        backoff = (backoff * 2).min(Duration::from_secs(30));
                        continue;
                    }
                }
            }
            r.bluez_sink = Some(bluez_sink.clone());
        }
        let _ = run_cmd("pactl", &["set-default-sink", SINK_NAME]).await;
        move_streams(&bluez_idx, SINK_NAME).await;

        info!(
            "EQ active via PulseAudio (preset '{}' → {bluez_sink})",
            preset.id
        );
        status.active();
        let err = pump(&preset, &bluez_sink).await.err();
        let msg = err.map(|e| e.to_string()).unwrap_or_default();
        warn!(
            "PulseAudio EQ pipeline stopped: {msg}; restarting in {}s",
            backoff.as_secs()
        );
        status.error(format!("EQ audio pipeline stopped: {msg}"));
        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(Duration::from_secs(30));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_both_sink_naming_schemes() {
        let addr: Address = "aa:bb:cc:dd:ee:ff".parse().unwrap();
        let pa = "0\talsa_output.pci.analog-stereo\tmodule-alsa-card.c\ts16le 2ch 44100Hz\tIDLE\n\
                  3\tbluez_sink.AA_BB_CC_DD_EE_FF.a2dp_sink\tmodule-bluez5-device.c\ts16le 2ch 44100Hz\tRUNNING\n";
        assert_eq!(
            match_bluez_sink(pa, addr),
            Some(("3".into(), "bluez_sink.AA_BB_CC_DD_EE_FF.a2dp_sink".into()))
        );
        let pw = "71\tbluez_output.AA_BB_CC_DD_EE_FF.1\tPipeWire\ts16le 2ch 48000Hz\tSUSPENDED\n";
        assert_eq!(
            match_bluez_sink(pw, addr),
            Some(("71".into(), "bluez_output.AA_BB_CC_DD_EE_FF.1".into()))
        );
        let other: Address = "11:22:33:44:55:66".parse().unwrap();
        assert_eq!(match_bluez_sink(pa, other), None);
    }
}
