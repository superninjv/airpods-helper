use crate::status::{DaemonState, Status};
use std::f64::consts::TAU;
use tauri::image::Image;

pub fn battery_level(s: &Status) -> i32 {
    if s.daemon != DaemonState::Running || !s.connected {
        return -1;
    }
    let levels: Vec<_> = [s.battery_left, s.battery_right]
        .into_iter()
        .filter(|level| (0..=100).contains(level))
        .collect();
    if levels.is_empty() {
        -1
    } else {
        (levels.iter().sum::<i32>() as f64 / levels.len() as f64).round() as i32
    }
}

pub fn icon(level: i32) -> Image<'static> {
    // Generated from tray-glyph.svg: 64×64 RGBA avoids a runtime image decoder.
    const GLYPH: &[u8; 64 * 64 * 4] = include_bytes!("../icons/tray-glyph.rgba");
    let mut rgba = GLYPH.to_vec();
    let color = match level {
        0..=20 => [255, 69, 58],
        21..=50 => [255, 159, 10],
        _ => [48, 209, 88],
    };
    let end = TAU * level.clamp(0, 100) as f64 / 100.0;
    for y in 0..64 {
        for x in 0..64 {
            let (dx, dy) = (x as f64 + 0.5 - 32.0, y as f64 + 0.5 - 32.0);
            let track = (3.0 - (dx.hypot(dy) - 28.0).abs()).clamp(0.0, 1.0);
            let angle = (-dx).atan2(dy).rem_euclid(TAU);
            let arc = if level > 0 {
                let caps = (3.0 - dx.hypot(dy - 28.0))
                    .max(3.0 - (dx + 28.0 * end.sin()).hypot(dy - 28.0 * end.cos()));
                (if angle <= end { track } else { 0.0 }).max(caps.clamp(0.0, 1.0))
            } else {
                0.0
            };
            let alpha = arc + track * 0.2 * (1.0 - arc);
            if alpha > 0.0 {
                let i = (y * 64 + x) * 4;
                for c in 0..3 {
                    rgba[i + c] = ((color[c] as f64 * arc + 242.0 * track * 0.2 * (1.0 - arc))
                        / alpha)
                        .round() as u8;
                }
                rgba[i + 3] = (alpha * 255.0).round() as u8;
            }
        }
    }
    Image::new_owned(rgba, 64, 64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "documentation capture; set AIRPODS_SCREENSHOTS"]
    fn export_screenshot() {
        let output = std::path::PathBuf::from(std::env::var("AIRPODS_SCREENSHOTS").unwrap());
        std::fs::create_dir_all(&output).unwrap();
        std::fs::write(output.join("tauri-tray.rgba"), icon(79).rgba()).unwrap();
    }

    #[test]
    fn ring_averages_only_reported_earbuds() {
        let mut s = Status {
            daemon: DaemonState::Running,
            connected: true,
            battery_left: 80,
            battery_right: 25,
            battery_case: 100,
            ..Status::default()
        };
        assert_eq!(battery_level(&s), 53);
        s.battery_case = 0;
        assert_eq!(battery_level(&s), 53);
        s.battery_left = -1;
        assert_eq!(battery_level(&s), 25);
        s.battery_right = 101;
        assert_eq!(battery_level(&s), -1);
        s.battery_left = 0;
        assert_eq!(battery_level(&s), 0);
        s.battery_left = 100;
        assert_eq!(battery_level(&s), 100);
        s.connected = false;
        assert_eq!(battery_level(&s), -1);
    }

    #[test]
    fn ring_starts_at_bottom_with_threshold_colors_and_transparent_corners() {
        let pixel = |image: &Image<'_>, x: usize, y: usize| {
            image.rgba()[(y * 64 + x) * 4..(y * 64 + x + 1) * 4].to_vec()
        };
        for (level, color) in [
            (20, [255, 69, 58]),
            (21, [255, 159, 10]),
            (50, [255, 159, 10]),
            (51, [48, 209, 88]),
        ] {
            let image = icon(level);
            assert_eq!(pixel(&image, 32, 59)[..3], color);
            assert_eq!(pixel(&image, 0, 0)[3], 0);
            assert!(
                image
                    .rgba()
                    .chunks_exact(4)
                    .filter(|p| p[..3] == [255, 255, 255] && p[3] > 200)
                    .count()
                    > 100
            );
        }
        let quarter = icon(25);
        assert!(pixel(&quarter, 4, 32)[3] > 200);
        assert!(pixel(&quarter, 32, 4)[3] < 100);
        assert!(pixel(&quarter, 59, 32)[3] < 100);
        assert!(pixel(&icon(-1), 32, 59)[3] < 100);
        assert_eq!(pixel(&icon(100), 59, 32)[3], 255);
    }
}
