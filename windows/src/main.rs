mod aap;
mod ble;
mod http;
mod l2cap;
mod state;

use std::sync::Arc;
use std::time::Duration;

use clap::{Parser, Subcommand};
use tokio::sync::{mpsc, Mutex};
use tracing::{error, info, warn};

use crate::aap::parser::AapEvent;
use crate::http::SharedCmdTx;
use crate::l2cap::{BtAddr, L2capCommand};
use crate::state::create_shared_state;

#[derive(Parser)]
#[command(
    name = "airpods-windows",
    about = "AirPods helper for Windows — BLE discovery + AAP protocol + HTTP API"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run the daemon (BLE scan, AAP connection, HTTP API on localhost:7654)
    Daemon,

    /// Show current status (queries the running daemon's HTTP API)
    Status {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },

    /// Show battery levels
    Battery {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },

    /// Get or set ANC mode (off, noise, transparency, adaptive)
    Anc {
        /// Mode to set (omit to show current)
        mode: Option<String>,
    },

    /// Get or set conversational awareness (on/off)
    Ca {
        /// on/off (omit to show current)
        toggle: Option<String>,
    },

    /// Get or set adaptive noise level (0-100)
    Noise {
        /// Level to set (omit to show current)
        level: Option<u8>,
    },

    /// Get or set one-bud ANC (on/off)
    OneBud {
        /// on/off (omit to show current)
        toggle: Option<String>,
    },
}

const API_BASE: &str = "http://127.0.0.1:7654";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Daemon => run_daemon().await,
        Command::Status { json } => cmd_status(json).await,
        Command::Battery { json } => cmd_battery(json).await,
        Command::Anc { mode } => cmd_anc(mode).await,
        Command::Ca { toggle } => cmd_ca(toggle).await,
        Command::Noise { level } => cmd_noise(level).await,
        Command::OneBud { toggle } => cmd_one_bud(toggle).await,
    }
}

// ============================================================================
// Daemon mode
// ============================================================================

async fn run_daemon() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("airpods_windows=info".parse().unwrap()),
        )
        .init();

    info!("airpods-windows daemon starting");

    let state = create_shared_state();
    let cmd_tx: SharedCmdTx = Arc::new(Mutex::new(None));

    // Channel for AAP events from L2CAP reader
    let (event_tx, mut event_rx) = mpsc::channel::<AapEvent>(64);

    // Start HTTP API server
    let http_state = state.clone();
    let http_cmd_tx = cmd_tx.clone();
    tokio::spawn(async move {
        if let Err(e) = http::serve(http_state, http_cmd_tx).await {
            error!("HTTP server error: {e}");
        }
    });

    info!("HTTP API started on http://127.0.0.1:7654");

    // BLE scan + L2CAP connection loop
    let adapter = ble::get_adapter().await?;
    info!("Bluetooth adapter ready");

    loop {
        // Scan for AirPods
        match ble::scan_for_airpods(&adapter, Duration::from_secs(30)).await {
            Ok(device) => {
                info!(
                    "found AirPods: {:?} at {}",
                    device.name, device.address
                );

                let bt_addr = BtAddr::from_bytes(device.address.into_inner());
                let (session_tx, session_rx) = mpsc::channel(32);
                *cmd_tx.lock().await = Some(session_tx);

                let l2cap_state = state.clone();
                let l2cap_event_tx = event_tx.clone();
                let l2cap_cmd_tx = cmd_tx.clone();

                // Run L2CAP session
                let handle = tokio::spawn(async move {
                    match l2cap::run(bt_addr, l2cap_state, session_rx, l2cap_event_tx).await {
                        Ok(()) => info!("L2CAP session ended cleanly"),
                        Err(e) => error!("L2CAP session error: {e}"),
                    }
                    *l2cap_cmd_tx.lock().await = None;
                });

                // Process AAP events while session is alive
                loop {
                    tokio::select! {
                        Some(event) = event_rx.recv() => {
                            match &event {
                                AapEvent::Disconnected => {
                                    state.reset();
                                    info!("AirPods disconnected");
                                    break;
                                }
                                AapEvent::DeviceInfo(info) => {
                                    info!("device info: {} (FW {})", info.model, info.firmware);
                                }
                                _ => {
                                    // Events are already applied to state in l2cap::apply_event
                                }
                            }
                        }
                        _ = async { handle.is_finished() }, if handle.is_finished() => {
                            info!("L2CAP task finished");
                            break;
                        }
                        _ = tokio::signal::ctrl_c() => {
                            info!("shutting down");
                            handle.abort();
                            return Ok(());
                        }
                    }
                }

                // Brief delay before re-scanning
                info!("waiting 5s before re-scanning...");
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
            Err(e) => {
                warn!("BLE scan failed: {e}");
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }
    }
}

// ============================================================================
// CLI client commands (talk to daemon via HTTP)
// ============================================================================

async fn http_get(path: &str) -> anyhow::Result<serde_json::Value> {
    let url = format!("{API_BASE}{path}");
    let resp = reqwest::get(&url).await.map_err(|e| {
        anyhow::anyhow!("failed to connect to daemon at {url} (is airpods-windows daemon running?): {e}")
    })?;
    let json: serde_json::Value = resp.json().await?;
    Ok(json)
}

async fn http_post(path: &str, body: serde_json::Value) -> anyhow::Result<serde_json::Value> {
    let url = format!("{API_BASE}{path}");
    let client = reqwest::Client::new();
    let resp = client.post(&url).json(&body).send().await.map_err(|e| {
        anyhow::anyhow!("failed to connect to daemon at {url}: {e}")
    })?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        anyhow::bail!("daemon returned {status}: {text}");
    }

    let json: serde_json::Value = resp.json().await?;
    Ok(json)
}

async fn cmd_status(json: bool) -> anyhow::Result<()> {
    let status = http_get("/status").await?;

    if json {
        println!("{}", serde_json::to_string_pretty(&status)?);
        return Ok(());
    }

    let connected = status["connected"].as_bool().unwrap_or(false);
    if !connected {
        println!("AirPods: not connected");
        return Ok(());
    }

    let model = status["model"].as_str().unwrap_or("");
    let firmware = status["firmware"].as_str().unwrap_or("");
    let anc = status["anc_mode"].as_str().unwrap_or("");
    let bl = status["battery_left"].as_i64().unwrap_or(-1);
    let br = status["battery_right"].as_i64().unwrap_or(-1);
    let bc = status["battery_case"].as_i64().unwrap_or(-1);
    let el = status["ear_left"].as_bool().unwrap_or(false);
    let er = status["ear_right"].as_bool().unwrap_or(false);
    let ca = status["conversational_awareness"].as_bool().unwrap_or(false);
    let noise = status["adaptive_noise_level"].as_u64().unwrap_or(0);
    let ob = status["one_bud_anc"].as_bool().unwrap_or(false);

    println!("{model}  (FW {firmware})");
    println!();
    print_battery("Left ", bl, status["charging_left"].as_bool().unwrap_or(false));
    print_battery("Right", br, status["charging_right"].as_bool().unwrap_or(false));
    print_battery("Case ", bc, status["charging_case"].as_bool().unwrap_or(false));
    println!();
    println!("ANC:    {anc}");
    if anc == "adaptive" {
        println!("Noise:  {noise}%");
    }
    println!("CA:     {}", if ca { "on" } else { "off" });
    println!("1-Bud:  {}", if ob { "on" } else { "off" });
    println!(
        "Ears:   L={} R={}",
        if el { "in" } else { "out" },
        if er { "in" } else { "out" }
    );

    Ok(())
}

fn print_battery(label: &str, level: i64, charging: bool) {
    if level < 0 {
        return;
    }
    let bar_len = (level as usize) / 5;
    let bar: String = "\u{2588}".repeat(bar_len);
    let empty: String = "\u{2591}".repeat(20 - bar_len);
    let charge = if charging { " \u{26A1}" } else { "" };
    println!("  {label}  {bar}{empty}  {level}%{charge}");
}

async fn cmd_battery(json: bool) -> anyhow::Result<()> {
    let battery = http_get("/battery").await?;

    if json {
        println!("{}", serde_json::to_string_pretty(&battery)?);
        return Ok(());
    }

    print_battery(
        "Left ",
        battery["left"].as_i64().unwrap_or(-1),
        battery["charging_left"].as_bool().unwrap_or(false),
    );
    print_battery(
        "Right",
        battery["right"].as_i64().unwrap_or(-1),
        battery["charging_right"].as_bool().unwrap_or(false),
    );
    print_battery(
        "Case ",
        battery["case"].as_i64().unwrap_or(-1),
        battery["charging_case"].as_bool().unwrap_or(false),
    );

    Ok(())
}

async fn cmd_anc(mode: Option<String>) -> anyhow::Result<()> {
    match mode {
        Some(m) => {
            let m = m.to_lowercase();
            match m.as_str() {
                "off" | "noise" | "transparency" | "adaptive" => {}
                _ => anyhow::bail!("invalid ANC mode: {m} (use: off, noise, transparency, adaptive)"),
            }
            let resp = http_post("/anc", serde_json::json!({ "mode": m })).await?;
            println!("ANC: {}", resp["anc_mode"].as_str().unwrap_or(&m));
        }
        None => {
            let status = http_get("/status").await?;
            println!("{}", status["anc_mode"].as_str().unwrap_or("unknown"));
        }
    }
    Ok(())
}

async fn cmd_ca(toggle: Option<String>) -> anyhow::Result<()> {
    match toggle {
        Some(t) => {
            let enabled = parse_toggle(&t)?;
            http_post("/ca", serde_json::json!({ "enabled": enabled })).await?;
            println!(
                "conversational awareness: {}",
                if enabled { "on" } else { "off" }
            );
        }
        None => {
            let status = http_get("/status").await?;
            let ca = status["conversational_awareness"].as_bool().unwrap_or(false);
            println!("{}", if ca { "on" } else { "off" });
        }
    }
    Ok(())
}

async fn cmd_noise(level: Option<u8>) -> anyhow::Result<()> {
    match level {
        Some(l) => {
            http_post("/noise", serde_json::json!({ "level": l })).await?;
            println!("adaptive noise level: {l}");
        }
        None => {
            let status = http_get("/status").await?;
            println!("{}", status["adaptive_noise_level"].as_u64().unwrap_or(0));
        }
    }
    Ok(())
}

async fn cmd_one_bud(toggle: Option<String>) -> anyhow::Result<()> {
    match toggle {
        Some(t) => {
            let enabled = parse_toggle(&t)?;
            http_post("/one-bud-anc", serde_json::json!({ "enabled": enabled })).await?;
            println!("one-bud ANC: {}", if enabled { "on" } else { "off" });
        }
        None => {
            let status = http_get("/status").await?;
            let ob = status["one_bud_anc"].as_bool().unwrap_or(false);
            println!("{}", if ob { "on" } else { "off" });
        }
    }
    Ok(())
}

fn parse_toggle(s: &str) -> anyhow::Result<bool> {
    match s.to_lowercase().as_str() {
        "on" | "true" | "1" | "yes" => Ok(true),
        "off" | "false" | "0" | "no" => Ok(false),
        _ => anyhow::bail!("invalid toggle: {s} (use: on/off)"),
    }
}
