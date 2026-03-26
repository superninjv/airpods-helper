use super::*;

/// Handshake packet -- must be sent first after L2CAP connection
pub const HANDSHAKE: [u8; 16] = [
    0x00, 0x00, 0x04, 0x00, 0x01, 0x00, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00,
];

/// Feature enable packet
pub const SET_FEATURES: [u8; 14] = [
    0x04, 0x00, 0x04, 0x00, 0x4D, 0x00, 0xD7, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// Subscribe to all notification types
pub const SUBSCRIBE_NOTIFICATIONS: [u8; 10] = [
    0x04, 0x00, 0x04, 0x00, 0x0F, 0x00, 0xFF, 0xFF, 0xFF, 0xFF,
];

/// Build a control command packet
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

/// Enable or disable volume swipe
pub fn set_volume_swipe(enabled: bool) -> [u8; 11] {
    control_command(SUB_VOLUME_SWIPE, if enabled { 0x01 } else { 0x02 })
}

/// Enable all listening modes (Off + Noise + Transparency + Adaptive)
pub const ENABLE_ALL_LISTENING_MODES: [u8; 11] = [
    HEADER[0], HEADER[1], HEADER[2], HEADER[3],
    CMD_CONTROL, 0x00,
    0x1A, 0x0F, 0x00, 0x00, 0x00,
];
