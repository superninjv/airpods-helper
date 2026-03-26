use super::*;
use thiserror::Error;
use tracing::warn;

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
    Disconnected,
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
    pub name: String,
    pub model: String,
    pub manufacturer: String,
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
        CMD_DEVICE_INFO => parse_device_info(data),
        CMD_CA_ACTIVITY => parse_ca_activity(&data[6..]),

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
}
