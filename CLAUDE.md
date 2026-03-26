# airpods-helper

Native Apple AirPods support for Linux. Rust daemon + AGS/GTK4 widgets.

## Architecture

```
┌─────────────────────────┐    D-Bus (org.costa.AirPods)    ┌──────────────────┐
│   daemon/ (Rust)        │ ◄──────────────────────────────► │  widget/ (TS)    │
│                         │   Properties + Signals + Methods │                  │
│  BlueZ ← BT adapter    │                                  │  AirPodsBattery  │
│  L2CAP ← AAP protocol  │                                  │  AirPodsPopup    │
│  State ← watch channels │                                  │  AirPodsService  │
│  EQ    ← PipeWire       │                                  └──────────────────┘
│  MPRIS ← ear detection  │
└─────────────────────────┘
```

### Daemon (`daemon/`)
Rust binary that speaks Apple Accessory Protocol (AAP) over L2CAP to AirPods, exposing state via D-Bus.

- **`main.rs`** — tokio event loop: BlueZ events, AAP events, D-Bus commands, EQ commands, reconnect
- **`aap/mod.rs`** — AAP constants, enums (AncMode, BatteryComponent, EarStatus), PSM 0x1001
- **`aap/parser.rs`** — parses raw AAP packets into typed events
- **`aap/commands.rs`** — builds AAP command packets (handshake, ANC, CA, etc.)
- **`bluez.rs`** — BlueZ adapter monitor, device detection (UUID + name fallback), connect helper
- **`l2cap.rs`** — L2CAP connection, handshake, read/write loop, applies events to state
- **`dbus.rs`** — zbus service: properties, methods (SetAncMode, SetEqPreset, etc.), signals
- **`state.rs`** — `SharedState` via `tokio::sync::watch`, used by all subsystems
- **`eq.rs`** — PipeWire parametric EQ via filter-chain config drop-in (`99-airpods-eq.conf`)
- **`mpris.rs`** — pause/resume media players on ear removal/insertion
- **`config.rs`** — TOML config from `~/.config/airpods-helper/config.toml`

### Widgets (`widget/`)
AGS (Astal GTK Shell) GTK4 widgets consumed by the Costa OS bar. Pure D-Bus clients.

- **`AirPodsBattery.tsx`** — bar button + popover (battery, ANC, CA, EQ, ear status). Self-contained D-Bus proxy, no external deps besides AGS/GTK4/GLib
- **`AirPodsPopup.tsx`** — layer-shell popup on device connect/disconnect (uses `gnim` state from AirPodsService)
- **`AirPodsService.ts`** — standalone D-Bus proxy with `gnim` reactive state (used by AirPodsPopup)
- **`index.ts`** — barrel exports

**Note:** `AirPodsBattery.tsx` has its own inline D-Bus proxy (no `gnim` dep). `AirPodsPopup.tsx` uses `AirPodsService.ts` which depends on `gnim`. These are two separate D-Bus client patterns that coexist.

### EQ Presets (`eq-presets/`)
TOML files defining parametric EQ bands. Installed to `~/.config/airpods-helper/eq/`.

## Build & Install

```bash
# Build daemon (requires Rust toolchain)
make daemon
# or: cd daemon && cargo build --release

# Install everything (daemon binary, systemd service, EQ presets, widget symlink)
make install

# Post-install: grant raw socket capability and enable service
sudo setcap 'cap_net_raw,cap_net_admin+eip' ~/.local/bin/airpods-daemon
systemctl --user enable --now airpods-daemon.service
```

## Config

`~/.config/airpods-helper/config.toml`:
- `[device]` — optional MAC address pin, name
- `[eq]` — active preset name, auto-load on connect
- `[ear_detection]` — pause/resume media on removal
- `[reconnect]` — auto-reconnect with backoff, max retries

## D-Bus Interface (`org.costa.AirPods`)

**Properties:** Connected, BatteryLeft/Right/Case, ChargingLeft/Right/Case, AncMode, EarLeft/Right, ConversationalAwareness, AdaptiveNoiseLevel, OneBudAnc, Model, Firmware, EqPreset

**Methods:** SetAncMode(s), SetConversationalAwareness(b), SetAdaptiveNoiseLevel(y), SetOneBudAnc(b), SetEqPreset(s), DisableEq(), ListEqPresets(), Reconnect()

**Signals:** DeviceConnected(s), DeviceDisconnected(), EarDetectionChanged(bb)

## Dependencies

### Daemon (Rust)
- `bluer` — BlueZ D-Bus bindings (L2CAP, device discovery)
- `zbus` — D-Bus service (async, tokio)
- `tokio` — async runtime
- `serde` + `toml` — config parsing
- `tracing` — structured logging

### Widget (TypeScript)
- AGS (Astal GTK Shell) — GTK4 widget framework
- `gnim` — reactive state (used by AirPodsPopup/Service, NOT by AirPodsBattery)
- GLib/Gio introspection — D-Bus proxy

### System
- BlueZ (bluetoothd)
- PipeWire + WirePlumber (for EQ filter chains)
- `cap_net_raw,cap_net_admin` on the daemon binary (L2CAP raw sockets)

## Costa OS Integration

The widget files are symlinked into `~/.config/ags/widget/airpods/` by `make install`. Costa OS also carries copies in `costa-os/shell/widget/airpods/` — keep both in sync when making widget changes.
