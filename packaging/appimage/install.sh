#!/bin/bash
set -euo pipefail

# Only the copied daemon gets capabilities; the GUI always runs as the user.
if [[ $(id -u) == 0 ]]; then
    echo 'Run this as your desktop user, not root.' >&2
    exit 1
fi
: "${APPDIR:?Run this through the AppImage with --install}"
: "${APPIMAGE:?Run this through the AppImage with --install}"
data_home="${XDG_DATA_HOME:-$HOME/.local/share}"
config_home="${XDG_CONFIG_HOME:-$HOME/.config}"
root="$data_home/airpods-helper"
# shortcut: old release directories are retained for rollback, prune when storage cleanup is needed.
service_file="$config_home/systemd/user/airpods-daemon.service"
dbus_file="$data_home/dbus-1/services/org.costa.AirPods.service"
desktop_file="$data_home/applications/org.costa.AirPodsHelper.desktop"
marker='# Managed by AirPods Helper AppImage'

for directory in "$data_home" "$config_home"; do
    if [[ "$directory" != /* || "$directory" == *$'\n'* || "$directory" == *$'\r'* ]]; then
        echo 'XDG data and config directories must be absolute paths without newlines.' >&2
        exit 1
    fi
done
for tool in systemctl pkexec setcap getcap ldd; do
    command -v "$tool" >/dev/null || { echo "Missing $tool. Install systemd, polkit and libcap first." >&2; exit 1; }
done
for file in "$service_file" "$dbus_file" "$desktop_file"; do
    if [[ -e "$file" ]] && ! grep -Fxq "$marker" "$file"; then
        echo "Refusing to replace an existing unmanaged file: $file" >&2
        echo 'Back it up and move it aside before installing the AppImage.' >&2
        exit 1
    fi
done

version=$(cat "$APPDIR/usr/share/airpods-helper/version")
if [[ ! "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+([.+-][a-zA-Z0-9.-]+)?$ ]]; then
    echo 'Invalid bundled version.' >&2
    exit 1
fi
mkdir -p "$root/releases"
staged=$(mktemp -d "$root/releases/$version.XXXXXX")
link="$root/.current-$$"
trap 'rm -f -- "$link"; if [[ -n "$staged" ]]; then rm -rf -- "$staged"; fi' EXIT
install -m755 "$APPIMAGE" "$staged/AirPods_Helper.AppImage"
install -m755 "$APPDIR/usr/bin/airpods-daemon" "$APPDIR/usr/bin/airpods-cli" "$staged/"

# The capability-enabled daemon uses host libraries (the loader ignores LD_LIBRARY_PATH).
libraries=$(env -u LD_LIBRARY_PATH -u LD_PRELOAD ldd "$staged/airpods-daemon" 2>&1) || {
    echo "Could not check the bundled daemon's host libraries: $libraries" >&2
    exit 1
}
if [[ "$libraries" == *'not found'* ]]; then
    echo "Install the missing daemon runtime libraries first: $libraries" >&2
    exit 1
fi
pkexec "$(command -v setcap)" 'cap_net_raw,cap_net_admin+eip' "$staged/airpods-daemon"
caps=$(getcap "$staged/airpods-daemon")
if [[ "$caps" != *cap_net_admin* || "$caps" != *cap_net_raw* || "$caps" != *'=eip'* ]]; then
    echo 'Could not grant Bluetooth capabilities; installation was not activated.' >&2
    exit 1
fi

# Quote separately: systemd and desktop entries have different expansion rules.
systemd_path=${root//\\/\\\\}
systemd_path=${systemd_path//\"/\\\"}
systemd_path=${systemd_path//%/%%}
systemd_path=${systemd_path//\$/\$\$}
desktop_path=${root//\\/\\\\}
desktop_path=${desktop_path//\"/\\\"}
desktop_path=${desktop_path//\`/\\\`}
desktop_path=${desktop_path//\$/\\\$}
desktop_path=${desktop_path//%/%%}

cat > "$staged/airpods-daemon.service" <<EOF
$marker
[Unit]
Description=AirPods Helper Daemon (AppImage)
After=bluetooth.target dbus.service
[Service]
Type=simple
ExecStart="$systemd_path/current/airpods-daemon"
Restart=on-failure
RestartSec=5
[Install]
WantedBy=default.target
EOF
cat > "$staged/org.costa.AirPods.service" <<EOF
$marker
[D-BUS Service]
Name=org.costa.AirPods
Exec="$desktop_path/current/airpods-daemon"
SystemdService=airpods-daemon.service
EOF
cat > "$staged/org.costa.AirPodsHelper.desktop" <<EOF
$marker
[Desktop Entry]
Type=Application
Name=AirPods Helper
Comment=AirPods battery and noise control
Exec="$desktop_path/current/AirPods_Helper.AppImage"
Icon=org.costa.AirPodsHelper
Categories=AudioVideo;Audio;Utility;
Terminal=false
StartupWMClass=airpods-app
EOF

ln -s "$staged" "$link"
mv -Tf "$link" "$root/current"
release="$staged"
staged=''
install -Dm644 "$release/airpods-daemon.service" "$service_file"
install -Dm644 "$release/org.costa.AirPods.service" "$dbus_file"
install -Dm644 "$release/org.costa.AirPodsHelper.desktop" "$desktop_file"
install -Dm644 "$APPDIR/usr/share/icons/hicolor/scalable/apps/org.costa.AirPodsHelper.svg" \
    "$data_home/icons/hicolor/scalable/apps/org.costa.AirPodsHelper.svg"
systemctl --user daemon-reload
systemctl --user enable airpods-daemon.service
systemctl --user restart airpods-daemon.service
printf '%s\n' "$version" > "$release/appimage-version"
echo "Installed GUI, daemon and CLI $version in $root/current"
echo 'Launch AirPods Helper from the application menu.'
