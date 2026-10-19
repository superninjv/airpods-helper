# UI screenshots

The [PR gallery](../pull-request.md) shows Controls, EQ editing, Devices and
Settings/About in KDE light/dark and Tauri. All device data is fictional;
captures never contact a running daemon.

Separate tray-icon previews use the production renderers at the demo earbuds'
average charge (79%). They are not live dock captures.

KDE captures the shared QML panel with Breeze Light/Dark in an offscreen window,
waiting for the tab indicator to finish moving. Tauri renders the real frontend
in headless Firefox with a test-only bridge. These images do not validate KWin
effects, native tray behavior or WebKit rendering.

From the repository root, regenerate KDE images after building `kde/build`:

```bash
QT_QPA_PLATFORM=offscreen QT_QUICK_BACKEND=software QT_ACCESSIBILITY=0 \
DBUS_SESSION_BUS_ADDRESS=unix:path=/tmp/airpods-helper-no-bus \
AIRPODS_SCREENSHOTS="$PWD/docs/screenshots" kde/build/qml-test screenshots
```

Regenerate Tauri images with Node, Firefox and the Tauri build dependencies:

```bash
node docs/screenshots/capture-tauri.cjs
```

The script exports the real Rust icon through an opt-in test; its browser
bridge is temporary and never shipped. When publishing
the PR, use GitHub attachment URLs or raw feature-branch URLs for the images.

Error previews remain available for development: set `PREVIEW_ERRORS` to `true`
in `app/src/main.js`, or `previewErrors` to `true` in
`kde/qml/PanelContent.qml`, then rebuild. Both default to `false`.
