# airpods-helper

AirPods support for Linux. A Rust daemon owns the AirPods session and exposes it on D-Bus; the CLI, Tauri app and AGS widgets are thin D-Bus clients. Keep the daemon lightweight: single-threaded tokio, no polling where an event exists, minimal dependencies.

## Layout

- **`aap/`** — protocol crate (no I/O): `parser.rs` (packets → `AapEvent`), `commands.rs` (packet builders), `models.rs` (model number → name/features), `buds.rs` (`BudTracker`: maps primary/secondary ear reports to left/right — the primary is the first bud in battery packets and swaps). Shared by `daemon/` and `windows/`.
- **`daemon/src/`**
  - `main.rs` — event loop and session lifecycle: BlueZ events, AAP session start/end (tagged with a session id), reconnect policy, EQ selection, SIGTERM/SIGINT shutdown.
  - `bluez.rs` — watches device `Connected` properties (never runs discovery except for explicit pair/scan — continuous discovery makes A2DP stutter), pair, quick-pair scan (Continuity proximity records).
  - `l2cap.rs` — AAP handshake + read/write loop; applies events to state.
  - `state.rs` — `SharedState` (tokio watch). `reset()` keeps EQ fields.
  - `dbus.rs` — `org.costa.AirPods`. **All `PropertiesChanged` come from `run_property_notifier`, which diffs state snapshots** — never emit property changes by hand. Settings are writable properties persisted via `config::update_config`.
  - `config.rs` — `SharedConfig` (RwLock); `save()` uses toml_edit so user comments survive.
  - `mpris.rs` — pause on bud removal, resume only what we paused.
  - `eq/` — `preset.rs` (load/validate/save; built-ins are `include_str!`'d from `eq-presets/`), `dsp.rs` (RBJ biquads), `pipewire.rs` (filter-chain in a supervised `pipewire -c` child; smart filter on WirePlumber ≥ 0.5, else pinned target + default-sink redirect), `pulse.rs` (null sink → parec → biquads → pacat), `mod.rs` (`EqManager`: backend detection, status, restore on stop).
- **`cli/`** — `airpods-cli`; reads state with one `GetAll`.
- **`app/`** — Tauri app, D-Bus client of the daemon.
- **`widget/`** — AGS/GTK4 widgets. Costa OS carries copies in `costa-os/shell/widget/airpods/`; keep them in sync.
- **`windows/`** — experimental; excluded from the workspace. Check with `cargo clippy --target x86_64-pc-windows-gnu` from `windows/`.

The D-Bus contract is `docs/dbus-api.md`; update it with any interface change.

## Build & test

```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p airpods-daemon -- --ignored --test-threads=1   # live PipeWire/Pulse routing tests (no audio played)
```

Running the daemon locally: `XDG_CONFIG_HOME=/tmp/x ./target/debug/airpods-daemon` (keeps your real config untouched). It needs `cap_net_raw,cap_net_admin` for L2CAP.

Never play audio in tests; verify routing structurally (`pw-link -l`, `pactl list short …`).

## Protocol notes

- L2CAP PSM 0x1001 (BR/EDR). Handshake → `SET_FEATURES` (host caps `0xFF`, needed for Adaptive/CA during playback) → subscribe → `ENABLE_ALL_LISTENING_MODES` (re-sent before switching to Off, which iCloud-synced settings can disable).
- Battery entries flagged disconnected carry stale levels → exposed as `-1`. AirPods Max report one `0x01` (headphones) component, mirrored into left/right.
- Device-info strings are positional (empty fields included).
- Stereo + mic: AirPods can stream the mic as AAC-ELD over AAP opcode `0x58` while A2DP keeps playing (LibrePods PR #655). Not implemented here yet — it's the next big feature. Opcodes `0x30`/`0x31` are BLE advertisement key requests, not an LE Audio gate.
- Sub-command table and sources: `aap/src/lib.rs`, LibrePods docs.
