use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tracing::info;
use zbus::{interface, object_server::SignalEmitter, Connection};

use crate::aap::AncMode;
use crate::eq::{EqCommand, EqPreset};
use crate::l2cap::L2capCommand;
use crate::state::SharedState;

/// Shared handle to the active L2CAP command sender (swapped per session)
pub type SharedCmdTx = Arc<Mutex<Option<mpsc::Sender<L2capCommand>>>>;

/// D-Bus service exposing AirPods state on org.costa.AirPods
pub struct AirPodsInterface {
    state: SharedState,
    cmd_tx: SharedCmdTx,
    reconnect_tx: mpsc::Sender<()>,
    eq_tx: mpsc::Sender<EqCommand>,
}

impl AirPodsInterface {
    pub fn new(
        state: SharedState,
        cmd_tx: SharedCmdTx,
        reconnect_tx: mpsc::Sender<()>,
        eq_tx: mpsc::Sender<EqCommand>,
    ) -> Self {
        Self {
            state,
            cmd_tx,
            reconnect_tx,
            eq_tx,
        }
    }
}

impl AirPodsInterface {
    async fn send_cmd(&self, cmd: L2capCommand) -> zbus::fdo::Result<()> {
        let guard = self.cmd_tx.lock().await;
        let tx = guard.as_ref()
            .ok_or_else(|| zbus::fdo::Error::Failed("not connected".into()))?;
        tx.send(cmd).await
            .map_err(|_| zbus::fdo::Error::Failed("L2CAP session ended".into()))?;
        Ok(())
    }
}

#[interface(name = "org.costa.AirPods")]
impl AirPodsInterface {
    // --- Properties ---

    #[zbus(property)]
    fn connected(&self) -> bool {
        self.state.current().connected
    }

    #[zbus(property)]
    fn battery_left(&self) -> i32 {
        self.state.current().battery_left
    }

    #[zbus(property)]
    fn battery_right(&self) -> i32 {
        self.state.current().battery_right
    }

    #[zbus(property)]
    fn battery_case(&self) -> i32 {
        self.state.current().battery_case
    }

    #[zbus(property)]
    fn charging_left(&self) -> bool {
        self.state.current().charging_left
    }

    #[zbus(property)]
    fn charging_right(&self) -> bool {
        self.state.current().charging_right
    }

    #[zbus(property)]
    fn charging_case(&self) -> bool {
        self.state.current().charging_case
    }

    #[zbus(property)]
    fn anc_mode(&self) -> String {
        self.state.current().anc_mode.as_str().to_string()
    }

    #[zbus(property)]
    fn ear_left(&self) -> bool {
        self.state.current().ear_left
    }

    #[zbus(property)]
    fn ear_right(&self) -> bool {
        self.state.current().ear_right
    }

    #[zbus(property)]
    fn conversational_awareness(&self) -> bool {
        self.state.current().conversational_awareness
    }

    #[zbus(property)]
    fn adaptive_noise_level(&self) -> u8 {
        self.state.current().adaptive_noise_level
    }

    #[zbus(property)]
    fn one_bud_anc(&self) -> bool {
        self.state.current().one_bud_anc
    }

    #[zbus(property)]
    fn model(&self) -> String {
        self.state.current().model.clone()
    }

    #[zbus(property)]
    fn firmware(&self) -> String {
        self.state.current().firmware.clone()
    }

    // --- Methods ---

    async fn set_anc_mode(&self, mode: &str) -> zbus::fdo::Result<()> {
        let anc_mode = AncMode::from_str(mode)
            .ok_or_else(|| zbus::fdo::Error::InvalidArgs(format!("invalid ANC mode: {mode}")))?;
        self.send_cmd(L2capCommand::SetAncMode(anc_mode)).await
    }

    async fn set_conversational_awareness(&self, enabled: bool) -> zbus::fdo::Result<()> {
        self.send_cmd(L2capCommand::SetConversationalAwareness(enabled)).await
    }

    async fn set_adaptive_noise_level(&self, level: u8) -> zbus::fdo::Result<()> {
        if level > 100 {
            return Err(zbus::fdo::Error::InvalidArgs("level must be 0-100".into()));
        }
        self.send_cmd(L2capCommand::SetAdaptiveNoiseLevel(level)).await
    }

    async fn set_one_bud_anc(&self, enabled: bool) -> zbus::fdo::Result<()> {
        self.send_cmd(L2capCommand::SetOneBudAnc(enabled)).await
    }

    async fn reconnect(&self) -> zbus::fdo::Result<()> {
        info!("reconnect requested via D-Bus");
        self.reconnect_tx
            .send(())
            .await
            .map_err(|_| zbus::fdo::Error::Failed("reconnect channel closed".into()))?;
        Ok(())
    }

    // --- EQ Properties ---

    #[zbus(property)]
    fn eq_preset(&self) -> String {
        self.state.current().eq_preset.clone()
    }

    // --- EQ Methods ---

    async fn set_eq_preset(&self, name: &str) -> zbus::fdo::Result<()> {
        info!("SetEqPreset requested: {name}");
        self.eq_tx
            .send(EqCommand::Apply(name.to_string()))
            .await
            .map_err(|_| zbus::fdo::Error::Failed("EQ channel closed".into()))?;
        Ok(())
    }

    async fn disable_eq(&self) -> zbus::fdo::Result<()> {
        info!("DisableEq requested via D-Bus");
        self.eq_tx
            .send(EqCommand::Disable)
            .await
            .map_err(|_| zbus::fdo::Error::Failed("EQ channel closed".into()))?;
        Ok(())
    }

    async fn list_eq_presets(&self) -> Vec<String> {
        EqPreset::list_available()
    }

    // --- Signals ---

    #[zbus(signal)]
    async fn device_connected(emitter: &SignalEmitter<'_>, model: &str) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn device_disconnected(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn ear_detection_changed(
        emitter: &SignalEmitter<'_>,
        left: bool,
        right: bool,
    ) -> zbus::Result<()>;
}

const OBJECT_PATH: &str = "/org/costa/AirPods";

/// Start the D-Bus service
pub async fn serve(
    state: SharedState,
    cmd_tx: SharedCmdTx,
    reconnect_tx: mpsc::Sender<()>,
    eq_tx: mpsc::Sender<EqCommand>,
) -> zbus::Result<Connection> {
    let iface = AirPodsInterface::new(state, cmd_tx, reconnect_tx, eq_tx);

    let connection = Connection::session().await?;
    connection
        .object_server()
        .at(OBJECT_PATH, iface)
        .await?;
    connection.request_name("org.costa.AirPods").await?;

    info!("D-Bus service running at org.costa.AirPods");
    Ok(connection)
}

/// Emit property change notifications on the D-Bus connection
pub async fn emit_properties_changed(connection: &Connection, changed_props: &[&str]) {
    let Ok(iface_ref) = connection
        .object_server()
        .interface::<_, AirPodsInterface>(OBJECT_PATH)
        .await
    else {
        return;
    };

    let emitter = iface_ref.signal_emitter();
    let iface = iface_ref.get().await;

    for prop in changed_props {
        let result = match *prop {
            "Connected" => iface.connected_changed(emitter).await,
            "BatteryLeft" => iface.battery_left_changed(emitter).await,
            "BatteryRight" => iface.battery_right_changed(emitter).await,
            "BatteryCase" => iface.battery_case_changed(emitter).await,
            "ChargingLeft" => iface.charging_left_changed(emitter).await,
            "ChargingRight" => iface.charging_right_changed(emitter).await,
            "ChargingCase" => iface.charging_case_changed(emitter).await,
            "AncMode" => iface.anc_mode_changed(emitter).await,
            "EarLeft" => iface.ear_left_changed(emitter).await,
            "EarRight" => iface.ear_right_changed(emitter).await,
            "ConversationalAwareness" => iface.conversational_awareness_changed(emitter).await,
            "AdaptiveNoiseLevel" => iface.adaptive_noise_level_changed(emitter).await,
            "OneBudAnc" => iface.one_bud_anc_changed(emitter).await,
            "Model" => iface.model_changed(emitter).await,
            "Firmware" => iface.firmware_changed(emitter).await,
            "EqPreset" => iface.eq_preset_changed(emitter).await,
            _ => Ok(()),
        };
        if let Err(e) = result {
            tracing::warn!("failed to emit PropertiesChanged for {prop}: {e}");
        }
    }
}

/// Emit the DeviceConnected signal
pub async fn emit_device_connected(connection: &Connection, model: &str) {
    if let Ok(iface_ref) = connection
        .object_server()
        .interface::<_, AirPodsInterface>(OBJECT_PATH)
        .await
    {
        let _ =
            AirPodsInterface::device_connected(iface_ref.signal_emitter(), model).await;
    }
}

/// Emit the DeviceDisconnected signal
pub async fn emit_device_disconnected(connection: &Connection) {
    if let Ok(iface_ref) = connection
        .object_server()
        .interface::<_, AirPodsInterface>(OBJECT_PATH)
        .await
    {
        let _ = AirPodsInterface::device_disconnected(iface_ref.signal_emitter()).await;
    }
}

/// Emit the EarDetectionChanged signal
pub async fn emit_ear_detection_changed(connection: &Connection, left: bool, right: bool) {
    if let Ok(iface_ref) = connection
        .object_server()
        .interface::<_, AirPodsInterface>(OBJECT_PATH)
        .await
    {
        let _ = AirPodsInterface::ear_detection_changed(
            iface_ref.signal_emitter(),
            left,
            right,
        )
        .await;
    }
}
