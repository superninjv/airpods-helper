use clap::{Parser, Subcommand};
use zbus::Connection;

#[zbus::proxy(
    interface = "org.costa.AirPods",
    default_service = "org.costa.AirPods",
    default_path = "/org/costa/AirPods"
)]
trait AirPods {
    #[zbus(property)]
    fn connected(&self) -> zbus::Result<bool>;

    #[zbus(property)]
    fn battery_left(&self) -> zbus::Result<i32>;

    #[zbus(property)]
    fn battery_right(&self) -> zbus::Result<i32>;

    #[zbus(property)]
    fn battery_case(&self) -> zbus::Result<i32>;

    #[zbus(property)]
    fn charging_left(&self) -> zbus::Result<bool>;

    #[zbus(property)]
    fn charging_right(&self) -> zbus::Result<bool>;

    #[zbus(property)]
    fn charging_case(&self) -> zbus::Result<bool>;

    #[zbus(property)]
    fn anc_mode(&self) -> zbus::Result<String>;

    #[zbus(property)]
    fn ear_left(&self) -> zbus::Result<bool>;

    #[zbus(property)]
    fn ear_right(&self) -> zbus::Result<bool>;

    #[zbus(property)]
    fn conversational_awareness(&self) -> zbus::Result<bool>;

    #[zbus(property)]
    fn adaptive_noise_level(&self) -> zbus::Result<u8>;

    #[zbus(property)]
    fn one_bud_anc(&self) -> zbus::Result<bool>;

    #[zbus(property)]
    fn model(&self) -> zbus::Result<String>;

    #[zbus(property)]
    fn model_name(&self) -> zbus::Result<String>;

    #[zbus(property)]
    fn firmware(&self) -> zbus::Result<String>;

    #[zbus(property)]
    fn volume_swipe(&self) -> zbus::Result<bool>;

    #[zbus(property)]
    fn adaptive_volume(&self) -> zbus::Result<bool>;

    #[zbus(property)]
    fn chime_volume(&self) -> zbus::Result<u8>;

    #[zbus(property)]
    fn audio_source(&self) -> zbus::Result<String>;

    #[zbus(property)]
    fn eq_preset(&self) -> zbus::Result<String>;

    #[zbus(property)]
    fn conversational_activity_state(&self) -> zbus::Result<String>;

    #[zbus(property)]
    fn features(&self) -> zbus::Result<Vec<String>>;

    fn set_anc_mode(&self, mode: &str) -> zbus::Result<()>;
    fn set_conversational_awareness(&self, enabled: bool) -> zbus::Result<()>;
    fn set_adaptive_noise_level(&self, level: u8) -> zbus::Result<()>;
    fn set_one_bud_anc(&self, enabled: bool) -> zbus::Result<()>;
    fn set_mic_mode(&self, mode: &str) -> zbus::Result<()>;
    fn set_eq_preset(&self, name: &str) -> zbus::Result<()>;
    fn disable_eq(&self) -> zbus::Result<()>;
    fn list_eq_presets(&self) -> zbus::Result<Vec<String>>;
    fn reconnect(&self) -> zbus::Result<()>;
    fn connect_to(&self, address: &str) -> zbus::Result<()>;
    fn disconnect(&self) -> zbus::Result<()>;
    fn list_paired(&self) -> zbus::Result<Vec<(String, String)>>;
    fn pair(&self, address: &str) -> zbus::Result<()>;
    fn quick_pair_scan(
        &self,
        duration_secs: u32,
    ) -> zbus::Result<Vec<(String, String, String, i16, bool)>>;
}

#[derive(Parser)]
#[command(name = "airpods-cli", about = "Control AirPods from the terminal")]
struct Cli {
    /// Output as JSON
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Show full device status
    Status,
    /// Show battery levels
    Battery,
    /// Get or set ANC mode (off, noise, transparency, adaptive)
    Anc {
        /// Mode to set (omit to show current)
        mode: Option<String>,
    },
    /// Get or set conversational awareness
    Ca {
        /// on/off (omit to show current)
        toggle: Option<String>,
    },
    /// Get or set adaptive noise level (0-100)
    Noise {
        /// Level to set (omit to show current)
        level: Option<u8>,
    },
    /// Get or set one-bud ANC
    OneBud {
        /// on/off (omit to show current)
        toggle: Option<String>,
    },
    /// Set primary microphone bud (auto, right, left)
    Mic {
        /// Mode: auto, right, or left
        mode: String,
    },
    /// Control EQ presets
    Eq {
        /// Preset name, "list", or "off" (omit to show current)
        action: Option<String>,
    },
    /// Trigger device reconnect (uses last-connected device)
    Reconnect,
    /// Connect to a paired AirPods by MAC address
    Connect {
        /// MAC address (e.g. AA:BB:CC:DD:EE:FF)
        address: String,
    },
    /// Pair a new AirPods (open case, status light flashing white)
    Pair {
        /// MAC address (e.g. AA:BB:CC:DD:EE:FF)
        address: String,
    },
    /// Quick-pair scan — look for nearby unpaired AirPods (case open) over LE
    Scan {
        /// Scan duration in seconds (default 10)
        #[arg(long, default_value_t = 10)]
        duration: u32,
    },
    /// Disconnect the currently-connected AirPods
    Disconnect,
    /// List paired AirPods known to BlueZ
    Paired,
    /// Diagnose installation health (binary, caps, BlueZ, PipeWire, daemon, D-Bus)
    Doctor,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Doctor runs without requiring a working daemon — it diagnoses why one isn't.
    if matches!(cli.command, Command::Doctor) {
        return cmd_doctor(cli.json).await;
    }

    let conn = Connection::session().await.map_err(|e| {
        anyhow::anyhow!("failed to connect to session bus: {e}")
    })?;

    let proxy = AirPodsProxy::new(&conn).await.map_err(|e| {
        anyhow::anyhow!("failed to create D-Bus proxy (is airpods-daemon running?): {e}")
    })?;

    match cli.command {
        Command::Status => cmd_status(&proxy, cli.json).await?,
        Command::Battery => cmd_battery(&proxy, cli.json).await?,
        Command::Anc { mode } => cmd_anc(&proxy, mode, cli.json).await?,
        Command::Ca { toggle } => cmd_ca(&proxy, toggle, cli.json).await?,
        Command::Noise { level } => cmd_noise(&proxy, level, cli.json).await?,
        Command::OneBud { toggle } => cmd_one_bud(&proxy, toggle, cli.json).await?,
        Command::Mic { mode } => {
            let m = mode.to_lowercase();
            match m.as_str() {
                "auto" | "automatic" | "right" | "left" => {
                    proxy.set_mic_mode(&m).await?;
                    println!("mic: {m}");
                }
                _ => anyhow::bail!("invalid mic mode: {m} (use: auto, right, left)"),
            }
        }
        Command::Eq { action } => cmd_eq(&proxy, action, cli.json).await?,
        Command::Reconnect => {
            proxy.reconnect().await?;
            println!("reconnect requested");
        }
        Command::Connect { address } => {
            proxy.connect_to(&address).await?;
            println!("connect requested: {address}");
        }
        Command::Pair { address } => {
            println!("pairing {address}... (open the AirPods case if you haven't)");
            proxy.pair(&address).await?;
            println!("paired and trusted: {address}");
        }
        Command::Scan { duration } => {
            if !cli.json {
                println!("scanning for nearby AirPods for {duration}s... (open a case to make AirPods discoverable)");
            }
            let candidates = proxy.quick_pair_scan(duration).await?;
            if cli.json {
                let arr: Vec<_> = candidates
                    .iter()
                    .map(|(a, n, m, r, p)| {
                        serde_json::json!({
                            "address": a, "name": n, "model": m,
                            "rssi": r, "in_pair_mode": p,
                        })
                    })
                    .collect();
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({ "candidates": arr }))?
                );
            } else if candidates.is_empty() {
                println!("no AirPods candidates found");
            } else {
                println!("\nfound {} candidate(s):\n", candidates.len());
                for (addr, name, model, rssi, in_pair_mode) in candidates {
                    let mark = if in_pair_mode { "★" } else { " " };
                    println!("  {mark} {addr}  {model:32}  RSSI {rssi:>4}  {name}");
                }
                println!("\n★ = looks like it's in pairing mode");
                println!("  pair with: airpods-cli pair <MAC>");
            }
        }
        Command::Disconnect => {
            proxy.disconnect().await?;
            println!("disconnect requested");
        }
        Command::Paired => {
            let devices = proxy.list_paired().await?;
            if cli.json {
                let arr: Vec<_> = devices
                    .iter()
                    .map(|(a, n)| serde_json::json!({ "address": a, "name": n }))
                    .collect();
                println!("{}", serde_json::to_string_pretty(&serde_json::json!({ "paired": arr }))?);
            } else if devices.is_empty() {
                println!("no paired AirPods");
            } else {
                for (addr, name) in devices {
                    println!("{addr}  {name}");
                }
            }
        }
        Command::Doctor => unreachable!("handled before proxy creation"),
    }

    Ok(())
}

async fn cmd_status(proxy: &AirPodsProxy<'_>, json: bool) -> anyhow::Result<()> {
    let connected = proxy.connected().await?;

    let features = proxy.features().await.unwrap_or_default();

    if json {
        let obj = serde_json::json!({
            "connected": connected,
            "model": proxy.model().await.unwrap_or_default(),
            "model_name": proxy.model_name().await.unwrap_or_default(),
            "firmware": proxy.firmware().await.unwrap_or_default(),
            "features": features,
            "battery_left": proxy.battery_left().await.unwrap_or(-1),
            "battery_right": proxy.battery_right().await.unwrap_or(-1),
            "battery_case": proxy.battery_case().await.unwrap_or(-1),
            "charging_left": proxy.charging_left().await.unwrap_or(false),
            "charging_right": proxy.charging_right().await.unwrap_or(false),
            "charging_case": proxy.charging_case().await.unwrap_or(false),
            "anc_mode": proxy.anc_mode().await.unwrap_or_default(),
            "ear_left": proxy.ear_left().await.unwrap_or(false),
            "ear_right": proxy.ear_right().await.unwrap_or(false),
            "conversational_awareness": proxy.conversational_awareness().await.unwrap_or(false),
            "conversational_activity": proxy.conversational_activity_state().await.unwrap_or_default(),
            "adaptive_noise_level": proxy.adaptive_noise_level().await.unwrap_or(0),
            "one_bud_anc": proxy.one_bud_anc().await.unwrap_or(false),
            "volume_swipe": proxy.volume_swipe().await.unwrap_or(false),
            "adaptive_volume": proxy.adaptive_volume().await.unwrap_or(false),
            "chime_volume": proxy.chime_volume().await.unwrap_or(0),
            "audio_source": proxy.audio_source().await.unwrap_or_default(),
            "eq_preset": proxy.eq_preset().await.unwrap_or_default(),
        });
        println!("{}", serde_json::to_string_pretty(&obj)?);
        return Ok(());
    }

    if !connected {
        println!("AirPods: not connected");
        return Ok(());
    }

    let has = |f: &str| features.iter().any(|s| s == f);

    let model = proxy.model().await.unwrap_or_default();
    let model_name = proxy.model_name().await.unwrap_or_default();
    let firmware = proxy.firmware().await.unwrap_or_default();
    let bl = proxy.battery_left().await.unwrap_or(-1);
    let br = proxy.battery_right().await.unwrap_or(-1);
    let bc = proxy.battery_case().await.unwrap_or(-1);
    let el = proxy.ear_left().await.unwrap_or(false);
    let er = proxy.ear_right().await.unwrap_or(false);
    let eq = proxy.eq_preset().await.unwrap_or_default();

    let display = if model_name.is_empty() { &model } else { &model_name };
    println!("{display}  ({model})  FW {firmware}");
    println!();
    print_battery("Left ", bl, proxy.charging_left().await.unwrap_or(false));
    print_battery("Right", br, proxy.charging_right().await.unwrap_or(false));
    print_battery("Case ", bc, proxy.charging_case().await.unwrap_or(false));
    println!();

    if has("anc") {
        let anc = proxy.anc_mode().await.unwrap_or_default();
        println!("ANC:    {anc}");
        if has("adaptive") && anc == "adaptive" {
            let noise = proxy.adaptive_noise_level().await.unwrap_or(0);
            println!("Noise:  {noise}%");
        }
    }
    if has("ca") {
        let ca = proxy.conversational_awareness().await.unwrap_or(false);
        println!("CA:     {}", if ca { "on" } else { "off" });
    }
    if has("one_bud_anc") {
        let ob = proxy.one_bud_anc().await.unwrap_or(false);
        println!("1-Bud:  {}", if ob { "on" } else { "off" });
    }

    println!("Ears:   L={} R={}", if el { "in" } else { "out" }, if er { "in" } else { "out" });
    if !eq.is_empty() {
        println!("EQ:     {eq}");
    }

    Ok(())
}

fn print_battery(label: &str, level: i32, charging: bool) {
    if level < 0 {
        return;
    }
    let bar_len = (level as usize) / 5;
    let bar: String = "█".repeat(bar_len);
    let empty: String = "░".repeat(20 - bar_len);
    let charge = if charging { " ⚡" } else { "" };
    println!("  {label}  {bar}{empty}  {level}%{charge}");
}

async fn cmd_battery(proxy: &AirPodsProxy<'_>, json: bool) -> anyhow::Result<()> {
    let bl = proxy.battery_left().await.unwrap_or(-1);
    let br = proxy.battery_right().await.unwrap_or(-1);
    let bc = proxy.battery_case().await.unwrap_or(-1);

    if json {
        let obj = serde_json::json!({
            "left": bl, "right": br, "case": bc,
            "charging_left": proxy.charging_left().await.unwrap_or(false),
            "charging_right": proxy.charging_right().await.unwrap_or(false),
            "charging_case": proxy.charging_case().await.unwrap_or(false),
        });
        println!("{}", serde_json::to_string_pretty(&obj)?);
        return Ok(());
    }

    print_battery("Left ", bl, proxy.charging_left().await.unwrap_or(false));
    print_battery("Right", br, proxy.charging_right().await.unwrap_or(false));
    print_battery("Case ", bc, proxy.charging_case().await.unwrap_or(false));
    Ok(())
}

async fn cmd_anc(proxy: &AirPodsProxy<'_>, mode: Option<String>, json: bool) -> anyhow::Result<()> {
    match mode {
        Some(m) => {
            let m = m.to_lowercase();
            match m.as_str() {
                "off" | "noise" | "transparency" | "adaptive" => {
                    proxy.set_anc_mode(&m).await?;
                    println!("ANC: {m}");
                }
                _ => anyhow::bail!("invalid ANC mode: {m} (use: off, noise, transparency, adaptive)"),
            }
        }
        None => {
            let mode = proxy.anc_mode().await?;
            if json {
                println!("{}", serde_json::json!({ "anc_mode": mode }));
            } else {
                println!("{mode}");
            }
        }
    }
    Ok(())
}

async fn cmd_ca(proxy: &AirPodsProxy<'_>, toggle: Option<String>, json: bool) -> anyhow::Result<()> {
    match toggle {
        Some(t) => {
            let enabled = parse_toggle(&t)?;
            proxy.set_conversational_awareness(enabled).await?;
            println!("conversational awareness: {}", if enabled { "on" } else { "off" });
        }
        None => {
            let ca = proxy.conversational_awareness().await?;
            if json {
                println!("{}", serde_json::json!({ "conversational_awareness": ca }));
            } else {
                println!("{}", if ca { "on" } else { "off" });
            }
        }
    }
    Ok(())
}

async fn cmd_noise(proxy: &AirPodsProxy<'_>, level: Option<u8>, json: bool) -> anyhow::Result<()> {
    match level {
        Some(l) => {
            proxy.set_adaptive_noise_level(l).await?;
            println!("adaptive noise level: {l}");
        }
        None => {
            let level = proxy.adaptive_noise_level().await?;
            if json {
                println!("{}", serde_json::json!({ "adaptive_noise_level": level }));
            } else {
                println!("{level}");
            }
        }
    }
    Ok(())
}

async fn cmd_one_bud(proxy: &AirPodsProxy<'_>, toggle: Option<String>, json: bool) -> anyhow::Result<()> {
    match toggle {
        Some(t) => {
            let enabled = parse_toggle(&t)?;
            proxy.set_one_bud_anc(enabled).await?;
            println!("one-bud ANC: {}", if enabled { "on" } else { "off" });
        }
        None => {
            let ob = proxy.one_bud_anc().await?;
            if json {
                println!("{}", serde_json::json!({ "one_bud_anc": ob }));
            } else {
                println!("{}", if ob { "on" } else { "off" });
            }
        }
    }
    Ok(())
}

async fn cmd_eq(proxy: &AirPodsProxy<'_>, action: Option<String>, json: bool) -> anyhow::Result<()> {
    match action.as_deref() {
        Some("list") => {
            let presets = proxy.list_eq_presets().await?;
            if json {
                println!("{}", serde_json::json!({ "presets": presets }));
            } else {
                for p in presets {
                    println!("{p}");
                }
            }
        }
        Some("off") => {
            proxy.disable_eq().await?;
            println!("EQ disabled");
        }
        Some(name) => {
            proxy.set_eq_preset(name).await?;
            println!("EQ: {name}");
        }
        None => {
            let preset = proxy.eq_preset().await?;
            if json {
                println!("{}", serde_json::json!({ "eq_preset": preset }));
            } else if preset.is_empty() {
                println!("off");
            } else {
                println!("{preset}");
            }
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

struct Check {
    name: &'static str,
    ok: bool,
    detail: String,
    fix: Option<String>,
}

async fn cmd_doctor(json: bool) -> anyhow::Result<()> {
    use std::process::Command as SysCommand;

    let mut checks: Vec<Check> = Vec::new();

    // 1. Daemon binary on PATH or in well-known locations
    let candidates = [
        std::env::var("HOME").map(|h| format!("{h}/.local/bin/airpods-daemon")).ok(),
        Some("/usr/local/bin/airpods-daemon".to_string()),
        Some("/usr/bin/airpods-daemon".to_string()),
    ];
    let mut daemon_path: Option<String> = None;
    for c in candidates.into_iter().flatten() {
        if std::path::Path::new(&c).exists() {
            daemon_path = Some(c);
            break;
        }
    }
    let path_for_msg = daemon_path.clone();
    match &path_for_msg {
        Some(p) => checks.push(Check {
            name: "airpods-daemon binary",
            ok: true,
            detail: format!("found at {p}"),
            fix: None,
        }),
        None => checks.push(Check {
            name: "airpods-daemon binary",
            ok: false,
            detail: "not found in ~/.local/bin, /usr/local/bin, or /usr/bin".into(),
            fix: Some("Install: `make install`, the .deb, or PKGBUILD".into()),
        }),
    }

    // 2. Capabilities on the binary
    if let Some(p) = &daemon_path {
        let out = SysCommand::new("getcap").arg(p).output();
        match out {
            Ok(o) if o.status.success() => {
                let text = String::from_utf8_lossy(&o.stdout);
                let has_caps = text.contains("cap_net_raw") && text.contains("cap_net_admin");
                checks.push(Check {
                    name: "L2CAP raw socket capability",
                    ok: has_caps,
                    detail: if has_caps {
                        "cap_net_raw + cap_net_admin set".into()
                    } else {
                        format!("missing — getcap reports: {}", text.trim())
                    },
                    fix: if has_caps {
                        None
                    } else {
                        Some(format!("sudo setcap 'cap_net_raw,cap_net_admin+eip' {p}"))
                    },
                });
            }
            _ => checks.push(Check {
                name: "L2CAP raw socket capability",
                ok: false,
                detail: "`getcap` not available — can't verify".into(),
                fix: Some("Install libcap (Arch: `pacman -S libcap`, Debian: `apt install libcap2-bin`)".into()),
            }),
        }
    }

    // 3. BlueZ available on system bus
    let bluez_ok = match zbus::Connection::system().await {
        Ok(c) => zbus::Proxy::new(
            &c,
            "org.bluez",
            "/org/bluez",
            "org.freedesktop.DBus.Peer",
        )
        .await
        .is_ok(),
        Err(_) => false,
    };
    checks.push(Check {
        name: "BlueZ system service",
        ok: bluez_ok,
        detail: if bluez_ok {
            "org.bluez reachable on system bus".into()
        } else {
            "org.bluez not found".into()
        },
        fix: if bluez_ok {
            None
        } else {
            Some("Start BlueZ: `sudo systemctl enable --now bluetooth.service`".into())
        },
    });

    // 4. PipeWire running
    let pw_ok = SysCommand::new("pw-cli")
        .args(["info", "0"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    checks.push(Check {
        name: "PipeWire",
        ok: pw_ok,
        detail: if pw_ok {
            "responding to `pw-cli info 0`".into()
        } else {
            "not responding (EQ requires PipeWire + WirePlumber)".into()
        },
        fix: if pw_ok {
            None
        } else {
            Some("Start PipeWire: `systemctl --user enable --now pipewire.service wireplumber.service`".into())
        },
    });

    // 5. User systemd unit installed + active
    let unit_status = SysCommand::new("systemctl")
        .args(["--user", "is-active", "airpods-daemon.service"])
        .output();
    let unit_active = matches!(&unit_status, Ok(o) if o.status.success());
    let unit_state = unit_status
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".into());
    checks.push(Check {
        name: "Systemd user unit",
        ok: unit_active,
        detail: format!("airpods-daemon.service: {unit_state}"),
        fix: if unit_active {
            None
        } else {
            Some("systemctl --user enable --now airpods-daemon.service".into())
        },
    });

    // 6. D-Bus service reachable
    let mut device_connected: Option<bool> = None;
    let mut device_label: Option<String> = None;
    let dbus_ok = match Connection::session().await {
        Ok(conn) => match AirPodsProxy::new(&conn).await {
            Ok(p) => {
                if let Ok(c) = p.connected().await {
                    device_connected = Some(c);
                }
                if device_connected == Some(true) {
                    let name = p.model_name().await.unwrap_or_default();
                    let fw = p.firmware().await.unwrap_or_default();
                    device_label = Some(if fw.is_empty() {
                        name
                    } else {
                        format!("{name} (FW {fw})")
                    });
                }
                true
            }
            Err(_) => false,
        },
        Err(_) => false,
    };
    checks.push(Check {
        name: "Daemon D-Bus service",
        ok: dbus_ok,
        detail: if dbus_ok {
            "org.costa.AirPods reachable".into()
        } else {
            "org.costa.AirPods unreachable — daemon not running or D-Bus activation broken".into()
        },
        fix: if dbus_ok {
            None
        } else {
            Some("Start the daemon: `systemctl --user start airpods-daemon.service`, then check `journalctl --user -u airpods-daemon -n 50`".into())
        },
    });

    // 7. AirPods connection state (informational only — failure isn't a config bug)
    if let Some(connected) = device_connected {
        let label = device_label.unwrap_or_default();
        checks.push(Check {
            name: "AirPods connection",
            ok: connected,
            detail: if connected {
                if label.is_empty() {
                    "connected".into()
                } else {
                    format!("connected — {label}")
                }
            } else {
                "no AirPods currently connected (informational)".into()
            },
            fix: if connected {
                None
            } else {
                Some("Connect via `bluetoothctl connect <MAC>` or just open the AirPods case near a paired adapter".into())
            },
        });
    }

    if json {
        let arr: Vec<_> = checks
            .iter()
            .map(|c| {
                serde_json::json!({
                    "name": c.name,
                    "ok": c.ok,
                    "detail": c.detail,
                    "fix": c.fix,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&serde_json::json!({ "checks": arr }))?);
        return Ok(());
    }

    println!("Checking airpods-helper installation...\n");
    let mut failures = 0;
    for c in &checks {
        let mark = if c.ok { "\u{2713}" } else { "\u{2717}" };
        println!("  {mark} {} — {}", c.name, c.detail);
        if !c.ok {
            if let Some(fix) = &c.fix {
                println!("    Fix: {fix}");
            }
            // Don't count the AirPods-connection check as a failure
            if c.name != "AirPods connection" {
                failures += 1;
            }
        }
    }
    println!();
    if failures == 0 {
        println!("Everything looks good.");
    } else {
        println!("Found {failures} issue(s). See fix hints above.");
        std::process::exit(1);
    }

    Ok(())
}
