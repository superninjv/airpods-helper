# airpods-windows

Windows port of [airpods-helper](../README.md). Provides the same AirPods control
features (battery, ANC, ear detection, conversational awareness) using Windows-native
Bluetooth APIs instead of BlueZ/D-Bus.

## Architecture

```
  BLE scan (btleplug/WinRT)  -->  discover AirPods address
  L2CAP (Winsock AF_BTH)     -->  AAP protocol (shared with Linux)
  HTTP API (axum)             -->  localhost:7654 (replaces D-Bus)
  CLI subcommands             -->  talk to HTTP API
```

## Requirements

- Windows 10 version 1809+ (Windows 11 recommended)
- Rust toolchain (rustup.rs)
- Windows SDK (installed with Visual Studio Build Tools or full VS)
- AirPods paired in Windows Bluetooth settings

## Build

```powershell
cd windows
cargo build --release
```

The binary is at `target\release\airpods-windows.exe`.

## Usage

### Start the daemon

```powershell
airpods-windows daemon
```

This will:
1. Scan for paired AirPods via BLE
2. Establish an L2CAP connection for AAP protocol
3. Start an HTTP API on `http://127.0.0.1:7654`
4. Reconnect automatically if the connection drops

### CLI commands

All CLI commands talk to the running daemon via HTTP.

```powershell
# Full device status
airpods-windows status
airpods-windows status --json

# Battery levels
airpods-windows battery
airpods-windows battery --json

# ANC mode (off, noise, transparency, adaptive)
airpods-windows anc              # show current
airpods-windows anc adaptive     # set mode

# Conversational awareness
airpods-windows ca               # show current
airpods-windows ca on            # enable
airpods-windows ca off           # disable

# Adaptive noise level (0-100, only in adaptive ANC mode)
airpods-windows noise            # show current
airpods-windows noise 75         # set level

# One-bud ANC
airpods-windows one-bud          # show current
airpods-windows one-bud on       # enable
```

### HTTP API

The daemon exposes a REST API on `http://127.0.0.1:7654`:

| Method | Path            | Description                    |
|--------|-----------------|--------------------------------|
| GET    | `/status`       | Full device state (JSON)       |
| GET    | `/battery`      | Battery levels + charging      |
| POST   | `/anc`          | Set ANC mode                   |
| POST   | `/ca`           | Set conversational awareness   |
| POST   | `/noise`        | Set adaptive noise level       |
| POST   | `/one-bud-anc`  | Set one-bud ANC                |

POST bodies are JSON, e.g.:
```json
{"mode": "adaptive"}
{"enabled": true}
{"level": 75}
```

## Known Limitations

1. **L2CAP via Winsock**: Uses `AF_BTH + BTHPROTO_L2CAP` which provides stream-oriented
   sockets. If packet boundary issues arise, a KMDF L2CAP bridge driver may be needed
   (see `NOTES.md`).

2. **No EQ support**: PipeWire EQ is Linux-specific. Windows audio equalization would
   require a different approach (e.g., Windows Audio Processing Objects).

3. **No MPRIS**: Media player pause/resume on ear detection is Linux-specific. Windows
   equivalent would use the System Media Transport Controls API.

4. **Pairing required**: AirPods must be paired in Windows Bluetooth settings before the
   daemon can connect.

5. **Single instance**: Only one daemon instance should run at a time (port 7654 conflict).

## Comparison with Linux Version

| Feature                | Linux (daemon/)          | Windows (windows/)       |
|------------------------|--------------------------|--------------------------|
| BLE discovery          | BlueZ D-Bus (bluer)      | btleplug (WinRT)         |
| L2CAP transport        | BlueZ L2CAP SeqPacket    | Winsock BTHPROTO_L2CAP   |
| AAP protocol           | Shared (identical)       | Shared (identical)       |
| IPC                    | D-Bus                    | HTTP localhost:7654      |
| EQ                     | PipeWire filter chains   | Not implemented          |
| Media control          | MPRIS                    | Not implemented          |
| Widget                 | AGS/GTK4                 | Not implemented          |
