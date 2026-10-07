//! AAC-ELD decoding through the system's libfdk-aac, loaded at runtime.
//!
//! We don't link against it: fdk-aac is non-free on some distros (Debian,
//! Fedora ship it separately or not at all), so the daemon has to build and run
//! without it. If the library is missing the mic feature reports itself
//! unavailable and everything else keeps working. Only a handful of functions
//! from the stable fdk-aac 2.x C API are used (see aacdecoder_lib.h).

use std::ffi::{CStr, c_char, c_int, c_uint, c_void};

/// Sonames to try, most specific first. fdk-aac 2.x keeps soname .so.2; the
/// bare name covers distros that only ship the dev symlink.
const LIBRARY_NAMES: &[&CStr] = &[c"libfdk-aac.so.2", c"libfdk-aac.so"];

/// `TRANSPORT_TYPE::TT_MP4_RAW`: access units handed over one at a time with
/// no transport framing, configured out of band with the ASC.
const TT_MP4_RAW: c_int = 0;
const AAC_DEC_OK: c_int = 0;
/// Output buffer for one decoded frame, in samples. ELD frames are 480/512
/// samples per channel; this leaves room for stereo plus slack.
const DECODE_BUF_SAMPLES: usize = 4096;

/// The leading fields of fdk-aac's `CStreamInfo`. Only these three are read,
/// and they have been first in the struct since fdk-aac 0.1.
#[repr(C)]
struct StreamInfoHead {
    sample_rate: c_int,
    frame_size: c_int,
    num_channels: c_int,
}

type Handle = *mut c_void;
type OpenFn = unsafe extern "C" fn(transport: c_int, layers: c_uint) -> Handle;
type ConfigRawFn = unsafe extern "C" fn(Handle, *mut *mut u8, *const c_uint) -> c_int;
type FillFn = unsafe extern "C" fn(Handle, *mut *mut u8, *const c_uint, *mut c_uint) -> c_int;
type DecodeFrameFn = unsafe extern "C" fn(Handle, *mut i16, c_int, c_uint) -> c_int;
type GetStreamInfoFn = unsafe extern "C" fn(Handle) -> *const StreamInfoHead;
type CloseFn = unsafe extern "C" fn(Handle);

/// The resolved library: just its function pointers, so it's cheap to copy
/// into each decoder. Never unloaded: dlclose buys nothing for a daemon that
/// may reopen it on the next connect.
#[derive(Clone, Copy)]
pub struct Library {
    open: OpenFn,
    config_raw: ConfigRawFn,
    fill: FillFn,
    decode_frame: DecodeFrameFn,
    get_stream_info: GetStreamInfoFn,
    close: CloseFn,
}

fn dlerror() -> String {
    // SAFETY: dlerror returns a thread-local, NUL-terminated string or null.
    let msg = unsafe { libc::dlerror() };
    if msg.is_null() {
        "unknown error".into()
    } else {
        unsafe { CStr::from_ptr(msg) }.to_string_lossy().into_owned()
    }
}

/// dlopen libfdk-aac by soname. The error says what to install.
fn open_library() -> Result<*mut c_void, String> {
    for name in LIBRARY_NAMES {
        // SAFETY: plain dlopen of a shared library by soname.
        let lib = unsafe { libc::dlopen(name.as_ptr() as *const c_char, libc::RTLD_NOW | libc::RTLD_LOCAL) };
        if !lib.is_null() {
            return Ok(lib);
        }
    }
    Err(format!(
        "libfdk-aac not found ({}); install it to use the AirPods microphone (Arch: libfdk-aac, Debian: libfdk-aac2 from non-free)",
        dlerror()
    ))
}

/// Look up one symbol and cast it to its function pointer type.
///
/// SAFETY: the caller names a symbol whose C signature matches `F`.
unsafe fn symbol<F: Copy>(lib: *mut c_void, name: &CStr) -> Result<F, String> {
    let ptr = unsafe { libc::dlsym(lib, name.as_ptr()) };
    if ptr.is_null() {
        return Err(format!("libfdk-aac has no {}: {}", name.to_string_lossy(), dlerror()));
    }
    Ok(unsafe { std::mem::transmute_copy::<*mut c_void, F>(&ptr) })
}

impl Library {
    /// Find and load libfdk-aac. The error says what to install.
    pub fn load() -> Result<Self, String> {
        let lib = open_library()?;
        // SAFETY: every signature below is copied from aacdecoder_lib.h (fdk-aac 2.x).
        unsafe {
            Ok(Self {
                open: symbol(lib, c"aacDecoder_Open")?,
                config_raw: symbol(lib, c"aacDecoder_ConfigRaw")?,
                fill: symbol(lib, c"aacDecoder_Fill")?,
                decode_frame: symbol(lib, c"aacDecoder_DecodeFrame")?,
                get_stream_info: symbol(lib, c"aacDecoder_GetStreamInfo")?,
                close: symbol(lib, c"aacDecoder_Close")?,
            })
        }
    }

    /// Open a decoder for raw access units described by `asc`.
    pub fn decoder(&self, asc: &[u8]) -> Result<Decoder, String> {
        // SAFETY: open takes plain integers; a null handle means failure.
        let handle = unsafe { (self.open)(TT_MP4_RAW, 1) };
        if handle.is_null() {
            return Err("aacDecoder_Open failed".into());
        }
        let decoder = Decoder {
            lib: *self,
            handle,
            pcm: vec![0; DECODE_BUF_SAMPLES],
        };
        let mut conf = asc.to_vec();
        let mut conf_ptr = conf.as_mut_ptr();
        let conf_len = conf.len() as c_uint;
        // SAFETY: one config buffer, valid for the duration of the call.
        let err = unsafe { (self.config_raw)(handle, &mut conf_ptr, &conf_len) };
        if err != AAC_DEC_OK {
            return Err(format!("decoder rejected the AAC-ELD config (fdk error 0x{err:04X})"));
        }
        Ok(decoder)
    }
}

/// One open AAC decoder. Closed on drop.
pub struct Decoder {
    lib: Library,
    handle: Handle,
    pcm: Vec<i16>,
}

// SAFETY: an fdk decoder instance has no thread affinity; it is only ever
// used by the one task that owns it.
unsafe impl Send for Decoder {}

impl Decoder {
    /// Decode one access unit to mono 16-bit PCM. If the stream turns out to
    /// have more than one channel, the first is kept, since the source is mono.
    pub fn decode(&mut self, au: &[u8]) -> Result<&[i16], String> {
        let mut input = au.to_vec();
        let mut input_ptr = input.as_mut_ptr();
        let input_len = input.len() as c_uint;
        let mut bytes_valid = input_len;
        // SAFETY: buffers outlive the calls; sizes are what we allocated.
        unsafe {
            let err = (self.lib.fill)(self.handle, &mut input_ptr, &input_len, &mut bytes_valid);
            if err != AAC_DEC_OK {
                return Err(format!("fill failed (fdk error 0x{err:04X})"));
            }
            let err = (self.lib.decode_frame)(self.handle, self.pcm.as_mut_ptr(), self.pcm.len() as c_int, 0);
            if err != AAC_DEC_OK {
                return Err(format!("decode failed (fdk error 0x{err:04X})"));
            }
            let info = (self.lib.get_stream_info)(self.handle);
            if info.is_null() {
                return Err("decoder returned no stream info".into());
            }
            let frame = (*info).frame_size.max(0) as usize;
            let channels = (*info).num_channels.max(1) as usize;
            let total = (frame * channels).min(self.pcm.len());
            if channels > 1 {
                // Keep channel 0 of the interleaved frame, packed at the front.
                for i in 0..frame.min(total / channels) {
                    self.pcm[i] = self.pcm[i * channels];
                }
            }
            Ok(&self.pcm[..frame.min(total)])
        }
    }

    /// The sample rate the decoder believes the stream has (from the ASC).
    pub fn sample_rate(&self) -> Option<u32> {
        // SAFETY: valid handle; null-checked below.
        let info = unsafe { (self.lib.get_stream_info)(self.handle) };
        (!info.is_null()).then(|| unsafe { (*info).sample_rate } as u32)
    }
}

impl Drop for Decoder {
    fn drop(&mut self) {
        // SAFETY: handle came from aacDecoder_Open and is closed exactly once.
        unsafe { (self.lib.close)(self.handle) }
    }
}

/// AAC-ELD encoder, only for feeding the mic path synthetic audio in the
/// tracer. Same library, encoder half of the API (aacenc_lib.h).
#[cfg(test)]
pub mod encoder {
    use super::*;

    const AACENC_OK: c_int = 0;
    // AACENC_PARAM ids from aacenc_lib.h.
    const AACENC_AOT: c_uint = 0x0100;
    const AACENC_BITRATE: c_uint = 0x0101;
    const AACENC_SAMPLERATE: c_uint = 0x0103;
    const AACENC_SBR_MODE: c_uint = 0x0104;
    const AACENC_GRANULE_LENGTH: c_uint = 0x0105;
    const AACENC_CHANNELMODE: c_uint = 0x0106;
    const AACENC_AFTERBURNER: c_uint = 0x0200;
    const AACENC_TRANSMUX: c_uint = 0x0300;
    const AOT_ER_AAC_ELD: c_uint = 39;
    const MODE_1: c_uint = 1;
    const IN_AUDIO_DATA: c_int = 0;
    const OUT_BITSTREAM_DATA: c_int = 3;

    #[repr(C)]
    struct InfoStruct {
        max_out_buf_bytes: c_uint,
        max_anc_bytes: c_uint,
        in_buf_fill_level: c_uint,
        input_channels: c_uint,
        frame_length: c_uint,
        n_delay: c_uint,
        n_delay_core: c_uint,
        conf_buf: [u8; 64],
        conf_size: c_uint,
    }
    #[repr(C)]
    struct BufDesc {
        num_bufs: c_int,
        bufs: *mut *mut c_void,
        buffer_identifiers: *mut c_int,
        buf_sizes: *mut c_int,
        buf_el_sizes: *mut c_int,
    }
    #[repr(C)]
    struct InArgs {
        num_in_samples: c_int,
        num_anc_bytes: c_int,
    }
    #[repr(C)]
    #[derive(Default)]
    struct OutArgs {
        num_out_bytes: c_int,
        num_in_samples: c_int,
        num_anc_bytes: c_int,
        bit_res_state: c_int,
    }

    type EncOpenFn = unsafe extern "C" fn(*mut Handle, c_uint, c_uint) -> c_int;
    type SetParamFn = unsafe extern "C" fn(Handle, c_uint, c_uint) -> c_int;
    type EncodeFn = unsafe extern "C" fn(Handle, *const BufDesc, *const BufDesc, *const InArgs, *mut OutArgs) -> c_int;
    type InfoFn = unsafe extern "C" fn(Handle, *mut InfoStruct) -> c_int;
    type EncCloseFn = unsafe extern "C" fn(*mut Handle) -> c_int;

    pub struct Encoder {
        handle: Handle,
        encode: EncodeFn,
        close: EncCloseFn,
        /// The AudioSpecificConfig the encoder produced.
        pub asc: Vec<u8>,
        pub frame_length: usize,
    }

    impl Encoder {
        /// Mono AAC-ELD, 480-sample frames, no SBR, raw access units: the
        /// stream shape the AirPods send.
        pub fn eld_mono(sample_rate: u32, bitrate: u32) -> Result<Self, String> {
            let lib = open_library()?;
            unsafe {
                let open: EncOpenFn = symbol(lib, c"aacEncOpen")?;
                let set: SetParamFn = symbol(lib, c"aacEncoder_SetParam")?;
                let encode: EncodeFn = symbol(lib, c"aacEncEncode")?;
                let info_fn: InfoFn = symbol(lib, c"aacEncInfo")?;
                let close: EncCloseFn = symbol(lib, c"aacEncClose")?;
                let mut handle: Handle = std::ptr::null_mut();
                if open(&mut handle, 0, 1) != AACENC_OK {
                    return Err("aacEncOpen failed".into());
                }
                for (param, value) in [
                    (AACENC_AOT, AOT_ER_AAC_ELD),
                    (AACENC_SAMPLERATE, sample_rate),
                    (AACENC_CHANNELMODE, MODE_1),
                    (AACENC_GRANULE_LENGTH, 480),
                    (AACENC_SBR_MODE, 0),
                    (AACENC_BITRATE, bitrate),
                    (AACENC_TRANSMUX, TT_MP4_RAW as c_uint),
                    (AACENC_AFTERBURNER, 1),
                ] {
                    let err = set(handle, param, value);
                    if err != AACENC_OK {
                        return Err(format!("aacEncoder_SetParam(0x{param:04X}, {value}) failed: 0x{err:04X}"));
                    }
                }
                // Null call applies the parameters.
                let err = encode(handle, std::ptr::null(), std::ptr::null(), std::ptr::null(), std::ptr::null_mut());
                if err != AACENC_OK {
                    return Err(format!("encoder init failed: 0x{err:04X}"));
                }
                let mut info: InfoStruct = std::mem::zeroed();
                if info_fn(handle, &mut info) != AACENC_OK {
                    return Err("aacEncInfo failed".into());
                }
                Ok(Self {
                    handle,
                    encode,
                    close,
                    asc: info.conf_buf[..info.conf_size as usize].to_vec(),
                    frame_length: info.frame_length as usize,
                })
            }
        }

        /// Encode one frame of mono PCM; returns the access unit (may be
        /// empty while the encoder fills its delay line).
        pub fn encode(&mut self, pcm: &[i16]) -> Result<Vec<u8>, String> {
            let mut out = vec![0u8; 2048];
            let mut in_ptr = pcm.as_ptr() as *mut c_void;
            let mut in_id = IN_AUDIO_DATA;
            let mut in_size = (pcm.len() * 2) as c_int;
            let mut in_el = 2 as c_int;
            let in_desc = BufDesc {
                num_bufs: 1,
                bufs: &mut in_ptr,
                buffer_identifiers: &mut in_id,
                buf_sizes: &mut in_size,
                buf_el_sizes: &mut in_el,
            };
            let mut out_ptr = out.as_mut_ptr() as *mut c_void;
            let mut out_id = OUT_BITSTREAM_DATA;
            let mut out_size = out.len() as c_int;
            let mut out_el = 1 as c_int;
            let out_desc = BufDesc {
                num_bufs: 1,
                bufs: &mut out_ptr,
                buffer_identifiers: &mut out_id,
                buf_sizes: &mut out_size,
                buf_el_sizes: &mut out_el,
            };
            let in_args = InArgs {
                num_in_samples: pcm.len() as c_int,
                num_anc_bytes: 0,
            };
            let mut out_args = OutArgs::default();
            // SAFETY: all descriptors point at live buffers of the stated sizes.
            let err = unsafe { (self.encode)(self.handle, &in_desc, &out_desc, &in_args, &mut out_args) };
            if err != AACENC_OK {
                return Err(format!("aacEncEncode failed: 0x{err:04X}"));
            }
            out.truncate(out_args.num_out_bytes.max(0) as usize);
            Ok(out)
        }
    }

    impl Drop for Encoder {
        fn drop(&mut self) {
            // SAFETY: handle from aacEncOpen, closed once.
            unsafe { (self.close)(&mut self.handle) };
        }
    }
}
