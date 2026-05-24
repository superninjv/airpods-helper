```
    ___   _      ___         __        __ __    __
   /   | (_)____/ _ \____   / /_____  / // /__ / /___  ___  _____
  / /| |/ / ___/ /_)/ __ \/ __  / __|/ _  / _ \ / __ \/ _ \/ ___/
 / ___ / / / / ___/ /_/ / /_/ /\__ \ / / /  __/ / /_/ /  __/ /
/_/  |_/_/_/ /_/   \____/\__,_/|___/_/ /_/\___/_/ .___/\___/_/
                                                /_/
```

[![CI](https://github.com/superninjv/airpods-helper/actions/workflows/ci.yml/badge.svg)](https://github.com/superninjv/airpods-helper/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Latest Release](https://img.shields.io/github/v/release/superninjv/airpods-helper)](https://github.com/superninjv/airpods-helper/releases)

# airpods-helper

Native Apple AirPods support for Linux — ANC control, transparency mode, battery levels, ear detection, parametric EQ, and more.

A lightweight Rust daemon communicates with AirPods over Bluetooth L2CAP using the Apple Accessory Protocol (AAP), exposes everything via D-Bus, and ships with a CLI tool and optional GTK4 desktop widgets.

## Supported Devices

| Device | Model Numbers | ANC | Adaptive | CA |
|--------|---------------|:---:|:--------:|:--:|
| AirPods 1 | A1523, A1722 | | | |
| AirPods 2 | A2031, A2032 | | | |
| AirPods 3 | A2564, A2565 | | | |
| AirPods 4 | A3050, A3053, A3054, A3058 | | | |
| AirPods 4 ANC | A3055, A3056, A3057, A3059 | ✓ | ✓ | ✓ |
| AirPods Pro | A2083, A2084, A2190 | ✓ | | |
| AirPods Pro 2 (Lightning) | A2698, A2699, A2700, A2931 | ✓ | ✓ | ✓ |
| AirPods Pro 2 (USB-C) | A2968, A3047, A3048, A3049 | ✓ | ✓ | ✓ |
| AirPods Pro 3 | A3063, A3064, A3065, A3122 | ✓ | ✓ | ✓ |
| AirPods Max | A2096 | ✓ | | |
| AirPods Max 2 | A3184 | ✓ | ✓ | ✓ |

The daemon auto-detects the connected model and exposes a `Features` D-Bus property so widgets and CLI only show controls your hardware supports.

## Features

- **ANC modes** — Off, Noise Cancellation, Transparency, Adaptive
- **Battery levels** — Left, Right, Case with charging status
- **Ear detection** — Auto-pause/resume media (MPRIS) when buds are removed/inserted
- **Conversational Awareness** — Enable/disable, with activity state tracking
- **Adaptive noise level** — Fine-tune noise cancellation intensity (0-100)
- **One-bud ANC** — ANC when wearing a single AirPod
- **Microphone Mode** — Select which bud is the primary mic (auto / left / right). See [Microphone Mode & the A2DP+Mic Limitation](#microphone-mode--the-a2dpmic-limitation) for what this does and does **not** do.
- **Parametric EQ** — PipeWire filter-chain presets (flat, bass-boost, vocal-clarity, crinacle). **⚠️ Not currently functional — see [Known Limitations](#known-limitations).**
- **Auto-reconnect** — Exponential backoff reconnection on disconnect
- **CLI tool** — Full terminal control (`airpods-cli status`, `airpods-cli anc noise`, etc.)
- **D-Bus interface** — `org.costa.AirPods` for integration with any desktop environment
- **GTK4 widgets** — Bar button + popover + connection popup for AGS (Astal GTK Shell)
- **Cross-platform desktop app** — Tauri-based tray/settings UI under `app/` (Linux today, Windows scaffolded)

## How It Works

The daemon communicates with AirPods using the **Apple Accessory Protocol (AAP)** — a proprietary binary protocol that Apple devices use over classic Bluetooth. The connection is established over an **L2CAP channel on PSM 0x1001**, bypassing the standard audio/HFP profiles to access device-level controls.

The protocol has been reverse-engineered by analyzing packet captures between AirPods and Apple devices. The daemon performs a multi-step handshake, then maintains a persistent connection where it sends commands (ANC mode changes, configuration) and receives unsolicited notifications (battery updates, ear detection events, firmware info).

All state is funneled into a shared watch channel and exposed over D-Bus, making the CLI and widgets simple, stateless clients.

## Quick Start

Three install paths are supported on Linux. Pick whichever matches your distro.

### Arch (PKGBUILD)

A working PKGBUILD ships in `packaging/PKGBUILD` (AUR submission pending). Build and install with:

```bash
cd packaging
makepkg -si
sudo setcap 'cap_net_raw,cap_net_admin+eip' /usr/bin/airpods-daemon
systemctl --user enable --now airpods-daemon.service
```

### Debian / Ubuntu (.deb)

```bash
./packaging/build-deb.sh
sudo dpkg -i packaging/airpods-helper_*_amd64.deb
sudo setcap 'cap_net_raw,cap_net_admin+eip' /usr/bin/airpods-daemon
systemctl --user enable --now airpods-daemon.service
```

### Any distro (from source, into `~/.local`)

```bash
# Requires: Rust toolchain, BlueZ dev headers (libdbus-1-dev / dbus-devel),
# PipeWire (for EQ), and an active user systemd session
make build
make install

# Grant L2CAP raw socket capability (required — daemon won't start without this)
sudo setcap 'cap_net_raw,cap_net_admin+eip' ~/.local/bin/airpods-daemon

# Enable and start
systemctl --user enable --now airpods-daemon.service
```

Verify the daemon is up:

```bash
airpods-cli status
journalctl --user -u airpods-daemon -f   # logs
```

### Pair AirPods

```bash
bluetoothctl
> scan on
> pair <MAC>
> trust <MAC>
> connect <MAC>
```

The daemon auto-detects AirPods by service UUID and establishes the AAP connection.

## CLI Reference

### Status

```bash
airpods-cli status                    # full status display
airpods-cli status --json             # JSON output for scripting
airpods-cli battery                   # battery levels only
```

### ANC Control

```bash
airpods-cli anc noise                 # noise cancellation
airpods-cli anc transparency          # transparency mode
airpods-cli anc adaptive              # adaptive mode
airpods-cli anc off                   # disable ANC
```

### Conversational Awareness

```bash
airpods-cli ca on                     # enable conversational awareness
airpods-cli ca off                    # disable conversational awareness
```

### Microphone Mode

Picks which bud's microphone is used when AirPods are routed as the input device (HFP/HSP mono call mode). See the explainer below — this does **not** enable concurrent stereo audio + mic.

```bash
airpods-cli mic auto                  # let the firmware pick (default)
airpods-cli mic left                  # force the left bud
airpods-cli mic right                 # force the right bud
```

### Equalizer

```bash
airpods-cli eq list                   # list available presets
airpods-cli eq bass-boost             # apply a preset
airpods-cli eq off                    # disable EQ
```

### Other

```bash
airpods-cli reconnect                 # trigger manual reconnect
```

## Architecture

```
┌─────────────────────────┐    D-Bus (org.costa.AirPods)    ┌──────────────────┐
│   daemon (Rust)         │ <-------------------------------> │  CLI / Widgets   │
│                         │   Properties + Signals + Methods │                  │
│  BlueZ <- BT adapter   │                                  │  airpods-cli     │
│  L2CAP <- AAP protocol  │                                  │  AirPodsBattery  │
│  State <- watch channels │                                  │  AirPodsPopup    │
│  EQ    <- PipeWire       │                                  └──────────────────┘
│  MPRIS <- ear detection  │
└─────────────────────────┘
```

The daemon speaks AAP over L2CAP (PSM 0x1001) to the AirPods, maintaining a persistent connection. All state is exposed via D-Bus properties with `PropertiesChanged` signals for reactive UIs. The CLI and widgets are pure D-Bus clients.

## Configuration

Config file: `~/.config/airpods-helper/config.toml`

```toml
[device]
# address = "AA:BB:CC:DD:EE:FF"  # auto-detected if not set

[eq]
active_preset = "flat"     # auto-loaded on connect
auto_load = true

[ear_detection]
pause_media = true         # pause on removal
resume_media = true        # resume on insertion

[reconnect]
auto_reconnect = true
max_retries = 3            # exponential backoff: 2s, 4s, 8s
```

## EQ Presets

Presets are TOML files in `~/.config/airpods-helper/eq/`:

| Preset | Description |
|--------|-------------|
| `flat` | No EQ (passthrough) |
| `bass-boost` | Enhanced low-end |
| `vocal-clarity` | Mid-range emphasis |
| `airpods-pro-crinacle` | Crinacle's AirPods Pro target |

Custom presets: create a `.toml` file with `name`, `description`, `preamp`, and `bands` (type, freq, q, gain).

## D-Bus API

**Service:** `org.costa.AirPods` | **Path:** `/org/costa/AirPods`

### Properties

| Property | Type | Description |
|----------|------|-------------|
| `Connected` | `b` | Device connected |
| `BatteryLeft` | `i` | Left bud battery (0-100, -1 if unknown) |
| `BatteryRight` | `i` | Right bud battery |
| `BatteryCase` | `i` | Case battery |
| `ChargingLeft` | `b` | Left bud charging |
| `ChargingRight` | `b` | Right bud charging |
| `ChargingCase` | `b` | Case charging |
| `AncMode` | `s` | off, noise, transparency, adaptive |
| `EarLeft` | `b` | Left bud in ear |
| `EarRight` | `b` | Right bud in ear |
| `ConversationalAwareness` | `b` | CA enabled |
| `ConversationalActivityState` | `s` | normal, speaking, stopped |
| `AdaptiveNoiseLevel` | `y` | 0-100 |
| `OneBudAnc` | `b` | Single-bud ANC |
| `Model` | `s` | Device model identifier (e.g. `airpods-pro-2-usb-c`) |
| `ModelName` | `s` | Human-readable model name |
| `Firmware` | `s` | Firmware version |
| `Features` | `as` | Capability list — subset of `anc`, `adaptive`, `ca`, `one_bud_anc`. Widgets and CLI use this to hide controls the connected hardware doesn't support. |
| `EqPreset` | `s` | Active EQ preset name |

### Methods

| Method | Args | Description |
|--------|------|-------------|
| `SetAncMode` | `(s)` | Set ANC mode |
| `SetConversationalAwareness` | `(b)` | Toggle CA |
| `SetAdaptiveNoiseLevel` | `(y)` | Set noise level 0-100 |
| `SetOneBudAnc` | `(b)` | Toggle one-bud ANC |
| `SetMicMode` | `(s)` | Primary mic bud: `auto`, `left`, or `right` |
| `SetEqPreset` | `(s)` | Apply EQ preset by name |
| `DisableEq` | — | Remove EQ filter chain |
| `ListEqPresets` | — | List available preset names |
| `Reconnect` | — | Trigger device reconnect |

### Signals

| Signal | Args | Description |
|--------|------|-------------|
| `DeviceConnected` | `(s)` | Model name |
| `DeviceDisconnected` | — | |
| `EarDetectionChanged` | `(bb)` | Left, right in-ear |

## Microphone Mode & the A2DP+Mic Limitation

### What Mic Mode actually does

The `SetMicMode` D-Bus method (and `airpods-cli mic …`) maps to AAP control sub-command `0x01` and selects which bud's microphone is treated as the primary input when AirPods are routed as the system input device:

- `auto` — firmware decides based on ear detection, signal quality, etc. (default)
- `left` — pin the left bud's mic
- `right` — pin the right bud's mic

This only matters when AirPods are negotiated as a **mono call device** (HFP/HSP / SCO / mSBC). It does **not** unlock simultaneous high-quality stereo audio + microphone input.

### Why you can't have A2DP-quality audio and the mic at the same time

Classic Bluetooth has a hard profile mutex: the AirPods can speak A2DP (stereo audio out) **or** HFP/HSP (mono audio in + mono audio out), never both at once. The moment any app opens the AirPods as an input device, PipeWire/WirePlumber drops the link from A2DP to mSBC/SCO, and your music quality collapses to 8–16 kHz mono. **This isn't a Linux bug — macOS hits the same brick wall.** Mac users will notice that joining a FaceTime/Zoom call while playing music tanks the audio quality identically; Apple just hides the transition more gracefully.

The AirPods Pro 2 (H2 chip) and later are physically capable of full-duplex **LE Audio (BAP/PACS over LC3)**, which would solve this — but Apple ships those endpoints **gated behind Magic Pairing crypto**. Specifically, AAP opcodes `0x30` / `0x31` perform an IRK / EncKey exchange against Apple's H2 device keys, and only after that does the AirPods advertise standard BAP service records. Without those keys, the AirPods refuse to expose LE Audio to a non-Apple host.

### Calling for contributors — reverse-engineer Magic Pairing

If you have the hardware, the patience, and ideally a USB BT sniffer or jailbroken iOS device, this is the **single highest-impact contribution** you could make to native AirPods support on Linux (and Android, and any non-Apple BT stack). What's needed:

1. **Packet captures** of a clean Magic Pairing handshake — iOS device ↔ AirPods Pro 2/3 — covering opcodes `0x30` and `0x31` end-to-end.
2. **Key material analysis** — figuring out whether Apple's IRK derivation is per-device-deterministic from public identifiers, or genuinely sealed inside the H2 / Secure Enclave.
3. **A handshake replay / impersonation PoC** that gets a Linux host past the gate so the AirPods will advertise BAP/PACS.

Even partial wins (e.g. confirming the exact gating opcode pair on a specific firmware, or documenting failure modes) help. See [LibrePods](https://github.com/kavishdevar/librepods) for the current state of community AAP research, and `daemon/src/aap/mod.rs` for the documented sub-command table this project uses. Open an issue if you want to coordinate before sinking time in.

## Known Limitations

- **Parametric EQ is not currently working.** The `airpods-cli eq` commands and `SetEqPreset` D-Bus method write a PipeWire filter-chain config and restart PipeWire, but the filter chain isn't actually taking effect on the AirPods output in practice. This is a known issue — the daemon plumbing is in place, the PipeWire integration needs fixing. Treat EQ as a work-in-progress; the preset files and config surface are stable.
- **No concurrent stereo audio + mic.** See the section above — this is a Bluetooth-stack-level limitation, not specific to this project. Magic Pairing reverse-engineering is the path forward; contributions welcome.
- **Windows daemon is out of sync.** The `windows/` crate is currently behind the Linux daemon on AAP coverage. Linux is the supported target today.
- **CI is currently red.** A regression introduced around v0.2.0 needs cleanup. Local builds work; the GitHub Actions badge above does not reflect runtime quality.

## Widget Integration (AGS)

The `widget/` directory contains GTK4 widgets for [AGS](https://github.com/Aylur/ags):

```typescript
import { AirPodsBattery } from "./airpods"

// Add to your bar
const bar = new Gtk.Box()
bar.append(AirPodsBattery())
```

Import `widget/style.css` in your AGS stylesheet for default dark theme styling.

## System Requirements

- **Linux** with BlueZ 5.x
- **PipeWire** + WirePlumber (for EQ)
- **Bluetooth adapter** supporting BR/EDR (classic Bluetooth)
- AirPods Pro, AirPods Pro 2, AirPods Max, AirPods 3/4 (any model with AAP support)

## Troubleshooting

**"Permission denied" on L2CAP connect:**
```bash
sudo setcap 'cap_net_raw,cap_net_admin+eip' ~/.local/bin/airpods-daemon
```

**Daemon not detecting AirPods:**
- Ensure AirPods are paired and connected via `bluetoothctl`
- Check `journalctl --user -u airpods-daemon -f` for logs
- Set `RUST_LOG=airpods_daemon=debug` for verbose output

**EQ causes audio dropout:**
Applying/removing EQ restarts PipeWire to reload the filter chain config. This causes a brief audio interruption.

**Widget not showing:**
Ensure the widget directory is symlinked: `ls -la ~/.config/ags/widget/airpods`

## Contributing

Contributions are welcome! Here is how to get started:

1. Fork the repository and create a feature branch
2. Install dependencies: Rust toolchain + `libdbus-1-dev` (or `dbus-devel` on Fedora)
3. Build and test: `make build && cargo test --workspace`
4. Run clippy: `cargo clippy --workspace -- -D warnings`
5. Open a pull request against `main`

### High-impact areas

- **🔓 Reverse-engineer Apple Magic Pairing (AAP 0x30 / 0x31).** Unlocks LE Audio BAP/PACS on AirPods Pro 2 and later, which is the only known path to concurrent stereo audio + mic on Linux. See [Microphone Mode & the A2DP+Mic Limitation](#microphone-mode--the-a2dpmic-limitation) for what specifically is needed — packet captures, key-derivation analysis, handshake replay PoC.
- **Fix the parametric EQ pipeline.** See [Known Limitations](#known-limitations) — the daemon writes a PipeWire filter chain but it isn't taking effect. Anyone familiar with WirePlumber session policy / `pw-cli` debugging would help.
- **Bring the Windows daemon back in sync.** The `windows/` crate has fallen behind on AAP coverage; CI has been red since v0.2.0.
- **Packet captures for new models / firmware.** Especially AirPods Pro 3 and AirPods Max 2. Open an issue to coordinate before sinking time in.

## License

MIT
