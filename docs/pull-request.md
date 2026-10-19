# A little love for AirPods on Linux: native KDE widget + Tauri AppImage

Hi! I am Niccolò Nicolosi, a CS engineer and PhD candidate doing research on compiler technologies at Politecnico di Milano. I came across your AirPods Helper project while searching for better integration for AirPods on Linux, and I really liked it! Even more than librepods, which is what I started with, because I found your solution to be more stable and refined on Linux.

Because I look forward to using it as my daily driver on my KDE Arch installation, I decided to have some fun and add a curated native widget for it, with the help of Codex GPT-6.1-Sol on high reasoning to implement it (it's a free-time project, after all). I also added a way to produce an AppImage, with metadata for it to be updatable, to facilitate distribution of your project with the original Tauri UI for desktops other than KDE. While at it, I also made the Tauri UI a bit nicer and consistent with its Plasma widget counterpart.

Hope you like it and welcome the additions! This project has the potential to be the standard AirPods integration for Linux, in my humble opinion.

A bullet list of the main changes below:

- Native KDE Plasma dock widget with theme integration, battery levels, listening modes, an EQ editor and a battery-ring icon.
- Resizable, pinnable Tauri panel with Controls, Devices and Settings tabs, matching controls and a battery-ring tray icon.
- Device renaming, automatically saved preferred-device selection, About details and dismissible error messages in both UIs.
- AppImage packaging with daemon setup, update metadata and a release workflow, plus an Arch packaging recipe for KDE.
- Tests and concise installation and distribution documentation.

Validated with Rust workspace, Tauri, frontend, installer and KDE offscreen tests, plus a local AppImage build/checksum. Live audio tests, KDE D-Bus integration, clean-distro installation and the GitHub release workflow still need external validation.

Screenshots below:

Demo device data. KDE uses the shared QML panel; Tauri uses its real frontend in
Firefox. See [capture instructions](screenshots/README.md).

| Feature | KDE light | KDE dark | Tauri |
| --- | --- | --- | --- |
| Controls | <img src="screenshots/kde-controls.png" width="220" alt="KDE light controls"> | <img src="screenshots/kde-dark-controls.png" width="220" alt="KDE dark controls"> | <img src="screenshots/tauri-controls.png" width="220" alt="Tauri controls"> |
| EQ editor | <img src="screenshots/kde-equalizer.png" width="220" alt="KDE light EQ editor"> | <img src="screenshots/kde-dark-equalizer.png" width="220" alt="KDE dark EQ editor"> | <img src="screenshots/tauri-equalizer.png" width="220" alt="Tauri EQ editor"> |
| Devices | <img src="screenshots/kde-devices.png" width="220" alt="KDE light devices"> | <img src="screenshots/kde-dark-devices.png" width="220" alt="KDE dark devices"> | <img src="screenshots/tauri-devices.png" width="220" alt="Tauri devices"> |
| Settings/About | <img src="screenshots/kde-settings.png" width="220" alt="KDE light settings"> | <img src="screenshots/kde-dark-settings.png" width="220" alt="KDE dark settings"> | <img src="screenshots/tauri-settings.png" width="220" alt="Tauri settings"> |

Tray icons:

| KDE light | KDE dark | Tauri |
| --- | --- | --- |
| <img src="screenshots/kde-tray.png" width="80" alt="KDE light tray icon"> | <img src="screenshots/kde-dark-tray.png" width="80" alt="KDE dark tray icon"> | <img src="screenshots/tauri-tray.png" width="80" alt="Tauri tray icon"> |
