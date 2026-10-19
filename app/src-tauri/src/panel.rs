use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Manager, PhysicalPosition, PhysicalSize, Runtime, State, Window};

#[derive(Default)]
pub struct PanelState {
    pinned: AtomicBool,
    focused: AtomicBool,
}

#[tauri::command]
pub fn set_panel_pinned(pinned: bool, state: State<'_, PanelState>) {
    state.pinned.store(pinned, Ordering::Relaxed);
}

#[tauri::command]
pub fn hide_panel(app: AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("main") {
        window.hide().map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub async fn open_releases() -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(|| {
        let status = Command::new("xdg-open")
            .arg("https://github.com/superninjv/airpods-helper/releases")
            .env_remove("LD_LIBRARY_PATH")
            .env_remove("LD_PRELOAD")
            .status()
            .map_err(|e| format!("Could not open releases: {e}"))?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("Could not open releases: {status}"))
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

// Choose the nearest screen edge and stay inside its usable work area.
fn popup_position(anchor: (i32, i32), area: (i32, i32, u32, u32), size: (u32, u32)) -> (i32, i32) {
    let (left, top, width, height) = area;
    let right = left + width as i32;
    let bottom = top + height as i32;
    let (w, h) = (size.0.min(width) as i32, size.1.min(height) as i32);
    let distances = [
        (anchor.1 - top).abs(),
        (bottom - anchor.1).abs(),
        (anchor.0 - left).abs(),
        (right - anchor.0).abs(),
    ];
    let edge = distances
        .iter()
        .enumerate()
        .min_by_key(|(_, distance)| *distance)
        .unwrap()
        .0;
    let (mut x, mut y) = (anchor.0 - w / 2, anchor.1 - h / 2);
    match edge {
        0 => y = top,
        1 => y = bottom - h,
        2 => x = left,
        _ => x = right - w,
    }
    (x.clamp(left, right - w), y.clamp(top, bottom - h))
}

pub fn show<R: Runtime>(app: &AppHandle<R>) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    if let Ok(anchor) = window.cursor_position() {
        if let Ok(Some(monitor)) = window.monitor_from_point(anchor.x, anchor.y) {
            if let Ok(size) = window.outer_size() {
                let area = monitor.work_area();
                if area.size.width > 0 && area.size.height > 0 {
                    let size = PhysicalSize::new(
                        size.width.min(area.size.width),
                        size.height.min(area.size.height),
                    );
                    let (x, y) = popup_position(
                        (anchor.x as i32, anchor.y as i32),
                        (
                            area.position.x,
                            area.position.y,
                            area.size.width,
                            area.size.height,
                        ),
                        (size.width, size.height),
                    );
                    let _ = window.set_size(size);
                    let _ = window.set_position(PhysicalPosition::new(x, y));
                }
            }
        }
    } else {
        // shortcut: Wayland does not expose global pointer/window positioning; the compositor places the popup.
        let _ = window.center();
    }
    let _ = window.show();
    let _ = window.unminimize();
    let _ = window.set_focus();
}

pub fn focus_changed<R: Runtime>(window: &Window<R>, focused: bool) {
    let state = window.state::<PanelState>();
    let previously_focused = state.focused.swap(focused, Ordering::Relaxed);
    if !focused && previously_focused && !state.pinned.load(Ordering::Relaxed) {
        let _ = window.hide();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn popup_stays_inside_work_area_at_each_edge() {
        let area = (0, 40, 1920, 1000);
        assert_eq!(popup_position((1900, 1060), area, (420, 560)), (1500, 480));
        assert_eq!(popup_position((960, 0), area, (420, 560)), (750, 40));
        assert_eq!(popup_position((0, 500), area, (420, 560)), (0, 220));
        assert_eq!(popup_position((1920, 500), area, (420, 560)), (1500, 220));
        assert_eq!(
            popup_position((-10, 500), (-1280, -200, 1280, 720), (420, 900)),
            (-420, -200)
        );
    }

    #[test]
    fn frontend_commands_have_permissions() {
        let capabilities: serde_json::Value =
            serde_json::from_str(include_str!("../capabilities/default.json")).unwrap();
        let permissions = capabilities["permissions"].as_array().unwrap();
        for command in [
            "hide_panel",
            "set_panel_pinned",
            "is_appimage",
            "install_bundle",
            "rename_device",
            "open_releases",
        ] {
            let permission = format!("allow-{}", command.replace('_', "-"));
            assert!(
                permissions.iter().any(|p| p.as_str() == Some(&permission)),
                "{command}"
            );
            assert!(
                include_str!("../build.rs").contains(&format!("\"{command}\"")),
                "{command}"
            );
        }
    }
}
