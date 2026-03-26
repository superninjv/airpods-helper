# airpods-helper

Native Apple AirPods support for Linux — ANC control, transparency mode, battery levels, ear detection, parametric EQ, and more.

A lightweight Rust daemon communicates with AirPods over Bluetooth L2CAP using the Apple Accessory Protocol (AAP), exposes everything via D-Bus, and ships with a CLI tool and optional GTK4 desktop widgets.

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

### Use

```bash
airpods-cli status                    # full status display
airpods-cli battery                   # battery levels
airpods-cli anc noise                 # switch to noise cancellation
airpods-cli anc transparency          # switch to transparency
airpods-cli ca on                     # enable conversational awareness
airpods-cli eq bass-boost             # apply EQ preset
airpods-cli eq list                   # list available presets
airpods-cli eq off                    # disable EQ
airpods-cli reconnect                 # trigger reconnect
airpods-cli status --json             # JSON output for scripting
```

## Architecture

```
┌─────────────────────────┐    D-Bus (org.costa.AirPods)    ┌──────────────────┐
│   daemon (Rust)         │ ◄──────────────────────────────► │  CLI / Widgets   │
│                         │   Properties + Signals + Methods │                  │
│  BlueZ ← BT adapter    │                                  │  airpods-cli     │
│  L2CAP ← AAP protocol  │                                  │  AirPodsBattery  │
│  State ← watch channels │                                  │  AirPodsPopup    │
│  EQ    ← PipeWire       │                                  └──────────────────┘
│  MPRIS ← ear detection  │
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

## License

MIT
