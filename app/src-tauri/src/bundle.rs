use std::ffi::OsStr;
use std::process::Command;

#[tauri::command]
pub fn is_appimage() -> bool {
    std::env::var_os("APPIMAGE").is_some()
}

fn run_installer(image: &OsStr) -> Result<(), String> {
    let output = Command::new(image)
        .arg("--install")
        .output()
        .map_err(|e| format!("Could not start the installer: {e}"))?;
    if output.status.success() {
        Ok(())
    } else {
        let message = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        Err(if message.is_empty() {
            format!("Installer failed: {}", output.status)
        } else {
            message
        })
    }
}

#[tauri::command]
pub async fn install_bundle() -> Result<(), String> {
    let image = std::env::var_os("APPIMAGE").ok_or("Run the AppImage to install its bundle")?;
    tauri::async_runtime::spawn_blocking(move || run_installer(&image))
        .await
        .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installer_result_is_reported() {
        assert!(run_installer(OsStr::new("/usr/bin/true")).is_ok());
        assert!(
            run_installer(OsStr::new("/usr/bin/false"))
                .unwrap_err()
                .contains("Installer failed")
        );
        assert!(
            run_installer(OsStr::new("/no/such/airpods-image"))
                .unwrap_err()
                .contains("Could not start")
        );
    }
}
