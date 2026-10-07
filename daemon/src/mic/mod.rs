//! AirPods microphone as a regular audio source, while A2DP keeps playing.
//!
//! The AirPods stream the mic as AAC-ELD over the AAP channel (opcode 0x58,
//! see `aap::mic`). This module offers an "AirPods Microphone" source while
//! the AAP session is up, and only asks the buds to stream while something is
//! actually recording from it. Streaming costs battery and makes the buds
//! behave as if a call were active, so it should never run unattended.
//!
//! One supervisor task lives for the whole daemon:
//!   - session up + enabled → load libfdk-aac, create the source, watch it;
//!   - first recorder appears → START (Conversational Awareness off while live,
//!     as LibrePods does, since it ducks audio when you speak);
//!   - audio SDUs → decode → write PCM into the source;
//!   - no audio for a while → STOP + START, a few times, then give up;
//!   - last recorder gone → STOP after a short grace period;
//!   - session down / disabled / shutdown → STOP if possible, remove the source.

mod fdk;
mod source;

use std::time::{Duration, Instant};
use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::JoinHandle;
use tracing::{debug, info, warn};

use crate::dbus::SharedCmdTx;
use crate::l2cap::L2capCommand;
use crate::state::{AirPodsState, SharedState};
pub use source::PipeSource;

/// Node name of the source. Audio servers remember the user's default source
/// by node name, so picking it once sticks across reconnects.
pub const SOURCE_NAME: &str = "airpods_mic";
pub const SOURCE_DESCRIPTION: &str = "AirPods Microphone";

/// Rate the decoded PCM is played out at. The ASC says 48 kHz, but the buds
/// pace frames for 64 kHz: 64000 / 480 ≈ 133 frames/s, against ~130 measured
/// on AirPods Pro 3, and decoding at 48 kHz audibly stretches the audio.
/// Source: LibrePods PR #655 (eld.rs) and closed PR #774. Unverified on other
/// models, hence the `[mic] sample_rate` override.
pub const DEFAULT_SAMPLE_RATE: u32 = 64_000;

/// How often the supervisor checks its timers.
const TICK: Duration = Duration::from_millis(250);
/// How long to wait for the first audio after START before resending it.
const START_TIMEOUT: Duration = Duration::from_secs(3);
/// Audio gap that counts as a stall while streaming. LibrePods PR #655 uses 2s.
const STALL_TIMEOUT: Duration = Duration::from_secs(2);
/// STOP + START attempts in a row without audio before giving up until the
/// recorders go away and come back.
const MAX_RESTARTS: u32 = 2;
/// Keep streaming this long after the last recorder leaves, so an app that
/// reopens its capture stream (common when switching devices) doesn't bounce
/// the buds in and out of their mic mode.
const IDLE_GRACE: Duration = Duration::from_secs(3);
/// Wait before retrying setup after the audio server went away.
const SETUP_RETRY: Duration = Duration::from_secs(5);
/// Upper bound on cleanup at daemon shutdown.
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Disabled, or no AirPods session.
    Off,
    /// Can't work on this system (no libfdk-aac, no audio server).
    Unavailable,
    /// Source offered; nobody recording, buds not streaming.
    Idle,
    /// START sent, waiting for the first audio.
    Starting,
    Streaming,
    Error,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Unavailable => "unavailable",
            Self::Idle => "idle",
            Self::Starting => "starting",
            Self::Streaming => "streaming",
            Self::Error => "error",
        }
    }
}

/// Where the source goes and what it's called. The daemon uses the
/// constants above; the tracer below uses its own name so it can run next to
/// a live daemon.
#[derive(Debug, Clone)]
pub struct SourceSpec {
    pub name: String,
    pub description: String,
    pub sample_rate: u32,
}

impl SourceSpec {
    pub fn new(sample_rate: u32) -> Self {
        Self {
            name: SOURCE_NAME.to_string(),
            description: SOURCE_DESCRIPTION.to_string(),
            sample_rate,
        }
    }
}

/// Handle the daemon keeps to the supervisor task.
pub struct MicHandle {
    shutdown: Option<oneshot::Sender<()>>,
    task: JoinHandle<()>,
}

impl MicHandle {
    /// Stop streaming and remove the source before the daemon exits. Must run
    /// while the AAP session is still up so STOP reaches the buds.
    pub async fn shutdown(mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
        if tokio::time::timeout(SHUTDOWN_TIMEOUT, &mut self.task).await.is_err() {
            warn!("microphone cleanup timed out");
            self.task.abort();
        }
    }
}

/// Start the supervisor. `audio_rx` carries raw 0x58 audio SDUs from the
/// L2CAP reader; `enabled` follows the `[mic] enabled` setting.
pub fn spawn(
    state: SharedState,
    cmd_tx: SharedCmdTx,
    audio_rx: mpsc::Receiver<Vec<u8>>,
    enabled: watch::Receiver<bool>,
    spec: SourceSpec,
) -> MicHandle {
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let supervisor = Supervisor {
        state_rx: state.subscribe(),
        state,
        cmd_tx,
        audio_rx,
        enabled,
        shutdown: shutdown_rx,
        spec,
    };
    MicHandle {
        shutdown: Some(shutdown_tx),
        task: tokio::spawn(supervisor.run()),
    }
}

/// Why a live source session ended.
enum End {
    /// Session dropped or the feature was turned off: wait for it to come back.
    Inactive,
    /// The audio server side broke; retry setup after a pause.
    Failed(String),
    Shutdown,
}

struct Supervisor {
    state: SharedState,
    state_rx: watch::Receiver<AirPodsState>,
    cmd_tx: SharedCmdTx,
    audio_rx: mpsc::Receiver<Vec<u8>>,
    enabled: watch::Receiver<bool>,
    shutdown: oneshot::Receiver<()>,
    spec: SourceSpec,
}

/// Streaming state while recorders are attached.
struct Stream {
    decoder: fdk::Decoder,
    /// When START was last sent.
    started: Instant,
    last_audio: Option<Instant>,
    /// STOP + START attempts since audio last arrived.
    restarts: u32,
    /// We switched Conversational Awareness off and owe a restore.
    restore_ca: bool,
    decode_errors: u64,
    frames: u64,
}

impl Supervisor {
    fn publish(&self, status: Status, error: impl Into<String>) {
        let error = error.into();
        self.state.update_if_changed(|s| {
            let changed = s.mic_status != status.as_str() || s.mic_error != error;
            s.mic_status = status.as_str().to_string();
            s.mic_error = error;
            changed
        });
    }

    fn active(&self) -> bool {
        self.state_rx.borrow().connected && *self.enabled.borrow()
    }

    async fn send(&self, cmd: L2capCommand) {
        let tx = self.cmd_tx.lock().await.clone();
        match tx {
            Some(tx) => {
                if let Err(e) = tx.try_send(cmd) {
                    warn!("couldn't send microphone command: {e}");
                }
            }
            None => debug!("no AAP session for microphone command"),
        }
    }

    async fn run(mut self) {
        source::cleanup_stale(&self.spec.name).await;
        loop {
            // Idle until a session is up and the feature is on. Audio that
            // arrives meanwhile (buds still streaming from before) is dropped.
            while !self.active() {
                self.publish(Status::Off, "");
                tokio::select! {
                    _ = &mut self.shutdown => return,
                    r = self.state_rx.changed() => if r.is_err() { return },
                    r = self.enabled.changed() => if r.is_err() { return },
                    Some(_) = self.audio_rx.recv() => {}
                }
            }

            let lib = match fdk::Library::load() {
                Ok(lib) => lib,
                Err(e) => {
                    warn!("microphone unavailable: {e}");
                    self.publish(Status::Unavailable, e);
                    if self.wait_inactive().await {
                        return;
                    }
                    continue;
                }
            };
            if let Err(e) = source::detect().await {
                warn!("microphone unavailable: {e}");
                self.publish(Status::Unavailable, e);
                if self.wait_inactive().await {
                    return;
                }
                continue;
            }
            let pipe = match PipeSource::create(&self.spec.name, &self.spec.description, self.spec.sample_rate).await {
                Ok(p) => p,
                Err(e) => {
                    warn!("{e}");
                    self.publish(Status::Error, e);
                    if self.pause(SETUP_RETRY).await {
                        return;
                    }
                    continue;
                }
            };
            info!("microphone source '{}' ready", self.spec.name);
            let end = self.serve(lib, pipe).await;
            match end {
                End::Shutdown => return,
                End::Inactive => {}
                End::Failed(e) => {
                    warn!("microphone source failed: {e}");
                    self.publish(Status::Error, e);
                    if self.pause(SETUP_RETRY).await {
                        return;
                    }
                }
            }
        }
    }

    /// Wait until the session or setting changes. Returns true on shutdown.
    async fn wait_inactive(&mut self) -> bool {
        while self.active() {
            tokio::select! {
                _ = &mut self.shutdown => return true,
                r = self.state_rx.changed() => if r.is_err() { return true },
                r = self.enabled.changed() => if r.is_err() { return true },
                Some(_) = self.audio_rx.recv() => {}
            }
        }
        false
    }

    /// Sleep, still honouring shutdown. Returns true on shutdown.
    async fn pause(&mut self, d: Duration) -> bool {
        tokio::select! {
            _ = &mut self.shutdown => true,
            _ = tokio::time::sleep(d) => false,
        }
    }

    /// Offer the source until the session ends, the feature is turned off,
    /// or something breaks. Always removes the source before returning.
    async fn serve(&mut self, lib: fdk::Library, mut pipe: PipeSource) -> End {
        let end = self.serve_inner(lib, &mut pipe).await;
        if pipe.dropped_writes > 0 {
            info!("microphone: {} PCM writes dropped (audio server not draining)", pipe.dropped_writes);
        }
        pipe.remove().await;
        end
    }

    async fn serve_inner(&mut self, lib: fdk::Library, pipe: &mut PipeSource) -> End {
        let (_watcher, mut changes) = match source::watch_recording_changes() {
            Ok(w) => w,
            Err(e) => return End::Failed(format!("pactl subscribe: {e}")),
        };
        let mut consumers = match pipe.consumers().await {
            Ok(n) => n,
            Err(e) => return End::Failed(e),
        };
        let mut stream: Option<Stream> = None;
        // Set when the buds wouldn't stream; cleared once all recorders leave,
        // so the next recording session gets a fresh try.
        let mut gave_up = false;
        let mut idle_since: Option<Instant> = None;
        let mut tick = tokio::time::interval(TICK);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        self.publish(Status::Idle, "");

        loop {
            tokio::select! {
                _ = &mut self.shutdown => {
                    self.stop(&mut stream).await;
                    return End::Shutdown;
                }
                r = self.state_rx.changed() => {
                    if r.is_err() || !self.state_rx.borrow().connected {
                        // Session is gone, so there's nothing to send STOP over.
                        if let Some(s) = stream.take() {
                            info!("microphone stream ended with the AAP session ({} frames)", s.frames);
                        }
                        return End::Inactive;
                    }
                }
                r = self.enabled.changed() => {
                    if r.is_err() || !*self.enabled.borrow() {
                        self.stop(&mut stream).await;
                        return End::Inactive;
                    }
                }
                tick_msg = changes.recv() => {
                    if tick_msg.is_none() {
                        self.stop(&mut stream).await;
                        return End::Failed("lost the audio server's event stream".into());
                    }
                    match pipe.consumers().await {
                        Ok(n) => {
                            if n != consumers {
                                debug!("microphone recorders: {consumers} -> {n}");
                            }
                            consumers = n;
                        }
                        Err(e) => {
                            self.stop(&mut stream).await;
                            return End::Failed(e);
                        }
                    }
                }
                Some(sdu) = self.audio_rx.recv() => {
                    if let Some(s) = stream.as_mut() {
                        if s.last_audio.is_none() {
                            self.publish(Status::Streaming, "");
                        }
                        s.last_audio = Some(Instant::now());
                        s.restarts = 0;
                        for au in aap::mic::access_units(&sdu) {
                            match s.decoder.decode(au) {
                                Ok(pcm) => {
                                    s.frames += 1;
                                    let samples = pcm.len();
                                    // During the idle grace nobody reads the
                                    // source, so the server doesn't drain the
                                    // FIFO; skip rather than count fake drops.
                                    if consumers > 0 {
                                        pipe.write(pcm);
                                    }
                                    if s.frames == 1 {
                                        // Stream info is only filled in after a decode.
                                        info!(
                                            "microphone streaming ({samples} samples/frame, decoder reports {} Hz, playing at {} Hz)",
                                            s.decoder.sample_rate().unwrap_or(0),
                                            self.spec.sample_rate
                                        );
                                    }
                                }
                                Err(e) => {
                                    s.decode_errors += 1;
                                    if s.decode_errors.is_power_of_two() {
                                        debug!("microphone decode error #{}: {e}", s.decode_errors);
                                    }
                                }
                            }
                        }
                    }
                }
                _ = tick.tick() => {
                    if consumers == 0 && gave_up {
                        gave_up = false;
                        self.publish(Status::Idle, "");
                    }
                    let wanted = consumers > 0 && !gave_up;
                    match (&mut stream, wanted) {
                        (None, true) => {
                            idle_since = None;
                            match lib.decoder(&aap::mic::ELD_ASC) {
                                Ok(decoder) => stream = Some(self.start(decoder).await),
                                Err(e) => {
                                    gave_up = true;
                                    warn!("microphone decoder: {e}");
                                    self.publish(Status::Error, e);
                                }
                            }
                        }
                        (Some(_), false) => {
                            let since = *idle_since.get_or_insert_with(Instant::now);
                            if gave_up || since.elapsed() >= IDLE_GRACE {
                                self.stop(&mut stream).await;
                                idle_since = None;
                                if !gave_up {
                                    self.publish(Status::Idle, "");
                                }
                            }
                        }
                        (Some(s), true) => {
                            idle_since = None;
                            let (waited, limit) = match s.last_audio {
                                Some(t) => (t.elapsed(), STALL_TIMEOUT),
                                None => (s.started.elapsed(), START_TIMEOUT),
                            };
                            if waited >= limit {
                                if s.restarts < MAX_RESTARTS {
                                    s.restarts += 1;
                                    warn!(
                                        "no microphone audio for {:.1}s; restarting the stream ({}/{MAX_RESTARTS})",
                                        waited.as_secs_f32(),
                                        s.restarts
                                    );
                                    self.send(L2capCommand::StopMicStream).await;
                                    self.send(L2capCommand::StartMicStream).await;
                                    s.started = Instant::now();
                                    s.last_audio = None;
                                } else {
                                    let msg = if s.frames == 0 {
                                        "the AirPods didn't send microphone audio (this model or firmware may not support it)"
                                    } else {
                                        "microphone audio stopped and didn't recover"
                                    };
                                    warn!("{msg}");
                                    gave_up = true;
                                    self.stop(&mut stream).await;
                                    self.publish(Status::Error, msg);
                                }
                            }
                        }
                        (None, false) => {
                            idle_since = None;
                        }
                    }
                }
            }
        }
    }

    /// Ask the buds to start streaming.
    async fn start(&self, decoder: fdk::Decoder) -> Stream {
        let restore_ca = self.state_rx.borrow().conversational_awareness;
        if restore_ca {
            debug!("turning Conversational Awareness off while the microphone streams");
            self.send(L2capCommand::SetConversationalAwareness(false)).await;
        }
        info!("recording started; asking the AirPods to stream the microphone");
        self.send(L2capCommand::StartMicStream).await;
        self.publish(Status::Starting, "");
        Stream {
            decoder,
            started: Instant::now(),
            last_audio: None,
            restarts: 0,
            restore_ca,
            decode_errors: 0,
            frames: 0,
        }
    }

    /// Ask the buds to stop and undo what `start` changed.
    async fn stop(&self, stream: &mut Option<Stream>) {
        let Some(s) = stream.take() else { return };
        self.send(L2capCommand::StopMicStream).await;
        if s.restore_ca {
            self.send(L2capCommand::SetConversationalAwareness(true)).await;
        }
        info!(
            "microphone stream stopped ({} frames, {} decode errors)",
            s.frames, s.decode_errors
        );
    }
}

/// End-to-end tracer for the mic path without AirPods. Run it with
/// `cargo test -p airpods-daemon mic_tracer -- --ignored --nocapture`
/// (needs libfdk-aac and a running PipeWire or PulseAudio).
///
/// It drives the real supervisor: fake buds answer START with real AAC-ELD
/// frames of a 1 kHz tone, framed into 0x58 SDUs and paced like the AirPods
/// (4 frames per 30 ms). `pw-record` captures from the source, and the tone
/// has to come out the other end at the right pitch. Everything past the
/// L2CAP socket runs as in the daemon.
#[cfg(test)]
mod tracer {
    use super::*;
    use crate::state::create_shared_state;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    const TONE_HZ: f64 = 1000.0;
    /// Matches the buds' pacing: 4 AUs of 480 samples per 30 ms at 64 kHz.
    const AUS_PER_SDU: usize = 4;
    const SDU_INTERVAL: Duration = Duration::from_millis(30);
    const RECORD_FOR: Duration = Duration::from_secs(3);
    const RECORD_RATE: u32 = 48_000;
    /// Bitrate for the synthetic stream; LibrePods measured ~80 kbps VBR.
    const BITRATE: u32 = 80_000;

    /// Poll the shared state until `mic_status` matches, or time out.
    async fn wait_status(state: &SharedState, want: &str, within: Duration) -> bool {
        let deadline = Instant::now() + within;
        while Instant::now() < deadline {
            if state.current().mic_status == want {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        false
    }

    /// Read a 16-bit mono WAV's samples (skips chunks until "data").
    fn read_wav(bytes: &[u8]) -> (u32, Vec<i16>) {
        let rate = u32::from_le_bytes(bytes[24..28].try_into().unwrap());
        let mut at = 12;
        while at + 8 <= bytes.len() {
            let id = &bytes[at..at + 4];
            let len = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().unwrap()) as usize;
            if id == b"data" {
                let end = (at + 8 + len).min(bytes.len());
                let samples = bytes[at + 8..end]
                    .chunks_exact(2)
                    .map(|c| i16::from_le_bytes([c[0], c[1]]))
                    .collect();
                return (rate, samples);
            }
            at += 8 + len;
        }
        (rate, Vec::new())
    }

    #[tokio::test(flavor = "current_thread")]
    #[ignore = "needs libfdk-aac and a live audio server"]
    async fn mic_tracer() {
        let _ = tracing_subscriber::fmt().with_test_writer().try_init();

        // Encode the tone as the buds would: 480-sample frames, sampled at the
        // playout rate, described by the same 48 kHz ASC the daemon decodes with.
        let mut encoder = fdk::encoder::Encoder::eld_mono(48_000, BITRATE).expect("ELD encoder");
        println!("encoder ASC {:02X?} (daemon decodes with {:02X?}), frame {}",
            encoder.asc, aap::mic::ELD_ASC, encoder.frame_length);
        assert_eq!(encoder.frame_length, aap::mic::ELD_FRAME_SAMPLES);
        let total_frames = (RECORD_FOR.as_secs_f64() + 4.0) * DEFAULT_SAMPLE_RATE as f64 / 480.0;
        let mut aus: Vec<Vec<u8>> = Vec::new();
        let mut n = 0u64;
        while aus.len() < total_frames as usize {
            let pcm: Vec<i16> = (0..480)
                .map(|_| {
                    let t = n as f64 / DEFAULT_SAMPLE_RATE as f64;
                    n += 1;
                    (8000.0 * (2.0 * std::f64::consts::PI * TONE_HZ * t).sin()) as i16
                })
                .collect();
            let au = encoder.encode(&pcm).expect("encode");
            if !au.is_empty() {
                aus.push(au);
            }
        }
        let max_au = aus.iter().map(Vec::len).max().unwrap_or(0);
        println!("encoded {} AUs, largest {} bytes", aus.len(), max_au);

        // The daemon's side: session "up", command channel, audio channel.
        let state = create_shared_state();
        state.update(|s| s.connected = true);
        let (l2cap_tx, mut l2cap_rx) = mpsc::channel::<L2capCommand>(32);
        let cmd_tx: SharedCmdTx = Arc::new(Mutex::new(Some(l2cap_tx)));
        let (audio_tx, audio_rx) = mpsc::channel::<Vec<u8>>(64);
        let (_enabled_tx, enabled_rx) = watch::channel(true);
        let spec = SourceSpec {
            name: "airpods_mic_tracer".into(),
            description: "AirPods Microphone (tracer)".into(),
            sample_rate: DEFAULT_SAMPLE_RATE,
        };
        let handle = spawn(state.clone(), cmd_tx, audio_rx, enabled_rx, spec.clone());

        // Fake buds: stream SDUs between START and STOP, log every command.
        let log: Arc<std::sync::Mutex<Vec<String>>> = Default::default();
        let buds_log = log.clone();
        let sent = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let buds_sent = sent.clone();
        let buds = tokio::spawn(async move {
            let mut streaming = false;
            let mut next = 0usize;
            let mut tick = tokio::time::interval(SDU_INTERVAL);
            loop {
                tokio::select! {
                    cmd = l2cap_rx.recv() => {
                        let Some(cmd) = cmd else { break };
                        buds_log.lock().unwrap().push(format!("{cmd:?}"));
                        match cmd {
                            L2capCommand::StartMicStream => streaming = true,
                            L2capCommand::StopMicStream => streaming = false,
                            _ => {}
                        }
                    }
                    _ = tick.tick(), if streaming && next < aus.len() => {
                        let units = aus[next..(next + AUS_PER_SDU).min(aus.len())]
                            .iter()
                            .enumerate()
                            .map(|(i, au)| ((next + i) as u32, au.as_slice()));
                        let sdu = aap::mic::build_audio_sdu(units);
                        next += AUS_PER_SDU;
                        buds_sent.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        let _ = audio_tx.try_send(sdu);
                    }
                }
            }
        });

        assert!(wait_status(&state, "idle", Duration::from_secs(5)).await,
            "source never became idle: {:?} {:?}", state.current().mic_status, state.current().mic_error);
        let status_before = state.current().mic_status;

        // Record from the source like any app would.
        let scratch = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../.scratch");
        std::fs::create_dir_all(&scratch).unwrap();
        let wav = scratch.join("mic-tracer.wav");
        let mut rec = tokio::process::Command::new("pw-record")
            .args(["--target", &spec.name, "--rate", &RECORD_RATE.to_string(), "--channels", "1"])
            .arg(&wav)
            .kill_on_drop(true)
            .spawn()
            .expect("pw-record");
        let reached_streaming = wait_status(&state, "streaming", Duration::from_secs(5)).await;
        tokio::time::sleep(RECORD_FOR).await;
        // SIGINT lets pw-record finish the WAV header.
        unsafe { libc::kill(rec.id().unwrap() as i32, libc::SIGINT) };
        let _ = rec.wait().await;

        let back_to_idle = wait_status(&state, "idle", IDLE_GRACE + Duration::from_secs(3)).await;
        let commands = log.lock().unwrap().clone();
        handle.shutdown().await;
        buds.abort();
        let leftover = crate::eq::run_cmd("pactl", &["list", "short", "modules"]).await.unwrap_or_default();
        let leaked = leftover.contains(&format!("source_name={}", spec.name));

        let bytes = std::fs::read(&wav).unwrap_or_default();
        let _ = std::fs::remove_file(&wav);
        let (rate, samples) = read_wav(&bytes);
        // Skip the first 0.5 s (stream start-up), measure the rest.
        let body = &samples[(rate as usize / 2).min(samples.len())..];
        let crossings = body.windows(2).filter(|w| w[0] < 0 && w[1] >= 0).count();
        let secs = body.len() as f64 / rate.max(1) as f64;
        let measured_hz = if secs > 0.0 { crossings as f64 / secs } else { 0.0 };
        let peak = body.iter().map(|s| s.unsigned_abs()).max().unwrap_or(0);
        let rms = (body.iter().map(|&s| (s as f64).powi(2)).sum::<f64>() / body.len().max(1) as f64).sqrt();

        println!("--- mic tracer report ---");
        println!("status before recording : {status_before}");
        println!("reached 'streaming'     : {reached_streaming}");
        println!("back to 'idle' after    : {back_to_idle}");
        println!("commands to buds        : {commands:?}");
        println!("SDUs sent by fake buds  : {}", sent.load(std::sync::atomic::Ordering::Relaxed));
        println!("recorded                : {} samples @ {rate} Hz ({:.2}s)", samples.len(), samples.len() as f64 / rate.max(1) as f64);
        println!("tone                    : expected {TONE_HZ} Hz, measured {measured_hz:.1} Hz");
        println!("level                   : peak {peak}, rms {rms:.0} (input amplitude 8000)");
        // Where the quiet samples are: gaps (dropouts) vs. the sine's own
        // zero crossings, which only give runs of a few samples.
        let mut runs: Vec<(usize, usize)> = Vec::new(); // (start, len)
        let mut i = 0;
        while i < body.len() {
            if body[i].unsigned_abs() < 64 {
                let start = i;
                while i < body.len() && body[i].unsigned_abs() < 64 {
                    i += 1;
                }
                if i - start >= 16 {
                    runs.push((start, i - start));
                }
            } else {
                i += 1;
            }
        }
        let to_ms = |n: usize| n as f64 * 1000.0 / rate.max(1) as f64;
        println!("silent gaps (>=16 smp)  : {} gaps, {:.1} ms total", runs.len(), to_ms(runs.iter().map(|r| r.1).sum()));
        for (start, len) in runs.iter().take(10) {
            println!("    at {:7.1} ms (after skip) : {:6.1} ms", to_ms(*start), to_ms(*len));
        }
        println!("source module leaked    : {leaked}");

        assert!(reached_streaming && back_to_idle && !leaked);
        assert!(commands.first().map(String::as_str) == Some("StartMicStream"));
        assert!(commands.iter().any(|c| c == "StopMicStream"));
        assert!((measured_hz - TONE_HZ).abs() < TONE_HZ * 0.03, "pitch off: {measured_hz:.1} Hz");
        assert!(runs.is_empty(), "audio has dropouts");
    }
}
