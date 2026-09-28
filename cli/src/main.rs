use clap::{Parser, Subcommand};
use std::collections::HashMap;
use zbus::zvariant::OwnedValue;
use zbus::Connection;

const BUS: &str = "org.costa.AirPods";
const PATH: &str = "/org/costa/AirPods";

type Band = (String, f64, f64, f64);

#[zbus::proxy(interface = "org.costa.AirPods", default_service = "org.costa.AirPods", default_path = "/org/costa/AirPods")]
trait AirPods {
    fn set_anc_mode(&self, mode: &str) -> zbus::Result<()>;
    fn set_conversational_awareness(&self, enabled: bool) -> zbus::Result<()>;
    fn set_adaptive_noise_level(&self, level: u8) -> zbus::Result<()>;
    fn set_one_bud_anc(&self, enabled: bool) -> zbus::Result<()>;
    fn set_volume_swipe(&self, enabled: bool) -> zbus::Result<()>;
    fn set_mic_mode(&self, mode: &str) -> zbus::Result<()>;
    fn set_eq_preset(&self, id: &str) -> zbus::Result<()>;
    fn disable_eq(&self) -> zbus::Result<()>;
    fn get_eq_presets(&self) -> zbus::Result<Vec<(String, String, String, bool)>>;
    fn get_eq_preset(&self, id: &str) -> zbus::Result<(String, String, f64, Vec<Band>)>;
    fn save_eq_preset(&self, id: &str, name: &str, description: &str, preamp: f64, bands: Vec<Band>) -> zbus::Result<()>;
    fn delete_eq_preset(&self, id: &str) -> zbus::Result<()>;
    fn reconnect(&self) -> zbus::Result<()>;
    fn connect_to(&self, address: &str) -> zbus::Result<()>;
    fn disconnect(&self) -> zbus::Result<()>;
    fn list_paired(&self) -> zbus::Result<Vec<(String, String)>>;
    fn pair(&self, address: &str) -> zbus::Result<()>;
    fn quick_pair_scan(&self, duration_secs: u32) -> zbus::Result<Vec<(String, String, String, i16, bool)>>;
}

#[derive(Parser)]
#[command(name = "airpods-cli", version, about = "Control AirPods from the terminal")]
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
    /// Get or set listening mode: off, noise, transparency, adaptive
    Anc { mode: Option<String> },
    /// Get or set adaptive noise level (0-100)
    Noise { level: Option<u8> },
    /// Get or set conversational awareness (on/off)
    Ca { toggle: Option<String> },
    /// Get or set ANC with one bud in (on/off)
    OneBud { toggle: Option<String> },
    /// Get or set volume swipe on the stem (on/off)
    Swipe { toggle: Option<String> },
    /// Get or set the primary microphone: auto, left, right
    Mic { mode: Option<String> },
    /// Equalizer: list, show, off, import, delete, or a preset id to apply
    Eq {
        #[command(subcommand)]
        action: Option<EqAction>,
    },
    /// Show settings, or change one: `set <name> <value>`
    Settings,
    /// Change a setting (pause-on-removal, resume-on-insert, auto-reconnect, eq-auto-load, preferred-device)
    Set { name: String, value: String },
    /// Connect to paired AirPods by MAC address
    Connect { address: String },
    /// Disconnect the connected AirPods (suppresses auto-reconnect)
    Disconnect,
    /// Reconnect to the last-connected AirPods
    Reconnect,
    /// List paired AirPods
    Paired,
    /// Pair new AirPods (case open, hold the button until the light flashes white)
    Pair { address: String },
    /// Scan for nearby AirPods that can be paired
    Scan {
        #[arg(long, default_value_t = 10)]
        duration: u32,
    },
    /// Diagnose the installation (capabilities, BlueZ, daemon, audio backend)
    Doctor,
}

#[derive(Subcommand)]
enum EqAction {
    /// List presets
    List,
    /// Show a preset's bands (default: the active one)
    Show { id: Option<String> },
    /// Turn EQ off
    Off,
    /// Import an AutoEQ "ParametricEQ.txt" (or Equalizer APO) file as a preset
    Import {
        file: std::path::PathBuf,
        /// Preset id (default: derived from the file name)
        #[arg(long)]
        id: Option<String>,
        /// Display name (default: the id)
        #[arg(long)]
        name: Option<String>,
        /// Apply it right away
        #[arg(long)]
        apply: bool,
    },
    /// Delete a user preset
    Delete { id: String },
    /// Apply a preset by id
    #[command(external_subcommand)]
    Apply(Vec<String>),
}

/// Settings exposed as writable D-Bus properties: (cli name, property, is_bool)
const SETTINGS: &[(&str, &str, bool)] = &[
    ("pause-on-removal", "PauseOnRemoval", true),
    ("resume-on-insert", "ResumeOnInsert", true),
    ("auto-reconnect", "AutoReconnect", true),
    ("eq-auto-load", "EqAutoLoad", true),
    ("preferred-device", "PreferredDevice", false),
];

struct Client {
    conn: Connection,
    proxy: AirPodsProxy<'static>,
}

type Props = HashMap<String, OwnedValue>;

fn get<T: TryFrom<OwnedValue>>(p: &Props, key: &str) -> Option<T> {
    p.get(key).and_then(|v| v.try_clone().ok()).and_then(|v| T::try_from(v).ok())
}
fn s(p: &Props, key: &str) -> String {
    get(p, key).unwrap_or_default()
}
fn b(p: &Props, key: &str) -> bool {
    get(p, key).unwrap_or(false)
}
fn i(p: &Props, key: &str) -> i32 {
    get(p, key).unwrap_or(-1)
}
fn on_off(v: bool) -> &'static str {
    if v { "on" } else { "off" }
}

impl Client {
    async fn new() -> anyhow::Result<Self> {
        let conn = Connection::session().await.map_err(|e| anyhow::anyhow!("can't reach the D-Bus session bus: {e}"))?;
        let proxy = AirPodsProxy::new(&conn).await?;
        Ok(Self { conn, proxy })
    }

    async fn props(&self) -> anyhow::Result<Props> {
        let props = zbus::fdo::PropertiesProxy::builder(&self.conn).destination(BUS)?.path(PATH)?.build().await?;
        let all = props.get_all(zbus::names::InterfaceName::from_static_str_unchecked(BUS)).await.map_err(friendly)?;
        Ok(all)
    }

    async fn set_prop(&self, name: &str, value: zbus::zvariant::Value<'_>) -> anyhow::Result<()> {
        let props = zbus::fdo::PropertiesProxy::builder(&self.conn).destination(BUS)?.path(PATH)?.build().await?;
        props
            .set(zbus::names::InterfaceName::from_static_str_unchecked(BUS), name, value)
            .await
            .map_err(friendly)
    }
}

/// Turn "service unknown" into something actionable.
fn friendly(e: impl std::fmt::Display) -> anyhow::Error {
    let msg = e.to_string();
    if msg.contains("ServiceUnknown") || msg.contains("was not provided by any .service files") {
        anyhow::anyhow!(
            "airpods-daemon isn't running.\n  Start it: systemctl --user enable --now airpods-daemon.service\n  Diagnose: airpods-cli doctor"
        )
    } else if let Some(rest) = msg.split_once(": ").map(|(_, r)| r).filter(|_| msg.starts_with("org.freedesktop.DBus.Error")) {
        anyhow::anyhow!("{rest}")
    } else {
        anyhow::anyhow!("{msg}")
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let cli = Cli::parse();
    if let Err(e) = run(cli).await {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

async fn run(cli: Cli) -> anyhow::Result<()> {
    if matches!(cli.command, Command::Doctor) {
        return doctor(cli.json).await;
    }
    let c = Client::new().await?;
    let json = cli.json;
    let print_json = |v: serde_json::Value| println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());

    match cli.command {
        Command::Status => status(&c.props().await?, json),
        Command::Battery => battery(&c.props().await?, json),
        Command::Anc { mode: Some(m) } => {
            c.proxy.set_anc_mode(&m.to_lowercase()).await.map_err(friendly)?;
            println!("listening mode: {m}");
        }
        Command::Noise { level: Some(l) } => {
            c.proxy.set_adaptive_noise_level(l).await.map_err(friendly)?;
            println!("adaptive noise level: {l}");
        }
        Command::Ca { toggle: Some(t) } => {
            c.proxy.set_conversational_awareness(parse_toggle(&t)?).await.map_err(friendly)?;
            println!("conversational awareness: {t}");
        }
        Command::OneBud { toggle: Some(t) } => {
            c.proxy.set_one_bud_anc(parse_toggle(&t)?).await.map_err(friendly)?;
            println!("one-bud ANC: {t}");
        }
        Command::Swipe { toggle: Some(t) } => {
            c.proxy.set_volume_swipe(parse_toggle(&t)?).await.map_err(friendly)?;
            println!("volume swipe: {t}");
        }
        Command::Mic { mode: Some(m) } => {
            c.proxy.set_mic_mode(&m.to_lowercase()).await.map_err(friendly)?;
            println!("microphone: {m}");
        }
        // Getters for the above
        Command::Anc { mode: None } => show_one(&c, "AncMode", json).await?,
        Command::Noise { level: None } => show_one(&c, "AdaptiveNoiseLevel", json).await?,
        Command::Ca { toggle: None } => show_one(&c, "ConversationalAwareness", json).await?,
        Command::OneBud { toggle: None } => show_one(&c, "OneBudAnc", json).await?,
        Command::Swipe { toggle: None } => show_one(&c, "VolumeSwipe", json).await?,
        Command::Mic { mode: None } => show_one(&c, "MicMode", json).await?,

        Command::Eq { action } => eq(&c, action, json).await?,

        Command::Settings => {
            let p = c.props().await?;
            if json {
                let map: serde_json::Map<_, _> =
                    SETTINGS.iter().map(|(n, prop, _)| (n.to_string(), to_json(p.get(*prop)))).collect();
                print_json(map.into());
            } else {
                for (name, prop, is_bool) in SETTINGS {
                    let v = if *is_bool { on_off(b(&p, prop)).to_string() } else { s(&p, prop) };
                    println!("{name:18} {}", if v.is_empty() { "(none)" } else { &v });
                }
            }
        }
        Command::Set { name, value } => {
            let (_, prop, is_bool) = SETTINGS
                .iter()
                .find(|(n, ..)| *n == name)
                .ok_or_else(|| anyhow::anyhow!("unknown setting '{name}' (see `airpods-cli settings`)"))?;
            if *is_bool {
                c.set_prop(prop, parse_toggle(&value)?.into()).await?;
            } else {
                let v = if matches!(value.as_str(), "none" | "\"\"" | "-") { "" } else { value.as_str() };
                c.set_prop(prop, v.into()).await?;
            }
            println!("{name}: {value}");
        }

        Command::Connect { address } => {
            c.proxy.connect_to(&address).await.map_err(friendly)?;
            println!("connecting to {address}");
        }
        Command::Disconnect => {
            c.proxy.disconnect().await.map_err(friendly)?;
            println!("disconnected");
        }
        Command::Reconnect => {
            c.proxy.reconnect().await.map_err(friendly)?;
            println!("reconnecting");
        }
        Command::Paired => {
            let devices = c.proxy.list_paired().await.map_err(friendly)?;
            if json {
                print_json(devices.iter().map(|(a, n)| serde_json::json!({ "address": a, "name": n })).collect());
            } else if devices.is_empty() {
                println!("no paired AirPods");
            } else {
                devices.iter().for_each(|(a, n)| println!("{a}  {n}"));
            }
        }
        Command::Pair { address } => {
            eprintln!("pairing {address}… keep the case open");
            c.proxy.pair(&address).await.map_err(friendly)?;
            println!("paired {address}");
        }
        Command::Scan { duration } => {
            if !json {
                eprintln!("scanning for {duration}s — open the AirPods case nearby…");
            }
            let found = c.proxy.quick_pair_scan(duration).await.map_err(friendly)?;
            if json {
                print_json(
                    found
                        .iter()
                        .map(|(a, n, m, r, p)| serde_json::json!({ "address": a, "name": n, "model": m, "rssi": r, "in_pair_mode": p }))
                        .collect(),
                );
            } else if found.is_empty() {
                println!("no AirPods found");
            } else {
                for (addr, name, model, rssi, pairing) in &found {
                    let mark = if *pairing { "★" } else { " " };
                    println!("{mark} {addr}  {model:28} {rssi:>4} dBm  {name}");
                }
                println!("\n★ = in pairing mode · pair with: airpods-cli pair <MAC>");
            }
        }
        Command::Doctor => unreachable!(),
    }
    Ok(())
}

async fn show_one(c: &Client, prop: &str, json: bool) -> anyhow::Result<()> {
    let p = c.props().await?;
    let v = to_json(p.get(prop));
    if json {
        println!("{}", serde_json::json!({ prop: v }));
    } else {
        match v {
            serde_json::Value::Bool(x) => println!("{}", on_off(x)),
            serde_json::Value::String(x) => println!("{x}"),
            other => println!("{other}"),
        }
    }
    Ok(())
}

fn to_json(v: Option<&OwnedValue>) -> serde_json::Value {
    use zbus::zvariant::Value;
    fn conv(v: &Value) -> serde_json::Value {
        match v {
            Value::Bool(b) => (*b).into(),
            Value::U8(n) => (*n).into(),
            Value::I16(n) => (*n).into(),
            Value::I32(n) => (*n).into(),
            Value::U32(n) => (*n).into(),
            Value::F64(n) => (*n).into(),
            Value::Str(s) => s.as_str().into(),
            Value::Array(a) => a.iter().map(conv).collect(),
            Value::Value(v) => conv(v),
            other => other.to_string().into(),
        }
    }
    v.map(|v| conv(v)).unwrap_or(serde_json::Value::Null)
}

fn is_headphones(p: &Props) -> bool {
    get::<Vec<String>>(p, "Features").unwrap_or_default().iter().any(|f| f == "headphones")
}

fn battery_line(label: &str, level: i32, charging: bool) {
    if level < 0 {
        println!("  {label:6} —");
        return;
    }
    let filled = (level.clamp(0, 100) as usize) / 5;
    let bar = format!("{}{}", "█".repeat(filled), "░".repeat(20 - filled));
    println!("  {label:6} {bar} {level:>3}%{}", if charging { " ⚡" } else { "" });
}

fn battery(p: &Props, json: bool) {
    if json {
        let keys = ["BatteryLeft", "BatteryRight", "BatteryCase", "ChargingLeft", "ChargingRight", "ChargingCase"];
        let map: serde_json::Map<_, _> = keys.iter().map(|k| (k.to_string(), to_json(p.get(*k)))).collect();
        println!("{}", serde_json::Value::from(map));
        return;
    }
    if !b(p, "Connected") {
        println!("not connected");
    } else if is_headphones(p) {
        battery_line("", i(p, "BatteryLeft"), b(p, "ChargingLeft"));
    } else {
        battery_line("Left", i(p, "BatteryLeft"), b(p, "ChargingLeft"));
        battery_line("Right", i(p, "BatteryRight"), b(p, "ChargingRight"));
        battery_line("Case", i(p, "BatteryCase"), b(p, "ChargingCase"));
    }
}

fn status(p: &Props, json: bool) {
    if json {
        let map: serde_json::Map<_, _> = p.iter().map(|(k, v)| (k.clone(), to_json(Some(v)))).collect();
        println!("{}", serde_json::to_string_pretty(&serde_json::Value::from(map)).unwrap_or_default());
        return;
    }
    if !b(p, "Connected") {
        println!("AirPods: not connected");
        return;
    }
    let features: Vec<String> = get(p, "Features").unwrap_or_default();
    let has = |f: &str| features.iter().any(|x| x == f);
    let name = Some(s(p, "ModelName")).filter(|n| !n.is_empty()).unwrap_or_else(|| s(p, "Model"));
    println!("{name}  ·  {}  ·  FW {}\n", s(p, "Address"), s(p, "Firmware"));
    battery(p, false);
    println!();

    let row = |k: &str, v: String| println!("  {k:14} {v}");
    if has("anc") {
        let mode = s(p, "AncMode");
        let extra = if mode == "adaptive" { format!(" (level {})", get::<u8>(p, "AdaptiveNoiseLevel").unwrap_or(0)) } else { String::new() };
        row("Listening", format!("{mode}{extra}"));
    }
    if has("ca") {
        row("Conv. aware", on_off(b(p, "ConversationalAwareness")).into());
    }
    if has("one_bud_anc") {
        row("One-bud ANC", on_off(b(p, "OneBudAnc")).into());
    }
    if !has("headphones") {
        let ear = |v| if v { "in" } else { "out" };
        row("Ears", format!("L {} · R {}", ear(b(p, "EarLeft")), ear(b(p, "EarRight"))));
    }
    row("Microphone", s(p, "MicMode"));
    let preset = s(p, "EqPreset");
    let eq = if preset.is_empty() { "off".to_string() } else { format!("{preset} ({})", s(p, "EqStatus")) };
    row("EQ", eq);
    let err = s(p, "EqError");
    if !err.is_empty() {
        row("", format!("⚠ {err}"));
    }
}

async fn eq(c: &Client, action: Option<EqAction>, json: bool) -> anyhow::Result<()> {
    match action {
        None | Some(EqAction::List) => {
            let presets = c.proxy.get_eq_presets().await.map_err(friendly)?;
            let p = c.props().await?;
            let active = s(&p, "EqPreset");
            if json {
                let list: Vec<_> = presets
                    .iter()
                    .map(|(id, name, desc, user)| serde_json::json!({ "id": id, "name": name, "description": desc, "user": user, "active": *id == active }))
                    .collect();
                println!("{}", serde_json::json!({ "active": active, "status": s(&p, "EqStatus"), "backend": s(&p, "EqBackend"), "error": s(&p, "EqError"), "presets": list }));
                return Ok(());
            }
            for (id, name, desc, user) in &presets {
                let mark = if *id == active { "●" } else { " " };
                let tag = if *user { " (user)" } else { "" };
                println!("{mark} {id:24} {name}{tag}{}", if desc.is_empty() { String::new() } else { format!(" — {desc}") });
            }
            let status = if active.is_empty() { "off".into() } else { s(&p, "EqStatus") };
            println!("\nEQ {status} · backend: {}", s(&p, "EqBackend"));
            let err = s(&p, "EqError");
            if !err.is_empty() {
                println!("⚠ {err}");
            }
        }
        Some(EqAction::Show { id }) => {
            let id = match id {
                Some(id) => id,
                None => Some(s(&c.props().await?, "EqPreset")).filter(|s| !s.is_empty()).ok_or_else(|| anyhow::anyhow!("EQ is off; pass a preset id"))?,
            };
            let (name, desc, preamp, bands) = c.proxy.get_eq_preset(&id).await.map_err(friendly)?;
            if json {
                let bands: Vec<_> = bands.iter().map(|(t, f, q, g)| serde_json::json!({ "type": t, "freq": f, "q": q, "gain": g })).collect();
                println!("{}", serde_json::json!({ "id": id, "name": name, "description": desc, "preamp": preamp, "bands": bands }));
                return Ok(());
            }
            println!("{name} ({id}){}", if desc.is_empty() { String::new() } else { format!("\n{desc}") });
            println!("\n  preamp {preamp:+.1} dB");
            for (t, f, q, g) in bands {
                println!("  {t:9} {f:>8.0} Hz   Q {q:<5.2} {g:+5.1} dB");
            }
        }
        Some(EqAction::Off) => {
            c.proxy.disable_eq().await.map_err(friendly)?;
            println!("EQ off");
        }
        Some(EqAction::Apply(args)) => {
            let id = args.first().ok_or_else(|| anyhow::anyhow!("missing preset id"))?;
            c.proxy.set_eq_preset(id).await.map_err(friendly)?;
            println!("EQ: {id}");
        }
        Some(EqAction::Delete { id }) => {
            c.proxy.delete_eq_preset(&id).await.map_err(friendly)?;
            println!("deleted {id}");
        }
        Some(EqAction::Import { file, id, name, apply }) => {
            let text = std::fs::read_to_string(&file).map_err(|e| anyhow::anyhow!("{}: {e}", file.display()))?;
            let (preamp, bands) = parse_parametric_eq(&text)?;
            let id = id.unwrap_or_else(|| slug(&file.file_stem().unwrap_or_default().to_string_lossy()));
            let name = name.unwrap_or_else(|| id.clone());
            let desc = format!("Imported from {}", file.file_name().unwrap_or_default().to_string_lossy());
            c.proxy.save_eq_preset(&id, &name, &desc, preamp, bands.clone()).await.map_err(friendly)?;
            println!("imported '{id}': preamp {preamp:+.1} dB, {} bands", bands.len());
            if apply {
                c.proxy.set_eq_preset(&id).await.map_err(friendly)?;
                println!("EQ: {id}");
            }
        }
    }
    Ok(())
}

/// "AirPods Pro 2 ParametricEQ" → "airpods-pro-2-parametriceq"
fn slug(s: &str) -> String {
    let raw: String = s.to_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    let s = raw.split('-').filter(|p| !p.is_empty()).collect::<Vec<_>>().join("-");
    s.chars().take(48).collect::<String>().trim_end_matches('-').to_string()
}

/// Parse AutoEQ `ParametricEQ.txt` / Equalizer APO syntax:
/// ```text
/// Preamp: -6.2 dB
/// Filter 1: ON LSC Fc 105 Hz Gain 4.5 dB Q 0.70
/// Filter 2: ON PK Fc 2000 Hz Gain -2.1 dB Q 1.41
/// ```
fn parse_parametric_eq(text: &str) -> anyhow::Result<(f64, Vec<Band>)> {
    let mut preamp = 0.0;
    let mut bands = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("Preamp:") {
            preamp = rest.split_whitespace().next().and_then(|v| v.parse().ok()).ok_or_else(|| anyhow::anyhow!("line {}: bad preamp", n + 1))?;
            continue;
        }
        if !line.starts_with("Filter") {
            continue;
        }
        let toks: Vec<&str> = line.split_whitespace().collect();
        if !toks.contains(&"ON") {
            continue; // disabled filter
        }
        let after = |key: &str| toks.iter().position(|t| *t == key).and_then(|i| toks.get(i + 1)).and_then(|v| v.parse::<f64>().ok());
        let ty = toks.iter().find_map(|t| {
            Some(match *t {
                "PK" | "PEQ" => "peaking",
                "LS" | "LSC" | "LSQ" => "lowshelf",
                "HS" | "HSC" | "HSQ" => "highshelf",
                "LP" | "LPQ" => "lowpass",
                "HP" | "HPQ" => "highpass",
                "NO" => "notch",
                _ => return None,
            })
        });
        let (Some(ty), Some(freq)) = (ty, after("Fc")) else {
            anyhow::bail!("line {}: unsupported filter: {line}", n + 1);
        };
        bands.push((ty.to_string(), freq, after("Q").unwrap_or(0.707), after("Gain").unwrap_or(0.0)));
    }
    if bands.is_empty() {
        anyhow::bail!("no enabled filters found — is this an AutoEQ ParametricEQ.txt?");
    }
    Ok((preamp, bands))
}

fn parse_toggle(s: &str) -> anyhow::Result<bool> {
    match s.to_lowercase().as_str() {
        "on" | "true" | "1" | "yes" => Ok(true),
        "off" | "false" | "0" | "no" => Ok(false),
        _ => anyhow::bail!("expected on/off, got '{s}'"),
    }
}

struct Check {
    name: &'static str,
    ok: bool,
    detail: String,
    fix: Option<String>,
    /// Failing this doesn't mean the install is broken.
    informational: bool,
}

async fn name_has_owner(conn: &Connection, name: &str) -> bool {
    let Ok(dbus) = zbus::fdo::DBusProxy::new(conn).await else { return false };
    let Ok(name) = zbus::names::BusName::try_from(name) else { return false };
    dbus.name_has_owner(name).await.unwrap_or(false)
}

async fn doctor(json: bool) -> anyhow::Result<()> {
    let mut checks: Vec<Check> = Vec::new();
    let mut check = |name, ok, detail: String, fix: Option<&str>, informational| {
        checks.push(Check { name, ok, detail, fix: (!ok).then(|| fix.map(str::to_string)).flatten(), informational })
    };

    // Daemon binary + capabilities
    let home = std::env::var("HOME").unwrap_or_default();
    let path_dirs = std::env::var("PATH").unwrap_or_default();
    let daemon = [format!("{home}/.local/bin"), "/usr/local/bin".into(), "/usr/bin".into()]
        .into_iter()
        .chain(path_dirs.split(':').map(str::to_string))
        .map(|dir| format!("{dir}/airpods-daemon"))
        .find(|p| std::path::Path::new(p).is_file());
    check("daemon binary", daemon.is_some(), daemon.clone().map_or("not found".into(), |p| format!("found at {p}")), Some("install with `make install`, the .deb, or the PKGBUILD"), false);
    if let Some(p) = &daemon {
        let caps = std::process::Command::new("getcap").arg(p).output().ok().filter(|o| o.status.success());
        let text = caps.as_ref().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
        let ok = text.contains("cap_net_raw") && text.contains("cap_net_admin");
        let detail = if caps.is_none() { "can't run getcap (install libcap)".into() } else if ok { "cap_net_raw + cap_net_admin".into() } else { "missing".into() };
        check("L2CAP capability", ok, detail, Some(&format!("sudo setcap 'cap_net_raw,cap_net_admin+eip' {p}")), false);
    }

    // BlueZ
    let bluez = match Connection::system().await {
        Ok(c) => name_has_owner(&c, "org.bluez").await,
        Err(_) => false,
    };
    check("BlueZ", bluez, if bluez { "running".into() } else { "org.bluez not on the system bus".into() }, Some("sudo systemctl enable --now bluetooth.service"), false);

    // Daemon over D-Bus (this also triggers D-Bus activation if needed)
    let mut props: Option<Props> = None;
    if let Ok(c) = Client::new().await {
        props = c.props().await.ok();
    }
    let running = props.is_some();
    let version = props.as_ref().map(|p| s(p, "Version")).unwrap_or_default();
    check("daemon", running, if running { format!("running (v{version})") } else { "not reachable on the session bus".into() },
        Some("systemctl --user enable --now airpods-daemon.service; then journalctl --user -u airpods-daemon -n 50"), false);

    if let Some(p) = &props {
        let backend = s(p, "EqBackend");
        let ok = backend != "none" && !backend.is_empty();
        let detail = match backend.as_str() {
            "pipewire" => "PipeWire (smart filter)".into(),
            "pipewire-legacy" => "PipeWire (WirePlumber < 0.5: EQ becomes the default output while on)".into(),
            "pulseaudio" => "PulseAudio (in-process DSP)".into(),
            _ => "no supported audio server — EQ unavailable".into(),
        };
        check("EQ backend", ok, detail, Some("EQ needs PipeWire, or PulseAudio with pactl/pacat/parec"), true);
        let connected = b(p, "Connected");
        let name = s(p, "ModelName");
        check("AirPods", connected, if connected { format!("connected — {name}") } else { "none connected".into() }, Some("open the case near this computer, or `airpods-cli connect <MAC>`"), true);
    }

    let failures = checks.iter().filter(|c| !c.ok && !c.informational).count();
    if json {
        let arr: Vec<_> = checks.iter().map(|c| serde_json::json!({ "name": c.name, "ok": c.ok, "detail": c.detail, "fix": c.fix, "informational": c.informational })).collect();
        println!("{}", serde_json::to_string_pretty(&serde_json::json!({ "ok": failures == 0, "checks": arr }))?);
    } else {
        for c in &checks {
            let mark = if c.ok { "✓" } else if c.informational { "·" } else { "✗" };
            println!("  {mark} {:18} {}", c.name, c.detail);
            if let Some(fix) = &c.fix {
                println!("    → {fix}");
            }
        }
        println!("\n{}", if failures == 0 { "Everything looks good.".to_string() } else { format!("{failures} problem(s) found.") });
    }
    if failures > 0 {
        std::process::exit(1);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_autoeq_file() {
        let text = "Preamp: -6.4 dB\nFilter 1: ON LSC Fc 105 Hz Gain 5.5 dB Q 0.70\nFilter 2: ON PK Fc 2270 Hz Gain -3.1 dB Q 1.95\nFilter 3: OFF PK Fc 100 Hz Gain 1 dB Q 1\nFilter 4: ON HSC Fc 10000 Hz Gain 2.0 dB Q 0.70\n";
        let (preamp, bands) = parse_parametric_eq(text).unwrap();
        assert_eq!(preamp, -6.4);
        assert_eq!(bands.len(), 3);
        assert_eq!(bands[0], ("lowshelf".into(), 105.0, 0.70, 5.5));
        assert_eq!(bands[1].0, "peaking");
        assert_eq!(bands[2], ("highshelf".into(), 10000.0, 0.70, 2.0));
    }

    #[test]
    fn rejects_non_eq_files() {
        assert!(parse_parametric_eq("hello world").is_err());
    }

    #[test]
    fn slugs() {
        assert_eq!(slug("AirPods Pro 2 ParametricEQ"), "airpods-pro-2-parametriceq");
        assert_eq!(slug("--x__y--"), "x-y");
    }
}
