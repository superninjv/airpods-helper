# Windows Testing Checklist

Testing guide for the Windows port of airpods-helper. Use this when
running the standalone `airpods-windows` binary or the Tauri app on
a Windows VM with Bluetooth passthrough.

## VM Setup

- [ ] Windows 10 1809+ or Windows 11 VM
- [ ] Bluetooth adapter passed through to VM (USB passthrough recommended)
- [ ] Rust toolchain installed (`rustup-init.exe` from rustup.rs)
- [ ] Visual Studio Build Tools with "Desktop development with C++"
- [ ] AirPods paired in Windows Bluetooth settings

## Build Verification

- [ ] `cd windows && cargo build --release` compiles without errors
- [ ] `cargo test` passes all unit tests
- [ ] `cargo clippy -- -D warnings` reports no warnings
- [ ] Binary produced at `target\release\airpods-windows.exe`

## BLE Discovery (btleplug / WinRT)

- [ ] `airpods-windows daemon` starts without panic
- [ ] BLE scan finds paired AirPods by service UUID
- [ ] BLE scan finds AirPods by Apple manufacturer data (0x004C) + name fallback
- [ ] BLE scan timeout works (30s, reports "no AirPods found")
- [ ] Bluetooth adapter not present: clean error message

## L2CAP Connection (Winsock AF_BTH)

This is the most uncertain part. Winsock Bluetooth L2CAP support is
documented but rarely used.

- [ ] `socket(AF_BTH, SOCK_STREAM, BTHPROTO_L2CAP)` succeeds
- [ ] `connect()` to PSM 0x1001 succeeds (may require AirPods to be paired and connected)
- [ ] Handshake packet sent and ACK received
- [ ] Feature enable packet sent and ACK received
- [ ] Notification subscribe sent
- [ ] If Winsock L2CAP fails: document the Windows error code for investigation

### Stream Framing Concern

Winsock L2CAP uses `SOCK_STREAM` (stream-oriented), while the Linux daemon
uses `SeqPacket` (message-oriented). AAP packets are small (typically <100
bytes) and are self-framing with a 4-byte header, so stream mode should work.
However, if packet boundaries are lost:

- [ ] Verify recv() returns complete AAP packets (not split or merged)
- [ ] If packets are split: need to implement length-prefixed framing
- [ ] If packets are merged: need to implement packet splitting at header boundaries

## AAP Protocol

- [ ] Battery updates received (left/right/case)
- [ ] Ear detection events received
- [ ] ANC mode changes received
- [ ] Device info (model + firmware) parsed correctly
- [ ] Conversational awareness state received
- [ ] Adaptive noise level received
- [ ] Volume swipe state received
- [ ] Audio source notifications received

## HTTP API

- [ ] `GET /status` returns full JSON state
- [ ] `GET /battery` returns battery levels
- [ ] `POST /anc` with `{"mode": "adaptive"}` changes ANC mode
- [ ] `POST /ca` with `{"enabled": true}` toggles CA
- [ ] `POST /noise` with `{"level": 75}` sets adaptive noise level
- [ ] `POST /one-bud-anc` with `{"enabled": false}` toggles one-bud ANC
- [ ] `POST /volume-swipe` with `{"enabled": true}` toggles volume swipe
- [ ] Error responses: 400 for bad input, 503 when not connected

## CLI Commands

- [ ] `airpods-windows status` shows formatted device info
- [ ] `airpods-windows status --json` outputs valid JSON
- [ ] `airpods-windows battery` shows battery bars
- [ ] `airpods-windows anc` shows current mode
- [ ] `airpods-windows anc adaptive` sets mode
- [ ] `airpods-windows ca on` enables CA
- [ ] `airpods-windows noise 75` sets adaptive noise level
- [ ] `airpods-windows one-bud on` enables one-bud ANC
- [ ] `airpods-windows volume-swipe off` disables volume swipe
- [ ] CLI shows helpful error when daemon is not running

## Reconnection

- [ ] Daemon reconnects when AirPods disconnect (5s delay)
- [ ] Daemon reconnects when AirPods come back in range
- [ ] Ctrl+C cleanly shuts down daemon (Winsock cleanup)

## Tauri App (if testing the desktop app)

- [ ] `cd app && npm install && npm run tauri build` produces .msi/.exe installer
- [ ] App launches with system tray icon
- [ ] Tray icon click toggles main window
- [ ] Tray menu shows ANC mode options
- [ ] Frontend shows battery, ANC, ear detection, features
- [ ] Feature toggles forward commands via HTTP API to standalone daemon
- [ ] App correctly shows "Disconnected" when daemon is not running

## Known Limitations (not bugs, expected behavior)

- No EQ support (PipeWire is Linux-only, no Windows audio APO equivalent yet)
- No MPRIS media control (Linux-only, Windows SMTC not implemented yet)
- AirPods must be paired in Windows Bluetooth settings before daemon can connect
- Only one daemon instance at a time (port 7654)
- Tauri app on Windows is a client to the standalone daemon (does not embed L2CAP)

## If Winsock L2CAP Fails

If `AF_BTH + BTHPROTO_L2CAP` cannot connect to PSM 0x1001, the fallback
approach is a KMDF kernel driver (see `NOTES.md`). Document:

- [ ] Windows error code from `connect()` failure
- [ ] Whether the PSM appears in SDP (`bthprops.cpl` Advanced settings)
- [ ] Whether Windows Bluetooth logs show the L2CAP request (Event Viewer >
      Microsoft-Windows-Bluetooth-BthLEPrepairing)
