//! Linux backend: a thin D-Bus client of `airpods-daemon` (`org.costa.AirPods`,
//! see `docs/dbus-api.md`). The daemon owns the one and only AAP session to the
//! AirPods; this module mirrors its properties into the [`Store`] and forwards
//! user actions as method calls / property writes.

use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use tokio::sync::OnceCell;
use zbus::names::InterfaceName;
use zbus::proxy::CacheProperties;
use zbus::zvariant::{DynamicDeserialize, DynamicType, Value};
use zbus::{Connection, DBusError, fdo};

use crate::status::{
    DaemonState, EqBand, EqPresetDetail, EqPresetInfo, PairedDevice, QuickPairCandidate,
    SharedStore, Status,
};

const BUS: &str = "org.costa.AirPods";
const PATH: &str = "/org/costa/AirPods";
const IFACE: &str = "org.costa.AirPods";

/// Default timeout for quick method calls.
const CALL_TIMEOUT: Duration = Duration::from_secs(10);
/// D-Bus activation can take a moment (systemd unit start, BlueZ probing).
const ACTIVATION_TIMEOUT: Duration = Duration::from_secs(15);

pub struct Client {
    store: SharedStore,
    conn: OnceCell<Connection>,
    proxy: OnceCell<zbus::Proxy<'static>>,
}

impl Client {
    pub fn new(store: SharedStore) -> Arc<Self> {
        Arc::new(Self {
            store,
            conn: OnceCell::new(),
            proxy: OnceCell::new(),
        })
    }

    /// Spawn the background task that keeps the store in sync with the daemon.
    pub fn start(self: &Arc<Self>) {
        let this = self.clone();
        tauri::async_runtime::spawn(async move { this.supervise().await });
    }

    async fn supervise(self: Arc<Self>) {
        let conn = loop {
            match Connection::session().await {
                Ok(c) => break c,
                Err(e) => {
                    self.mark_absent(format!("Cannot connect to the D-Bus session bus: {e}"));
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
            }
        };
        let _ = self.conn.set(conn.clone());

        loop {
            if let Err(e) = self.watch(&conn).await {
                tracing::warn!("D-Bus watch loop ended: {e}");
                self.mark_absent(describe(&e));
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    }

    /// Subscribe to owner and property changes, then (re)load everything.
    async fn watch(&self, conn: &Connection) -> zbus::Result<()> {
        let dbus = fdo::DBusProxy::new(conn).await?;
        let mut owner_changes = dbus
            .receive_name_owner_changed_with_args(&[(0, BUS)])
            .await?;
        let props = fdo::PropertiesProxy::builder(conn)
            .destination(BUS)?
            .path(PATH)?
            .cache_properties(CacheProperties::No)
            .build()
            .await?;
        // Subscribe before the initial load so no change can slip in between.
        let mut changes = props.receive_properties_changed().await?;

        self.activate_and_load(conn, &props).await;

        loop {
            tokio::select! {
                Some(sig) = owner_changes.next() => {
                    let args = sig.args()?;
                    if args.new_owner().is_some() {
                        tracing::info!("airpods-daemon appeared on the bus");
                        self.load(&props).await;
                    } else {
                        tracing::info!("airpods-daemon left the bus");
                        self.mark_absent("The AirPods daemon stopped.".into());
                    }
                }
                Some(sig) = changes.next() => {
                    let Ok(args) = sig.args() else { continue };
                    if args.interface_name().as_str() != IFACE {
                        continue;
                    }
                    let preset_before = self.store.current().eq_preset;
                    self.store.update(|s| {
                        for (name, value) in args.changed_properties() {
                            apply_prop(s, name, value);
                        }
                    });
                    for name in args.invalidated_properties().iter() {
                        if let Ok(v) = props.get(iface(), name).await {
                            self.store.update(|s| apply_prop(s, name, &v));
                        }
                    }
                    // A preset we don't know about (e.g. created via the CLI).
                    let s = self.store.current();
                    if s.eq_preset != preset_before
                        && !s.eq_preset.is_empty()
                        && !s.eq_presets.iter().any(|p| p.id == s.eq_preset)
                    {
                        let _ = self.refresh_presets().await;
                    }
                }
                else => return Ok(()),
            }
        }
    }

    /// Ping the daemon (which D-Bus-activates it if installed), then load.
    async fn activate_and_load(&self, conn: &Connection, props: &fdo::PropertiesProxy<'_>) {
        self.store.update(|s| {
            if s.daemon != DaemonState::Running {
                s.daemon = DaemonState::Starting;
            }
        });
        let ping = async {
            let peer = fdo::PeerProxy::builder(conn)
                .destination(BUS)?
                .path(PATH)?
                .cache_properties(CacheProperties::No)
                .build()
                .await?;
            peer.ping().await
        };
        match tokio::time::timeout(ACTIVATION_TIMEOUT, ping).await {
            Ok(Ok(())) => self.load(props).await,
            Ok(Err(e)) => {
                // Anything other than "no such service" means someone answered.
                if name_has_owner(conn).await {
                    self.load(props).await;
                } else {
                    self.mark_absent(describe(&e));
                }
            }
            Err(_) => self.mark_absent("Timed out waiting for the AirPods daemon to start.".into()),
        }
    }

    async fn load(&self, props: &fdo::PropertiesProxy<'_>) {
        match props.get_all(iface()).await {
            Ok(all) => {
                self.store.update(|s| {
                    s.clear_daemon_fields();
                    s.daemon = DaemonState::Running;
                    for (name, value) in &all {
                        apply_prop(s, name, value);
                    }
                    // v2 adds EqStatus and the settings properties.
                    s.daemon_outdated =
                        !all.contains_key("EqStatus") || !all.contains_key("AutoReconnect");
                });
                if let Err(e) = self.refresh_presets().await {
                    tracing::warn!("could not load EQ presets: {e}");
                }
            }
            Err(e) => self.mark_absent(describe_fdo(&e)),
        }
    }

    fn mark_absent(&self, reason: String) {
        self.store.update(|s| {
            s.clear_daemon_fields();
            s.daemon = DaemonState::Absent;
            s.daemon_error = reason;
        });
    }

    fn conn(&self) -> Result<&Connection, String> {
        self.conn
            .get()
            .ok_or_else(|| "Not connected to the D-Bus session bus.".to_string())
    }

    async fn proxy(&self) -> Result<&zbus::Proxy<'static>, String> {
        let conn = self.conn()?.clone();
        self.proxy
            .get_or_try_init(|| async move {
                zbus::proxy::Builder::new(&conn)
                    .destination(BUS)?
                    .path(PATH)?
                    .interface(IFACE)?
                    .cache_properties(CacheProperties::No)
                    .build()
                    .await
            })
            .await
            .map_err(|e| describe(&e))
    }

    async fn call<B, R>(
        &self,
        method: &'static str,
        body: &B,
        timeout: Duration,
    ) -> Result<R, String>
    where
        B: serde::Serialize + DynamicType,
        R: for<'d> DynamicDeserialize<'d>,
    {
        let proxy = self.proxy().await?;
        match tokio::time::timeout(timeout, proxy.call(method, body)).await {
            Ok(r) => r.map_err(|e| describe(&e)),
            Err(_) => Err(format!("{method} timed out")),
        }
    }

    /// Re-run activation/load now (e.g. user pressed "Retry").
    pub async fn retry(&self) -> Result<(), String> {
        let conn = self.conn()?.clone();
        let props = fdo::PropertiesProxy::builder(&conn)
            .destination(BUS)
            .and_then(|b| b.path(PATH))
            .map_err(|e| describe(&e))?
            .cache_properties(CacheProperties::No)
            .build()
            .await
            .map_err(|e| describe(&e))?;
        self.activate_and_load(&conn, &props).await;
        let s = self.store.current();
        if s.daemon == DaemonState::Running {
            Ok(())
        } else {
            Err(s.daemon_error)
        }
    }

    // ── Controls ──────────────────────────────────────────────────────────

    pub async fn set_anc_mode(&self, mode: String) -> Result<(), String> {
        if !matches!(mode.as_str(), "off" | "noise" | "transparency" | "adaptive") {
            return Err(format!("invalid listening mode '{mode}'"));
        }
        self.call("SetAncMode", &(mode,), CALL_TIMEOUT).await
    }

    pub async fn set_adaptive_noise_level(&self, level: u8) -> Result<(), String> {
        self.call("SetAdaptiveNoiseLevel", &(level.min(100),), CALL_TIMEOUT)
            .await
    }

    pub async fn set_conversational_awareness(&self, enabled: bool) -> Result<(), String> {
        self.call("SetConversationalAwareness", &(enabled,), CALL_TIMEOUT)
            .await
    }

    pub async fn set_one_bud_anc(&self, enabled: bool) -> Result<(), String> {
        self.call("SetOneBudAnc", &(enabled,), CALL_TIMEOUT).await
    }

    pub async fn set_volume_swipe(&self, enabled: bool) -> Result<(), String> {
        self.call("SetVolumeSwipe", &(enabled,), CALL_TIMEOUT).await
    }

    pub async fn set_mic_mode(&self, mode: String) -> Result<(), String> {
        if !matches!(mode.as_str(), "auto" | "left" | "right") {
            return Err(format!("invalid mic mode '{mode}'"));
        }
        self.call("SetMicMode", &(mode,), CALL_TIMEOUT).await
    }

    // ── Settings (read-write properties) ──────────────────────────────────

    pub async fn set_setting(&self, key: &str, value: serde_json::Value) -> Result<(), String> {
        let (prop, v): (&str, Value<'static>) = match key {
            "pause_on_removal" => ("PauseOnRemoval", Value::Bool(json_bool(&value)?)),
            "resume_on_insert" => ("ResumeOnInsert", Value::Bool(json_bool(&value)?)),
            "auto_reconnect" => ("AutoReconnect", Value::Bool(json_bool(&value)?)),
            "eq_auto_load" => ("EqAutoLoad", Value::Bool(json_bool(&value)?)),
            "mic_source" => ("MicSource", Value::Bool(json_bool(&value)?)),
            "preferred_device" => {
                let mac = value
                    .as_str()
                    .ok_or("expected a string")?
                    .trim()
                    .to_ascii_uppercase();
                if !mac.is_empty() && !is_mac(&mac) {
                    return Err(format!(
                        "'{mac}' is not a valid MAC address (AA:BB:CC:DD:EE:FF)"
                    ));
                }
                ("PreferredDevice", Value::from(mac))
            }
            other => return Err(format!("unknown setting '{other}'")),
        };
        let proxy = self.proxy().await?;
        match tokio::time::timeout(CALL_TIMEOUT, proxy.set_property(prop, v)).await {
            Ok(r) => r.map_err(|e| describe_fdo(&e)),
            Err(_) => Err(format!("setting {prop} timed out")),
        }
    }

    // ── Devices ───────────────────────────────────────────────────────────

    pub async fn list_paired(&self) -> Result<Vec<PairedDevice>, String> {
        let list: Vec<(String, String)> = self.call("ListPaired", &(), CALL_TIMEOUT).await?;
        Ok(list
            .into_iter()
            .map(|(address, name)| PairedDevice { address, name })
            .collect())
    }

    pub async fn connect(&self, address: String) -> Result<(), String> {
        let address = normalize_mac(&address)?;
        self.call("ConnectTo", &(address,), Duration::from_secs(40))
            .await
    }

    pub async fn disconnect(&self) -> Result<(), String> {
        self.call("Disconnect", &(), Duration::from_secs(20)).await
    }

    pub async fn reconnect(&self) -> Result<(), String> {
        self.call("Reconnect", &(), Duration::from_secs(40)).await
    }

    pub async fn pair(&self, address: String) -> Result<(), String> {
        let address = normalize_mac(&address)?;
        // Daemon allows ~20s for the device to show up plus pairing itself.
        self.call("Pair", &(address,), Duration::from_secs(60))
            .await
    }

    pub async fn quick_pair_scan(&self, seconds: u32) -> Result<Vec<QuickPairCandidate>, String> {
        let seconds = seconds.clamp(1, 60);
        let raw: Vec<(String, String, String, i16, bool)> = self
            .call(
                "QuickPairScan",
                &(seconds,),
                Duration::from_secs(u64::from(seconds) + 15),
            )
            .await?;
        Ok(raw
            .into_iter()
            .map(
                |(address, name, model, rssi, in_pair_mode)| QuickPairCandidate {
                    address,
                    name,
                    model,
                    rssi,
                    in_pair_mode,
                },
            )
            .collect())
    }

    // ── EQ ────────────────────────────────────────────────────────────────

    pub async fn refresh_presets(&self) -> Result<Vec<EqPresetInfo>, String> {
        let presets: Vec<EqPresetInfo> = match self
            .call::<_, Vec<(String, String, String, bool)>>("GetEqPresets", &(), CALL_TIMEOUT)
            .await
        {
            Ok(list) => list
                .into_iter()
                .map(|(id, name, description, user_editable)| EqPresetInfo {
                    id,
                    name,
                    description,
                    user_editable,
                })
                .collect(),
            // v1 daemons only have ListEqPresets (ids).
            Err(_) => self
                .call::<_, Vec<String>>("ListEqPresets", &(), CALL_TIMEOUT)
                .await?
                .into_iter()
                .map(|id| EqPresetInfo {
                    name: title_case(&id),
                    id,
                    description: String::new(),
                    user_editable: false,
                })
                .collect(),
        };
        self.store.update(|s| s.eq_presets = presets.clone());
        Ok(presets)
    }

    pub async fn get_eq_preset(&self, id: String) -> Result<EqPresetDetail, String> {
        type Raw = (String, String, f64, Vec<(String, f64, f64, f64)>);
        let proxy = self.proxy().await?;
        let reply = tokio::time::timeout(CALL_TIMEOUT, proxy.call_method("GetEqPreset", &(&id,)))
            .await
            .map_err(|_| "GetEqPreset timed out".to_string())?
            .map_err(|e| describe(&e))?;
        let body = reply.body();
        // Accept both a 4-out-arg reply (`ssda(sddd)`) and a single struct
        // out-arg (`(ssda(sddd))`) — the spec table can be read either way.
        let (name, description, preamp, bands): Raw = match body.deserialize::<Raw>() {
            Ok(r) => r,
            Err(_) => {
                body.deserialize::<(Raw,)>()
                    .map_err(|e| format!("unexpected GetEqPreset reply: {e}"))?
                    .0
            }
        };
        Ok(EqPresetDetail {
            id,
            name,
            description,
            preamp,
            bands: bands
                .into_iter()
                .map(|(kind, freq, q, gain)| EqBand {
                    kind,
                    freq,
                    q,
                    gain,
                })
                .collect(),
        })
    }

    pub async fn set_eq_preset(&self, id: String) -> Result<(), String> {
        if id.is_empty() {
            return self.call("DisableEq", &(), CALL_TIMEOUT).await;
        }
        self.call("SetEqPreset", &(id,), CALL_TIMEOUT).await
    }

    pub async fn save_eq_preset(&self, preset: EqPresetDetail) -> Result<(), String> {
        validate_preset(&preset)?;
        let bands: Vec<(String, f64, f64, f64)> = preset
            .bands
            .into_iter()
            .map(|b| (b.kind, b.freq, b.q, b.gain))
            .collect();
        self.call::<_, ()>(
            "SaveEqPreset",
            &(
                preset.id,
                preset.name,
                preset.description,
                preset.preamp,
                bands,
            ),
            CALL_TIMEOUT,
        )
        .await?;
        let _ = self.refresh_presets().await;
        Ok(())
    }

    pub async fn delete_eq_preset(&self, id: String) -> Result<(), String> {
        self.call::<_, ()>("DeleteEqPreset", &(id,), CALL_TIMEOUT)
            .await?;
        let _ = self.refresh_presets().await;
        Ok(())
    }
}

fn iface() -> InterfaceName<'static> {
    InterfaceName::from_static_str_unchecked(IFACE)
}

async fn name_has_owner(conn: &Connection) -> bool {
    let Ok(dbus) = fdo::DBusProxy::new(conn).await else {
        return false;
    };
    let Ok(name) = zbus::names::BusName::try_from(BUS) else {
        return false;
    };
    dbus.name_has_owner(name).await.unwrap_or(false)
}

// ── Property mapping ──────────────────────────────────────────────────────

fn unwrap_variant<'a, 'b>(v: &'b Value<'a>) -> &'b Value<'a> {
    match v {
        Value::Value(inner) => unwrap_variant(inner),
        other => other,
    }
}

fn v_bool(v: &Value) -> Option<bool> {
    match unwrap_variant(v) {
        Value::Bool(b) => Some(*b),
        _ => None,
    }
}

fn v_i64(v: &Value) -> Option<i64> {
    Some(match unwrap_variant(v) {
        Value::U8(n) => i64::from(*n),
        Value::I16(n) => i64::from(*n),
        Value::U16(n) => i64::from(*n),
        Value::I32(n) => i64::from(*n),
        Value::U32(n) => i64::from(*n),
        Value::I64(n) => *n,
        Value::U64(n) => i64::try_from(*n).ok()?,
        _ => return None,
    })
}

fn v_string(v: &Value) -> Option<String> {
    match unwrap_variant(v) {
        Value::Str(s) => Some(s.as_str().to_owned()),
        _ => None,
    }
}

fn v_strv(v: &Value) -> Option<Vec<String>> {
    match unwrap_variant(v) {
        Value::Array(a) => Some(a.inner().iter().filter_map(v_string).collect()),
        _ => None,
    }
}

/// Apply one daemon property to the status. Unknown names are ignored so the
/// app keeps working against newer daemons.
fn apply_prop(s: &mut Status, name: &str, v: &Value) {
    fn set<T>(slot: &mut T, v: Option<T>) {
        if let Some(v) = v {
            *slot = v;
        }
    }
    let battery = |v: &Value| v_i64(v).map(|n| n.clamp(-1, 100) as i32);
    match name {
        "Connected" => set(&mut s.connected, v_bool(v)),
        "Address" => set(&mut s.address, v_string(v)),
        "Model" => set(&mut s.model, v_string(v)),
        "ModelName" => set(&mut s.model_name, v_string(v)),
        "Firmware" => set(&mut s.firmware, v_string(v)),
        "Features" => set(&mut s.features, v_strv(v)),
        "BatteryLeft" => set(&mut s.battery_left, battery(v)),
        "BatteryRight" => set(&mut s.battery_right, battery(v)),
        "BatteryCase" => set(&mut s.battery_case, battery(v)),
        "ChargingLeft" => set(&mut s.charging_left, v_bool(v)),
        "ChargingRight" => set(&mut s.charging_right, v_bool(v)),
        "ChargingCase" => set(&mut s.charging_case, v_bool(v)),
        "EarLeft" => set(&mut s.ear_left, v_bool(v)),
        "EarRight" => set(&mut s.ear_right, v_bool(v)),
        "AncMode" => set(&mut s.anc_mode, v_string(v)),
        "AdaptiveNoiseLevel" => set(
            &mut s.adaptive_noise_level,
            v_i64(v).map(|n| n.clamp(0, 100) as u8),
        ),
        "ConversationalAwareness" => set(&mut s.conversational_awareness, v_bool(v)),
        "ConversationalActivityState" => set(&mut s.conversational_activity_state, v_string(v)),
        "OneBudAnc" => set(&mut s.one_bud_anc, v_bool(v)),
        "VolumeSwipe" => set(&mut s.volume_swipe, v_bool(v)),
        "MicMode" => set(&mut s.mic_mode, v_string(v)),
        "Version" => set(&mut s.version, v_string(v)),
        "EqPreset" => set(&mut s.eq_preset, v_string(v)),
        "EqStatus" => set(&mut s.eq_status, v_string(v)),
        "EqError" => set(&mut s.eq_error, v_string(v)),
        "EqBackend" => set(&mut s.eq_backend, v_string(v)),
        "MicStatus" => set(&mut s.mic_status, v_string(v)),
        "MicError" => set(&mut s.mic_error, v_string(v)),
        "PauseOnRemoval" => set(&mut s.pause_on_removal, v_bool(v)),
        "ResumeOnInsert" => set(&mut s.resume_on_insert, v_bool(v)),
        "AutoReconnect" => set(&mut s.auto_reconnect, v_bool(v)),
        "PreferredDevice" => set(&mut s.preferred_device, v_string(v)),
        "EqAutoLoad" => set(&mut s.eq_auto_load, v_bool(v)),
        "MicSource" => set(&mut s.mic_source, v_bool(v)),
        _ => {}
    }
}

// ── Errors & validation ───────────────────────────────────────────────────

const NOT_RUNNING: &str = "The AirPods daemon is not running.";

fn describe_named(name: &str, detail: Option<&str>) -> String {
    match name {
        "org.freedesktop.DBus.Error.ServiceUnknown"
        | "org.freedesktop.DBus.Error.NameHasNoOwner" => NOT_RUNNING.into(),
        "org.freedesktop.DBus.Error.UnknownMethod"
        | "org.freedesktop.DBus.Error.UnknownProperty" => {
            format!(
                "The running airpods-daemon does not support this yet — update it. ({})",
                detail.unwrap_or(name)
            )
        }
        "org.freedesktop.DBus.Error.NoReply" | "org.freedesktop.DBus.Error.Timeout" => {
            "The AirPods daemon did not answer in time.".into()
        }
        n if n.starts_with("org.freedesktop.DBus.Error.Spawn") => format!(
            "The AirPods daemon failed to start: {}",
            detail.unwrap_or(name)
        ),
        _ => detail
            .filter(|d| !d.is_empty())
            .map(capitalize)
            .unwrap_or_else(|| name.to_string()),
    }
}

fn describe(e: &zbus::Error) -> String {
    match e {
        zbus::Error::MethodError(name, detail, _) => {
            describe_named(name.as_str(), detail.as_deref())
        }
        zbus::Error::FDO(fe) => describe_fdo(fe),
        other => other.to_string(),
    }
}

fn describe_fdo(e: &fdo::Error) -> String {
    match e {
        fdo::Error::ZBus(z) => describe(z),
        other => describe_named(other.name().as_str(), other.description()),
    }
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}

fn title_case(id: &str) -> String {
    id.split(['-', '_'])
        .filter(|w| !w.is_empty())
        .map(capitalize)
        .collect::<Vec<_>>()
        .join(" ")
}

fn json_bool(v: &serde_json::Value) -> Result<bool, String> {
    v.as_bool().ok_or_else(|| "expected true/false".to_string())
}

fn is_mac(s: &str) -> bool {
    let parts: Vec<&str> = s.split(':').collect();
    parts.len() == 6
        && parts
            .iter()
            .all(|p| p.len() == 2 && p.chars().all(|c| c.is_ascii_hexdigit()))
}

fn normalize_mac(s: &str) -> Result<String, String> {
    let mac = s.trim().to_ascii_uppercase();
    if is_mac(&mac) {
        Ok(mac)
    } else {
        Err(format!(
            "'{}' is not a valid MAC address (AA:BB:CC:DD:EE:FF)",
            s.trim()
        ))
    }
}

/// Mirror of the daemon's limits so the user gets an immediate, specific error.
fn validate_preset(p: &EqPresetDetail) -> Result<(), String> {
    let id_ok = !p.id.is_empty()
        && p.id.len() <= 48
        && p.id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if !id_ok {
        return Err("Preset id must be 1–48 characters of a–z, 0–9 or '-'.".into());
    }
    if p.name.trim().is_empty() {
        return Err("Give the preset a name.".into());
    }
    if p.bands.len() > 16 {
        return Err("At most 16 bands are supported.".into());
    }
    if !(-24.0..=12.0).contains(&p.preamp) {
        return Err("Preamp must be between −24 and +12 dB.".into());
    }
    for (i, b) in p.bands.iter().enumerate() {
        let n = i + 1;
        if !matches!(
            b.kind.as_str(),
            "peaking" | "lowshelf" | "highshelf" | "lowpass" | "highpass" | "notch"
        ) {
            return Err(format!("Band {n}: unknown filter type '{}'.", b.kind));
        }
        if !(20.0..=20000.0).contains(&b.freq) {
            return Err(format!("Band {n}: frequency must be 20–20000 Hz."));
        }
        if !(0.1..=10.0).contains(&b.q) {
            return Err(format!("Band {n}: Q must be 0.1–10."));
        }
        if !(-24.0..=24.0).contains(&b.gain) {
            return Err(format!("Band {n}: gain must be −24 to +24 dB."));
        }
    }
    Ok(())
}
