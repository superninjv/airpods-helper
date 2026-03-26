#!/bin/bash
set -euo pipefail

VERSION="0.1.0"
DIST="airpods-helper-${VERSION}-x86_64-linux"

cd "$(dirname "$0")/.."

# Build
cargo build --workspace --release

# Clean and create dist
rm -rf "packaging/$DIST" "packaging/$DIST.tar.gz"
mkdir -p "packaging/$DIST"/{bin,systemd,dbus,eq-presets}

# Binaries
cp target/release/airpods-daemon "packaging/$DIST/bin/"
cp target/release/airpods-cli "packaging/$DIST/bin/"

# Service files
cp daemon/airpods-daemon.service "packaging/$DIST/systemd/"
cp daemon/org.costa.AirPods.service "packaging/$DIST/dbus/"

# EQ presets + config
cp eq-presets/*.toml "packaging/$DIST/eq-presets/"
cp config.example.toml "packaging/$DIST/"
cp LICENSE "packaging/$DIST/"

# Install script
cat > "packaging/$DIST/install.sh" << 'INSTALL'
#!/bin/bash
set -euo pipefail

PREFIX="${PREFIX:-$HOME/.local}"
echo "Installing airpods-helper to $PREFIX..."

install -Dm755 bin/airpods-daemon "$PREFIX/bin/airpods-daemon"
install -Dm755 bin/airpods-cli "$PREFIX/bin/airpods-cli"
install -Dm644 systemd/airpods-daemon.service "$HOME/.config/systemd/user/airpods-daemon.service"
install -Dm644 dbus/org.costa.AirPods.service "$HOME/.local/share/dbus-1/services/org.costa.AirPods.service"

install -dm755 "$HOME/.config/airpods-helper/eq/"
cp -n eq-presets/*.toml "$HOME/.config/airpods-helper/eq/" 2>/dev/null || true
cp -n config.example.toml "$HOME/.config/airpods-helper/config.toml" 2>/dev/null || true

systemctl --user daemon-reload

echo ""
echo "Installed. Next steps:"
echo "  sudo setcap 'cap_net_raw,cap_net_admin+eip' $PREFIX/bin/airpods-daemon"
echo "  systemctl --user enable --now airpods-daemon.service"
INSTALL
chmod +x "packaging/$DIST/install.sh"

# Create tarball
cd packaging
tar czf "$DIST.tar.gz" "$DIST"
rm -rf "$DIST"

echo "Built: packaging/$DIST.tar.gz"
