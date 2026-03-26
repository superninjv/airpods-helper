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

| Device | Model Numbers |
|--------|---------------|
| AirPods Pro | A2084 |
| AirPods Pro 2 (Lightning) | A2698, A2699 |
| AirPods Pro 2 (USB-C) | A3047, A3048 |
| AirPods 3 | A2564, A2565 |
| AirPods 4 | A3131, A3130 |
| AirPods 4 ANC | A3914, A3913 |
| AirPods Max | A2096 |
| AirPods Max 2 | A3526, A3527 |

## Features

- **ANC modes** — Off, Noise Cancellation, Transparency, Adaptive
- **Battery levels** — Left, Right, Case with charging status
- **Ear detection** — Auto-pause/resume media (MPRIS) when buds are removed/inserted
- **Conversational Awareness** — Enable/disable, with activity state tracking
- **Adaptive noise level** — Fine-tune noise cancellation intensity (0-100)
- **One-bud ANC** — ANC when wearing a single AirPod
- **Parametric EQ** — PipeWire filter-chain presets (flat, bass-boost, vocal-clarity, crinacle)
- **Auto-reconnect** — Exponential backoff reconnection on disconnect
- **CLI tool** — Full terminal control (`airpods-cli status`, `airpods-cli anc noise`, etc.)
- **D-Bus interface** — `org.costa.AirPods` for integration with any desktop environment
- **GTK4 widgets** — Bar button + popover + connection popup for AGS (Astal GTK Shell)

## How It Works

The daemon communicates with AirPods using the **Apple Accessory Protocol (AAP)** — a proprietary binary protocol that Apple devices use over classic Bluetooth. The connection is established over an **L2CAP channel on PSM 0x1001**, bypassing the standard audio/HFP profiles to access device-level controls.

The protocol has been reverse-engineered by analyzing packet captures between AirPods and Apple devices. The daemon performs a multi-step handshake, then maintains a persistent connection where it sends commands (ANC mode changes, configuration) and receives unsolicited notifications (battery updates, ear detection events, firmware info).

All state is funneled into a shared watch channel and exposed over D-Bus, making the CLI and widgets simple, stateless clients.

## Quick Start

### Build

```bash
# Requires: Rust toolchain, BlueZ dev headers (libdbus-1-dev / dbus-devel)
make build
```

### Install

```bash
make install

# Grant L2CAP raw socket capability
sudo setcap 'cap_net_raw,cap_net_admin+eip' ~/.local/bin/airpods-daemon

# Enable and start
systemctl --user enable --now airpods-daemon.service
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
| `Model` | `s` | Device model name |
| `Firmware` | `s` | Firmware version |
| `EqPreset` | `s` | Active EQ preset name |

### Methods

| Method | Args | Description |
|--------|------|-------------|
| `SetAncMode` | `(s)` | Set ANC mode |
| `SetConversationalAwareness` | `(b)` | Toggle CA |
| `SetAdaptiveNoiseLevel` | `(y)` | Set noise level 0-100 |
| `SetOneBudAnc` | `(b)` | Toggle one-bud ANC |
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

If you have AirPods hardware and want to help expand protocol support (new models, new commands), packet captures are extremely valuable -- open an issue to coordinate.

## License

MIT
