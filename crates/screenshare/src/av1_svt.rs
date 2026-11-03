use std::ffi::{CStr, c_char, c_void};
use std::ptr;
use std::sync::{Mutex, Once, PoisonError};

use crate::codec::{Codec, VideoEncoder};
use crate::{EncodedFrame, EncoderConfig, Error, I420Frame, Result};

mod ffi {
    #![allow(
        non_upper_case_globals,
        non_camel_case_types,
        non_snake_case,
        dead_code,
        unsafe_op_in_unsafe_fn,
        clippy::all
    )]

    include!(concat!(env!("OUT_DIR"), "/svt.rs"));
}

/// SVT-AV1 preset (0 slowest … 13 fastest) for realtime coding; the self-test (FR-7.56) decides
/// whether the machine is fast enough at all.
const PRESET: i8 = 10;

/// The fastest preset that still has the screen-content tools: SVT-AV1 turns them off in realtime
/// mode from preset 9 on. They cost about four times the encode time of [`PRESET`] (1080p30: about
/// 70% of the frame budget on a 16-thread desktop CPU, 720p30 about 40%), which is why Sharp text
/// lowers the frame rate first (FR-7.38).
const SCREEN_CONTENT_PRESET: i8 = 8;

/// Temporal layers (pattern 0-2-1-2). SVT-AV1's low-delay CBR mode allows at most three
/// (hierarchical levels 0–2), so unlike VP9 and the hardware encoders it has no fourth layer at
/// 60 fps; software AV1 is capped at 30 fps anyway (FR-7.37).
pub const AV1_TEMPORAL_LAYERS: u8 = 3;

/// Presentation times are milliseconds.
const TIMEBASE_MS: i64 = 1000;

/// Serialises creating and shutting down encoders: SVT-AV1 sets up process-wide state (CPU
/// feature dispatch, logging) there without locking, and parallel calls crashed.
static LIFECYCLE: Mutex<()> = Mutex::new(());

/// SVT-AV1 logs its version banner and configuration at info level to stdout, and warns about
/// tools the realtime configuration turns off on purpose (every encoder start). The app only
/// wants its errors, on stderr like the rest of its diagnostics.
static LOG_CALLBACK: Once = Once::new();

/// How the platform passes a C `va_list` to a function (a pointer, an array or a struct), taken
/// from the callback type bindgen generated so it matches `vsnprintf` on every target.
type VaList = <ffi::SvtAv1LogCallback as LastArgument>::Type;

trait LastArgument {
    type Type;
}

impl<A, B, C, D, E> LastArgument for Option<unsafe extern "C" fn(A, B, C, D, E)> {
    type Type = E;
}

/// Receives every SVT-AV1 log line (on its worker threads too, never on the audio thread).
unsafe extern "C" fn svt_log(
    _context: *mut c_void,
    level: ffi::SvtAv1LogLevel,
    tag: *const c_char,
    format: *const c_char,
    args: VaList,
) {
    if level > ffi::SVT_AV1_LOG_ERROR {
        return;
    }
    let mut buffer = [0 as c_char; 512];
    // SAFETY: SVT-AV1 passes a printf format with its matching arguments; vsnprintf truncates to
    // the buffer and always terminates it.
    let message = unsafe {
        if ffi::vsnprintf(buffer.as_mut_ptr(), buffer.len() as _, format, args) < 0 {
            return;
        }
        CStr::from_ptr(buffer.as_ptr()).to_string_lossy()
    };
    let tag = if tag.is_null() {
        "SVT-AV1".into()
    } else {
        // SAFETY: a non-null tag is a NUL-terminated string.
        unsafe { CStr::from_ptr(tag) }.to_string_lossy()
    };
    eprintln!("{tag}: {}", message.trim_end());
}

/// Software AV1 options that only exist for this encoder.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Av1Options {
    /// AV1's screen-content tools (palette, intra block copy) for "Sharp text" (FR-7.38).
    pub screen_content: bool,
}

/// Realtime AV1 encoder on SVT-AV1 (low delay, CBR) with [`AV1_TEMPORAL_LAYERS`] temporal layers.
pub struct Av1Encoder {
    handle: *mut ffi::EbComponentType,
    config: EncoderConfig,
    options: Av1Options,
    /// Live bitrate or frame-rate changes not yet handed to SVT-AV1; they ride along with the
    /// next frame.
    pending_bitrate: bool,
    pending_fps: bool,
}

// SAFETY: the SVT-AV1 handle is owned exclusively by this value and only used through
// `&mut self`; SVT-AV1 runs its own worker threads and has no affinity to the calling thread.
unsafe impl Send for Av1Encoder {}

impl Av1Encoder {
    pub fn new(config: EncoderConfig, options: Av1Options) -> Result<Self> {
        validate(&config)?;
        let handle = open(&config, options)?;
        Ok(Self {
            handle,
            config,
            options,
            pending_bitrate: false,
            pending_fps: false,
        })
    }

    pub fn config(&self) -> EncoderConfig {
        self.config
    }

    pub fn options(&self) -> Av1Options {
        self.options
    }

    /// Encodes one frame; see [`VideoEncoder::encode`].
    pub fn encode(
        &mut self,
        frame: &I420Frame,
        pts: i64,
        force_keyframe: bool,
    ) -> Result<Vec<EncodedFrame>> {
        if frame.width() != self.config.width || frame.height() != self.config.height {
            return Err(Error::InvalidFrame(format!(
                "frame is {}x{}, encoder is set to {}x{}",
                frame.width(),
                frame.height(),
                self.config.width,
                self.config.height
            )));
        }
        let (w, h) = (frame.width() as usize, frame.height() as usize);
        let chroma_w = w.div_ceil(2);
        let luma = w * h;
        let chroma = chroma_w * h.div_ceil(2);
        let base = frame.data().as_ptr() as *mut u8;

        let mut rate = ffi::SvtAv1RateInfo {
            seq_qp: 0,
            target_bit_rate: bits_per_second(self.config.bitrate_kbps),
        };
        let mut frame_rate = ffi::SvtAv1FrameRateInfo {
            frame_rate_numerator: self.config.fps,
            frame_rate_denominator: 1,
        };
        let mut events = Vec::with_capacity(2);
        if self.pending_bitrate {
            events.push(event(ffi::RATE_CHANGE_EVENT, &mut rate));
        }
        if self.pending_fps {
            events.push(event(ffi::FRAME_RATE_CHANGE_EVENT, &mut frame_rate));
        }
        // SVT-AV1 walks the events as a linked list.
        for index in 1..events.len() {
            let next: *mut ffi::EbPrivDataNode = &mut events[index];
            events[index - 1].next = next;
        }

        // SAFETY: the input descriptors point into `frame`, which outlives the send call; SVT-AV1
        // copies the picture and the event list during svt_av1_enc_send_picture and never writes
        // to the input planes. Output buffers are copied out before they are released.
        unsafe {
            let mut planes = ffi::EbSvtIOFormat {
                luma: base,
                cb: base.add(luma),
                cr: base.add(luma + chroma),
                y_stride: w as u32,
                cr_stride: chroma_w as u32,
                cb_stride: chroma_w as u32,
            };
            let mut input = std::mem::zeroed::<ffi::EbBufferHeaderType>();
            input.size = std::mem::size_of::<ffi::EbBufferHeaderType>() as u32;
            input.p_buffer = (&mut planes as *mut ffi::EbSvtIOFormat).cast();
            input.n_filled_len = (luma + 2 * chroma) as u32;
            input.n_alloc_len = input.n_filled_len;
            input.pts = pts;
            input.pic_type = if force_keyframe {
                ffi::EB_AV1_KEY_PICTURE
            } else {
                ffi::EB_AV1_INVALID_PICTURE
            };
            input.p_app_private = events.first_mut().map_or(ptr::null_mut(), |first| {
                (first as *mut ffi::EbPrivDataNode).cast()
            });
            check(
                ffi::svt_av1_enc_send_picture(self.handle, &mut input),
                "encode",
            )?;
            self.pending_bitrate = false;
            self.pending_fps = false;

            // Low delay: one packet per picture, and this call blocks until it is ready.
            let mut packet: *mut ffi::EbBufferHeaderType = ptr::null_mut();
            check(
                ffi::svt_av1_enc_get_packet(self.handle, &mut packet, 0),
                "get packet",
            )?;
            if packet.is_null() {
                return Err(codec_error("get packet", "no packet".into()));
            }
            let out = &*packet;
            let result = if out.flags & ffi::EB_BUFFERFLAG_ERROR_MASK != 0 {
                Err(codec_error(
                    "encode",
                    format!("error flags {:#x}", out.flags),
                ))
            } else {
                Ok(EncodedFrame {
                    data: std::slice::from_raw_parts(out.p_buffer, out.n_filled_len as usize)
                        .to_vec(),
                    keyframe: out.pic_type == ffi::EB_AV1_KEY_PICTURE,
                    temporal_layer: out.temporal_layer_index,
                    pts: out.pts,
                })
            };
            ffi::svt_av1_enc_release_out_buffer(&mut packet);
            let frame = result?;
            if frame.temporal_layer >= AV1_TEMPORAL_LAYERS {
                return Err(codec_error(
                    "read temporal layer id",
                    format!("unexpected layer id {}", frame.temporal_layer),
                ));
            }
            Ok(vec![frame])
        }
    }

    /// Changes the output size. SVT-AV1 starts a new stream, so the next frame is a keyframe
    /// (FR-7.39).
    pub fn set_resolution(&mut self, width: u32, height: u32) -> Result<()> {
        self.reopen(EncoderConfig {
            width,
            height,
            ..self.config
        })
    }

    /// Changes the total bitrate from the next frame on.
    pub fn set_bitrate(&mut self, bitrate_kbps: u32) -> Result<()> {
        validate(&EncoderConfig {
            bitrate_kbps,
            ..self.config
        })?;
        self.config.bitrate_kbps = bitrate_kbps;
        self.pending_bitrate = true;
        Ok(())
    }

    /// Changes the frame rate from the next frame on.
    pub fn set_fps(&mut self, fps: u32) -> Result<()> {
        let config = EncoderConfig { fps, ..self.config };
        validate(&config)?;
        self.config = config;
        self.pending_fps = true;
        Ok(())
    }

    /// Switches the screen-content tools; a new stream starting with a keyframe.
    pub fn set_options(&mut self, options: Av1Options) -> Result<()> {
        if options == self.options {
            return Ok(());
        }
        let handle = open(&self.config, options)?;
        close(self.handle);
        self.handle = handle;
        self.options = options;
        Ok(())
    }

    fn reopen(&mut self, config: EncoderConfig) -> Result<()> {
        validate(&config)?;
        // The new encoder is opened first so a failure leaves the old one working.
        let handle = open(&config, self.options)?;
        close(self.handle);
        self.handle = handle;
        self.config = config;
        self.pending_bitrate = false;
        self.pending_fps = false;
        Ok(())
    }
}

impl VideoEncoder for Av1Encoder {
    fn codec(&self) -> Codec {
        Codec::Av1
    }

    fn config(&self) -> EncoderConfig {
        Av1Encoder::config(self)
    }

    fn temporal_layers(&self) -> u8 {
        AV1_TEMPORAL_LAYERS
    }

    fn encode(
        &mut self,
        frame: &I420Frame,
        pts: i64,
        force_keyframe: bool,
    ) -> Result<Vec<EncodedFrame>> {
        Av1Encoder::encode(self, frame, pts, force_keyframe)
    }

    fn set_resolution(&mut self, width: u32, height: u32) -> Result<()> {
        Av1Encoder::set_resolution(self, width, height)
    }

    fn set_bitrate(&mut self, bitrate_kbps: u32) -> Result<()> {
        Av1Encoder::set_bitrate(self, bitrate_kbps)
    }

    fn set_fps(&mut self, fps: u32) -> Result<()> {
        Av1Encoder::set_fps(self, fps)
    }
}

impl Drop for Av1Encoder {
    fn drop(&mut self) {
        close(self.handle);
    }
}

fn bits_per_second(kbps: u32) -> u32 {
    kbps.saturating_mul(1000)
}

fn event<T>(node_type: ffi::PrivDataType, data: &mut T) -> ffi::EbPrivDataNode {
    ffi::EbPrivDataNode {
        node_type,
        data: (data as *mut T).cast(),
        size: std::mem::size_of::<T>() as u32,
        next: ptr::null_mut(),
    }
}

fn validate(config: &EncoderConfig) -> Result<()> {
    if config.width < 8 || config.height < 8 {
        return Err(Error::InvalidConfig(format!(
            "size {}x{} (AV1 needs even sizes of at least 8x8)",
            config.width, config.height
        )));
    }
    if config.fps == 0 || i64::from(config.fps) > TIMEBASE_MS {
        return Err(Error::InvalidConfig(format!("fps {}", config.fps)));
    }
    if config.bitrate_kbps == 0 {
        return Err(Error::InvalidConfig("bitrate 0".into()));
    }
    Ok(())
}

/// Creates and starts an SVT-AV1 encoder for `config`.
fn open(config: &EncoderConfig, options: Av1Options) -> Result<*mut ffi::EbComponentType> {
    let _lifecycle = LIFECYCLE.lock().unwrap_or_else(PoisonError::into_inner);
    // SAFETY: the callback must be registered before the first handle, which fixes the logging
    // setup; the lifecycle lock keeps it apart from other encoders being created.
    LOG_CALLBACK
        .call_once(|| unsafe { ffi::svt_av1_set_log_callback(Some(svt_log), ptr::null_mut()) });
    // SAFETY: svt_av1_enc_init_handle fills the configuration with defaults before it is changed;
    // the handle is released again on every error path.
    unsafe {
        let mut handle = ptr::null_mut();
        let mut cfg = std::mem::zeroed::<ffi::EbSvtAv1EncConfiguration>();
        check(
            ffi::svt_av1_enc_init_handle(&mut handle, &mut cfg),
            "encoder init",
        )?;
        configure(&mut cfg, config, options);
        let started = check(
            ffi::svt_av1_enc_set_parameter(handle, &mut cfg),
            "configure encoder",
        )
        .and_then(|()| check(ffi::svt_av1_enc_init(handle), "start encoder"));
        if let Err(err) = started {
            ffi::svt_av1_enc_deinit_handle(handle);
            return Err(err);
        }
        Ok(handle)
    }
}

fn configure(cfg: &mut ffi::EbSvtAv1EncConfiguration, config: &EncoderConfig, options: Av1Options) {
    cfg.enc_mode = if options.screen_content {
        SCREEN_CONTENT_PRESET
    } else {
        PRESET
    };
    cfg.source_width = config.width;
    cfg.source_height = config.height;
    cfg.frame_rate_numerator = config.fps;
    cfg.frame_rate_denominator = 1;
    cfg.encoder_bit_depth = 8;

    // Real-time low delay: every frame only references earlier ones, no lookahead, so each frame
    // comes out of the encode call that took it in.
    cfg.rtc = true;
    cfg.pred_structure = ffi::LOW_DELAY;
    // 2^levels frames per mini-GOP, layers 0..=levels.
    cfg.hierarchical_levels = u32::from(AV1_TEMPORAL_LAYERS) - 1;
    cfg.look_ahead_distance = 0;
    cfg.enable_tf = 0;
    cfg.scene_change_detection = 0;

    // Keyframes only on request (new viewer, packet loss) or a new stream. Low delay honours a
    // per-picture key request without `force_key_frames`.
    cfg.intra_period_length = -1;
    cfg.intra_refresh_type = ffi::SVT_AV1_KF_REFRESH;

    cfg.rate_control_mode = ffi::SVT_AV1_RC_MODE_CBR as u8;
    cfg.target_bit_rate = bits_per_second(config.bitrate_kbps);
    // The same tight buffer as the VP9 encoder: a throttled viewer's upload has little room for
    // bursts after a step down.
    cfg.starting_buffer_level_ms = 500;
    cfg.optimal_buffer_level_ms = 600;
    cfg.maximum_buffer_size_ms = 700;
    cfg.under_shoot_pct = 50;
    cfg.over_shoot_pct = 50;

    cfg.screen_content_mode = u32::from(options.screen_content);
    cfg.level_of_parallelism = parallelism();
}

fn close(handle: *mut ffi::EbComponentType) {
    // SAFETY: `handle` was started by `open` and is closed exactly once by its owner. SVT-AV1
    // wants the end of the stream signalled and its last packet collected before shutdown.
    unsafe {
        let mut eos = std::mem::zeroed::<ffi::EbBufferHeaderType>();
        eos.size = std::mem::size_of::<ffi::EbBufferHeaderType>() as u32;
        eos.flags = ffi::EB_BUFFERFLAG_EOS;
        eos.pic_type = ffi::EB_AV1_INVALID_PICTURE;
        if ffi::svt_av1_enc_send_picture(handle, &mut eos) == ffi::EB_ErrorNone {
            loop {
                let mut packet: *mut ffi::EbBufferHeaderType = ptr::null_mut();
                let status = ffi::svt_av1_enc_get_packet(handle, &mut packet, 1);
                if status != ffi::EB_ErrorNone || packet.is_null() {
                    break;
                }
                let last = (*packet).flags & ffi::EB_BUFFERFLAG_EOS != 0;
                ffi::svt_av1_enc_release_out_buffer(&mut packet);
                if last {
                    break;
                }
            }
        }
        let _lifecycle = LIFECYCLE.lock().unwrap_or_else(PoisonError::into_inner);
        let status = ffi::svt_av1_enc_deinit(handle);
        if status != ffi::EB_ErrorNone {
            eprintln!("SVT-AV1 encoder shutdown failed: {status:#x}");
        }
        ffi::svt_av1_enc_deinit_handle(handle);
    }
}

fn parallelism() -> u32 {
    // SVT-AV1's low levels of parallelism: level 1 is about two and a half times slower at
    // 1080p30, and above level 3 the gain is a few percent while the game or app being shared
    // loses cores (measured with the `encode_speed` example).
    if std::thread::available_parallelism().map_or(1, |n| n.get()) > 8 {
        3
    } else {
        2
    }
}

fn check(status: ffi::EbErrorType, operation: &'static str) -> Result<()> {
    if status == ffi::EB_ErrorNone {
        return Ok(());
    }
    Err(codec_error(operation, format!("error {status:#x}")))
}

fn codec_error(operation: &'static str, message: String) -> Error {
    Error::Codec {
        library: "SVT-AV1",
        operation,
        message,
    }
}
