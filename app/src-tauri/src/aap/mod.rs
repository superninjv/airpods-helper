pub mod commands;
pub mod parser;

/// AAP (Apple Accessory Protocol) constants
///
/// Transport: L2CAP, PSM 0x1001 (4097)
/// All control packets share header: 04 00 04 00 [cmd] 00 [payload]

/// L2CAP PSM for AAP control channel
pub const AAP_PSM: u16 = 0x1001;

/// AirPods service UUID for device identification
pub const AIRPODS_SERVICE_UUID: &str = "74ec2172-0bad-4d01-8f77-997b2be0722a";

/// Common packet header for control commands
pub const HEADER: [u8; 4] = [0x04, 0x00, 0x04, 0x00];

/// Command IDs (byte at offset 4)
pub const CMD_BATTERY: u8 = 0x04;
pub const CMD_EAR_DETECTION: u8 = 0x06;
pub const CMD_CONTROL: u8 = 0x09;
pub const CMD_AUDIO_SOURCE: u8 = 0x0E;
pub const CMD_HEAD_TRACKING: u8 = 0x17;
pub const CMD_STEM_PRESS: u8 = 0x19;
pub const CMD_DEVICE_INFO: u8 = 0x1D;
pub const CMD_CONNECTED_DEVICES: u8 = 0x2E;
pub const CMD_CA_ACTIVITY: u8 = 0x4B;
pub const CMD_EQ_DATA: u8 = 0x53;

/// Control sub-commands (byte at offset 6, under CMD_CONTROL)
pub const SUB_ANC_MODE: u8 = 0x0D;
pub const SUB_DOUBLE_CLICK_INTERVAL: u8 = 0x17;
pub const SUB_CLICK_HOLD_INTERVAL: u8 = 0x18;
pub const SUB_ONE_BUD_ANC: u8 = 0x1B;
pub const SUB_CHIME_VOLUME: u8 = 0x1F;
pub const SUB_VOLUME_SWIPE_INTERVAL: u8 = 0x23;
pub const SUB_CALL_MANAGEMENT: u8 = 0x24;
pub const SUB_VOLUME_SWIPE: u8 = 0x25;
pub const SUB_ADAPTIVE_VOLUME: u8 = 0x26;
pub const SUB_CONVERSATIONAL_AWARENESS: u8 = 0x28;
pub const SUB_HEARING_AID: u8 = 0x2C;
pub const SUB_ADAPTIVE_NOISE_LEVEL: u8 = 0x2E;
pub const SUB_GAIN_SWIPE: u8 = 0x2F;
pub const SUB_HEARING_ASSIST: u8 = 0x33;
pub const SUB_SLEEP_DETECTION: u8 = 0x35;

/// ANC mode values (byte at offset 7)
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum AncMode {
    Off = 0x01,
    NoiseCancellation = 0x02,
    Transparency = 0x03,
    Adaptive = 0x04,
}

impl AncMode {
    pub fn from_byte(b: u8) -> Option<Self> {
        match b {
            0x01 => Some(Self::Off),
            0x02 => Some(Self::NoiseCancellation),
            0x03 => Some(Self::Transparency),
            0x04 => Some(Self::Adaptive),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::NoiseCancellation => "noise",
            Self::Transparency => "transparency",
            Self::Adaptive => "adaptive",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "off" => Some(Self::Off),
            "noise" => Some(Self::NoiseCancellation),
            "transparency" => Some(Self::Transparency),
            "adaptive" => Some(Self::Adaptive),
            _ => None,
        }
    }
}

/// Battery component identifiers
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatteryComponent {
    Right = 0x02,
    Left = 0x04,
    Case = 0x08,
}

impl BatteryComponent {
    pub fn from_byte(b: u8) -> Option<Self> {
        match b {
            0x02 => Some(Self::Right),
            0x04 => Some(Self::Left),
            0x08 => Some(Self::Case),
            _ => None,
        }
    }
}

/// Battery charging status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChargingStatus {
    Unknown = 0x00,
    Charging = 0x01,
    Discharging = 0x02,
    Disconnected = 0x04,
}

impl ChargingStatus {
    pub fn from_byte(b: u8) -> Option<Self> {
        match b {
            0x00 => Some(Self::Unknown),
            0x01 => Some(Self::Charging),
            0x02 => Some(Self::Discharging),
            0x04 => Some(Self::Disconnected),
            _ => None,
        }
    }
}

/// Ear detection status for a single pod
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EarStatus {
    InEar = 0x00,
    OutOfEar = 0x01,
    InCase = 0x02,
}

impl EarStatus {
    pub fn from_byte(b: u8) -> Option<Self> {
        match b {
            0x00 => Some(Self::InEar),
            0x01 => Some(Self::OutOfEar),
            0x02 => Some(Self::InCase),
            _ => None,
        }
    }

    pub fn is_in_ear(&self) -> bool {
        matches!(self, Self::InEar)
    }
}

/// Disconnection sentinel
pub const DISCONNECT_PACKET: [u8; 4] = [0x00, 0x01, 0x00, 0x00];
