# KDE Plasma widget

A native Qt 6/Kirigami dock widget using the existing `org.costa.AirPods` D-Bus
API. Plasma supplies the theme, popup frame, dock attachment, resizing and open
indicator; KWin supplies blur and slide effects when enabled. The widget
activates the installed daemon on first use, and the daemon keeps running when
the popup closes. It does not open another Bluetooth connection.

The Controls, Devices and Settings tabs include battery/charging and ear
status, supported listening modes, adaptive level, conversational awareness,
one-earbud ANC, volume swipe, microphone controls, EQ presets and a 16-band
editor, pairing, reconnection preferences and About details. Controls follow
the connected model's capabilities. Renaming changes the BlueZ alias on this
computer, not the device name on other computers. Update the daemon as well as
the widget to enable the new rename method.

The icon's battery ring starts at the bottom and averages reported left/right
earbud levels, excluding the case. The popup initially fits battery/listening
controls, or the EQ card when disconnected. User resizing takes precedence;
content scrolls within that size. Plasma handles the native pin and dismissal.

## Build and install

Build against the host Qt/Plasma libraries. On Arch, install dependencies:

```bash
sudo pacman -S --needed cmake extra-cmake-modules ninja qt6-base qt6-declarative qt6-svg qt6-wayland \
  kirigami kstatusnotifieritem kdbusaddons kcolorscheme kconfig libplasma layer-shell-qt
```

From the repository root, with the daemon already installed:

```bash
cmake -S kde -B kde/build -G Ninja -DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_PREFIX=/usr
cmake --build kde/build --parallel 4
ctest --test-dir kde/build --output-on-failure
sudo cmake --install kde/build --prefix /usr --component PlasmaWidget
systemctl --user restart plasma-plasmashell.service
```

Enter panel **Edit Mode → Add Widgets**, find **AirPods Helper**, and place it
directly on the dock beside the system tray. Direct placement avoids the tray
container's shared title bar. No separate GUI autostart entry is needed.
Disable any older AirPods Helper tray/autostart entry to avoid duplicate icons.

If the widget is missing, check:

```bash
kpackagetool6 --type Plasma/Applet --list --global
```

The list should include `org.costa.airpods`. The plugin install directory
defaults to Arch's `lib/qt6/plugins/plasma/applets`; other distributions should
set `AIRPODS_PLASMA_PLUGIN_DIR` to their Qt plugin directory plus
`/plasma/applets` at configure time. Reload Plasma after installing updates.

## Standalone development client

`./kde/build/airpods-helper-kde` runs the shared UI as a standalone tray panel.
It supports native edge placement, resizing and Keep Open, but the Plasma
widget is recommended for complete dock integration. To install this optional
client, use `cmake --install kde/build --prefix ~/.local` without the widget
component. It uses the installed daemon.

## Distribution

Ship one package per interface, sharing the daemon and D-Bus API:

- **KDE:** `packaging/PKGBUILD-kde` packages the widget, daemon, CLI and activation
  files for Arch. From `packaging/`, build with `makepkg -p PKGBUILD-kde -s` and
  install the resulting package with pacman. It replaces the base daemon
  package. Updates belong in the AUR or a signed pacman repository; neither
  channel has been published yet.
- **Other desktops:** the [Tauri AppImage](../packaging/appimage/README.md)
  bundles the GUI, daemon, CLI and installer, without Qt/KDE libraries.

Before publishing the Arch recipe, target a release containing `kde/` and pin
its checksums; the existing `v0.3.0` archive predates this widget. Native packages
must match the target distribution's Qt/Plasma stack. Debian/RPM recipes remain
to be added. A KDE Store `.plasmoid` alone cannot install the C++ plugin and
daemon; see [KDE's packaging guidance](https://community.kde.org/Plasma/DeveloperGuide).

## Tests and screenshots

Tests cover theme changes, navigation, EQ editing, battery rendering, popup
placement and dismissal. D-Bus integration uses a private bus with activation
disabled. They do not pair devices or change a running daemon. See the
[screenshot instructions](../docs/screenshots/README.md) for reproducible light
and dark captures and the optional error-preview switches.
