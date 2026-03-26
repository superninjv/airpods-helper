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

    fn set_anc_mode(&self, mode: &str) -> zbus::Result<()>;
    fn set_conversational_awareness(&self, enabled: bool) -> zbus::Result<()>;
    fn set_adaptive_noise_level(&self, level: u8) -> zbus::Result<()>;
    fn set_one_bud_anc(&self, enabled: bool) -> zbus::Result<()>;
    fn set_eq_preset(&self, name: &str) -> zbus::Result<()>;
    fn disable_eq(&self) -> zbus::Result<()>;
    fn list_eq_presets(&self) -> zbus::Result<Vec<String>>;
    fn reconnect(&self) -> zbus::Result<()>;
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
    /// Control EQ presets
    Eq {
        /// Preset name, "list", or "off" (omit to show current)
        action: Option<String>,
    },
    /// Trigger device reconnect
    Reconnect,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

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
        Command::Eq { action } => cmd_eq(&proxy, action, cli.json).await?,
        Command::Reconnect => {
            proxy.reconnect().await?;
            println!("reconnect requested");
        }
    }

    Ok(())
}

async fn cmd_status(proxy: &AirPodsProxy<'_>, json: bool) -> anyhow::Result<()> {
    let connected = proxy.connected().await?;

    if json {
        let obj = serde_json::json!({
            "connected": connected,
            "model": proxy.model().await.unwrap_or_default(),
            "model_name": proxy.model_name().await.unwrap_or_default(),
            "firmware": proxy.firmware().await.unwrap_or_default(),
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

    let model = proxy.model().await.unwrap_or_default();
    let model_name = proxy.model_name().await.unwrap_or_default();
    let firmware = proxy.firmware().await.unwrap_or_default();
    let anc = proxy.anc_mode().await.unwrap_or_default();
    let bl = proxy.battery_left().await.unwrap_or(-1);
    let br = proxy.battery_right().await.unwrap_or(-1);
    let bc = proxy.battery_case().await.unwrap_or(-1);
    let el = proxy.ear_left().await.unwrap_or(false);
    let er = proxy.ear_right().await.unwrap_or(false);
    let ca = proxy.conversational_awareness().await.unwrap_or(false);
    let noise = proxy.adaptive_noise_level().await.unwrap_or(0);
    let ob = proxy.one_bud_anc().await.unwrap_or(false);
    let eq = proxy.eq_preset().await.unwrap_or_default();

    let display = if model_name.is_empty() { &model } else { &model_name };
    println!("{display}  ({model})  FW {firmware}");
    println!();
    print_battery("Left ", bl, proxy.charging_left().await.unwrap_or(false));
    print_battery("Right", br, proxy.charging_right().await.unwrap_or(false));
    print_battery("Case ", bc, proxy.charging_case().await.unwrap_or(false));
    println!();
    println!("ANC:    {anc}");
    if anc == "adaptive" {
        println!("Noise:  {noise}%");
    }
    println!("CA:     {}", if ca { "on" } else { "off" });
    println!("1-Bud:  {}", if ob { "on" } else { "off" });
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
