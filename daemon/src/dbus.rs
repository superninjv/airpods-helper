//! `org.costa.AirPods` D-Bus service. See `docs/dbus-api.md` for the contract.

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, mpsc, oneshot, watch};
use tracing::{info, warn};
use zbus::object_server::{Interface, InterfaceRef, SignalEmitter};
use zbus::zvariant::Value;
use zbus::{Connection, fdo, interface};

use crate::config::{self, SharedConfig};
use crate::eq::{EqBand, EqPreset, FilterType, PresetError, Source};
use crate::l2cap::L2capCommand;
use crate::state::{AirPodsState, SharedState};
use aap::{AncMode, MicMode};

pub const OBJECT_PATH: &str = "/org/costa/AirPods";
pub const BUS_NAME: &str = "org.costa.AirPods";

/// Shared handle to the active L2CAP command sender (swapped per session)
pub type SharedCmdTx = Arc<Mutex<Option<mpsc::Sender<L2capCommand>>>>;

/// Requests from D-Bus clients that the main loop handles.
#[derive(Debug)]
pub enum Control {
    Reconnect,
    /// The user explicitly disconnected — don't auto-reconnect.
    UserDisconnected,
    /// Select an EQ preset by id, or `None` to disable EQ. The sender is
    /// signalled once the change has taken effect.
    EqSelect(Option<String>, oneshot::Sender<()>),
    /// A preset file changed on disk; re-apply if it's the active one.
    EqPresetChanged(String, oneshot::Sender<()>),
}

pub struct AirPodsInterface {
    state: SharedState,
    config: SharedConfig,
    cmd_tx: SharedCmdTx,
    control: mpsc::Sender<Control>,
}

fn failed(msg: impl std::fmt::Display) -> fdo::Error {
    fdo::Error::Failed(msg.to_string())
}

fn preset_error(e: PresetError) -> fdo::Error {
    match e {
        PresetError::InvalidId(_) | PresetError::Invalid(_) => {
            fdo::Error::InvalidArgs(e.to_string())
        }
        _ => failed(e),
    }
}

impl AirPodsInterface {
    async fn send_cmd(&self, cmd: L2capCommand) -> fdo::Result<()> {
        let guard = self.cmd_tx.lock().await;
        let tx = guard.as_ref().ok_or_else(|| failed("not connected"))?;
        tx.send(cmd)
            .await
            .map_err(|_| failed("AirPods session ended"))
    }

    async fn control(&self, msg: Control) -> fdo::Result<()> {
        self.control
            .send(msg)
            .await
            .map_err(|_| failed("daemon is shutting down"))
    }

    /// Send a control message and wait until the main loop has handled it,
    /// so the reply means "done" and a following property read is current.
    async fn control_wait(
        &self,
        make: impl FnOnce(oneshot::Sender<()>) -> Control,
    ) -> fdo::Result<()> {
        let (tx, rx) = oneshot::channel();
        self.control(make(tx)).await?;
        rx.await.map_err(|_| failed("daemon is shutting down"))
    }

    fn s(&self) -> AirPodsState {
        self.state.current()
    }

    fn save_setting(&self, f: impl FnOnce(&mut config::Config)) -> fdo::Result<()> {
        config::update_config(&self.config, f)
            .map_err(|e| failed(format!("failed to save config: {e}")))
    }
}

type Band = (String, f64, f64, f64);

fn bands_from_wire(bands: Vec<Band>) -> fdo::Result<Vec<EqBand>> {
    bands
        .into_iter()
        .enumerate()
        .map(|(i, (ty, freq, q, gain))| {
            let filter_type = FilterType::parse(&ty.to_ascii_lowercase()).ok_or_else(|| {
                fdo::Error::InvalidArgs(format!("band {}: unknown filter type '{ty}'", i + 1))
            })?;
            Ok(EqBand {
                filter_type,
                freq,
                q,
                gain,
            })
        })
        .collect()
}

#[interface(name = "org.costa.AirPods")]
impl AirPodsInterface {
    // ─── Device properties ────────────────────────────────────────────

    #[zbus(property)]
    fn connected(&self) -> bool {
        self.s().connected
    }
    #[zbus(property)]
    fn address(&self) -> String {
        self.s().address
    }
    #[zbus(property)]
    fn model(&self) -> String {
        self.s().model
    }
    #[zbus(property)]
    fn model_name(&self) -> String {
        self.s().model_name
    }
    #[zbus(property)]
    fn firmware(&self) -> String {
        self.s().firmware
    }
    #[zbus(property)]
    fn features(&self) -> Vec<String> {
        self.s().features
    }
    #[zbus(property)]
    fn battery_left(&self) -> i32 {
        self.s().battery_left
    }
    #[zbus(property)]
    fn battery_right(&self) -> i32 {
        self.s().battery_right
    }
    #[zbus(property)]
    fn battery_case(&self) -> i32 {
        self.s().battery_case
    }
    #[zbus(property)]
    fn charging_left(&self) -> bool {
        self.s().charging_left
    }
    #[zbus(property)]
    fn charging_right(&self) -> bool {
        self.s().charging_right
    }
    #[zbus(property)]
    fn charging_case(&self) -> bool {
        self.s().charging_case
    }
    #[zbus(property)]
    fn ear_left(&self) -> bool {
        self.s().ear_left
    }
    #[zbus(property)]
    fn ear_right(&self) -> bool {
        self.s().ear_right
    }
    #[zbus(property)]
    fn anc_mode(&self) -> String {
        self.s().anc_mode.as_str().to_string()
    }
    #[zbus(property)]
    fn adaptive_noise_level(&self) -> u8 {
        self.s().adaptive_noise_level
    }
    #[zbus(property)]
    fn conversational_awareness(&self) -> bool {
        self.s().conversational_awareness
    }
    #[zbus(property)]
    fn conversational_activity_state(&self) -> String {
        self.s().conversational_activity
    }
    #[zbus(property)]
    fn one_bud_anc(&self) -> bool {
        self.s().one_bud_anc
    }
    #[zbus(property)]
    fn volume_swipe(&self) -> bool {
        self.s().volume_swipe
    }
    #[zbus(property)]
    fn adaptive_volume(&self) -> bool {
        self.s().adaptive_volume
    }
    #[zbus(property)]
    fn chime_volume(&self) -> u8 {
        self.s().chime_volume
    }
    #[zbus(property)]
    fn audio_source(&self) -> String {
        self.s().audio_source
    }
    #[zbus(property)]
    fn mic_mode(&self) -> String {
        self.s().mic_mode
    }
    #[zbus(property)]
    fn version(&self) -> String {
        env!("CARGO_PKG_VERSION").to_string()
    }

    // ─── EQ properties ────────────────────────────────────────────────

    #[zbus(property)]
    fn eq_preset(&self) -> String {
        self.s().eq_preset
    }
    #[zbus(property)]
    fn eq_status(&self) -> String {
        self.s().eq_status
    }
    #[zbus(property)]
    fn eq_error(&self) -> String {
        self.s().eq_error
    }
    #[zbus(property)]
    fn eq_backend(&self) -> String {
        self.s().eq_backend
    }

    // ─── Settings (read-write, persisted) ─────────────────────────────

    #[zbus(property)]
    fn pause_on_removal(&self) -> bool {
        config::read(&self.config, |c| c.ear_detection.pause_media)
    }
    #[zbus(property)]
    fn set_pause_on_removal(&mut self, v: bool) -> fdo::Result<()> {
        self.save_setting(|c| c.ear_detection.pause_media = v)
    }

    #[zbus(property)]
    fn resume_on_insert(&self) -> bool {
        config::read(&self.config, |c| c.ear_detection.resume_media)
    }
    #[zbus(property)]
    fn set_resume_on_insert(&mut self, v: bool) -> fdo::Result<()> {
        self.save_setting(|c| c.ear_detection.resume_media = v)
    }

    #[zbus(property)]
    fn auto_reconnect(&self) -> bool {
        config::read(&self.config, |c| c.reconnect.auto_reconnect)
    }
    #[zbus(property)]
    fn set_auto_reconnect(&mut self, v: bool) -> fdo::Result<()> {
        self.save_setting(|c| c.reconnect.auto_reconnect = v)
    }

    #[zbus(property)]
    fn eq_auto_load(&self) -> bool {
        config::read(&self.config, |c| c.eq.auto_load)
    }
    #[zbus(property)]
    fn set_eq_auto_load(&mut self, v: bool) -> fdo::Result<()> {
        self.save_setting(|c| c.eq.auto_load = v)
    }

    #[zbus(property)]
    fn preferred_device(&self) -> String {
        config::read(&self.config, |c| {
            c.device.address.clone().unwrap_or_default()
        })
    }
    #[zbus(property)]
    fn set_preferred_device(&mut self, v: String) -> fdo::Result<()> {
        let v = v.trim().to_ascii_uppercase();
        if !v.is_empty() && v.parse::<bluer::Address>().is_err() {
            return Err(fdo::Error::InvalidArgs(format!(
                "invalid MAC address '{v}'"
            )));
        }
        self.save_setting(|c| c.device.address = (!v.is_empty()).then_some(v))
    }

    // ─── Controls ─────────────────────────────────────────────────────

    async fn set_anc_mode(&self, mode: &str) -> fdo::Result<()> {
        let anc_mode = AncMode::parse(mode)
            .ok_or_else(|| fdo::Error::InvalidArgs(format!("invalid ANC mode: {mode}")))?;
        self.send_cmd(L2capCommand::SetAncMode(anc_mode)).await
    }

    async fn set_conversational_awareness(&self, enabled: bool) -> fdo::Result<()> {
        self.send_cmd(L2capCommand::SetConversationalAwareness(enabled))
            .await
    }

    async fn set_adaptive_noise_level(&self, level: u8) -> fdo::Result<()> {
        if level > 100 {
            return Err(fdo::Error::InvalidArgs("level must be 0-100".into()));
        }
        self.send_cmd(L2capCommand::SetAdaptiveNoiseLevel(level))
            .await
    }

    async fn set_one_bud_anc(&self, enabled: bool) -> fdo::Result<()> {
        self.send_cmd(L2capCommand::SetOneBudAnc(enabled)).await
    }

    async fn set_volume_swipe(&self, enabled: bool) -> fdo::Result<()> {
        self.send_cmd(L2capCommand::SetVolumeSwipe(enabled)).await
    }

    /// Set which bud is the primary microphone. Accepts "auto", "right", "left".
    async fn set_mic_mode(&self, mode: &str) -> fdo::Result<()> {
        let mic_mode = MicMode::parse(mode)
            .ok_or_else(|| fdo::Error::InvalidArgs(format!("invalid mic mode: {mode}")))?;
        self.send_cmd(L2capCommand::SetMicMode(mic_mode)).await
    }

    // ─── Devices ──────────────────────────────────────────────────────

    async fn reconnect(&self) -> fdo::Result<()> {
        info!("reconnect requested via D-Bus");
        self.control(Control::Reconnect).await
    }

    /// Trigger a BlueZ-level connect to the given AirPods MAC.
    /// The AAP session starts automatically once BlueZ reports the connection.
    async fn connect_to(&self, address: &str) -> fdo::Result<()> {
        info!("ConnectTo requested via D-Bus: {address}");
        let addr: bluer::Address = address
            .parse()
            .map_err(|e| fdo::Error::InvalidArgs(format!("invalid MAC '{address}': {e}")))?;
        crate::bluez::connect_device(addr)
            .await
            .map_err(|e| failed(format!("Bluetooth connect failed: {e}")))
    }

    /// Disconnect the connected AirPods at the BlueZ level. Auto-reconnect is
    /// suppressed until the next time they connect.
    async fn disconnect(&self) -> fdo::Result<()> {
        info!("Disconnect requested via D-Bus");
        let addr = crate::bluez::currently_connected_airpods()
            .await
            .map_err(|e| failed(format!("BlueZ query failed: {e}")))?
            .ok_or_else(|| failed("no AirPods currently connected"))?;
        self.control(Control::UserDisconnected).await?;
        crate::bluez::disconnect_device(addr)
            .await
            .map_err(|e| failed(format!("Bluetooth disconnect failed: {e}")))
    }

    /// Pair (and trust) AirPods by MAC. They must be in pairing mode.
    async fn pair(&self, address: &str) -> fdo::Result<()> {
        info!("Pair requested via D-Bus: {address}");
        let addr: bluer::Address = address
            .parse()
            .map_err(|e| fdo::Error::InvalidArgs(format!("invalid MAC '{address}': {e}")))?;
        crate::bluez::pair_and_trust(addr)
            .await
            .map_err(|e| failed(format!("pairing failed: {e}")))
    }

    /// LE scan for nearby AirPods. Returns (mac, name, model, rssi, in_pair_mode).
    async fn quick_pair_scan(
        &self,
        duration_secs: u32,
    ) -> fdo::Result<Vec<(String, String, String, i16, bool)>> {
        info!("QuickPairScan requested via D-Bus, duration={duration_secs}s");
        let candidates = crate::bluez::quick_pair_scan(duration_secs.clamp(1, 30))
            .await
            .map_err(|e| failed(format!("Bluetooth scan failed: {e}")))?;
        Ok(candidates
            .into_iter()
            .map(|c| {
                (
                    c.address.to_string(),
                    c.name,
                    c.model_hint,
                    c.rssi,
                    c.in_pair_mode,
                )
            })
            .collect())
    }

    /// Paired AirPods known to BlueZ: (mac, name).
    async fn list_paired(&self) -> fdo::Result<Vec<(String, String)>> {
        let paired = crate::bluez::list_paired_airpods()
            .await
            .map_err(|e| failed(format!("BlueZ query failed: {e}")))?;
        Ok(paired
            .into_iter()
            .map(|(a, n)| (a.to_string(), n))
            .collect())
    }

    // ─── EQ ───────────────────────────────────────────────────────────

    async fn list_eq_presets(&self) -> Vec<String> {
        EqPreset::list().into_iter().map(|(p, _)| p.id).collect()
    }

    /// (id, name, description, user_editable)
    async fn get_eq_presets(&self) -> Vec<(String, String, String, bool)> {
        EqPreset::list()
            .into_iter()
            .map(|(p, src)| (p.id, p.name, p.description, src == Source::User))
            .collect()
    }

    /// (name, description, preamp, [(type, freq, q, gain)])
    async fn get_eq_preset(&self, id: &str) -> fdo::Result<(String, String, f64, Vec<Band>)> {
        let (p, _) = EqPreset::load(id).map_err(preset_error)?;
        let bands = p
            .bands
            .iter()
            .map(|b| (b.filter_type.as_str().to_string(), b.freq, b.q, b.gain))
            .collect();
        Ok((p.name, p.description, p.preamp, bands))
    }

    async fn set_eq_preset(&self, id: &str) -> fdo::Result<()> {
        info!("SetEqPreset requested: {id}");
        EqPreset::load(id).map_err(preset_error)?;
        self.control_wait(|done| Control::EqSelect(Some(id.to_string()), done)).await
    }

    async fn disable_eq(&self) -> fdo::Result<()> {
        info!("DisableEq requested via D-Bus");
        self.control_wait(|done| Control::EqSelect(None, done)).await
    }

    async fn save_eq_preset(
        &self,
        id: &str,
        name: &str,
        description: &str,
        preamp: f64,
        bands: Vec<Band>,
    ) -> fdo::Result<()> {
        let preset = EqPreset {
            id: id.to_string(),
            name: name.trim().to_string(),
            description: description.trim().to_string(),
            preamp,
            bands: bands_from_wire(bands)?,
        };
        let path = preset.save().map_err(preset_error)?;
        info!("saved EQ preset '{id}' to {}", path.display());
        self.control_wait(|done| Control::EqPresetChanged(id.to_string(), done)).await
    }

    async fn delete_eq_preset(&self, id: &str) -> fdo::Result<()> {
        EqPreset::delete(id).map_err(preset_error)?;
        info!("deleted EQ preset '{id}'");
        // A built-in with the same id may now show through; the main loop
        // re-applies or disables as appropriate.
        self.control_wait(|done| Control::EqPresetChanged(id.to_string(), done)).await
    }

    // ─── Signals ──────────────────────────────────────────────────────

    #[zbus(signal)]
    pub async fn device_connected(emitter: &SignalEmitter<'_>, model: &str) -> zbus::Result<()>;

    #[zbus(signal)]
    pub async fn device_disconnected(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;

    #[zbus(signal)]
    pub async fn ear_detection_changed(
        emitter: &SignalEmitter<'_>,
        left: bool,
        right: bool,
    ) -> zbus::Result<()>;
}

/// Start the D-Bus service.
pub async fn serve(
    state: SharedState,
    config: SharedConfig,
    cmd_tx: SharedCmdTx,
    control: mpsc::Sender<Control>,
) -> anyhow::Result<Connection> {
    let iface = AirPodsInterface {
        state,
        config,
        cmd_tx,
        control,
    };
    let connection = Connection::session().await?;
    connection.object_server().at(OBJECT_PATH, iface).await?;
    connection.request_name(BUS_NAME).await.map_err(|e| {
        anyhow::anyhow!("can't claim {BUS_NAME} on the session bus ({e}) — is another airpods-daemon already running?")
    })?;
    info!("D-Bus service running at {BUS_NAME}");
    Ok(connection)
}

/// Properties whose value is derived from [`AirPodsState`], with the value
/// each one exposes. Anything that differs between two snapshots is
/// announced in a single `PropertiesChanged` signal.
fn state_properties(s: &AirPodsState) -> Vec<(&'static str, Value<'static>)> {
    vec![
        ("Connected", s.connected.into()),
        ("Address", s.address.clone().into()),
        ("Model", s.model.clone().into()),
        ("ModelName", s.model_name.clone().into()),
        ("Firmware", s.firmware.clone().into()),
        ("Features", s.features.clone().into()),
        ("BatteryLeft", s.battery_left.into()),
        ("BatteryRight", s.battery_right.into()),
        ("BatteryCase", s.battery_case.into()),
        ("ChargingLeft", s.charging_left.into()),
        ("ChargingRight", s.charging_right.into()),
        ("ChargingCase", s.charging_case.into()),
        ("EarLeft", s.ear_left.into()),
        ("EarRight", s.ear_right.into()),
        ("AncMode", s.anc_mode.as_str().into()),
        ("AdaptiveNoiseLevel", s.adaptive_noise_level.into()),
        ("ConversationalAwareness", s.conversational_awareness.into()),
        (
            "ConversationalActivityState",
            s.conversational_activity.clone().into(),
        ),
        ("OneBudAnc", s.one_bud_anc.into()),
        ("VolumeSwipe", s.volume_swipe.into()),
        ("AdaptiveVolume", s.adaptive_volume.into()),
        ("ChimeVolume", s.chime_volume.into()),
        ("AudioSource", s.audio_source.clone().into()),
        ("MicMode", s.mic_mode.clone().into()),
        ("EqPreset", s.eq_preset.clone().into()),
        ("EqStatus", s.eq_status.clone().into()),
        ("EqError", s.eq_error.clone().into()),
        ("EqBackend", s.eq_backend.clone().into()),
    ]
}

/// Watch the shared state and emit `PropertiesChanged` (plus the
/// convenience signals) for whatever changed. This is the only place state
/// changes are announced, so no code path can forget to.
pub async fn run_property_notifier(connection: Connection, mut rx: watch::Receiver<AirPodsState>) {
    let iface: InterfaceRef<AirPodsInterface> =
        match connection.object_server().interface(OBJECT_PATH).await {
            Ok(i) => i,
            Err(e) => {
                warn!("property notifier disabled: {e}");
                return;
            }
        };
    let emitter = iface.signal_emitter();
    let mut prev = rx.borrow_and_update().clone();

    while rx.changed().await.is_ok() {
        let cur = rx.borrow_and_update().clone();
        let old = state_properties(&prev);
        let changed: HashMap<&str, Value<'_>> = state_properties(&cur)
            .into_iter()
            .zip(old)
            .filter(|((_, new), (_, old))| new != old)
            .map(|((name, v), _)| (name, v))
            .collect();

        if !changed.is_empty()
            && let Err(e) = fdo::Properties::properties_changed(
                emitter,
                AirPodsInterface::name(),
                changed,
                Cow::Borrowed(&[]),
            )
            .await
        {
            warn!("failed to emit PropertiesChanged: {e}");
        }

        // Announce "connected" once the model is known, so listeners get a name.
        let became_ready =
            cur.connected && !cur.model.is_empty() && (!prev.connected || prev.model.is_empty());
        if became_ready {
            let name = if cur.model_name.is_empty() {
                cur.model.as_str()
            } else {
                cur.model_name.as_str()
            };
            let _ = AirPodsInterface::device_connected(emitter, name).await;
        }
        if prev.connected && !cur.connected {
            let _ = AirPodsInterface::device_disconnected(emitter).await;
        }
        if cur.connected && (cur.ear_left, cur.ear_right) != (prev.ear_left, prev.ear_right) {
            let _ =
                AirPodsInterface::ear_detection_changed(emitter, cur.ear_left, cur.ear_right).await;
        }
        prev = cur;
    }
}
