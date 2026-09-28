mod aap;
mod bluez;
mod config;
mod dbus;
mod eq;
mod l2cap;
mod models;
mod mpris;
mod state;

use bluer::Address;
use std::sync::Arc;
use std::time::Duration;
use tokio::signal::unix::{SignalKind, signal};
use tokio::sync::{Mutex, mpsc};
use tokio::task::JoinHandle;
use tracing::{error, info, warn};

use crate::aap::parser::AapEvent;
use crate::bluez::BlueZEvent;
use crate::config::SharedConfig;
use crate::dbus::{Control, SharedCmdTx};
use crate::eq::{EqManager, EqPreset};
use crate::state::{SharedState, create_shared_state};

/// Messages from an L2CAP session task, tagged with its session id so late
/// messages from a superseded session are ignored.
enum SessionMsg {
    Event(u64, AapEvent),
    Ended(u64),
}

struct ActiveSession {
    id: u64,
    address: Address,
    handle: JoinHandle<()>,
}

struct Daemon {
    state: SharedState,
    config: SharedConfig,
    cmd_tx: SharedCmdTx,
    session_tx: mpsc::Sender<SessionMsg>,
    eq: EqManager,
    session: Option<ActiveSession>,
    next_session_id: u64,
    reconnect: Option<JoinHandle<()>>,
    last_address: Option<Address>,
    /// Set when the user asked to disconnect, so we don't fight them.
    user_disconnected: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "airpods_daemon=info".into()),
        )
        .init();

    info!("airpods-daemon {} starting", env!("CARGO_PKG_VERSION"));

    let config = config::shared(config::Config::load());
    let state = create_shared_state();
    let cmd_tx: SharedCmdTx = Arc::new(Mutex::new(None));
    let (control_tx, mut control_rx) = mpsc::channel::<Control>(16);
    let (bluez_tx, mut bluez_rx) = mpsc::channel::<BlueZEvent>(16);
    let (session_tx, mut session_rx) = mpsc::channel::<SessionMsg>(64);

    let connection = dbus::serve(state.clone(), config.clone(), cmd_tx.clone(), control_tx).await?;
    tokio::spawn(dbus::run_property_notifier(
        connection.clone(),
        state.subscribe(),
    ));
    tokio::spawn(mpris::watch_ear_detection(
        state.subscribe(),
        config.clone(),
    ));

    let backend_choice = config::read(&config, |c| c.eq.backend);
    let mut daemon = Daemon {
        eq: EqManager::new(state.clone(), backend_choice).await,
        state,
        config,
        cmd_tx,
        session_tx,
        session: None,
        next_session_id: 0,
        reconnect: None,
        last_address: None,
        user_disconnected: false,
    };
    daemon.restore_eq_selection().await;

    // BlueZ monitor; restarts if BlueZ or the adapter goes away.
    tokio::spawn(async move {
        loop {
            match bluez::monitor(bluez_tx.clone()).await {
                Ok(()) => info!("BlueZ monitor ended, restarting in 5s"),
                Err(e) => error!("BlueZ monitor error: {e}, restarting in 5s"),
            }
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    });

    let mut sigterm = signal(SignalKind::terminate())?;
    let mut sigint = signal(SignalKind::interrupt())?;
    info!("waiting for AirPods connection...");

    loop {
        tokio::select! {
            Some(event) = bluez_rx.recv() => match event {
                BlueZEvent::AirPodsConnected(addr) => daemon.on_bt_connected(addr).await,
                BlueZEvent::AirPodsDisconnected(addr) => daemon.on_bt_disconnected(addr).await,
            },
            Some(msg) = session_rx.recv() => match msg {
                SessionMsg::Event(id, event) if daemon.is_current(id) => daemon.on_aap_event(event).await,
                SessionMsg::Ended(id) if daemon.is_current(id) => daemon.on_session_ended().await,
                _ => {}
            },
            Some(ctl) = control_rx.recv() => daemon.on_control(ctl).await,
            _ = sigterm.recv() => break,
            _ = sigint.recv() => break,
        }
    }

    info!("shutting down");
    daemon.eq.shutdown().await;
    if let Some(s) = daemon.session.take() {
        s.handle.abort();
    }
    if let Some(r) = daemon.reconnect.take() {
        r.abort();
    }
    Ok(())
}

impl Daemon {
    fn is_current(&self, id: u64) -> bool {
        self.session.as_ref().is_some_and(|s| s.id == id)
    }

    /// Load the persisted EQ selection into the manager (applied on connect).
    async fn restore_eq_selection(&mut self) {
        let id = config::read(&self.config, |c| c.eq.active_preset.clone());
        let preset = if id.is_empty() {
            None
        } else {
            match EqPreset::load(&id) {
                Ok((p, _)) => Some(p),
                Err(e) => {
                    warn!("configured EQ preset unavailable: {e}");
                    None
                }
            }
        };
        let id = preset.as_ref().map(|p| p.id.clone()).unwrap_or_default();
        self.state.update(|s| s.eq_preset = id);
        self.eq.select(preset).await;
    }

    async fn on_bt_connected(&mut self, addr: Address) {
        if let Some(pinned) = config::read(&self.config, |c| c.preferred_device())
            && pinned != addr
        {
            info!("ignoring AirPods {addr}: preferred device is {pinned}");
            return;
        }
        if let Some(s) = &self.session {
            if s.address == addr {
                return; // duplicate notification (e.g. monitor restart)
            }
            info!(
                "ignoring AirPods {addr}: already connected to {}",
                s.address
            );
            return;
        }
        if let Some(r) = self.reconnect.take() {
            r.abort();
        }
        self.user_disconnected = false;
        self.last_address = Some(addr);
        self.start_session(addr).await;
    }

    async fn start_session(&mut self, addr: Address) {
        info!("AirPods {addr} connected, starting AAP session");
        self.next_session_id += 1;
        let id = self.next_session_id;
        let (tx, rx) = mpsc::channel(32);
        *self.cmd_tx.lock().await = Some(tx);

        let state = self.state.clone();
        let session_tx = self.session_tx.clone();
        let handle = tokio::spawn(async move {
            let (event_tx, mut event_rx) = mpsc::channel::<AapEvent>(64);
            let forward_tx = session_tx.clone();
            let forward = tokio::spawn(async move {
                while let Some(ev) = event_rx.recv().await {
                    if forward_tx.send(SessionMsg::Event(id, ev)).await.is_err() {
                        break;
                    }
                }
            });
            match l2cap::run(addr, state, rx, event_tx).await {
                Ok(()) => info!("AAP session ended"),
                Err(e) => error!("AAP session error: {e}"),
            }
            let _ = forward.await;
            let _ = session_tx.send(SessionMsg::Ended(id)).await;
        });
        self.session = Some(ActiveSession {
            id,
            address: addr,
            handle,
        });
    }

    /// Tear down the current session and publish the disconnected state.
    async fn end_session(&mut self) {
        if let Some(s) = self.session.take() {
            s.handle.abort();
        }
        *self.cmd_tx.lock().await = None;
        self.state.reset();
        self.eq.set_device(None).await;
    }

    async fn on_bt_disconnected(&mut self, addr: Address) {
        if self.session.as_ref().is_none_or(|s| s.address != addr) {
            return;
        }
        info!("AirPods {addr} disconnected");
        self.end_session().await;
        self.maybe_reconnect(addr);
    }

    /// The AAP channel closed but Bluetooth may still be up (e.g. the AirPods
    /// rebooted the channel after a firmware hiccup). Retry the session if so.
    async fn on_session_ended(&mut self) {
        let Some(addr) = self.session.as_ref().map(|s| s.address) else {
            return;
        };
        self.end_session().await;
        let still_connected =
            matches!(bluez::currently_connected_airpods().await, Ok(Some(a)) if a == addr);
        if still_connected && !self.user_disconnected {
            info!("Bluetooth still connected; restarting AAP session in 3s");
            tokio::time::sleep(Duration::from_secs(3)).await;
            if self.session.is_none() {
                self.start_session(addr).await;
            }
        }
    }

    fn maybe_reconnect(&mut self, addr: Address) {
        let (enabled, retries) = config::read(&self.config, |c| {
            (c.reconnect.auto_reconnect, c.reconnect.max_retries)
        });
        if !enabled || self.user_disconnected || retries == 0 {
            return;
        }
        if let Some(r) = self.reconnect.take() {
            r.abort();
        }
        self.reconnect = Some(tokio::spawn(reconnect_with_backoff(addr, retries)));
    }

    async fn on_aap_event(&mut self, event: AapEvent) {
        if let AapEvent::DeviceInfo(_) = event {
            // Model is known now; start EQ for this device if configured.
            let auto_load = config::read(&self.config, |c| c.eq.auto_load);
            let addr = self.session.as_ref().map(|s| s.address);
            if auto_load {
                self.eq.set_device(addr).await;
            }
        }
    }

    async fn on_control(&mut self, ctl: Control) {
        match ctl {
            Control::Reconnect => {
                let Some(addr) = self.last_address else {
                    warn!("reconnect requested but no known device address");
                    return;
                };
                self.user_disconnected = false;
                if let Some(r) = self.reconnect.take() {
                    r.abort();
                }
                let retries = config::read(&self.config, |c| c.reconnect.max_retries).max(1);
                self.reconnect = Some(tokio::spawn(reconnect_with_backoff(addr, retries)));
            }
            Control::UserDisconnected => {
                self.user_disconnected = true;
                if let Some(r) = self.reconnect.take() {
                    r.abort();
                }
            }
            Control::EqSelect(id) => self.select_eq(id).await,
            Control::EqPresetChanged(id) => {
                if self.eq.preset_id() == Some(id.as_str()) {
                    // Re-read from disk (or fall back to the built-in, or
                    // disable if the preset no longer exists at all).
                    let still_exists = EqPreset::load(&id).is_ok();
                    self.select_eq(still_exists.then_some(id)).await;
                }
            }
        }
    }

    async fn select_eq(&mut self, id: Option<String>) {
        let preset = match id.as_deref().map(EqPreset::load) {
            None => None,
            Some(Ok((p, _))) => Some(p),
            Some(Err(e)) => {
                warn!("can't select EQ preset: {e}");
                return;
            }
        };
        let id = preset.as_ref().map(|p| p.id.clone()).unwrap_or_default();
        if let Err(e) = config::update_config(&self.config, |c| c.eq.active_preset = id.clone()) {
            warn!("failed to persist EQ selection: {e}");
        }
        self.state.update(|s| s.eq_preset = id);
        // Selecting a preset while connected applies it even if auto-load is off.
        let device = self
            .session
            .as_ref()
            .filter(|_| self.state.current().connected)
            .map(|s| s.address);
        self.eq.select(preset).await;
        self.eq.set_device(device).await;
    }
}

/// Attempt to reconnect to AirPods with exponential backoff
async fn reconnect_with_backoff(address: Address, max_retries: u32) {
    let mut delay = Duration::from_secs(2);
    for attempt in 1..=max_retries {
        tokio::time::sleep(delay).await;
        info!("reconnect attempt {attempt}/{max_retries} to {address}");
        match bluez::connect_device(address).await {
            Ok(()) => {
                info!("reconnect succeeded on attempt {attempt}");
                return;
            }
            Err(e) => {
                warn!("reconnect attempt {attempt} failed: {e}");
                delay = (delay * 2).min(Duration::from_secs(60));
            }
        }
    }
    info!("giving up reconnecting to {address} after {max_retries} attempts");
}
