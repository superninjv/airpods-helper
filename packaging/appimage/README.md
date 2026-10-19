# General Linux AppImage

The x86_64 AppImage bundles the Tauri GUI, daemon and CLI. KDE's native widget
uses a [separate distro package](../../kde/README.md#distribution).

## Run and install

Quit any older Tauri instance, then launch the local build:

```bash
./packaging/appimage-build/AirPods_Helper-x86_64.AppImage
```

The resizable panel has Controls, Devices and Settings tabs. It hides on focus
loss unless **Keep Open** is pinned. Escape or the hide button hides it;
**Open panel** in the tray menu reopens it, and **Quit** exits. Drag an edge to
resize or the title to move it. Wayland controls standalone panel placement;
use the KDE widget for Plasma's dock anchoring and popup effects.

Launching alone does not install services. Choose **Install / update** in the
app, or run as your normal desktop user:

```bash
./packaging/appimage-build/AirPods_Helper-x86_64.AppImage --install
```

Polkit grants Bluetooth capabilities to a copied daemon; the GUI stays
unprivileged. Installation adds the launcher, CLI, user systemd service and
D-Bus activation entry. Releases live in `~/.local/share/airpods-helper/releases/`;
`current` selects the active version. Settings and EQ presets are preserved.
Failed authentication leaves the previous release active. Unmanaged user
launcher/service files are refused rather than overwritten; the error names
the files to review. A user service takes precedence over distro service files.

Append `--minimized` to start in the tray. Without FUSE, use
`--appimage-extract-and-run` when launching. Both UIs can use the same daemon;
update the daemon too when installing features such as device renaming.

The host needs BlueZ, systemd, polkit with a desktop authentication agent,
libcap (`setcap`/`getcap`), libdbus and compatible graphics/system libraries.
PipeWire/PulseAudio and libfdk-aac are optional as described in the main README.
GNOME needs an AppIndicator extension for the tray.

## Updates

Publish `AirPods_Helper-x86_64.AppImage` with matching `.zsync` metadata and a
`.sha256` checksum. The image embeds the stable upstream update channel:

```bash
appimageupdatetool ~/.local/share/airpods-helper/current/AirPods_Helper.AppImage
```

Quit the old GUI, open the updater's output file, and choose **Install / update**
to deploy the matching daemon/CLI and restart the service. Updating the image
alone does not update the running daemon. Release downloads use the same flow;
unpublished draft releases are not offered as updates. **Releases and updates**
opens the release page; it does not install updates itself.

## Build and release

Install Rust, GTK 3, WebKitGTK 4.1, Ayatana AppIndicator, libdbus and libcap build
dependencies, plus bubblewrap and xdg-dbus-proxy. Put `linuxdeploy` and Tauri's
`linuxdeploy-plugin-gtk.sh` on `PATH` as in `.github/workflows/appimage.yml`, then:

```bash
bash packaging/build-appimage.sh
python3 packaging/appimage/test_install.py
node --test app/tests/*.test.cjs
```

Cargo embeds the static frontend; no Node build step or Tauri CLI is required.
The script runs Tauri tests and bundles GTK/WebKit, AppIndicator and the
installer. For offline builds, set `LDAI_RUNTIME_FILE` to a cached runtime.

The workflow builds on Ubuntu 22.04, checks the installer, smoke-tests the GUI
under Xvfb without daemon activation, and uploads the image, update metadata
and checksum. Align Cargo/Tauri/package versions before tagging a release.
Tagged builds create a draft for maintainer review and publication. Clean-distro
installation and upgrades still need release testing; builds on newer distros
can require newer host libraries. Forks must change `LDAI_UPDATE_INFORMATION`
before publishing their own update channel.
