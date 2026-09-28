//! System tray: status line, listening modes, EQ presets, show/hide, quit.
//! The menu is rebuilt only when something it displays changes.

use tauri::menu::{
    CheckMenuItemBuilder, IsMenuItem, Menu, MenuBuilder, MenuItemBuilder, PredefinedMenuItem,
    SubmenuBuilder,
};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, Runtime};

use crate::Backend;
use std::sync::Mutex;

use crate::status::{DaemonState, SharedStore, Status};

pub const TRAY_ID: &str = "main";

const ANC_MODES: [(&str, &str); 4] = [
    ("off", "Off"),
    ("noise", "Noise Cancellation"),
    ("transparency", "Transparency"),
    ("adaptive", "Adaptive"),
];

/// Modes the connected model supports, in display order.
pub fn supported_anc_modes(s: &Status) -> Vec<(&'static str, &'static str)> {
    if !s.has("anc") {
        return Vec::new();
    }
    ANC_MODES
        .iter()
        .copied()
        .filter(|(id, _)| *id != "adaptive" || s.has("adaptive"))
        .collect()
}

pub fn create<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("AirPods Helper")
        .menu(&build_menu(app, &Status::default())?)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| on_menu_event(app, event.id().as_ref()))
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                crate::toggle_main_window(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

/// Cheap fingerprint of everything the menu shows.
fn menu_key(s: &Status) -> String {
    let presets: Vec<&str> = s
        .eq_presets
        .iter()
        .flat_map(|p| [p.id.as_str(), p.name.as_str()])
        .collect();
    format!(
        "{:?}|{}|{}|{:?}|{}|{}|{}|{}",
        s.daemon,
        s.connected,
        s.display_name(),
        s.features,
        s.anc_mode,
        s.eq_preset,
        presets.join("\u{1f}"),
        s.battery_summary(),
    )
}

/// What was last applied to the tray: (menu fingerprint, tooltip).
#[derive(Default)]
pub struct TrayCache(Mutex<(String, String)>);

/// Keep the tray in sync with `s`.
pub fn sync<R: Runtime>(app: &AppHandle<R>, s: &Status) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };
    let cache = app.state::<TrayCache>();
    let mut cache = cache.0.lock().unwrap_or_else(|e| e.into_inner());
    let key = menu_key(s);
    if key != cache.0 {
        match build_menu(app, s) {
            Ok(menu) => {
                let _ = tray.set_menu(Some(menu));
                cache.0 = key;
            }
            Err(e) => tracing::warn!("failed to rebuild tray menu: {e}"),
        }
    }
    let tooltip = s.tooltip();
    if tooltip != cache.1 {
        let _ = tray.set_tooltip(Some(&tooltip));
        cache.1 = tooltip;
    }
}

/// Rebuild the menu even if nothing changed (check items toggle themselves
/// when clicked, which may not match the daemon's state).
pub fn force_refresh<R: Runtime>(app: &AppHandle<R>) {
    {
        let cache = app.state::<TrayCache>();
        cache.0.lock().unwrap_or_else(|e| e.into_inner()).0.clear();
    }
    let status = app.state::<SharedStore>().current();
    sync(app, &status);
}

fn build_menu<R: Runtime>(app: &AppHandle<R>, s: &Status) -> tauri::Result<Menu<R>> {
    let headline = match s.daemon {
        DaemonState::Absent => "Daemon not running".to_string(),
        DaemonState::Starting => "Connecting to daemon…".to_string(),
        DaemonState::Running if !s.connected => "AirPods disconnected".to_string(),
        DaemonState::Running => {
            let battery = s.battery_summary();
            if battery.is_empty() {
                s.display_name().to_string()
            } else {
                format!("{} — {battery}", s.display_name())
            }
        }
    };
    let status_item = MenuItemBuilder::with_id("status", headline)
        .enabled(false)
        .build(app)?;

    let mut items: Vec<Box<dyn IsMenuItem<R>>> = vec![Box::new(status_item)];

    let running = s.daemon == DaemonState::Running;
    let modes = supported_anc_modes(s);
    if running && s.connected && !modes.is_empty() {
        items.push(Box::new(PredefinedMenuItem::separator(app)?));
        for (id, label) in modes {
            items.push(Box::new(
                CheckMenuItemBuilder::with_id(format!("anc:{id}"), label)
                    .checked(s.anc_mode == id)
                    .build(app)?,
            ));
        }
    }

    if running && !s.eq_presets.is_empty() {
        let mut eq = SubmenuBuilder::with_id(app, "eq", "Equalizer").item(
            &CheckMenuItemBuilder::with_id("eq:", "Off")
                .checked(s.eq_preset.is_empty())
                .build(app)?,
        );
        eq = eq.separator();
        for p in &s.eq_presets {
            eq = eq.item(
                &CheckMenuItemBuilder::with_id(format!("eq:{}", p.id), &p.name)
                    .checked(s.eq_preset == p.id)
                    .build(app)?,
            );
        }
        items.push(Box::new(PredefinedMenuItem::separator(app)?));
        items.push(Box::new(eq.build()?));
    }

    if running && s.connected {
        items.push(Box::new(
            MenuItemBuilder::with_id("disconnect", "Disconnect").build(app)?,
        ));
    }

    items.push(Box::new(PredefinedMenuItem::separator(app)?));
    items.push(Box::new(
        MenuItemBuilder::with_id("toggle", "Show / Hide Window").build(app)?,
    ));
    items.push(Box::new(
        MenuItemBuilder::with_id("quit", "Quit").build(app)?,
    ));

    let refs: Vec<&dyn IsMenuItem<R>> = items.iter().map(|b| b.as_ref()).collect();
    MenuBuilder::new(app).items(&refs).build()
}

fn on_menu_event<R: Runtime>(app: &AppHandle<R>, id: &str) {
    match id {
        "quit" => app.exit(0),
        "toggle" => crate::toggle_main_window(app),
        _ => {
            let backend = app.state::<std::sync::Arc<Backend>>().inner().clone();
            let app = app.clone();
            let id = id.to_string();
            tauri::async_runtime::spawn(async move {
                let result = if let Some(mode) = id.strip_prefix("anc:") {
                    backend.set_anc_mode(mode.to_string()).await
                } else if let Some(preset) = id.strip_prefix("eq:") {
                    backend.set_eq_preset(preset.to_string()).await
                } else if id == "disconnect" {
                    backend.disconnect().await
                } else {
                    Ok(())
                };
                if let Err(e) = result {
                    tracing::warn!("tray action {id} failed: {e}");
                    let _ = app.emit("backend-error", e);
                }
                // A CheckMenuItem toggles itself on click; force a rebuild so
                // it reflects the daemon's state, not the click.
                force_refresh(&app);
            });
        }
    }
}
