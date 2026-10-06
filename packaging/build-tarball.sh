#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."

VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
DIST="airpods-helper-${VERSION}-x86_64-linux"


# Build
cargo build --workspace --profile dist

# Clean and create dist
rm -rf "packaging/$DIST" "packaging/$DIST.tar.gz"
mkdir -p "packaging/$DIST"/{bin,systemd,dbus}

# Binaries
cp target/dist/airpods-daemon "packaging/$DIST/bin/"
cp target/dist/airpods-cli "packaging/$DIST/bin/"

# Service files
cp daemon/airpods-daemon.service "packaging/$DIST/systemd/"
cp daemon/org.costa.AirPods.service "packaging/$DIST/dbus/"

# Config
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

install -dm755 "$HOME/.config/airpods-helper"
cp -n config.example.toml "$HOME/.config/airpods-helper/config.toml" 2>/dev/null || true

systemctl --user daemon-reload

echo ""
# The service/activation files point at ~/.local/bin; fix them for other prefixes.
for f in "$HOME/.config/systemd/user/airpods-daemon.service" "$HOME/.local/share/dbus-1/services/org.costa.AirPods.service"; do
    sed -i "s|%h/.local/bin/airpods-daemon|$PREFIX/bin/airpods-daemon|" "$f"
done
# Re-applying caps is needed on every upgrade (replacing the binary drops them).
echo "Granting Bluetooth L2CAP capability (needs sudo)..."
sudo setcap 'cap_net_raw,cap_net_admin+eip' "$PREFIX/bin/airpods-daemon" || \
    echo "  setcap failed; run: sudo setcap 'cap_net_raw,cap_net_admin+eip' $PREFIX/bin/airpods-daemon"
systemctl --user try-restart airpods-daemon.service 2>/dev/null || true

echo ""
echo "Installed. Next steps:"
echo "  systemctl --user enable --now airpods-daemon.service"
echo "  airpods-cli doctor"
INSTALL
chmod +x "packaging/$DIST/install.sh"

# Create tarball
cd packaging
tar czf "$DIST.tar.gz" "$DIST"
rm -rf "$DIST"

echo "Built: packaging/$DIST.tar.gz"
