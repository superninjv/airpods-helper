//! Microphone audio over AAP (opcode 0x58).
//!
//! AirPods can stream the microphone as AAC-ELD over the AAP channel while
//! A2DP keeps playing, so the host gets stereo output and a mic at once. The
//! host sends START, the buds answer with a steady run of audio SDUs, and STOP
//! ends it. Nothing is acknowledged and there is no keepalive.
//!
//! Source: LibrePods PR #655 (linux-rust/src/bluetooth/aacp.rs and
//! aacp_audio.rs), bytes taken from Apple packet captures. Verified there on
//! AirPods Pro 3 only. Most header fields are undocumented; this module only
//! relies on the ones every known implementation checks.

use super::HEADER;

/// AAP opcode carrying both the mic control packets and the audio.
pub const CMD_MIC_AUDIO: u8 = 0x58;

/// Start streaming the microphone. Layout as far as it is understood:
/// `04 00 04 00 | 58 00 | 00 00 (control) | 09 00 (payload length) | payload`.
/// The payload's `0x82` (130) matches Apple's "AAC-ELD-Stereo48K-10ms" mode id,
/// which may be a coincidence; the rest is opaque.
pub const START: [u8; 19] = [
    HEADER[0], HEADER[1], HEADER[2], HEADER[3],
    CMD_MIC_AUDIO, 0x00, 0x00, 0x00, 0x09, 0x00,
    0x00, 0x01, 0x82, 0x00, 0x00, 0x00, 0x04, 0x96, 0x00,
];

/// Stop streaming the microphone (2-byte control payload).
pub const STOP: [u8; 12] = [
    HEADER[0], HEADER[1], HEADER[2], HEADER[3],
    CMD_MIC_AUDIO, 0x00, 0x00, 0x00, 0x02, 0x00,
    0x03, 0x01,
];

/// Subtype (u16 LE at offset 6) of a 0x58 packet that carries audio; control
/// packets use 0x0000.
const SUBTYPE_AUDIO: u16 = 0x0001;

/// Audio SDUs start with a 22-byte header. Only bytes 0..8 are understood
/// (header, opcode, subtype); 8..22 are skipped by every implementation.
const AUDIO_HEADER_LEN: usize = 22;

/// Each access unit is framed as `[timestamp: u32 LE][length: u8][AU bytes]`.
const AU_PREFIX_LEN: usize = 5;

/// The AirPods' AAC-ELD stream config (AudioSpecificConfig): AOT 39 (ER AAC
/// ELD), 48 kHz table index, mono, 480-sample frames, no LD-SBR. Used to set
/// up the decoder; frames arrive as raw access units with no transport framing.
pub const ELD_ASC: [u8; 4] = [0xF8, 0xE6, 0x30, 0x00];

/// PCM samples one access unit decodes to (the ASC's 480-sample frame).
pub const ELD_FRAME_SAMPLES: usize = 480;

/// Largest SDU seen in practice is over 1 KB (four AUs plus header); a
/// SEQPACKET recv truncates anything past the buffer silently, so receive
/// buffers must be at least this big. Value from LibrePods PR #655.
pub const MAX_SDU_LEN: usize = 4096;

fn u16le(data: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([data[at], data[at + 1]])
}

/// Is this an incoming mic audio SDU? Checked before the general parser,
/// since these arrive ~33 times a second while streaming.
pub fn is_audio(sdu: &[u8]) -> bool {
    sdu.len() >= 8
        && sdu[0..4] == HEADER
        && u16le(sdu, 4) == CMD_MIC_AUDIO as u16
        && u16le(sdu, 6) == SUBTYPE_AUDIO
}

/// Walk the access units in an audio SDU, in order. A truncated trailing
/// entry is dropped rather than read past the end.
pub fn access_units(sdu: &[u8]) -> impl Iterator<Item = &[u8]> {
    let mut offset = AUDIO_HEADER_LEN;
    std::iter::from_fn(move || {
        if offset + AU_PREFIX_LEN > sdu.len() {
            return None;
        }
        let len = sdu[offset + 4] as usize;
        let start = offset + AU_PREFIX_LEN;
        let end = start + len;
        if end > sdu.len() {
            return None;
        }
        offset = end;
        Some(&sdu[start..end])
    })
}

/// Build an audio SDU the way the AirPods do. Used to feed the daemon's mic
/// path synthetic audio when no AirPods are around.
pub fn build_audio_sdu<'a>(units: impl IntoIterator<Item = (u32, &'a [u8])>) -> Vec<u8> {
    let mut sdu = vec![0u8; AUDIO_HEADER_LEN];
    sdu[0..4].copy_from_slice(&HEADER);
    sdu[4..6].copy_from_slice(&(CMD_MIC_AUDIO as u16).to_le_bytes());
    sdu[6..8].copy_from_slice(&SUBTYPE_AUDIO.to_le_bytes());
    for (timestamp, au) in units {
        let len = u8::try_from(au.len()).expect("access unit longer than 255 bytes");
        sdu.extend_from_slice(&timestamp.to_le_bytes());
        sdu.push(len);
        sdu.extend_from_slice(au);
    }
    sdu
}

