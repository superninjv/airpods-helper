# airpods-helper

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Latest Release](https://img.shields.io/github/v/release/superninjv/airpods-helper)](https://github.com/superninjv/airpods-helper/releases)

AirPods support for Linux: noise control, battery levels, ear detection, a parametric EQ, and a CLI. The background daemon is a single ~3 MB Rust binary that sits at about 6 MB of RAM and uses no measurable CPU when idle.

The daemon talks to the AirPods over Bluetooth using Apple's accessory protocol (AAP, L2CAP PSM 0x1001) and publishes everything on D-Bus. The CLI, the desktop app and the AGS bar widgets all use that D-Bus interface.

## Features

- **Listening modes:** Off, Noise Cancellation, Transparency and Adaptive, plus the adaptive noise level
- **Battery:** left, right and case, with charging state
- **Ear detection:** pauses media when you take a bud out and resumes it when you put it back (MPRIS). It only resumes a player it paused itself.
- **Parametric EQ** on PipeWire or PulseAudio. It has built-in presets and custom presets, and can import AutoEQ files.
- **Conversational Awareness**, one-bud ANC, volume swipe, and primary microphone selection
- **Microphone without losing stereo** (experimental): an "AirPods Microphone" source that streams over Apple's own channel, so music and calls stay in A2DP stereo. See [below](#microphone-and-stereo-audio-at-the-same-time).
- **Pairing:** scan for AirPods in pairing mode, pair, connect, disconnect, and reconnect automatically
- **Per-model controls:** the daemon detects the model and exposes only the controls it supports

## Supported devices

| Device | Model numbers | ANC | Adaptive | Conv. Awareness |
|---|---|:-:|:-:|:-:|
| AirPods (1st, 2nd gen) | A1523, A1722, A2031, A2032 | | | |
| AirPods (3rd gen) | A2564, A2565 | | | |
| AirPods 4 | A3050, A3053, A3054, A3058 | | | |
| AirPods 4 (ANC) | A3055, A3056, A3057, A3059 | ✓ | ✓ | ✓ |
| AirPods Pro | A2083, A2084, A2190 | ✓ | | |
| AirPods Pro 2 (Lightning / USB-C) | A2698, A2699, A2700, A2931, A2968, A3047, A3048, A3049 | ✓ | ✓ | ✓ |
| AirPods Pro 3 | A3063, A3064, A3065, A3122 | ✓ | ✓ | ✓ |
| AirPods Max (Lightning / USB-C) | A2096, A3184 | ✓ | | |

Battery, ear detection and EQ work on every model. Models not in this table get every control, and the firmware ignores any it doesn't support.

## Install

Every install path grants the daemon the `cap_net_raw,cap_net_admin` capability it needs to open the L2CAP socket. Once installed, run `airpods-cli doctor`.

**Arch Linux**
```bash
cd packaging && makepkg -si
systemctl --user enable --now airpods-daemon.service
```

**Debian / Ubuntu**
```bash
./packaging/build-deb.sh
sudo apt install ./packaging/airpods-helper_*_amd64.deb
systemctl --user enable --now airpods-daemon.service
```

**Prebuilt tarball (any distro):** download it from [Releases](https://github.com/superninjv/airpods-helper/releases), then:
```bash
tar xzf airpods-helper-*-x86_64-linux.tar.gz && cd airpods-helper-*/
./install.sh          # installs to ~/.local; set PREFIX=... to change
systemctl --user enable --now airpods-daemon.service
```

**From source.** This needs a Rust toolchain and the D-Bus development headers (`libdbus-1-dev` or `dbus-devel`).
```bash
cargo build --release
install -Dm755 -t ~/.local/bin target/release/airpods-daemon target/release/airpods-cli
sudo setcap 'cap_net_raw,cap_net_admin+eip' ~/.local/bin/airpods-daemon
install -Dm644 -t ~/.config/systemd/user daemon/airpods-daemon.service
install -Dm644 -t ~/.local/share/dbus-1/services daemon/org.costa.AirPods.service
systemctl --user enable --now airpods-daemon.service
```

Upgrading replaces the binary, which drops its capabilities. The packages and scripts above re-apply them for you. If you copy the binary by hand, run `setcap` again.

### Pairing

Open the case near the computer and hold the button until the light flashes white. Then:

```bash
airpods-cli scan            # lists AirPods in pairing mode
airpods-cli pair AA:BB:CC:DD:EE:FF
```

AirPods you've already paired with `bluetoothctl` or your desktop's Bluetooth settings are picked up automatically when they connect.

## Using the CLI

```bash
airpods-cli status              # everything at a glance (--json for scripts)
airpods-cli battery
airpods-cli anc noise           # off | noise | transparency | adaptive
airpods-cli noise 40            # adaptive noise level, 0-100
airpods-cli ca off              # conversational awareness
airpods-cli one-bud on          # ANC with a single bud in
airpods-cli swipe off           # volume swipe on the stem
airpods-cli mic left            # primary mic: auto | left | right

airpods-cli eq                  # list presets, show the active one and EQ status
airpods-cli eq bass-boost       # apply a preset
airpods-cli eq off

airpods-cli settings            # show settings
airpods-cli set pause-on-removal off
airpods-cli set preferred-device AA:BB:CC:DD:EE:FF

airpods-cli paired | connect <MAC> | disconnect | reconnect
airpods-cli doctor              # check the installation
```

Running a getter with no argument shows the current value, for example `airpods-cli anc`.

## Equalizer

The EQ only affects audio going to the AirPods. Other outputs are untouched. How it's applied depends on the audio server:

| Audio server | How | Notes |
|---|---|---|
| PipeWire + WirePlumber ≥ 0.5 | A filter-chain registered as a WirePlumber *smart filter* on the AirPods | Transparent: your default output doesn't change, and it survives switches to call mode. This is the preferred mode. |
| PipeWire + older WirePlumber | A filter-chain linked to the AirPods sink | "AirPods EQ" becomes the default output while the EQ is on, and the previous default is restored afterwards. |
| PulseAudio | A null sink, with the EQ applied by the daemon's own biquad filters and sent on to the AirPods | Needs `pactl`, `parec` and `pacat`. Adds about 30–50 ms of latency. |

`airpods-cli eq` and `airpods-cli doctor` show which backend is in use. To force one, set `backend` in the `[eq]` section of the config.

### Presets

The built-in presets are `flat`, `bass-boost`, `vocal-clarity` and `airpods-pro-crinacle`. Your own presets live in `~/.config/airpods-helper/eq/<id>.toml`. A user preset with the same id as a built-in overrides it.

```toml
name = "My preset"
description = "Warmer, less sibilant"
preamp = -4.0                 # dB; keep the peak at or below 0 dB to avoid clipping

[[bands]]
type = "lowshelf"             # peaking, lowshelf, highshelf, lowpass, highpass, notch
freq = 105.0                  # Hz, 20–20000
q = 0.7                       # 0.1–10
gain = 4.0                    # dB, ±24
```

To use an [AutoEQ](https://github.com/jaakkopasanen/AutoEq) profile for your AirPods, import its `ParametricEQ.txt`. Equalizer APO files work too.

```bash
airpods-cli eq import "AirPods Pro 2 ParametricEQ.txt" --name "Pro 2 (AutoEQ)" --apply
```

You can also create and edit presets in the desktop app, which draws the frequency response as you go.

## Desktop app and widgets

- **Desktop app** (`app/`): a Tauri tray app with a settings window. It covers every control, the EQ editor, pairing and the settings. It's a client of the daemon, so the daemon must be installed. To build it: `cd app/src-tauri && cargo build --release`.
- **AGS widgets** (`widget/`): a GTK4 bar button and popover for [AGS](https://github.com/Aylur/ags), plus a popup when AirPods connect.

  ```typescript
  import AirPodsBattery from "./airpods/AirPodsBattery"
  bar.append(AirPodsBattery())
  ```

  Link `widget/` into your AGS config: `ln -s "$PWD/widget" ~/.config/ags/widget/airpods`. Include `widget/style.css` in your stylesheet.
- **Anything else** can use the D-Bus interface, which is documented in [docs/dbus-api.md](docs/dbus-api.md). Properties emit `PropertiesChanged`, so there's no need to poll.

## Configuration

The config file is `~/.config/airpods-helper/config.toml`. Every setting can also be changed live with `airpods-cli set` or from the app, and the daemon keeps your comments when it writes the file. See [config.example.toml](config.example.toml) for the full list.

```toml
[device]
# address = "AA:BB:CC:DD:EE:FF"   # only use this pair; others are ignored

[eq]
active_preset = "bass-boost"      # "" = off
auto_load = true
backend = "auto"                  # auto | pipewire | pulseaudio

[ear_detection]
pause_media = true
resume_media = true

[reconnect]
auto_reconnect = true             # not after you disconnect on purpose
max_retries = 3

[mic]
enabled = true                    # offer the "AirPods Microphone" source
```

## Microphone and stereo audio at the same time

Normally, when an app opens a Bluetooth headset's microphone, the link drops from A2DP (stereo, high quality) to the headset profile (mono, 16 kHz). AirPods can avoid that: they can send the microphone as AAC-ELD over the same Apple control channel this project already uses, while A2DP keeps playing.

**Experimental.** While AirPods are connected, the daemon offers an **AirPods Microphone** source. Pick it in your app or system sound settings like any other mic. Audio servers remember the choice, so it sticks across reconnects. Music keeps playing in stereo the whole time the mic is in use; the two run side by side. To save battery, the buds only send mic audio while something is recording from the source: recording starts it, and it stops a few seconds after the last app lets go. Volume meters (pavucontrol, the desktop's sound settings) don't count as recording. Conversational Awareness is paused while the mic is live and restored afterwards.

- **Needs** `libfdk-aac`, loaded at runtime (Arch: `libfdk-aac`; Debian/Ubuntu: `libfdk-aac2` from non-free/multiverse), and PipeWire (with pipewire-pulse) or PulseAudio. Without the library, everything else works and `airpods-cli doctor` says what's missing.
- **Status:** `airpods-cli status` shows `Mic source` as `idle`, `starting`, `streaming`, `error` or `unavailable`, with the reason when something's wrong.
- **Turn it off:** `airpods-cli set mic-source off`.
- **Don't** pick the AirPods' own headset-profile input (`bluez_input…`). That one still switches the link to mono.
- **Tested on:** AirPods Pro 3 in [LibrePods PR #655](https://github.com/librepods-org/librepods/pull/655), where the protocol comes from. Here, the decode-to-source path has been checked with synthetic AAC-ELD; reports from real AirPods Pro 2, AirPods 4 and others are very welcome. If the buds don't answer, the status says so after a few seconds instead of hanging.

`airpods-cli mic left|right|auto` still picks which bud's microphone is primary.

## Troubleshooting

Start with `airpods-cli doctor`. It checks the capability, BlueZ, the daemon and the EQ backend, and prints a fix for anything that's wrong.

- **Logs:** `journalctl --user -u airpods-daemon -f`. For protocol-level detail, add `Environment=RUST_LOG=airpods_daemon=debug` with `systemctl --user edit airpods-daemon`.
- **"Permission denied" in the log:** the binary lost its capability, usually after a manual upgrade. Run `sudo setcap 'cap_net_raw,cap_net_admin+eip' $(command -v airpods-daemon)`.
- **EQ status is `waiting`:** the AirPods' audio output doesn't exist yet or right now, for example while connecting or during a call. The EQ comes back on its own.
- **An orphan "AirPods EQ" output after upgrading from 0.2.x:** old versions left a file in `~/.config/pipewire/pipewire.conf.d/`. The daemon now deletes it on startup, so restart PipeWire once to clear it (`systemctl --user restart pipewire`).

## Development

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p airpods-daemon -- --ignored --test-threads=1   # routing tests against your running PipeWire; plays no audio
```

| Crate | What it is |
|---|---|
| `aap/` | Protocol only: packet parser and builders, model table. |
| `daemon/` | BlueZ monitor, AAP session, D-Bus service, EQ backends, MPRIS. |
| `cli/` | `airpods-cli`. |
| `app/` | Tauri desktop app. |

Protocol details come from [LibrePods](https://github.com/librepods-org/librepods), whose research made this project possible. Captures from new models or firmware are always useful, so open an issue.

## License

MIT
