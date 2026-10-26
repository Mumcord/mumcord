use std::mem::MaybeUninit;
use std::os::raw::{c_int, c_ulong};
use std::ptr;

use crate::ffi::{self, check};
use crate::{Error, I420Frame, Result};

/// Number of temporal layers (L1T3). Layer ids run 0..TEMPORAL_LAYERS.
pub const TEMPORAL_LAYERS: u8 = 3;

/// libvpx's two-frame pattern for three layers: layer 0 every 4th frame, layer 1 in between,
/// layer 2 on every odd frame.
const LAYER_PATTERN: [u32; 4] = [0, 2, 1, 2];

/// Cumulative share of the bitrate each layer may use, in percent (libvpx's suggested split).
const LAYER_BITRATE_PERCENT: [u32; 3] = [40, 60, 100];

/// Presentation times are milliseconds.
const TIMEBASE_DEN: c_int = 1000;

/// Fastest realtime preset that still keeps text readable; tuned with the `encode_speed` example.
const CPU_USED: c_int = 7;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EncoderConfig {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub bitrate_kbps: u32,
}

/// One compressed VP9 frame, ready to send.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EncodedFrame {
    pub data: Vec<u8>,
    pub keyframe: bool,
    /// Temporal layer 0..[`TEMPORAL_LAYERS`]. A viewer that skips all frames above some layer
    /// still decodes the rest; keyframes are always layer 0.
    pub temporal_layer: u8,
    /// Milliseconds, as passed to [`Encoder::encode`].
    pub pts: i64,
}

/// Realtime VP9 encoder with L1T3 temporal layers.
pub struct Encoder {
    // Boxed: libvpx keeps pointers into the context and configuration between calls.
    ctx: Box<ffi::vpx_codec_ctx_t>,
    cfg: Box<ffi::vpx_codec_enc_cfg_t>,
    fps: u32,
}

// SAFETY: the libvpx context is owned exclusively by this value and only used through `&mut self`;
// libvpx has no thread affinity, so moving it to another thread is fine.
unsafe impl Send for Encoder {}

impl Encoder {
    pub fn new(config: EncoderConfig) -> Result<Self> {
        validate(&config)?;
        // SAFETY: plain libvpx calls on locally owned, correctly sized structs; every status is
        // checked before the context is used further.
        unsafe {
            let iface = ffi::vpx_codec_vp9_cx();
            let mut cfg = Box::new(MaybeUninit::<ffi::vpx_codec_enc_cfg_t>::zeroed().assume_init());
            let status = ffi::vpx_codec_enc_config_default(iface, &mut *cfg, 0);
            check(ptr::null_mut(), status, "default encoder config")?;

            cfg.g_w = config.width;
            cfg.g_h = config.height;
            cfg.g_timebase = ffi::vpx_rational {
                num: 1,
                den: TIMEBASE_DEN,
            };
            cfg.g_threads = threads();
            cfg.g_lag_in_frames = 0;
            // Without it, dropped upper-layer frames would still have updated the entropy context
            // the next base-layer frame decodes against.
            cfg.g_error_resilient = ffi::VPX_ERROR_RESILIENT_DEFAULT;
            cfg.g_pass = ffi::VPX_RC_ONE_PASS;
            cfg.rc_end_usage = ffi::VPX_CBR;
            cfg.rc_min_quantizer = 2;
            // The full range: no frame is ever dropped, so on busy content (scrolling text) a
            // lower cap left rate control no room and the stream ran at about twice the target.
            cfg.rc_max_quantizer = 63;
            cfg.rc_undershoot_pct = 50;
            cfg.rc_overshoot_pct = 50;
            cfg.rc_buf_initial_sz = 500;
            cfg.rc_buf_optimal_sz = 600;
            // Little headroom above the optimal level: after a step down (720p15 → 480p15) the
            // resize keyframe comes out tiny and a 1000 ms buffer let the next frames spend the
            // saved bits at once, about 30% over the new target for a second, more than a
            // throttled viewer's upload has room for.
            cfg.rc_buf_sz = 700;
            // Keep every frame: the sender decides what to drop via the temporal layers.
            cfg.rc_dropframe_thresh = 0;
            // Keyframes only on request (new viewer, packet loss) or a size change.
            cfg.kf_mode = ffi::VPX_KF_DISABLED;

            cfg.ss_number_layers = 1;
            cfg.ts_number_layers = u32::from(TEMPORAL_LAYERS);
            cfg.ts_periodicity = LAYER_PATTERN.len() as u32;
            cfg.ts_layer_id[..LAYER_PATTERN.len()].copy_from_slice(&LAYER_PATTERN);
            cfg.ts_rate_decimator[..3].copy_from_slice(&[4, 2, 1]);
            cfg.temporal_layering_mode = ffi::VP9E_TEMPORAL_LAYERING_MODE_0212 as c_int;
            set_layer_bitrates(&mut cfg, config.bitrate_kbps);

            let mut ctx = Box::new(MaybeUninit::<ffi::vpx_codec_ctx_t>::zeroed().assume_init());
            let status = ffi::vpx_codec_enc_init_ver(
                &mut *ctx,
                iface,
                &*cfg,
                0,
                ffi::VPX_ENCODER_ABI_VERSION as c_int,
            );
            check(&mut *ctx, status, "encoder init")?;
            let mut encoder = Self {
                ctx,
                cfg,
                fps: config.fps,
            };
            encoder.configure_controls()?;
            Ok(encoder)
        }
    }

    pub fn config(&self) -> EncoderConfig {
        EncoderConfig {
            width: self.cfg.g_w,
            height: self.cfg.g_h,
            fps: self.fps,
            bitrate_kbps: self.cfg.rc_target_bitrate,
        }
    }

    /// Encodes one frame. `pts` is in milliseconds and must increase. With `force_keyframe` the
    /// frame becomes a keyframe (and thus layer 0).
    pub fn encode(
        &mut self,
        frame: &I420Frame,
        pts: i64,
        force_keyframe: bool,
    ) -> Result<Vec<EncodedFrame>> {
        if frame.width() != self.cfg.g_w || frame.height() != self.cfg.g_h {
            return Err(Error::InvalidFrame(format!(
                "frame is {}x{}, encoder is set to {}x{}",
                frame.width(),
                frame.height(),
                self.cfg.g_w,
                self.cfg.g_h
            )));
        }
        let flags = if force_keyframe {
            ffi::VPX_EFLAG_FORCE_KF as ffi::vpx_enc_frame_flags_t
        } else {
            0
        };
        let duration = c_ulong::from(TIMEBASE_DEN as u32 / self.fps);
        // SAFETY: the image only borrows `frame`'s buffer for the duration of the encode call;
        // libvpx reads but never writes input planes. Packet buffers are copied out before the
        // next libvpx call invalidates them.
        unsafe {
            let img = wrap_image(frame);
            let status = ffi::vpx_codec_encode(
                &mut *self.ctx,
                &img,
                pts,
                duration,
                flags,
                c_ulong::from(ffi::VPX_DL_REALTIME),
            );
            check(&mut *self.ctx, status, "encode")?;

            let mut layer = MaybeUninit::<ffi::vpx_svc_layer_id_t>::zeroed().assume_init();
            let status = ffi::vpx_codec_control_(
                &mut *self.ctx,
                ffi::VP9E_GET_SVC_LAYER_ID as c_int,
                &mut layer as *mut ffi::vpx_svc_layer_id_t,
            );
            check(&mut *self.ctx, status, "read temporal layer id")?;
            let temporal_layer = u8::try_from(layer.temporal_layer_id)
                .ok()
                .filter(|id| *id < TEMPORAL_LAYERS)
                .ok_or_else(|| Error::Codec {
                    operation: "read temporal layer id",
                    message: format!("unexpected layer id {}", layer.temporal_layer_id),
                })?;

            let mut frames = Vec::new();
            let mut iter: ffi::vpx_codec_iter_t = ptr::null();
            loop {
                let pkt = ffi::vpx_codec_get_cx_data(&mut *self.ctx, &mut iter);
                if pkt.is_null() {
                    break;
                }
                if (*pkt).kind != ffi::VPX_CODEC_CX_FRAME_PKT {
                    continue;
                }
                let out = &(*pkt).data.frame;
                frames.push(EncodedFrame {
                    data: std::slice::from_raw_parts(out.buf as *const u8, out.sz).to_vec(),
                    keyframe: out.flags & ffi::VPX_FRAME_IS_KEY != 0,
                    temporal_layer,
                    pts: out.pts,
                });
            }
            Ok(frames)
        }
    }

    /// Changes the output size (the 720p ↔ 480p quality steps). Frames passed to
    /// [`Self::encode`] afterwards must have the new size. Growing beyond the initial size makes
    /// libvpx start with a keyframe; shrinking reuses the scaled previous frames.
    pub fn set_resolution(&mut self, width: u32, height: u32) -> Result<()> {
        if width == 0 || height == 0 {
            return Err(Error::InvalidConfig(format!("size {width}x{height}")));
        }
        self.cfg.g_w = width;
        self.cfg.g_h = height;
        self.apply_config("set resolution")
    }

    /// Changes the total bitrate, split across the temporal layers as at creation.
    pub fn set_bitrate(&mut self, bitrate_kbps: u32) -> Result<()> {
        if bitrate_kbps == 0 {
            return Err(Error::InvalidConfig("bitrate 0".into()));
        }
        set_layer_bitrates(&mut self.cfg, bitrate_kbps);
        self.apply_config("set bitrate")
    }

    fn apply_config(&mut self, operation: &'static str) -> Result<()> {
        // SAFETY: the context is initialised and the configuration is a valid, owned struct.
        let status = unsafe { ffi::vpx_codec_enc_config_set(&mut *self.ctx, &*self.cfg) };
        check(&mut *self.ctx, status, operation)
    }

    fn configure_controls(&mut self) -> Result<()> {
        let multithreaded = c_int::from(self.cfg.g_threads > 1);
        let controls: [(ffi::vp8e_enc_control_id, c_int, &'static str); 9] = [
            (ffi::VP8E_SET_CPUUSED, CPU_USED, "set speed"),
            (
                ffi::VP9E_SET_TUNE_CONTENT,
                ffi::VP9E_CONTENT_SCREEN as c_int,
                "tune for screen content",
            ),
            // Cyclic refresh: spreads quality updates over frames instead of keyframe spikes.
            (ffi::VP9E_SET_AQ_MODE, 3, "set aq mode"),
            (ffi::VP8E_SET_STATIC_THRESHOLD, 1, "set static threshold"),
            (ffi::VP9E_SET_TILE_COLUMNS, 2, "set tile columns"),
            (
                ffi::VP9E_SET_ROW_MT,
                multithreaded,
                "set row multithreading",
            ),
            (
                ffi::VP9E_SET_FRAME_PARALLEL_DECODING,
                0,
                "set frame parallel",
            ),
            (ffi::VP9E_SET_NOISE_SENSITIVITY, 0, "disable denoiser"),
            (ffi::VP9E_SET_SVC, 1, "enable temporal layers"),
        ];
        for (id, value, operation) in controls {
            // SAFETY: each of these controls takes a single int argument.
            let status = unsafe { ffi::vpx_codec_control_(&mut *self.ctx, id as c_int, value) };
            check(&mut *self.ctx, status, operation)?;
        }

        // SAFETY: zeroed is a valid value for this plain C struct; libvpx copies it.
        let status = unsafe {
            let mut svc = MaybeUninit::<ffi::vpx_svc_extra_cfg_t>::zeroed().assume_init();
            for layer in 0..usize::from(TEMPORAL_LAYERS) {
                svc.max_quantizers[layer] = self.cfg.rc_max_quantizer as c_int;
                svc.min_quantizers[layer] = self.cfg.rc_min_quantizer as c_int;
                svc.scaling_factor_num[layer] = 1;
                svc.scaling_factor_den[layer] = 1;
            }
            svc.temporal_layering_mode = self.cfg.temporal_layering_mode;
            ffi::vpx_codec_control_(
                &mut *self.ctx,
                ffi::VP9E_SET_SVC_PARAMETERS as c_int,
                &mut svc as *mut ffi::vpx_svc_extra_cfg_t,
            )
        };
        check(&mut *self.ctx, status, "set layer parameters")
    }
}

impl Drop for Encoder {
    fn drop(&mut self) {
        // SAFETY: the context was initialised in `new` and is destroyed exactly once.
        unsafe {
            ffi::vpx_codec_destroy(&mut *self.ctx);
        }
    }
}

fn validate(config: &EncoderConfig) -> Result<()> {
    if config.width == 0 || config.height == 0 {
        return Err(Error::InvalidConfig(format!(
            "size {}x{}",
            config.width, config.height
        )));
    }
    if config.fps == 0 || config.fps > TIMEBASE_DEN as u32 {
        return Err(Error::InvalidConfig(format!("fps {}", config.fps)));
    }
    if config.bitrate_kbps == 0 {
        return Err(Error::InvalidConfig("bitrate 0".into()));
    }
    Ok(())
}

fn set_layer_bitrates(cfg: &mut ffi::vpx_codec_enc_cfg_t, bitrate_kbps: u32) {
    cfg.rc_target_bitrate = bitrate_kbps;
    for (layer, percent) in LAYER_BITRATE_PERCENT.iter().enumerate() {
        let kbps = bitrate_kbps.saturating_mul(*percent) / 100;
        cfg.ts_target_bitrate[layer] = kbps;
        // VP9 reads the per-layer targets from here; `ts_target_bitrate` is the VP8 field.
        cfg.layer_target_bitrate[layer] = kbps;
    }
}

fn threads() -> u32 {
    // More than four threads gains little at 720p (tile columns cap the parallelism) and takes
    // cores from the game or app being shared.
    std::thread::available_parallelism()
        .map_or(1, |n| n.get().min(4))
        .try_into()
        .unwrap_or(1)
}

/// Describes `frame`'s packed planes as a libvpx image without copying.
///
/// # Safety
/// The returned image points into `frame` and must not outlive it.
unsafe fn wrap_image(frame: &I420Frame) -> ffi::vpx_image_t {
    let (w, h) = (frame.width(), frame.height());
    let chroma_w = w.div_ceil(2);
    // SAFETY: vpx_img_wrap only fills in the struct; the planes are overwritten below with the
    // packed layout because libvpx would otherwise round odd strides up.
    unsafe {
        let mut img = MaybeUninit::<ffi::vpx_image_t>::zeroed().assume_init();
        let base = frame.data().as_ptr() as *mut u8;
        ffi::vpx_img_wrap(&mut img, ffi::VPX_IMG_FMT_I420, w, h, 1, base);
        let luma = w as usize * h as usize;
        let chroma = chroma_w as usize * h.div_ceil(2) as usize;
        img.planes[0] = base;
        img.planes[1] = base.add(luma);
        img.planes[2] = base.add(luma + chroma);
        img.stride[0] = w as c_int;
        img.stride[1] = chroma_w as c_int;
        img.stride[2] = chroma_w as c_int;
        img
    }
}
