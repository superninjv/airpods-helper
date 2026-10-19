#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
repo="$PWD"
build="$repo/packaging/appimage-build"
appdir="$build/AppDir"
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)

if [[ $(uname -m) != x86_64 ]]; then
    echo 'The AppImage release currently targets x86_64.' >&2
    exit 1
fi
for tool in cargo linuxdeploy linuxdeploy-plugin-gtk.sh pkg-config; do
    command -v "$tool" >/dev/null || { echo "Missing build tool: $tool" >&2; exit 1; }
done
cargo build --workspace --profile dist --locked
cargo build --manifest-path app/src-tauri/Cargo.toml --release --locked
cargo test --manifest-path app/src-tauri/Cargo.toml --release --locked

# Reuse the build cache, but never an AppDir containing the retired KDE bundle.
rm -rf "$appdir"
install -Dm755 app/src-tauri/target/release/airpods-app "$appdir/usr/bin/airpods-app"
install -Dm755 target/dist/airpods-daemon target/dist/airpods-cli "$appdir/usr/bin/"
install -Dm755 packaging/appimage/install.sh "$appdir/usr/share/airpods-helper/install.sh"
printf '%s\n' "$version" > "$appdir/usr/share/airpods-helper/version"
install -Dm644 LICENSE "$appdir/usr/share/licenses/airpods-helper/LICENSE"
install -Dm644 app/src-tauri/icons/icon.svg "$appdir/usr/share/icons/hicolor/scalable/apps/org.costa.AirPodsHelper.svg"
install -Dm644 packaging/appimage/org.costa.AirPodsHelper.desktop "$appdir/usr/share/applications/org.costa.AirPodsHelper.desktop"

# WebKit's subprocesses are not ELF dependencies of the main application.
webkit_dir="$(pkg-config --variable=libdir webkit2gtk-4.1)/webkit2gtk-4.1"
if [[ ! -d "$webkit_dir" ]]; then
    webkit_dir="$(pkg-config --variable=libexecdir webkit2gtk-4.1)/webkit2gtk-4.1"
fi
[[ "$webkit_dir" == /usr/* ]] || { echo "WebKit must be installed under /usr for this bundle" >&2; exit 1; }
webkit_payload="$appdir$webkit_dir"
for file in WebKitWebProcess WebKitNetworkProcess injected-bundle/libwebkit2gtkinjectedbundle.so; do
    install -Dm755 "$webkit_dir/$file" "$webkit_payload/$file"
done
if [[ -f "$webkit_dir/WebKitGPUProcess" ]]; then
    install -Dm755 "$webkit_dir/WebKitGPUProcess" "$webkit_payload/WebKitGPUProcess"
fi
# WebKit's sandbox helpers also use paths relocated by the GTK plugin.
for helper in bwrap xdg-dbus-proxy; do
    helper_path="$(command -v "$helper")"
    install -Dm755 "$helper_path" "$appdir$helper_path"
done
# Tray support loads AppIndicator dynamically rather than linking it into the app.
indicator="$(pkg-config --variable=libdir ayatana-appindicator3-0.1)/libayatana-appindicator3.so.1"
export APPIMAGE_EXTRACT_AND_RUN=1
export LDAI_UPDATE_INFORMATION='gh-releases-zsync|superninjv|airpods-helper|latest|AirPods_Helper-x86_64.AppImage.zsync'
mkdir -p "$build/output"
export LDAI_OUTPUT="$build/output/AirPods_Helper-x86_64.AppImage"
linuxdeploy --appdir "$appdir" --plugin gtk --library "$indicator" \
    --deploy-deps-only "$webkit_payload"
rm -f "$appdir/AppRun" "$appdir/AppRun.wrapped"
cd "$build/output"
linuxdeploy --appdir "$appdir" --custom-apprun "$repo/packaging/appimage/AppRun" --output appimage
# GTK hooks wrap the dispatcher; never silently replace its --install handling.
cmp "$repo/packaging/appimage/AppRun" "$appdir/AppRun.wrapped"
test -s "$LDAI_OUTPUT"
test -s "$LDAI_OUTPUT.zsync"
mv -f "$LDAI_OUTPUT" "$build/AirPods_Helper-x86_64.AppImage"
mv -f "$LDAI_OUTPUT.zsync" "$build/AirPods_Helper-x86_64.AppImage.zsync"
cd "$build"
sha256sum AirPods_Helper-x86_64.AppImage > AirPods_Helper-x86_64.AppImage.sha256
echo "Built $build/AirPods_Helper-x86_64.AppImage (Tauri GUI, daemon and CLI)"
