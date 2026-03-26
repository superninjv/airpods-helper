use super::*;
use thiserror::Error;
use tracing::{debug, warn};

#[derive(Error, Debug)]
pub enum ParseError {
    #[error("packet too short: {0} bytes")]
    TooShort(usize),
    #[error("unknown command: 0x{0:02X}")]
    UnknownCommand(u8),
    #[error("invalid data in packet")]
    InvalidData,
}

/// Parsed AAP events from incoming packets
#[derive(Debug, Clone)]
pub enum AapEvent {
    HandshakeAck,
    FeaturesAck,
    Battery(BatteryUpdate),
    AncMode(AncMode),
    EarDetection(EarDetectionUpdate),
    ConversationalAwareness(bool),
    ConversationalActivity(CaActivity),
    DeviceInfo(DeviceInfoUpdate),
    AdaptiveNoiseLevel(u8),
    OneBudAnc(bool),
    VolumeSwipe(bool),
    AdaptiveVolume(bool),
    ChimeVolume(u8),
    HeadTracking(#[allow(dead_code)] Vec<u8>),
    AudioSource(AudioSource),
    Disconnected,
}

/// Active audio source reported by AirPods
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioSource {
    None,
    Call,
    Media,
    Unknown(u8),
}

#[derive(Debug, Clone)]
pub struct BatteryUpdate {
    pub left: Option<BatteryEntry>,
    pub right: Option<BatteryEntry>,
    pub case: Option<BatteryEntry>,
}

#[derive(Debug, Clone, Copy)]
pub struct BatteryEntry {
    pub level: u8,
    pub charging: bool,
    #[allow(dead_code)] // populated from packet, exposed in future version
    pub connected: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct EarDetectionUpdate {
    pub primary: EarStatus,
    pub secondary: EarStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaActivity {
    Speaking,
    Stopped,
    Normal,
}

#[derive(Debug, Clone)]
pub struct DeviceInfoUpdate {
    #[allow(dead_code)] // parsed from packet, exposed in future version
    pub name: String,
    pub model: String,
    #[allow(dead_code)]
    pub manufacturer: String,
    #[allow(dead_code)]
    pub serial: String,
    pub firmware: String,
}

/// Parse a raw AAP packet into a typed event
pub fn parse(data: &[u8]) -> Result<AapEvent, ParseError> {
    if data.len() < 4 {
        return Err(ParseError::TooShort(data.len()));
    }

    // Check for disconnect sentinel
    if data == DISCONNECT_PACKET {
        return Ok(AapEvent::Disconnected);
    }

    // Handshake ACK: starts with 01 00 04 00
    if data.len() >= 4 && data[0] == 0x01 && data[1] == 0x00 && data[2] == 0x04 && data[3] == 0x00
    {
        return Ok(AapEvent::HandshakeAck);
    }

    // Control packets: header 04 00 04 00
    if data.len() < 6 {
        return Err(ParseError::TooShort(data.len()));
    }

    if data[0..4] != HEADER {
        return Err(ParseError::UnknownCommand(data[0]));
    }

    let cmd = data[4];

    match cmd {
        // Features ACK (0x2B)
        0x2B => Ok(AapEvent::FeaturesAck),

        CMD_BATTERY => parse_battery(&data[6..]),
        CMD_EAR_DETECTION => parse_ear_detection(&data[6..]),
        CMD_CONTROL => parse_control(&data[6..]),
        CMD_AUDIO_SOURCE => parse_audio_source(&data[6..]),
        CMD_HEAD_TRACKING => {
            // Spatial audio head tracking data — high frequency, variable length
            debug!("head tracking data, len={}", data.len());
            Ok(AapEvent::HeadTracking(data[6..].to_vec()))
        }
        CMD_STEM_PRESS => {
            // Stem press events are handled by the OS bluetooth stack
            debug!("stem press event, len={}", data.len());
            Err(ParseError::UnknownCommand(cmd))
        }
        CMD_DEVICE_INFO => parse_device_info(data),
        CMD_CONNECTED_DEVICES => {
            // List of paired/connected devices — informational
            debug!("connected devices notification, len={}", data.len());
            Err(ParseError::UnknownCommand(cmd))
        }
        CMD_CA_ACTIVITY => parse_ca_activity(&data[6..]),
        CMD_EQ_DATA => {
            // EQ settings data from device (140 bytes typical)
            debug!("EQ data packet, len={}", data.len());
            Err(ParseError::UnknownCommand(cmd))
        }

        // Known but unhandled commands — log at debug to reduce noise
        0x02 => {
            // Likely device capabilities/features exchange
            debug!("capabilities packet, len={}", data.len());
            Err(ParseError::UnknownCommand(cmd))
        }
        0x08 => {
            debug!("unknown command 0x08, len={}", data.len());
            Err(ParseError::UnknownCommand(cmd))
        }
        0x0C => {
            debug!("unknown command 0x0C, len={}", data.len());
            Err(ParseError::UnknownCommand(cmd))
        }
        0x10 | 0x11 => {
            // Smart routing / device handoff
            debug!("smart routing packet 0x{cmd:02X}, len={}", data.len());
            Err(ParseError::UnknownCommand(cmd))
        }
        0x12 => {
            // Secondary control (case sounds, transparency customization)
            debug!("secondary control packet, len={}", data.len());
            Err(ParseError::UnknownCommand(cmd))
        }
        0x14 => {
            // Connected device MAC address
            debug!("connected MAC notification, len={}", data.len());
            Err(ParseError::UnknownCommand(cmd))
        }
        0x4E => {
            debug!("unknown command 0x4E, len={}", data.len());
            Err(ParseError::UnknownCommand(cmd))
        }
        0x52 => {
            debug!("unknown command 0x52, len={}", data.len());
            Err(ParseError::UnknownCommand(cmd))
        }
        0x55 => {
            debug!("unknown command 0x55, len={}", data.len());
            Err(ParseError::UnknownCommand(cmd))
        }

        _ => {
            warn!("unknown AAP command: 0x{cmd:02X}, len={}", data.len());
            Err(ParseError::UnknownCommand(cmd))
        }
    }
}

/// Parse battery notification payload (after 6-byte header)
/// Format: [count] then repeating 5-byte entries: [component, 0x01, level, status, 0x01]
fn parse_battery(payload: &[u8]) -> Result<AapEvent, ParseError> {
    if payload.is_empty() {
        return Err(ParseError::TooShort(0));
    }

    let count = payload[0] as usize;
    let entries = &payload[1..];

    if entries.len() < count * 5 {
        return Err(ParseError::TooShort(entries.len()));
    }

    let mut update = BatteryUpdate {
        left: None,
        right: None,
        case: None,
    };

    for i in 0..count {
        let offset = i * 5;
        let component_byte = entries[offset];
        let level = entries[offset + 2];
        let status = ChargingStatus::from_byte(entries[offset + 3]);

        let entry = BatteryEntry {
            level,
            charging: matches!(status, Some(ChargingStatus::Charging)),
            connected: !matches!(status, Some(ChargingStatus::Disconnected)),
        };

        match BatteryComponent::from_byte(component_byte) {
            Some(BatteryComponent::Left) => update.left = Some(entry),
            Some(BatteryComponent::Right) => update.right = Some(entry),
            Some(BatteryComponent::Case) => update.case = Some(entry),
            None => warn!("unknown battery component: 0x{component_byte:02X}"),
        }
    }

    Ok(AapEvent::Battery(update))
}

/// Parse ear detection payload (after 6-byte header)
/// Format: [primary_status, secondary_status]
fn parse_ear_detection(payload: &[u8]) -> Result<AapEvent, ParseError> {
    if payload.len() < 2 {
        return Err(ParseError::TooShort(payload.len()));
    }

    let primary = EarStatus::from_byte(payload[0]).ok_or(ParseError::InvalidData)?;
    let secondary = EarStatus::from_byte(payload[1]).ok_or(ParseError::InvalidData)?;

    Ok(AapEvent::EarDetection(EarDetectionUpdate {
        primary,
        secondary,
    }))
}

/// Parse control command payload (after 6-byte header)
/// Format: [sub_cmd, value, 0x00, 0x00, 0x00]
fn parse_control(payload: &[u8]) -> Result<AapEvent, ParseError> {
    if payload.len() < 2 {
        return Err(ParseError::TooShort(payload.len()));
    }

    let sub_cmd = payload[0];
    let value = payload[1];

    match sub_cmd {
        SUB_ANC_MODE => {
            let mode = AncMode::from_byte(value).ok_or(ParseError::InvalidData)?;
            Ok(AapEvent::AncMode(mode))
        }
        SUB_CONVERSATIONAL_AWARENESS => {
            let enabled = value == 0x01;
            Ok(AapEvent::ConversationalAwareness(enabled))
        }
        SUB_ADAPTIVE_NOISE_LEVEL => {
            Ok(AapEvent::AdaptiveNoiseLevel(value))
        }
        SUB_ONE_BUD_ANC => {
            let enabled = value == 0x01;
            Ok(AapEvent::OneBudAnc(enabled))
        }
        SUB_VOLUME_SWIPE => {
            let enabled = value == 0x01;
            Ok(AapEvent::VolumeSwipe(enabled))
        }
        SUB_ADAPTIVE_VOLUME => {
            let enabled = value == 0x01;
            Ok(AapEvent::AdaptiveVolume(enabled))
        }
        SUB_CHIME_VOLUME => {
            Ok(AapEvent::ChimeVolume(value))
        }

        // Known sub-commands — log at debug to reduce noise
        SUB_DOUBLE_CLICK_INTERVAL => {
            debug!("double-click interval: 0x{value:02X}");
            Err(ParseError::UnknownCommand(sub_cmd))
        }
        SUB_CLICK_HOLD_INTERVAL => {
            debug!("click-hold interval: 0x{value:02X}");
            Err(ParseError::UnknownCommand(sub_cmd))
        }
        SUB_VOLUME_SWIPE_INTERVAL => {
            debug!("volume swipe interval: 0x{value:02X}");
            Err(ParseError::UnknownCommand(sub_cmd))
        }
        SUB_CALL_MANAGEMENT => {
            debug!("call management config: 0x{value:02X}");
            Err(ParseError::UnknownCommand(sub_cmd))
        }
        0x29 => {
            // SSL — undocumented
            debug!("control sub-command 0x29 (SSL): 0x{value:02X}");
            Err(ParseError::UnknownCommand(sub_cmd))
        }
        SUB_HEARING_AID => {
            debug!("hearing aid config: 0x{value:02X}");
            Err(ParseError::UnknownCommand(sub_cmd))
        }
        SUB_GAIN_SWIPE => {
            debug!("gain swipe config: 0x{value:02X}");
            Err(ParseError::UnknownCommand(sub_cmd))
        }
        SUB_HEARING_ASSIST => {
            debug!("hearing assist config: 0x{value:02X}");
            Err(ParseError::UnknownCommand(sub_cmd))
        }
        SUB_SLEEP_DETECTION => {
            debug!("sleep detection config: 0x{value:02X}");
            Err(ParseError::UnknownCommand(sub_cmd))
        }
        0x3E => {
            debug!("control sub-command 0x3E: 0x{value:02X}");
            Err(ParseError::UnknownCommand(sub_cmd))
        }

        _ => {
            warn!("unhandled control sub-command: 0x{sub_cmd:02X} = 0x{value:02X}");
            Err(ParseError::UnknownCommand(sub_cmd))
        }
    }
}

/// Parse device info packet
/// Format: header(5) + unknown(6) + null-terminated strings
fn parse_device_info(data: &[u8]) -> Result<AapEvent, ParseError> {
    if data.len() < 12 {
        return Err(ParseError::TooShort(data.len()));
    }

    // Strings start at offset 11, each null-terminated
    let strings: Vec<String> = data[11..]
        .split(|&b| b == 0x00)
        .filter(|s| !s.is_empty())
        .map(|s| String::from_utf8_lossy(s).into_owned())
        .collect();

    Ok(AapEvent::DeviceInfo(DeviceInfoUpdate {
        name: strings.first().cloned().unwrap_or_default(),
        model: strings.get(1).cloned().unwrap_or_default(),
        manufacturer: strings.get(2).cloned().unwrap_or_default(),
        serial: strings.get(3).cloned().unwrap_or_default(),
        firmware: strings.get(4).cloned().unwrap_or_default(),
    }))
}

/// Parse audio source notification (cmd 0x0E)
/// Format (after header): [source_type, ...]
fn parse_audio_source(payload: &[u8]) -> Result<AapEvent, ParseError> {
    if payload.is_empty() {
        return Err(ParseError::TooShort(0));
    }

    let source = match payload[0] {
        0x00 => AudioSource::None,
        0x01 => AudioSource::Call,
        0x02 => AudioSource::Media,
        other => AudioSource::Unknown(other),
    };

    Ok(AapEvent::AudioSource(source))
}

/// Parse conversational awareness activity notification (cmd 0x4B)
/// Format (after header): [0x02, 0x00, 0x01, level]
fn parse_ca_activity(payload: &[u8]) -> Result<AapEvent, ParseError> {
    if payload.len() < 4 {
        return Err(ParseError::TooShort(payload.len()));
    }

    let level = payload[3];
    let activity = match level {
        0x01 | 0x02 => CaActivity::Speaking,
        0x03 => CaActivity::Stopped,
        _ => CaActivity::Normal,
    };

    Ok(AapEvent::ConversationalActivity(activity))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_disconnect() {
        let data = [0x00, 0x01, 0x00, 0x00];
        let event = parse(&data).unwrap();
        assert!(matches!(event, AapEvent::Disconnected));
    }

    #[test]
    fn test_parse_handshake_ack() {
        let data = [0x01, 0x00, 0x04, 0x00, 0x02, 0x00];
        let event = parse(&data).unwrap();
        assert!(matches!(event, AapEvent::HandshakeAck));
    }

    #[test]
    fn test_parse_battery() {
        // Right=100% discharging, Left=99% charging, Case=17% discharging
        let data = [
            0x04, 0x00, 0x04, 0x00, 0x04, 0x00, // header
            0x03, // count=3
            0x02, 0x01, 0x64, 0x02, 0x01, // Right, 100%, discharging
            0x04, 0x01, 0x63, 0x01, 0x01, // Left, 99%, charging
            0x08, 0x01, 0x11, 0x02, 0x01, // Case, 17%, discharging
        ];
        let event = parse(&data).unwrap();
        if let AapEvent::Battery(b) = event {
            let left = b.left.unwrap();
            assert_eq!(left.level, 99);
            assert!(left.charging);

            let right = b.right.unwrap();
            assert_eq!(right.level, 100);
            assert!(!right.charging);

            let case = b.case.unwrap();
            assert_eq!(case.level, 17);
            assert!(!case.charging);
        } else {
            panic!("expected Battery event");
        }
    }

    #[test]
    fn test_parse_anc_mode() {
        for (mode_byte, expected_str) in [
            (0x01, "off"),
            (0x02, "noise"),
            (0x03, "transparency"),
            (0x04, "adaptive"),
        ] {
            let data = [0x04, 0x00, 0x04, 0x00, 0x09, 0x00, 0x0D, mode_byte, 0x00, 0x00, 0x00];
            let event = parse(&data).unwrap();
            if let AapEvent::AncMode(mode) = event {
                assert_eq!(mode.as_str(), expected_str);
            } else {
                panic!("expected AncMode event for byte 0x{mode_byte:02X}");
            }
        }
    }

    #[test]
    fn test_parse_ear_detection() {
        // Primary in ear, secondary out of ear
        let data = [0x04, 0x00, 0x04, 0x00, 0x06, 0x00, 0x00, 0x01];
        let event = parse(&data).unwrap();
        if let AapEvent::EarDetection(ed) = event {
            assert!(ed.primary.is_in_ear());
            assert!(!ed.secondary.is_in_ear());
        } else {
            panic!("expected EarDetection event");
        }
    }

    #[test]
    fn test_parse_conversational_awareness() {
        let data = [0x04, 0x00, 0x04, 0x00, 0x09, 0x00, 0x28, 0x01, 0x00, 0x00, 0x00];
        let event = parse(&data).unwrap();
        assert!(matches!(event, AapEvent::ConversationalAwareness(true)));

        let data = [0x04, 0x00, 0x04, 0x00, 0x09, 0x00, 0x28, 0x02, 0x00, 0x00, 0x00];
        let event = parse(&data).unwrap();
        assert!(matches!(event, AapEvent::ConversationalAwareness(false)));
    }

    #[test]
    fn test_parse_adaptive_noise_level() {
        let data = [0x04, 0x00, 0x04, 0x00, 0x09, 0x00, 0x2E, 0x32, 0x00, 0x00, 0x00];
        let event = parse(&data).unwrap();
        assert!(matches!(event, AapEvent::AdaptiveNoiseLevel(50)));
    }

    #[test]
    fn test_parse_one_bud_anc() {
        let data = [0x04, 0x00, 0x04, 0x00, 0x09, 0x00, 0x1B, 0x01, 0x00, 0x00, 0x00];
        let event = parse(&data).unwrap();
        assert!(matches!(event, AapEvent::OneBudAnc(true)));

        let data = [0x04, 0x00, 0x04, 0x00, 0x09, 0x00, 0x1B, 0x02, 0x00, 0x00, 0x00];
        let event = parse(&data).unwrap();
        assert!(matches!(event, AapEvent::OneBudAnc(false)));
    }

    #[test]
    fn test_parse_volume_swipe() {
        let data = [0x04, 0x00, 0x04, 0x00, 0x09, 0x00, 0x25, 0x01, 0x00, 0x00, 0x00];
        let event = parse(&data).unwrap();
        assert!(matches!(event, AapEvent::VolumeSwipe(true)));

        let data = [0x04, 0x00, 0x04, 0x00, 0x09, 0x00, 0x25, 0x02, 0x00, 0x00, 0x00];
        let event = parse(&data).unwrap();
        assert!(matches!(event, AapEvent::VolumeSwipe(false)));
    }

    #[test]
    fn test_parse_adaptive_volume() {
        let data = [0x04, 0x00, 0x04, 0x00, 0x09, 0x00, 0x26, 0x01, 0x00, 0x00, 0x00];
        let event = parse(&data).unwrap();
        assert!(matches!(event, AapEvent::AdaptiveVolume(true)));

        let data = [0x04, 0x00, 0x04, 0x00, 0x09, 0x00, 0x26, 0x02, 0x00, 0x00, 0x00];
        let event = parse(&data).unwrap();
        assert!(matches!(event, AapEvent::AdaptiveVolume(false)));
    }

    #[test]
    fn test_parse_chime_volume() {
        let data = [0x04, 0x00, 0x04, 0x00, 0x09, 0x00, 0x1F, 0x50, 0x00, 0x00, 0x00];
        let event = parse(&data).unwrap();
        assert!(matches!(event, AapEvent::ChimeVolume(0x50)));
    }

    #[test]
    fn test_parse_audio_source() {
        let data = [0x04, 0x00, 0x04, 0x00, 0x0E, 0x00, 0x02, 0x00, 0x00];
        let event = parse(&data).unwrap();
        assert!(matches!(event, AapEvent::AudioSource(AudioSource::Media)));

        let data = [0x04, 0x00, 0x04, 0x00, 0x0E, 0x00, 0x01, 0x00, 0x00];
        let event = parse(&data).unwrap();
        assert!(matches!(event, AapEvent::AudioSource(AudioSource::Call)));

        let data = [0x04, 0x00, 0x04, 0x00, 0x0E, 0x00, 0x00, 0x00, 0x00];
        let event = parse(&data).unwrap();
        assert!(matches!(event, AapEvent::AudioSource(AudioSource::None)));
    }

    #[test]
    fn test_parse_head_tracking() {
        // Head tracking packets are variable length
        let data = [0x04, 0x00, 0x04, 0x00, 0x17, 0x00, 0x01, 0x02, 0x03];
        let event = parse(&data).unwrap();
        if let AapEvent::HeadTracking(payload) = event {
            assert_eq!(payload, vec![0x01, 0x02, 0x03]);
        } else {
            panic!("expected HeadTracking event");
        }
    }

    #[test]
    fn test_known_commands_debug_not_warn() {
        // These should return Err but NOT log at warn level (they use debug)
        // We just verify they parse without panicking and return the expected error
        let commands: &[u8] = &[0x02, 0x08, 0x0C, 0x4E, 0x52, 0x53, 0x55];
        for &cmd in commands {
            let data = [0x04, 0x00, 0x04, 0x00, cmd, 0x00, 0x00, 0x00, 0x00, 0x00];
            let result = parse(&data);
            assert!(result.is_err(), "command 0x{cmd:02X} should return Err");
        }
    }

    #[test]
    fn test_known_control_subcmds_debug_not_warn() {
        // Known but unhandled control sub-commands should not panic
        let subcmds: &[u8] = &[0x17, 0x18, 0x23, 0x24, 0x29, 0x2C, 0x2F, 0x33, 0x35, 0x3E];
        for &sub in subcmds {
            let data = [0x04, 0x00, 0x04, 0x00, 0x09, 0x00, sub, 0x00, 0x00, 0x00, 0x00];
            let result = parse(&data);
            assert!(result.is_err(), "sub-command 0x{sub:02X} should return Err");
        }
    }
}
