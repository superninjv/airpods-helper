//! HTTP API server on localhost:7654.
//!
//! Replaces D-Bus as the IPC mechanism on Windows. Exposes the same state and
//! control surface as the Linux daemon's D-Bus interface.
//!
//! ## Endpoints
//!
//! - `GET  /status`          — full state JSON
//! - `GET  /battery`         — battery levels + charging status
//! - `POST /anc`             — set ANC mode (body: `{"mode": "off|noise|transparency|adaptive"}`)
//! - `POST /ca`              — set CA (body: `{"enabled": true|false}`)
//! - `POST /noise`           — set adaptive noise level (body: `{"level": 0-100}`)
//! - `POST /one-bud-anc`     — set one-bud ANC (body: `{"enabled": true|false}`)

use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    routing::{get, post},
};
use serde::Deserialize;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tracing::info;

use crate::aap::AncMode;
use crate::l2cap::L2capCommand;
use crate::state::SharedState;

/// Shared command sender (swapped per L2CAP session, just like Linux daemon)
pub type SharedCmdTx = Arc<Mutex<Option<mpsc::Sender<L2capCommand>>>>;

/// Application state shared across HTTP handlers
#[derive(Clone)]
pub struct AppState {
    pub state: SharedState,
    pub cmd_tx: SharedCmdTx,
}

/// Start the HTTP server on localhost:7654
pub async fn serve(state: SharedState, cmd_tx: SharedCmdTx) -> anyhow::Result<()> {
    let app_state = AppState { state, cmd_tx };

    let app = Router::new()
        .route("/status", get(get_status))
        .route("/battery", get(get_battery))
        .route("/anc", post(post_anc))
        .route("/ca", post(post_ca))
        .route("/noise", post(post_noise))
        .route("/one-bud-anc", post(post_one_bud_anc))
        .with_state(app_state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:7654").await?;
    info!("HTTP API listening on http://127.0.0.1:7654");

    axum::serve(listener, app).await?;

    Ok(())
}

// === Handlers ===

async fn get_status(State(app): State<AppState>) -> Json<serde_json::Value> {
    let s = app.state.current();
    Json(serde_json::json!({
        "connected": s.connected,
        "model": s.model,
        "firmware": s.firmware,
        "battery_left": s.battery_left,
        "battery_right": s.battery_right,
        "battery_case": s.battery_case,
        "charging_left": s.charging_left,
        "charging_right": s.charging_right,
        "charging_case": s.charging_case,
        "anc_mode": s.anc_mode.as_str(),
        "ear_left": s.ear_left,
        "ear_right": s.ear_right,
        "conversational_awareness": s.conversational_awareness,
        "conversational_activity": s.conversational_activity,
        "adaptive_noise_level": s.adaptive_noise_level,
        "one_bud_anc": s.one_bud_anc,
    }))
}

async fn get_battery(State(app): State<AppState>) -> Json<serde_json::Value> {
    let s = app.state.current();
    Json(serde_json::json!({
        "left": s.battery_left,
        "right": s.battery_right,
        "case": s.battery_case,
        "charging_left": s.charging_left,
        "charging_right": s.charging_right,
        "charging_case": s.charging_case,
    }))
}

#[derive(Deserialize)]
struct AncRequest {
    mode: String,
}

async fn post_anc(
    State(app): State<AppState>,
    Json(req): Json<AncRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let mode = AncMode::from_str(&req.mode).ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            format!("invalid ANC mode: {} (use: off, noise, transparency, adaptive)", req.mode),
        )
    })?;

    send_command(&app.cmd_tx, L2capCommand::SetAncMode(mode)).await?;
    Ok(Json(serde_json::json!({ "anc_mode": mode.as_str() })))
}

#[derive(Deserialize)]
struct ToggleRequest {
    enabled: bool,
}

async fn post_ca(
    State(app): State<AppState>,
    Json(req): Json<ToggleRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    send_command(
        &app.cmd_tx,
        L2capCommand::SetConversationalAwareness(req.enabled),
    )
    .await?;
    Ok(Json(
        serde_json::json!({ "conversational_awareness": req.enabled }),
    ))
}

#[derive(Deserialize)]
struct NoiseLevelRequest {
    level: u8,
}

async fn post_noise(
    State(app): State<AppState>,
    Json(req): Json<NoiseLevelRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    send_command(
        &app.cmd_tx,
        L2capCommand::SetAdaptiveNoiseLevel(req.level),
    )
    .await?;
    Ok(Json(
        serde_json::json!({ "adaptive_noise_level": req.level }),
    ))
}

async fn post_one_bud_anc(
    State(app): State<AppState>,
    Json(req): Json<ToggleRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    send_command(&app.cmd_tx, L2capCommand::SetOneBudAnc(req.enabled)).await?;
    Ok(Json(serde_json::json!({ "one_bud_anc": req.enabled })))
}

/// Send a command to the active L2CAP session
async fn send_command(
    cmd_tx: &SharedCmdTx,
    cmd: L2capCommand,
) -> Result<(), (StatusCode, String)> {
    let guard = cmd_tx.lock().await;
    match guard.as_ref() {
        Some(tx) => tx.send(cmd).await.map_err(|_| {
            (
                StatusCode::SERVICE_UNAVAILABLE,
                "L2CAP session ended".to_string(),
            )
        }),
        None => Err((
            StatusCode::SERVICE_UNAVAILABLE,
            "AirPods not connected".to_string(),
        )),
    }
}
