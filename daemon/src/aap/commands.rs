use super::*;

/// Handshake packet — must be sent first after L2CAP connection
pub const HANDSHAKE: [u8; 16] = [
    0x00, 0x00, 0x04, 0x00, 0x01, 0x00, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00,
];

/// Feature enable packet — enables conversational awareness during playback + adaptive transparency
pub const SET_FEATURES: [u8; 14] = [
    0x04, 0x00, 0x04, 0x00, 0x4D, 0x00, 0xD7, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// Subscribe to all notification types (battery, ear detection, ANC, etc.)
pub const SUBSCRIBE_NOTIFICATIONS: [u8; 10] = [
    0x04, 0x00, 0x04, 0x00, 0x0F, 0x00, 0xFF, 0xFF, 0xFF, 0xFF,
];

/// Build a control command packet
/// Format: 04 00 04 00 09 00 [sub_cmd] [value] 00 00 00
fn control_command(sub_cmd: u8, value: u8) -> [u8; 11] {
    [
        HEADER[0],
        HEADER[1],
        HEADER[2],
        HEADER[3],
        CMD_CONTROL,
        0x00,
        sub_cmd,
        value,
        0x00,
        0x00,
        0x00,
    ]
}

/// Set ANC mode
pub fn set_anc_mode(mode: AncMode) -> [u8; 11] {
    control_command(SUB_ANC_MODE, mode as u8)
}

/// Enable or disable conversational awareness
pub fn set_conversational_awareness(enabled: bool) -> [u8; 11] {
    control_command(
        SUB_CONVERSATIONAL_AWARENESS,
        if enabled { 0x01 } else { 0x02 },
    )
}

/// Set adaptive noise level (0-100)
pub fn set_adaptive_noise_level(level: u8) -> [u8; 11] {
    control_command(SUB_ADAPTIVE_NOISE_LEVEL, level.min(100))
}

/// Enable or disable ANC when wearing a single AirPod
pub fn set_one_bud_anc(enabled: bool) -> [u8; 11] {
    control_command(SUB_ONE_BUD_ANC, if enabled { 0x01 } else { 0x02 })
}

/// Set which listening modes are available in the rotation.
/// Bitmask: 0x01=Off, 0x02=Noise, 0x04=Transparency, 0x08=Adaptive
/// 0x0F = all modes enabled
#[allow(dead_code)]
pub fn set_listening_mode_configs(modes: u8) -> [u8; 11] {
    control_command(0x1A, modes)
}

/// Enable all listening modes (Off + Noise + Transparency + Adaptive)
pub const ENABLE_ALL_LISTENING_MODES: [u8; 11] = [
    HEADER[0], HEADER[1], HEADER[2], HEADER[3],
    CMD_CONTROL, 0x00,
    0x1A, 0x0F, 0x00, 0x00, 0x00,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_handshake_bytes() {
        assert_eq!(HANDSHAKE.len(), 16);
        assert_eq!(HANDSHAKE[0..4], [0x00, 0x00, 0x04, 0x00]);
    }

    #[test]
    fn test_set_anc_mode() {
        let pkt = set_anc_mode(AncMode::NoiseCancellation);
        assert_eq!(
            pkt,
            [0x04, 0x00, 0x04, 0x00, 0x09, 0x00, 0x0D, 0x02, 0x00, 0x00, 0x00]
        );

        let pkt = set_anc_mode(AncMode::Transparency);
        assert_eq!(pkt[7], 0x03);

        let pkt = set_anc_mode(AncMode::Adaptive);
        assert_eq!(pkt[7], 0x04);

        let pkt = set_anc_mode(AncMode::Off);
        assert_eq!(pkt[7], 0x01);
    }

    #[test]
    fn test_set_conversational_awareness() {
        let enable = set_conversational_awareness(true);
        assert_eq!(enable[6], 0x28);
        assert_eq!(enable[7], 0x01);

        let disable = set_conversational_awareness(false);
        assert_eq!(disable[7], 0x02);
    }

    #[test]
    fn test_set_adaptive_noise_level() {
        let pkt = set_adaptive_noise_level(50);
        assert_eq!(pkt[6], 0x2E);
        assert_eq!(pkt[7], 50);

        // Clamped to 100
        let pkt = set_adaptive_noise_level(200);
        assert_eq!(pkt[7], 100);
    }

    #[test]
    fn test_set_one_bud_anc() {
        let enable = set_one_bud_anc(true);
        assert_eq!(enable[6], 0x1B);
        assert_eq!(enable[7], 0x01);

        let disable = set_one_bud_anc(false);
        assert_eq!(disable[7], 0x02);
    }

    #[test]
    fn test_subscribe_notifications() {
        assert_eq!(SUBSCRIBE_NOTIFICATIONS.len(), 10);
        assert_eq!(SUBSCRIBE_NOTIFICATIONS[4], 0x0F);
    }
}
