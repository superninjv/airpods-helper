# D-Bus API — `org.costa.AirPods`

**Bus:** session · **Service:** `org.costa.AirPods` · **Path:** `/org/costa/AirPods` · **Interface:** `org.costa.AirPods`

The daemon is the single owner of the AirPods L2CAP/AAP session. Every UI (CLI,
Tauri app, AGS widget) is a thin client of this interface. The service file
`org.costa.AirPods.service` makes the bus auto-start the daemon on first use.

All properties emit `org.freedesktop.DBus.Properties.PropertiesChanged` when
they change, so clients should subscribe instead of polling.

## Device properties (read-only)

| Property | Type | Notes |
|---|---|---|
| `Connected` | `b` | AAP control session up (handshake exchanged). Controls are rejected with `not connected` until this is true. |
| `Address` | `s` | MAC of the connected AirPods, `""` when disconnected |
| `Model` | `s` | Apple model number, e.g. `A2698` |
| `ModelName` | `s` | e.g. `AirPods Pro 2 (Lightning)` |
| `Firmware` | `s` | |
| `Features` | `as` | Subset of: `anc`, `adaptive`, `ca`, `one_bud_anc`, `headphones`. `headphones` = over-ear (AirPods Max): a single battery (mirrored in `BatteryLeft`/`BatteryRight`), no case, no per-bud ear status. Empty while connecting. |
| `BatteryLeft` / `BatteryRight` / `BatteryCase` | `i` | 0–100 (clamped), `-1` = unknown / not reported (e.g. bud in closed case) |
| `ChargingLeft` / `ChargingRight` / `ChargingCase` | `b` | |
| `EarLeft` / `EarRight` | `b` | In-ear. Correctly follows primary-bud swaps. |
| `AncMode` | `s` | `off`, `noise`, `transparency`, `adaptive` |
| `AdaptiveNoiseLevel` | `y` | 0–100 |
| `ConversationalAwareness` | `b` | |
| `ConversationalActivityState` | `s` | `normal`, `speaking`, `stopped` |
| `OneBudAnc` | `b` | |
| `VolumeSwipe` | `b` | |
| `AdaptiveVolume` | `b` | as reported by the firmware |
| `ChimeVolume` | `y` | as reported by the firmware |
| `AudioSource` | `s` | `none`, `call`, `media`, `unknown` |
| `MicMode` | `s` | `auto`, `left`, `right` — reported by the firmware when it sends it, otherwise the last value a client set |
| `Version` | `s` | daemon version |

## EQ properties (read-only)

| Property | Type | Notes |
|---|---|---|
| `EqPreset` | `s` | **Preset id** (file stem, e.g. `bass-boost`) of the selected preset; `""` = EQ off. Persisted in config. |
| `EqStatus` | `s` | `off` · `active` (filter running in front of the AirPods) · `waiting` (preset selected, AirPods audio sink not available yet) · `error` · `unsupported` (no usable audio server) |
| `EqError` | `s` | Human-readable reason when `EqStatus` is `error`/`unsupported`, else `""` |
| `EqBackend` | `s` | `pipewire` (WirePlumber ≥ 0.5 smart filter — transparent, default sink untouched), `pipewire-legacy` (older WirePlumber — EQ sink becomes the default output while active), `pulseaudio`, or `none` |

## Microphone source (read-only)

While the AAP session is up and `MicSource` is on, the daemon offers an audio source named `airpods_mic` ("AirPods Microphone") that carries the buds' microphone over AAP (AAC-ELD, opcode `0x58`) while A2DP keeps playing. The buds only stream while at least one stream records from the source; peak-detect streams (volume meters) and corked streams don't count.

| Property | Type | Notes |
|---|---|---|
| `MicStatus` | `s` | `off` (disabled or no AirPods session) · `unavailable` (no libfdk-aac or no PulseAudio-compatible server) · `idle` (source offered, nobody recording) · `starting` (asked the buds to stream, no audio yet) · `streaming` · `error` (see `MicError`; clears when the last recorder leaves) |
| `MicError` | `s` | Human-readable reason for `unavailable`/`error`, else `""` |

## Settings (read-write properties, persisted to `config.toml`)

| Property | Type | Default |
|---|---|---|
| `PauseOnRemoval` | `b` | `true` — pause MPRIS media when a bud is removed |
| `ResumeOnInsert` | `b` | `true` — resume what we paused when the bud goes back in |
| `AutoReconnect` | `b` | `true` |
| `PreferredDevice` | `s` | `""` — MAC; when set, other AirPods are ignored |
| `EqAutoLoad` | `b` | `true` — apply `EqPreset` whenever the AirPods' Bluetooth link comes up (independent of the AAP control session) |
| `MicSource` | `b` | `true` — offer the `airpods_mic` source (`[mic] enabled`). Turning it off stops any stream and removes the source. |

Setting an invalid value (e.g. malformed MAC) returns `org.freedesktop.DBus.Error.InvalidArgs`. Changes made through D-Bus emit `PropertiesChanged`; hand edits to `config.toml` take effect on the daemon's next config write or restart and are not signalled. If `config.toml` doesn't parse, setters fail rather than overwrite it.

## Methods

### Controls (fail with `org.freedesktop.DBus.Error.Failed: not connected` when disconnected)
| Method | In | Out |
|---|---|---|
| `SetAncMode` | `s mode` | |
| `SetAdaptiveNoiseLevel` | `y level` (0–100) | |
| `SetConversationalAwareness` | `b` | |
| `SetOneBudAnc` | `b` | |
| `SetVolumeSwipe` | `b` | |
| `SetMicMode` | `s` (`auto`/`left`/`right`) | |

### Devices
| Method | In | Out | Notes |
|---|---|---|---|
| `ListPaired` | | `a(ss)` (mac, name) | |
| `ConnectTo` | `s mac` | | BlueZ connect; AAP follows automatically |
| `Disconnect` | | | Fails if nothing is connected. Suppresses auto-reconnect until the AirPods connect again. |
| `Pair` | `s mac` | | pair + trust; ~20s timeout |
| `QuickPairScan` | `u seconds` | `a(sssnb)` (mac, name, model, rssi, in_pair_mode) | |
| `Reconnect` | | | Fails if no AirPods have connected since the daemon started. |

### EQ
| Method | In | Out | Notes |
|---|---|---|---|
| `ListEqPresets` | | `as` | ids, sorted (kept for compatibility) |
| `GetEqPresets` | | `a(sssb)` | (id, name, description, user_editable) |
| `GetEqPreset` | `s id` | `s name, s description, d preamp_db, a(sddd) bands` | four out-args; bands are (type, freq_hz, q, gain_db), type ∈ `peaking`,`lowshelf`,`highshelf`,`lowpass`,`highpass`,`notch` |
| `SetEqPreset` | `s id` | | select + persist; applied now if connected (even with `EqAutoLoad` off), else on connect. Returns once the change has taken effect. |
| `DisableEq` | | | `EqPreset` becomes `""` |
| `SaveEqPreset` | `s id, s name, s description, d preamp, a(sddd) bands` | | writes `~/.config/airpods-helper/eq/<id>.toml`; id must match `[a-z0-9-]{1,48}`; if it's the active preset it is re-applied live. Limits: ≤ 16 bands, 20 ≤ freq ≤ 20000, 0.1 ≤ q ≤ 10, −24 ≤ gain ≤ 24, −24 ≤ preamp ≤ 12 |
| `DeleteEqPreset` | `s id` | | user presets only. If it was the active preset: a built-in with the same id (which the user copy overrode) becomes active again; otherwise EQ is disabled. |

## Signals
| Signal | Args |
|---|---|
| `DeviceConnected` | `s model_name` |
| `DeviceDisconnected` | |
| `EarDetectionChanged` | `b left, b right` |
