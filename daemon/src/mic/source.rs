//! The "AirPods Microphone" source apps record from.
//!
//! Built on `module-pipe-source`: the audio server reads raw PCM from a FIFO
//! we write the decoded mic audio into. Both PulseAudio and pipewire-pulse
//! implement the module, so one code path covers both servers, driven through
//! `pactl` like the PulseAudio EQ backend.
//!
//! "Is anyone recording?" is answered by counting the source-outputs attached
//! to our source, and `pactl subscribe` tells us when that may have changed.

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;
use std::process::Stdio;
use tokio::io::AsyncBufReadExt;
use tokio::process::{Child, Command};
use tokio::sync::mpsc;
use tracing::{debug, warn};

use crate::eq::{die_with_parent, run_cmd};

/// The module that turns a FIFO into a source.
const MODULE: &str = "module-pipe-source";

fn runtime_dir() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("airpods-helper")
}

/// Is there a pactl-compatible audio server we can load the module into?
pub async fn detect() -> Result<(), String> {
    run_cmd("pactl", &["info"])
        .await
        .map(|_| ())
        .map_err(|e| format!("no PulseAudio-compatible audio server (pactl info: {e})"))
}

/// Unload sources a crashed daemon left behind (matched by source name).
pub async fn cleanup_stale(name: &str) {
    let Ok(modules) = run_cmd("pactl", &["list", "short", "modules"]).await else {
        return;
    };
    let marker = format!("source_name={name} ");
    for line in modules.lines() {
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() >= 3 && cols[1] == MODULE && format!("{} ", cols[2]).contains(&marker) {
            debug!("unloading stale mic source module {}", cols[0]);
            let _ = run_cmd("pactl", &["unload-module", cols[0]]).await;
        }
    }
}

/// A loaded pipe source and the write end of its FIFO. Call [`remove`] to
/// unload it; dropping without that leaves the module for `cleanup_stale`.
///
/// [`remove`]: PipeSource::remove
pub struct PipeSource {
    name: String,
    module_id: String,
    fifo: PathBuf,
    writer: File,
    /// Writes dropped because the server wasn't draining the FIFO.
    pub dropped_writes: u64,
}

impl PipeSource {
    /// Create the FIFO, load the module, and open the FIFO for writing.
    pub async fn create(name: &str, description: &str, rate: u32) -> Result<Self, String> {
        let dir = runtime_dir();
        tokio::fs::create_dir_all(&dir)
            .await
            .map_err(|e| format!("{}: {e}", dir.display()))?;
        let fifo = dir.join(format!("{name}.fifo"));
        let _ = std::fs::remove_file(&fifo);
        let c_path = std::ffi::CString::new(fifo.as_os_str().as_encoded_bytes())
            .map_err(|_| "FIFO path contains a NUL byte".to_string())?;
        // Owner-only: the FIFO carries microphone audio.
        // SAFETY: mkfifo with a valid NUL-terminated path.
        if unsafe { libc::mkfifo(c_path.as_ptr(), 0o600) } != 0 {
            return Err(format!("mkfifo {}: {}", fifo.display(), io::Error::last_os_error()));
        }

        // The description has a space, so it needs the nested quoting PA's
        // modarg parser expects: outer ' for the argument, inner " for the value.
        let args = [
            "load-module".to_string(),
            MODULE.to_string(),
            format!("source_name={name}"),
            format!("file={}", fifo.display()),
            "format=s16le".to_string(),
            format!("rate={rate}"),
            "channels=1".to_string(),
            format!("source_properties='device.description=\"{description}\" device.icon_name=audio-headset'"),
        ];
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let module_id = match run_cmd("pactl", &args).await {
            Ok(out) => out.trim().to_string(),
            Err(e) => {
                let _ = std::fs::remove_file(&fifo);
                return Err(format!("couldn't create the microphone source: {e}"));
            }
        };

        // The server holds the read end now, so a non-blocking open succeeds.
        // Non-blocking writes mean a stalled server costs us dropped audio,
        // never a stuck daemon.
        let writer = match OpenOptions::new()
            .write(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(&fifo)
        {
            Ok(f) => f,
            Err(e) => {
                let _ = run_cmd("pactl", &["unload-module", &module_id]).await;
                let _ = std::fs::remove_file(&fifo);
                return Err(format!("couldn't open {}: {e}", fifo.display()));
            }
        };
        Ok(Self {
            name: name.to_string(),
            module_id,
            fifo,
            writer,
            dropped_writes: 0,
        })
    }

    /// Push PCM to the source. If the FIFO is full the frame is dropped.
    pub fn write(&mut self, pcm: &[i16]) {
        let bytes: Vec<u8> = pcm.iter().flat_map(|s| s.to_le_bytes()).collect();
        match self.writer.write(&bytes) {
            Ok(n) if n == bytes.len() => {}
            // A short write would misalign every following sample; pipe writes
            // under PIPE_BUF are atomic, so this only happens on a nearly full
            // pipe. Count it like a drop.
            Ok(_) => self.dropped_writes += 1,
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => self.dropped_writes += 1,
            Err(e) => {
                self.dropped_writes += 1;
                debug!("mic FIFO write failed: {e}");
            }
        }
    }

    /// How many streams are recording from this source right now.
    pub async fn consumers(&self) -> Result<usize, String> {
        let sources = run_cmd("pactl", &["list", "short", "sources"])
            .await
            .map_err(|e| e.to_string())?;
        let Some(index) = sources
            .lines()
            .map(|l| l.split('\t').collect::<Vec<_>>())
            .find(|c| c.len() >= 2 && c[1] == self.name)
            .map(|c| c[0].to_string())
        else {
            return Err("the microphone source disappeared".into());
        };
        let outputs = run_cmd("pactl", &["list", "short", "source-outputs"])
            .await
            .map_err(|e| e.to_string())?;
        Ok(outputs
            .lines()
            .filter(|l| l.split('\t').nth(1) == Some(index.as_str()))
            .count())
    }

    /// Unload the module and delete the FIFO.
    pub async fn remove(self) {
        if let Err(e) = run_cmd("pactl", &["unload-module", &self.module_id]).await {
            warn!("failed to unload the microphone source: {e}");
        }
        let _ = std::fs::remove_file(&self.fifo);
    }
}

/// Run `pactl subscribe` and send a tick whenever a source or source-output
/// changes, which is when the consumer count may have changed. The child is
/// killed when the returned handle is dropped.
pub fn watch_recording_changes() -> io::Result<(Child, mpsc::Receiver<()>)> {
    let mut cmd = Command::new("pactl");
    cmd.arg("subscribe")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let mut child = die_with_parent(&mut cmd).spawn()?;
    let stdout = child.stdout.take().expect("stdout is piped");
    let (tx, rx) = mpsc::channel(1);
    tokio::spawn(async move {
        let mut lines = tokio::io::BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            // e.g. "Event 'new' on source-output #42"
            if line.contains(" on source-output ") || line.contains(" on source ") {
                // A pending tick already covers this one.
                let _ = tx.try_send(());
            }
        }
    });
    Ok((child, rx))
}

