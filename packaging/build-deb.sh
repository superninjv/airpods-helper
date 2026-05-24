#!/bin/bash
set -euo pipefail

VERSION="0.2.2"
PKGNAME="airpods-helper"
ARCH="amd64"
STAGING="$PKGNAME-${VERSION}_${ARCH}"

cd "$(dirname "$0")/.."

# Build
cargo build --workspace --release

# Clean staging
rm -rf "packaging/$STAGING" "packaging/${STAGING}.deb"
mkdir -p "packaging/$STAGING/DEBIAN"
mkdir -p "packaging/$STAGING/usr/bin"
mkdir -p "packaging/$STAGING/usr/lib/systemd/user"
mkdir -p "packaging/$STAGING/usr/share/dbus-1/services"
mkdir -p "packaging/$STAGING/usr/share/airpods-helper/eq-presets"
mkdir -p "packaging/$STAGING/usr/share/doc/airpods-helper"
mkdir -p "packaging/$STAGING/usr/share/licenses/airpods-helper"

# Binaries
cp target/release/airpods-daemon "packaging/$STAGING/usr/bin/"
cp target/release/airpods-cli "packaging/$STAGING/usr/bin/"

# Systemd service (fix path)
sed 's|%h/.local/bin/airpods-daemon|/usr/bin/airpods-daemon|' \
    daemon/airpods-daemon.service > "packaging/$STAGING/usr/lib/systemd/user/airpods-daemon.service"

# D-Bus activation (fix path)
sed 's|%h/.local/bin/airpods-daemon|/usr/bin/airpods-daemon|' \
    daemon/org.costa.AirPods.service > "packaging/$STAGING/usr/share/dbus-1/services/org.costa.AirPods.service"

# EQ presets
cp eq-presets/*.toml "packaging/$STAGING/usr/share/airpods-helper/eq-presets/"

# Docs
cp config.example.toml "packaging/$STAGING/usr/share/doc/airpods-helper/"
cp LICENSE "packaging/$STAGING/usr/share/licenses/airpods-helper/"

# Control file
cat > "packaging/$STAGING/DEBIAN/control" << EOF
Package: airpods-helper
Version: $VERSION
Section: sound
Priority: optional
Architecture: $ARCH
Depends: bluez, dbus, libcap2-bin
Recommends: pipewire, wireplumber
Maintainer: Jack Hernandez <jack@synoros.io>
Description: Native AirPods support for Linux
 ANC control, battery levels, ear detection with MPRIS auto-pause,
 parametric EQ via PipeWire, auto-reconnect, CLI tool, and D-Bus interface.
Homepage: https://github.com/superninjv/airpods-helper
EOF

# Post-install
cat > "packaging/$STAGING/DEBIAN/postinst" << 'EOF'
#!/bin/bash
# Grant L2CAP raw socket capability — we have root here so the user doesn't need a separate sudo step.
if ! setcap 'cap_net_raw,cap_net_admin+eip' /usr/bin/airpods-daemon 2>/dev/null; then
    echo ">> setcap failed — run manually: sudo setcap 'cap_net_raw,cap_net_admin+eip' /usr/bin/airpods-daemon"
fi

cat <<'MSG'

  airpods-helper installed. Next steps:
    systemctl --user enable --now airpods-daemon.service
    airpods-cli doctor

  Docs: https://github.com/superninjv/airpods-helper
MSG
EOF
chmod 755 "packaging/$STAGING/DEBIAN/postinst"

# Build .deb
dpkg-deb --build "packaging/$STAGING"
rm -rf "packaging/$STAGING"

echo "Built: packaging/${STAGING}.deb"
