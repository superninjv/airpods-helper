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
    ConversationalActivity(#[allow(dead_code)] CaActivity),
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
    #[allow(dead_code)]
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
    #[allow(dead_code)]
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

    if data == DISCONNECT_PACKET {
        return Ok(AapEvent::Disconnected);
    }

    if data.len() >= 4 && data[0] == 0x01 && data[1] == 0x00 && data[2] == 0x04 && data[3] == 0x00
    {
        return Ok(AapEvent::HandshakeAck);
    }

    if data.len() < 6 {
        return Err(ParseError::TooShort(data.len()));
    }

    if data[0..4] != HEADER {
        return Err(ParseError::UnknownCommand(data[0]));
    }

    let cmd = data[4];

    match cmd {
        0x2B => Ok(AapEvent::FeaturesAck),
        CMD_BATTERY => parse_battery(&data[6..]),
        CMD_EAR_DETECTION => parse_ear_detection(&data[6..]),
        CMD_CONTROL => parse_control(&data[6..]),
        CMD_AUDIO_SOURCE => parse_audio_source(&data[6..]),
        CMD_HEAD_TRACKING => {
            debug!("head tracking data, len={}", data.len());
            Ok(AapEvent::HeadTracking(data[6..].to_vec()))
        }
        CMD_STEM_PRESS => {
            debug!("stem press event, len={}", data.len());
            Err(ParseError::UnknownCommand(cmd))
        }
        CMD_DEVICE_INFO => parse_device_info(data),
        CMD_CONNECTED_DEVICES => {
            debug!("connected devices notification, len={}", data.len());
            Err(ParseError::UnknownCommand(cmd))
        }
        CMD_CA_ACTIVITY => parse_ca_activity(&data[6..]),
        CMD_EQ_DATA => {
            debug!("EQ data packet, len={}", data.len());
            Err(ParseError::UnknownCommand(cmd))
        }
        0x02 | 0x08 | 0x0C | 0x10 | 0x11 | 0x12 | 0x14 | 0x4E | 0x52 | 0x55 => {
            debug!("known unhandled command 0x{cmd:02X}, len={}", data.len());
            Err(ParseError::UnknownCommand(cmd))
        }
        _ => {
            warn!("unknown AAP command: 0x{cmd:02X}, len={}", data.len());
            Err(ParseError::UnknownCommand(cmd))
        }
    }
}

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
        SUB_CONVERSATIONAL_AWARENESS => Ok(AapEvent::ConversationalAwareness(value == 0x01)),
        SUB_ADAPTIVE_NOISE_LEVEL => Ok(AapEvent::AdaptiveNoiseLevel(value)),
        SUB_ONE_BUD_ANC => Ok(AapEvent::OneBudAnc(value == 0x01)),
        SUB_VOLUME_SWIPE => Ok(AapEvent::VolumeSwipe(value == 0x01)),
        SUB_ADAPTIVE_VOLUME => Ok(AapEvent::AdaptiveVolume(value == 0x01)),
        SUB_CHIME_VOLUME => Ok(AapEvent::ChimeVolume(value)),
        SUB_DOUBLE_CLICK_INTERVAL | SUB_CLICK_HOLD_INTERVAL | SUB_VOLUME_SWIPE_INTERVAL
        | SUB_CALL_MANAGEMENT | SUB_HEARING_AID | SUB_GAIN_SWIPE | SUB_HEARING_ASSIST
        | SUB_SLEEP_DETECTION | 0x29 | 0x3E => {
            debug!("known control sub-command 0x{sub_cmd:02X}: 0x{value:02X}");
            Err(ParseError::UnknownCommand(sub_cmd))
        }
        _ => {
            warn!("unhandled control sub-command: 0x{sub_cmd:02X} = 0x{value:02X}");
            Err(ParseError::UnknownCommand(sub_cmd))
        }
    }
}

fn parse_device_info(data: &[u8]) -> Result<AapEvent, ParseError> {
    if data.len() < 12 {
        return Err(ParseError::TooShort(data.len()));
    }

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
