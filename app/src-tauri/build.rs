/// App commands exposed to the webview. Declaring them here generates an
/// `allow-<command>` permission per command; only those granted in
/// `capabilities/default.json` are callable from the frontend.
const COMMANDS: &[&str] = &[
    "get_status",
    "retry_daemon",
    "set_anc_mode",
    "set_adaptive_noise_level",
    "set_conversational_awareness",
    "set_one_bud_anc",
    "set_volume_swipe",
    "set_mic_mode",
    "set_setting",
    "set_start_on_login",
    "list_paired",
    "connect_device",
    "disconnect_device",
    "reconnect",
    "pair_device",
    "quick_pair_scan",
    "refresh_eq_presets",
    "get_eq_preset",
    "set_eq_preset",
    "save_eq_preset",
    "delete_eq_preset",
];

fn main() {
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(COMMANDS)),
    )
    .expect("failed to run tauri-build");
}
