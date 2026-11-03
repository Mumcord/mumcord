//! Intel Quick Sync AV1 and VP9 encoder through oneVPL (Windows). The statically linked oneVPL
//! dispatcher finds Intel's runtime (part of the GPU driver) itself.
//!
//! The encoder runs synchronously with low-latency constant bitrate: no B-frames, one frame in
//! flight, and a rate-control buffer of one frame. Frames go in through a system-memory NV12
//! surface owned by the encoder.
//!
//! Temporal layers use the VP9 layer buffer or (AV1) the universal one, dyadic like the software
//! encoders: 3 layers up to 30 fps, 4 above, or fewer when the runtime refuses that many. oneVPL
//! doesn't report a frame's layer, so for AV1 it is read from the OBU extension headers of the
//! frame ([`obu`]). VP9 frames (and AV1 frames without extension headers) get it from the dyadic
//! pattern, which the runtime restarts at every keyframe.
//!
//! On Linux, Intel GPUs are driven through VA-API instead, so [`open`] refuses there; the code
//! still compiles on Linux so the tests (which mock oneVPL) run in Linux CI too.

use std::{mem, ptr, thread, time::Duration};

use super::{HardwareEncoder, ffi::vpl, obu};
use crate::{Codec, EncodedFrame, EncoderConfig, Error, I420Frame, Result, VideoEncoder};

const ENCODER: HardwareEncoder = HardwareEncoder::Qsv;

/// The most temporal layers any share uses (60 fps down to 7.5 fps).
const MAX_LAYERS: u8 = 4;

/// Temporal layers for `fps`: like the software encoders 3 up to 30 fps, and 4 above, so a
/// viewer of a 60 fps share can still drop to 30, 15 or 7.5 fps.
fn wanted_layers(fps: u32) -> u8 {
    if fps > 30 { MAX_LAYERS } else { 3 }
}

/// Cumulative share of the bitrate up to each layer, in percent, by layer count (libvpx's split
/// for three layers, extended to four).
fn layer_bitrate_percent(layers: u8) -> &'static [u32] {
    match layers {
        1 => &[100],
        2 => &[60, 100],
        3 => &[40, 60, 100],
        _ => &[25, 40, 60, 100],
    }
}

/// Temporal layer of the frame `since_keyframe` frames after the last keyframe, in a dyadic
/// pattern of `layers` layers (for three: 0, 2, 1, 2, 0, …).
fn layer_of(since_keyframe: u32, layers: u8) -> u8 {
    let top = u32::from(layers.max(1) - 1);
    let position = since_keyframe % (1 << top);
    if position == 0 {
        0
    } else {
        // `position` is below 2^top, so it has fewer than `top` trailing zeros.
        u8::try_from(top - position.trailing_zeros()).unwrap_or(0)
    }
}

/// Highest frame rate accepted; screen shares stop at 60 fps.
const MAX_FPS: u32 = 240;

/// How long one frame may take on the GPU before the encoder gives up on it.
const SYNC_TIMEOUT_MS: u32 = 1000;

/// How often a busy GPU is asked again for one frame, a millisecond apart.
const BUSY_RETRIES: u32 = 100;

// The oneVPL enums are signed on MSVC and unsigned elsewhere; the fields they go into are not.
#[allow(clippy::unnecessary_cast)]
mod value {
    use super::vpl;

    pub const CODEC_AV1: u32 = vpl::MFX_CODEC_AV1 as u32;
    pub const CODEC_VP9: u32 = vpl::MFX_CODEC_VP9 as u32;
    pub const IMPL_HARDWARE: u32 = vpl::MFX_IMPL_TYPE_HARDWARE as u32;
    pub const VARIANT_U32: vpl::mfxVariantType = vpl::MFX_VARIANT_TYPE_U32;
    pub const FOURCC_NV12: u32 = vpl::MFX_FOURCC_NV12 as u32;
    pub const CHROMA_420: u16 = vpl::MFX_CHROMAFORMAT_YUV420 as u16;
    pub const PROGRESSIVE: u16 = vpl::MFX_PICSTRUCT_PROGRESSIVE as u16;
    pub const CODING_ON: u16 = vpl::MFX_CODINGOPTION_ON as u16;
    pub const TARGET_BALANCED: u16 = vpl::MFX_TARGETUSAGE_BALANCED as u16;
    pub const RATE_CBR: u16 = vpl::MFX_RATECONTROL_CBR as u16;
    pub const IN_SYSTEM_MEMORY: u16 = vpl::MFX_IOPATTERN_IN_SYSTEM_MEMORY as u16;
    pub const EXT_VP9_LAYERS: u32 = vpl::MFX_EXTBUFF_VP9_TEMPORAL_LAYERS as u32;
    pub const EXT_LAYERS: u32 = vpl::MFX_EXTBUFF_UNIVERSAL_TEMPORAL_LAYERS as u32;
    pub const FRAME_I: u16 = vpl::MFX_FRAMETYPE_I as u16;
    pub const FRAME_REF: u16 = vpl::MFX_FRAMETYPE_REF as u16;
    pub const FRAME_IDR: u16 = vpl::MFX_FRAMETYPE_IDR as u16;
}

/// `MFX_STRUCT_VERSION(1, 0)`, the version of `mfxVariant` these bindings describe.
const VARIANT_VERSION: u16 = 1 << 8;

/// Returns a C struct of the oneVPL bindings with every field zero.
///
/// # Safety
/// `T` must be a plain C struct (integers, pointers, unions of those), for which all zero bytes
/// is valid; oneVPL reads zero as "not set" for every field.
unsafe fn zeroed<T>() -> T {
    // SAFETY: see the function's contract.
    unsafe { mem::zeroed() }
}

fn codec_error(operation: &'static str, status: vpl::mfxStatus) -> Error {
    Error::Codec {
        library: "oneVPL",
        operation,
        message: format!("mfxStatus {status}"),
    }
}

/// Fails on errors; warnings (positive statuses) only mean oneVPL adjusted something.
fn check(status: vpl::mfxStatus, operation: &'static str) -> Result<()> {
    if status < vpl::MFX_ERR_NONE {
        Err(codec_error(operation, status))
    } else {
        Ok(())
    }
}

/// The oneVPL functions the encoder calls: the statically linked dispatcher ([`VPL`]) or, in
/// tests, a mock.
struct Api {
    load: unsafe extern "C" fn() -> vpl::mfxLoader,
    unload: unsafe extern "C" fn(vpl::mfxLoader),
    create_config: unsafe extern "C" fn(vpl::mfxLoader) -> vpl::mfxConfig,
    set_filter:
        unsafe extern "C" fn(vpl::mfxConfig, *const vpl::mfxU8, vpl::mfxVariant) -> vpl::mfxStatus,
    create_session:
        unsafe extern "C" fn(vpl::mfxLoader, vpl::mfxU32, *mut vpl::mfxSession) -> vpl::mfxStatus,
    close_session: unsafe extern "C" fn(vpl::mfxSession) -> vpl::mfxStatus,
    query: unsafe extern "C" fn(
        vpl::mfxSession,
        *mut vpl::mfxVideoParam,
        *mut vpl::mfxVideoParam,
    ) -> vpl::mfxStatus,
    init: unsafe extern "C" fn(vpl::mfxSession, *mut vpl::mfxVideoParam) -> vpl::mfxStatus,
    reset: unsafe extern "C" fn(vpl::mfxSession, *mut vpl::mfxVideoParam) -> vpl::mfxStatus,
    close: unsafe extern "C" fn(vpl::mfxSession) -> vpl::mfxStatus,
    encode: unsafe extern "C" fn(
        vpl::mfxSession,
        *mut vpl::mfxEncodeCtrl,
        *mut vpl::mfxFrameSurface1,
        *mut vpl::mfxBitstream,
        *mut vpl::mfxSyncPoint,
    ) -> vpl::mfxStatus,
    sync: unsafe extern "C" fn(vpl::mfxSession, vpl::mfxSyncPoint, vpl::mfxU32) -> vpl::mfxStatus,
}

static VPL: Api = Api {
    load: vpl::MFXLoad,
    unload: vpl::MFXUnload,
    create_config: vpl::MFXCreateConfig,
    set_filter: vpl::MFXSetConfigFilterProperty,
    create_session: vpl::MFXCreateSession,
    close_session: vpl::MFXClose,
    query: vpl::MFXVideoENCODE_Query,
    init: vpl::MFXVideoENCODE_Init,
    reset: vpl::MFXVideoENCODE_Reset,
    close: vpl::MFXVideoENCODE_Close,
    encode: vpl::MFXVideoENCODE_EncodeFrameAsync,
    sync: vpl::MFXVideoCORE_SyncOperation,
};

pub(super) fn open(codec: Codec, config: EncoderConfig) -> Result<Box<dyn VideoEncoder>> {
    if cfg!(not(windows)) {
        return Err(ENCODER.unavailable("Intel GPUs use VA-API on Linux"));
    }
    Ok(Box::new(QsvEncoder::new(&VPL, codec, config)?))
}

fn validate(config: &EncoderConfig) -> Result<()> {
    // Surfaces are padded to 16 pixels and oneVPL keeps sizes in 16 bits.
    let limit = u32::from(u16::MAX - 15);
    if config.width == 0 || config.height == 0 || config.width > limit || config.height > limit {
        return Err(Error::InvalidConfig(format!(
            "size {}x{}",
            config.width, config.height
        )));
    }
    validate_fps(config.fps)?;
    validate_bitrate(config.bitrate_kbps)
}

fn validate_fps(fps: u32) -> Result<()> {
    if fps == 0 || fps > MAX_FPS {
        return Err(Error::InvalidConfig(format!("fps {fps}")));
    }
    Ok(())
}

fn validate_bitrate(bitrate_kbps: u32) -> Result<()> {
    if bitrate_kbps == 0 {
        return Err(Error::InvalidConfig("bitrate 0".into()));
    }
    Ok(())
}

fn align16(value: u32) -> u16 {
    // `validate` keeps sizes at most `u16::MAX - 15`.
    u16::try_from(value.next_multiple_of(16)).unwrap_or(u16::MAX - 15)
}

/// The dispatcher and one session on an Intel GPU that can encode the codec.
struct Session {
    api: &'static Api,
    loader: vpl::mfxLoader,
    session: vpl::mfxSession,
}

impl Session {
    fn open(api: &'static Api, codec: Codec) -> Result<Self> {
        // SAFETY: MFXLoad has no preconditions.
        let loader = unsafe { (api.load)() };
        if loader.is_null() {
            return Err(ENCODER.unavailable("the oneVPL dispatcher failed to start"));
        }
        let mut this = Self {
            api,
            loader,
            session: ptr::null_mut(),
        };
        let codec_id = match codec {
            Codec::Av1 => value::CODEC_AV1,
            Codec::Vp9 => value::CODEC_VP9,
        };
        this.filter(c"mfxImplDescription.Impl", value::IMPL_HARDWARE)?;
        this.filter(
            c"mfxImplDescription.mfxEncoderDescription.encoder.CodecID",
            codec_id,
        )?;
        // SAFETY: a loader with its filters set; `session` is written on success.
        let status = unsafe { (api.create_session)(loader, 0, &mut this.session) };
        if status != vpl::MFX_ERR_NONE || this.session.is_null() {
            return Err(ENCODER.unavailable(format!(
                "no Intel GPU with a {codec:?} encoder (mfxStatus {status})"
            )));
        }
        Ok(this)
    }

    /// Only lets implementations through whose property `name` equals `value`.
    fn filter(&self, name: &std::ffi::CStr, value: u32) -> Result<()> {
        // SAFETY: the loader is alive; a null config is checked below.
        let config = unsafe { (self.api.create_config)(self.loader) };
        // SAFETY: plain C struct.
        let mut variant: vpl::mfxVariant = unsafe { zeroed() };
        variant.Version.Version = VARIANT_VERSION;
        variant.Type = value::VARIANT_U32;
        variant.Data.U32 = value;
        let status = if config.is_null() {
            vpl::MFX_ERR_NULL_PTR
        } else {
            // SAFETY: a config of this loader and a NUL-terminated property name.
            unsafe { (self.api.set_filter)(config, name.as_ptr().cast(), variant) }
        };
        if status == vpl::MFX_ERR_NONE {
            Ok(())
        } else {
            Err(ENCODER.unavailable(format!(
                "oneVPL refused the filter {name:?} (mfxStatus {status})"
            )))
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // SAFETY: the session and loader were created here and are released once, session
        // first; the encoder on the session is closed before (see `QsvEncoder::drop`).
        unsafe {
            if !self.session.is_null() {
                (self.api.close_session)(self.session);
            }
            (self.api.unload)(self.loader);
        }
    }
}

/// Encoder parameters with their extension buffers, boxed so the pointers between them stay put.
struct Params {
    video: vpl::mfxVideoParam,
    vp9_layers: vpl::mfxExtVP9TemporalLayers,
    av1_layers: vpl::mfxExtTemporalLayers,
    av1_layer_list: [vpl::mfxTemporalLayer; MAX_LAYERS as usize],
    ext: [*mut vpl::mfxExtBuffer; 1],
}

impl Params {
    fn new(codec: Codec, config: &EncoderConfig, layers: u8) -> Box<Self> {
        // SAFETY: plain C structs and null pointers.
        let mut params: Box<Self> = Box::new(unsafe { zeroed() });
        // oneVPL keeps rates in 16 bits, scaled by a shared multiplier.
        let multiplier = config.bitrate_kbps.div_ceil(u32::from(u16::MAX)).max(1);
        let scaled = |kbps: u32| u16::try_from(kbps / multiplier).unwrap_or(u16::MAX);
        let target = scaled(config.bitrate_kbps);
        // One frame of buffer, so a frame never queues behind the previous one.
        let buffer_kb = scaled((config.bitrate_kbps / 8 / config.fps).max(1));

        // SAFETY: plain C struct.
        let mut mfx: vpl::mfxInfoMFX = unsafe { zeroed() };
        mfx.LowPower = value::CODING_ON;
        mfx.BRCParamMultiplier = u16::try_from(multiplier).unwrap_or(u16::MAX);
        mfx.FrameInfo = frame_info(config);
        mfx.CodecId = match codec {
            Codec::Av1 => value::CODEC_AV1,
            Codec::Vp9 => value::CODEC_VP9,
        };
        mfx.__bindgen_anon_1.__bindgen_anon_1 = vpl::mfxInfoMFX__bindgen_ty_1__bindgen_ty_1 {
            TargetUsage: value::TARGET_BALANCED,
            // Keyframes only on request (or every 65535 frames).
            GopPicSize: u16::MAX,
            GopRefDist: 1,
            GopOptFlag: 0,
            IdrInterval: 0,
            RateControlMethod: value::RATE_CBR,
            __bindgen_anon_1: vpl::mfxInfoMFX__bindgen_ty_1__bindgen_ty_1__bindgen_ty_1 {
                InitialDelayInKB: buffer_kb,
            },
            BufferSizeInKB: buffer_kb,
            __bindgen_anon_2: vpl::mfxInfoMFX__bindgen_ty_1__bindgen_ty_1__bindgen_ty_2 {
                TargetKbps: target,
            },
            __bindgen_anon_3: vpl::mfxInfoMFX__bindgen_ty_1__bindgen_ty_1__bindgen_ty_3 {
                MaxKbps: target,
            },
            NumSlice: 0,
            NumRefFrame: 0,
            EncodedOrder: 0,
        };
        params.video.__bindgen_anon_1 = vpl::mfxVideoParam__bindgen_ty_1 { mfx };
        params.video.AsyncDepth = 1;
        params.video.IOPattern = value::IN_SYSTEM_MEMORY;

        if layers > 1 {
            let percents = layer_bitrate_percent(layers);
            let layer_kbps =
                |layer: usize| config.bitrate_kbps.saturating_mul(percents[layer]) / 100;
            let ext = match codec {
                Codec::Vp9 => {
                    let buffer = &mut params.vp9_layers;
                    buffer.Header.BufferId = value::EXT_VP9_LAYERS;
                    buffer.Header.BufferSz = buffer_size::<vpl::mfxExtVP9TemporalLayers>();
                    for (layer, entry) in buffer.Layer.iter_mut().take(percents.len()).enumerate() {
                        entry.FrameRateScale = 1 << layer;
                        entry.TargetKbps = scaled(layer_kbps(layer));
                    }
                    ptr::addr_of_mut!(buffer.Header)
                }
                Codec::Av1 => {
                    for (layer, entry) in params
                        .av1_layer_list
                        .iter_mut()
                        .take(percents.len())
                        .enumerate()
                    {
                        entry.FrameRateScale = 1 << layer;
                        entry.__bindgen_anon_1.__bindgen_anon_1.TargetKbps = layer_kbps(layer);
                    }
                    let list = params.av1_layer_list.as_mut_ptr();
                    let buffer = &mut params.av1_layers;
                    buffer.Header.BufferId = value::EXT_LAYERS;
                    buffer.Header.BufferSz = buffer_size::<vpl::mfxExtTemporalLayers>();
                    buffer.NumLayers = u16::from(layers);
                    buffer.Layers = list;
                    ptr::addr_of_mut!(buffer.Header)
                }
            };
            params.ext[0] = ext;
            params.video.ExtParam = params.ext.as_mut_ptr();
            params.video.NumExtParam = 1;
        }
        params
    }
}

/// An NV12 picture of the configured size, padded to 16 pixels, at the configured frame rate.
fn frame_info(config: &EncoderConfig) -> vpl::mfxFrameInfo {
    // SAFETY: plain C struct.
    let mut info: vpl::mfxFrameInfo = unsafe { zeroed() };
    info.FourCC = value::FOURCC_NV12;
    info.ChromaFormat = value::CHROMA_420;
    info.PicStruct = value::PROGRESSIVE;
    info.FrameRateExtN = config.fps;
    info.FrameRateExtD = 1;
    info.__bindgen_anon_1.__bindgen_anon_1 = vpl::mfxFrameInfo__bindgen_ty_1__bindgen_ty_1 {
        Width: align16(config.width),
        Height: align16(config.height),
        CropX: 0,
        CropY: 0,
        CropW: u16::try_from(config.width).unwrap_or(u16::MAX),
        CropH: u16::try_from(config.height).unwrap_or(u16::MAX),
    };
    info
}

fn buffer_size<T>() -> u32 {
    u32::try_from(mem::size_of::<T>()).unwrap_or(u32::MAX)
}

impl Params {
    /// Number of temporal layers these parameters ask for (after a query: what oneVPL accepted).
    fn layers(&self, codec: Codec) -> u8 {
        if self.video.NumExtParam == 0 {
            return 1;
        }
        let count = match codec {
            Codec::Vp9 => self
                .vp9_layers
                .Layer
                .iter()
                .take_while(|layer| layer.FrameRateScale > 0)
                .count(),
            Codec::Av1 => usize::from(self.av1_layers.NumLayers),
        };
        u8::try_from(count).unwrap_or(0)
    }
}

/// A system-memory NV12 picture the encoder reads its input from.
struct Surface {
    surface: Box<vpl::mfxFrameSurface1>,
    data: Vec<u8>,
    pitch: usize,
}

// SAFETY: the surface only points into `data`, which moves with it.
unsafe impl Send for Surface {}

impl Surface {
    fn new(config: &EncoderConfig) -> Self {
        let (width, height) = (align16(config.width), align16(config.height));
        let pitch = usize::from(width);
        let luma = pitch * usize::from(height);
        let mut data = vec![0; luma + luma / 2];
        // SAFETY: plain C struct.
        let mut surface: Box<vpl::mfxFrameSurface1> = Box::new(unsafe { zeroed() });
        surface.Info = frame_info(config);
        surface.Data.__bindgen_anon_2.Pitch = width;
        let y = data.as_mut_ptr();
        surface.Data.__bindgen_anon_3.Y = y;
        // SAFETY: the chroma plane starts right after the luma plane, inside `data`.
        surface.Data.__bindgen_anon_4.UV = unsafe { y.add(luma) };
        Self {
            surface,
            data,
            pitch,
        }
    }

    /// Copies an I420 frame of the configured size into the NV12 picture.
    fn fill(&mut self, frame: &I420Frame) {
        let (width, height) = (frame.width() as usize, frame.height() as usize);
        let pitch = self.pitch;
        let (luma, chroma) = self.data.split_at_mut(pitch * height.next_multiple_of(16));
        for (target, source) in luma
            .chunks_exact_mut(pitch)
            .zip(frame.y().chunks_exact(width))
        {
            target[..width].copy_from_slice(source);
        }
        let chroma_width = width.div_ceil(2);
        let rows = chroma
            .chunks_exact_mut(pitch)
            .zip(frame.u().chunks_exact(chroma_width))
            .zip(frame.v().chunks_exact(chroma_width));
        for ((target, u), v) in rows {
            for ((pair, &u), &v) in target.as_chunks_mut::<2>().0.iter_mut().zip(u).zip(v) {
                *pair = [u, v];
            }
        }
    }
}

/// Room for one compressed frame: never more than the raw picture, with a floor for tiny ones.
fn bitstream_buffer(config: &EncoderConfig) -> Vec<u8> {
    let raw = usize::from(align16(config.width)) * usize::from(align16(config.height)) * 3 / 2;
    vec![0; raw.max(1 << 16)]
}

/// The most temporal layers (up to [`wanted_layers`]) the GPU accepts for `config`; 1 when it
/// takes none.
fn supported_layers(session: &Session, codec: Codec, config: &EncoderConfig) -> u8 {
    (2..=wanted_layers(config.fps))
        .rev()
        .find(|&layers| {
            let mut wanted = Params::new(codec, config, layers);
            let mut answer = Params::new(codec, config, layers);
            // SAFETY: both parameter sets and their buffers outlive the call.
            let status = unsafe {
                (session.api.query)(session.session, &mut wanted.video, &mut answer.video)
            };
            status >= vpl::MFX_ERR_NONE && answer.layers(codec) == layers
        })
        .unwrap_or(1)
}

/// Initialises the encoder on `session`; the first frame it encodes is a keyframe.
fn start(session: &Session, codec: Codec, config: &EncoderConfig) -> Result<(Box<Params>, u8)> {
    let layers = supported_layers(session, codec, config);
    let mut params = Params::new(codec, config, layers);
    // SAFETY: the parameters and their buffers outlive the call.
    let status = unsafe { (session.api.init)(session.session, &mut params.video) };
    if status < vpl::MFX_ERR_NONE {
        return Err(ENCODER.unavailable(format!(
            "the GPU can't encode {codec:?} at {}x{} {} fps (mfxStatus {status})",
            config.width, config.height, config.fps
        )));
    }
    Ok((params, layers))
}

/// The Quick Sync AV1 or VP9 encoder.
struct QsvEncoder {
    session: Session,
    codec: Codec,
    config: EncoderConfig,
    /// The parameters the running encoder was initialised or last reset with; oneVPL may keep
    /// pointers into their buffers.
    params: Box<Params>,
    layers: u8,
    /// Frames since the last keyframe, which give the next frame's temporal layer.
    since_keyframe: u32,
    surface: Surface,
    bitstream: Vec<u8>,
}

// SAFETY: the oneVPL session and the pointers into the encoder's buffers are only used through
// `&mut self`, from one thread at a time.
unsafe impl Send for QsvEncoder {}

impl QsvEncoder {
    fn new(api: &'static Api, codec: Codec, config: EncoderConfig) -> Result<Self> {
        validate(&config)?;
        let session = Session::open(api, codec)?;
        let (params, layers) = start(&session, codec, &config)?;
        Ok(Self {
            session,
            codec,
            config,
            params,
            layers,
            since_keyframe: 0,
            surface: Surface::new(&config),
            bitstream: bitstream_buffer(&config),
        })
    }

    fn api(&self) -> &'static Api {
        self.session.api
    }

    /// Starts the encoder over with `config`, so the next frame is a keyframe. On failure the
    /// previous settings are restored if the GPU still takes them.
    fn restart(&mut self, config: EncoderConfig) -> Result<()> {
        // SAFETY: an initialised encoder on this session.
        unsafe { (self.api().close)(self.session.session) };
        let (params, layers) = match start(&self.session, self.codec, &config) {
            Ok(started) => started,
            Err(err) => {
                let (params, layers) = start(&self.session, self.codec, &self.config)?;
                self.params = params;
                self.layers = layers;
                self.since_keyframe = 0;
                return Err(err);
            }
        };
        if (config.width, config.height) != (self.config.width, self.config.height) {
            self.surface = Surface::new(&config);
            self.bitstream = bitstream_buffer(&config);
        }
        self.config = config;
        self.params = params;
        self.layers = layers;
        self.since_keyframe = 0;
        Ok(())
    }

    /// Applies a new bitrate or frame rate. Within the same layer count the running encoder is
    /// reset in place (no keyframe); a different layer count needs a restart.
    fn change(&mut self, config: EncoderConfig, operation: &'static str) -> Result<()> {
        if wanted_layers(config.fps) != wanted_layers(self.config.fps) {
            return self.restart(config);
        }
        let mut params = Params::new(self.codec, &config, self.layers);
        // SAFETY: an initialised encoder; the parameters and their buffers outlive the call and
        // are kept afterwards.
        let status = unsafe { (self.api().reset)(self.session.session, &mut params.video) };
        check(status, operation)?;
        self.config = config;
        self.params = params;
        Ok(())
    }

    /// Hands the filled surface to the GPU and waits for the compressed frame. Returns its
    /// bytes and whether it is a keyframe, or `None` when the encoder held the frame back.
    fn run(&mut self, force_keyframe: bool) -> Result<Option<(Vec<u8>, bool)>> {
        let api = self.api();
        // SAFETY: plain C struct.
        let mut keyframe_ctrl: vpl::mfxEncodeCtrl = unsafe { zeroed() };
        keyframe_ctrl.FrameType = value::FRAME_I | value::FRAME_REF | value::FRAME_IDR;
        let ctrl: *mut vpl::mfxEncodeCtrl = if force_keyframe {
            &mut keyframe_ctrl
        } else {
            ptr::null_mut()
        };
        // SAFETY: plain C struct.
        let mut bitstream: vpl::mfxBitstream = unsafe { zeroed() };
        bitstream.Data = self.bitstream.as_mut_ptr();
        bitstream.MaxLength = u32::try_from(self.bitstream.len()).unwrap_or(u32::MAX);
        let mut sync: vpl::mfxSyncPoint = ptr::null_mut();
        let mut status = vpl::MFX_WRN_DEVICE_BUSY;
        for _ in 0..BUSY_RETRIES {
            // SAFETY: the control, surface and bitstream outlive the call and the wait below.
            status = unsafe {
                (api.encode)(
                    self.session.session,
                    ctrl,
                    &mut *self.surface.surface,
                    &mut bitstream,
                    &mut sync,
                )
            };
            if status != vpl::MFX_WRN_DEVICE_BUSY {
                break;
            }
            thread::sleep(Duration::from_millis(1));
        }
        match status {
            // Only happens with B-frames or look-ahead, which are off.
            vpl::MFX_ERR_MORE_DATA => return Ok(None),
            vpl::MFX_WRN_DEVICE_BUSY => return Err(codec_error("encode (GPU busy)", status)),
            _ => check(status, "encode")?,
        }
        // SAFETY: the sync point of the call above.
        let status = unsafe { (api.sync)(self.session.session, sync, SYNC_TIMEOUT_MS) };
        if status != vpl::MFX_ERR_NONE {
            return Err(codec_error("wait for the encoded frame", status));
        }
        let start = bitstream.DataOffset as usize;
        let data = self
            .bitstream
            .get(start..start + bitstream.DataLength as usize)
            .ok_or_else(|| {
                codec_error("read the encoded frame", vpl::MFX_ERR_UNDEFINED_BEHAVIOR)
            })?;
        let keyframe = bitstream.FrameType & (value::FRAME_I | value::FRAME_IDR) != 0;
        Ok(Some((data.to_vec(), keyframe)))
    }
}

impl QsvEncoder {
    /// The temporal layer of a compressed frame: for AV1 from its OBU extension header when the
    /// runtime writes one, otherwise from the dyadic pattern.
    fn temporal_layer(&self, data: &[u8], keyframe: bool) -> Result<u8> {
        let pattern = layer_of(self.since_keyframe, self.layers);
        if self.codec != Codec::Av1 || keyframe {
            return Ok(pattern);
        }
        match obu::temporal_id(data) {
            Ok(Some(id)) if id < self.layers => Ok(id),
            Ok(Some(id)) => Err(Error::Codec {
                library: "oneVPL",
                operation: "read the temporal layer",
                message: format!("layer {id} of {}", self.layers),
            }),
            Ok(None) => Ok(pattern),
            Err(message) => Err(Error::Codec {
                library: "oneVPL",
                operation: "read the temporal layer",
                message: message.into(),
            }),
        }
    }
}

impl Drop for QsvEncoder {
    fn drop(&mut self) {
        // SAFETY: the encoder on this session; the session itself is closed afterwards when
        // `self.session` drops.
        unsafe { (self.api().close)(self.session.session) };
    }
}

impl VideoEncoder for QsvEncoder {
    fn codec(&self) -> Codec {
        self.codec
    }

    fn config(&self) -> EncoderConfig {
        self.config
    }

    fn temporal_layers(&self) -> u8 {
        self.layers
    }

    fn encode(
        &mut self,
        frame: &I420Frame,
        pts: i64,
        force_keyframe: bool,
    ) -> Result<Vec<EncodedFrame>> {
        if frame.width() != self.config.width || frame.height() != self.config.height {
            return Err(Error::InvalidFrame(format!(
                "{}x{} frame for a {}x{} encoder",
                frame.width(),
                frame.height(),
                self.config.width,
                self.config.height
            )));
        }
        self.surface.fill(frame);
        let Some((data, keyframe)) = self.run(force_keyframe)? else {
            return Ok(Vec::new());
        };
        if keyframe {
            self.since_keyframe = 0;
        }
        let temporal_layer = self.temporal_layer(&data, keyframe)?;
        self.since_keyframe = self.since_keyframe.wrapping_add(1);
        Ok(vec![EncodedFrame {
            data,
            keyframe,
            temporal_layer,
            pts,
        }])
    }

    fn set_resolution(&mut self, width: u32, height: u32) -> Result<()> {
        let config = EncoderConfig {
            width,
            height,
            ..self.config
        };
        validate(&config)?;
        self.restart(config)
    }

    fn set_bitrate(&mut self, bitrate_kbps: u32) -> Result<()> {
        validate_bitrate(bitrate_kbps)?;
        let config = EncoderConfig {
            bitrate_kbps,
            ..self.config
        };
        self.change(config, "set bitrate")
    }

    fn set_fps(&mut self, fps: u32) -> Result<()> {
        validate_fps(fps)?;
        let config = EncoderConfig { fps, ..self.config };
        self.change(config, "set frame rate")
    }
}

/// No Intel GPU runs in CI, so these tests drive the encoder through mocked oneVPL functions
/// that behave like the runtime: they record what the encoder asks for, cap the temporal layers
/// like a GPU with fewer layers would, and hand back frames with the runtime's frame types.
#[cfg(test)]
mod tests {
    use std::{cell::RefCell, ffi::CStr};

    use super::*;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Call {
        Load,
        CreateSession,
        Query,
        Init,
        Reset,
        Encode,
        Sync,
        Close,
        CloseSession,
        Unload,
    }

    /// What an Init or Reset call asked for.
    #[derive(Clone, Debug, PartialEq, Eq)]
    struct Setup {
        codec: u32,
        size: (u16, u16),
        crop: (u16, u16),
        fps: (u32, u32),
        fourcc: u32,
        low_power: u16,
        target_usage: u16,
        rate_control: u16,
        target_kbps: u16,
        max_kbps: u16,
        buffer_kb: u16,
        initial_delay_kb: u16,
        multiplier: u16,
        gop_size: u16,
        gop_ref_dist: u16,
        async_depth: u16,
        io_pattern: u16,
        /// Frame-rate scale and target bitrate of every temporal layer.
        layers: Vec<(u16, u32)>,
    }

    struct Mock {
        dispatcher: bool,
        session_status: vpl::mfxStatus,
        max_layers: u8,
        init_status: vpl::mfxStatus,
        reset_status: vpl::mfxStatus,
        busy: u32,
        encode_status: vpl::mfxStatus,
        calls: Vec<Call>,
        filters: Vec<(String, u32)>,
        setups: Vec<Setup>,
        /// Whether the next frame becomes a keyframe (the first after Init, or a forced one).
        keyframe_due: bool,
        forced: Vec<bool>,
        input: Vec<u8>,
        frames: u8,
        /// Temporal ids the AV1 runtime writes into OBU extension headers, frame after frame;
        /// empty for none.
        obu_ids: Vec<u8>,
        /// Largest picture width the GPU takes.
        max_width: u16,
        av1: bool,
    }

    impl Default for Mock {
        fn default() -> Self {
            Self {
                dispatcher: true,
                session_status: vpl::MFX_ERR_NONE,
                max_layers: 4,
                init_status: vpl::MFX_ERR_NONE,
                reset_status: vpl::MFX_ERR_NONE,
                busy: 0,
                encode_status: vpl::MFX_ERR_NONE,
                calls: Vec::new(),
                filters: Vec::new(),
                setups: Vec::new(),
                keyframe_due: false,
                forced: Vec::new(),
                input: Vec::new(),
                frames: 0,
                obu_ids: Vec::new(),
                max_width: 4096,
                av1: false,
            }
        }
    }

    thread_local! {
        static MOCK: RefCell<Mock> = RefCell::new(Mock::default());
    }

    fn mock<R>(f: impl FnOnce(&mut Mock) -> R) -> R {
        MOCK.with(|mock| f(&mut mock.borrow_mut()))
    }

    fn calls() -> Vec<Call> {
        mock(|m| m.calls.clone())
    }

    fn record(call: Call) {
        mock(|m| m.calls.push(call));
    }

    fn handle<T>() -> *mut T {
        ptr::NonNull::dangling().as_ptr()
    }

    /// Reads the parameters the encoder passed, including its temporal layer buffer.
    ///
    /// # Safety
    /// `par` must be valid parameters from the encoder.
    unsafe fn setup(par: *const vpl::mfxVideoParam) -> Setup {
        // SAFETY: valid parameters; the encoder always fills the `mfx` member.
        unsafe {
            let par = &*par;
            let mfx = &par.__bindgen_anon_1.mfx;
            let size = &mfx.FrameInfo.__bindgen_anon_1.__bindgen_anon_1;
            let gop = &mfx.__bindgen_anon_1.__bindgen_anon_1;
            let mut layers = Vec::new();
            for i in 0..usize::from(par.NumExtParam) {
                let ext = *par.ExtParam.add(i);
                if (*ext).BufferId == value::EXT_VP9_LAYERS {
                    let vp9 = &*ext.cast::<vpl::mfxExtVP9TemporalLayers>();
                    for layer in vp9.Layer.iter().take_while(|l| l.FrameRateScale > 0) {
                        layers.push((layer.FrameRateScale, u32::from(layer.TargetKbps)));
                    }
                } else if (*ext).BufferId == value::EXT_LAYERS {
                    let av1 = &*ext.cast::<vpl::mfxExtTemporalLayers>();
                    for i in 0..usize::from(av1.NumLayers) {
                        let layer = &*av1.Layers.add(i);
                        let rate = layer.__bindgen_anon_1.__bindgen_anon_1.TargetKbps;
                        layers.push((layer.FrameRateScale, rate));
                    }
                }
            }
            Setup {
                codec: mfx.CodecId,
                size: (size.Width, size.Height),
                crop: (size.CropW, size.CropH),
                fps: (mfx.FrameInfo.FrameRateExtN, mfx.FrameInfo.FrameRateExtD),
                fourcc: mfx.FrameInfo.FourCC,
                low_power: mfx.LowPower,
                target_usage: gop.TargetUsage,
                rate_control: gop.RateControlMethod,
                target_kbps: gop.__bindgen_anon_2.TargetKbps,
                max_kbps: gop.__bindgen_anon_3.MaxKbps,
                buffer_kb: gop.BufferSizeInKB,
                initial_delay_kb: gop.__bindgen_anon_1.InitialDelayInKB,
                multiplier: mfx.BRCParamMultiplier,
                gop_size: gop.GopPicSize,
                gop_ref_dist: gop.GopRefDist,
                async_depth: par.AsyncDepth,
                io_pattern: par.IOPattern,
                layers,
            }
        }
    }

    unsafe extern "C" fn load() -> vpl::mfxLoader {
        record(Call::Load);
        if mock(|m| m.dispatcher) {
            handle()
        } else {
            ptr::null_mut()
        }
    }

    unsafe extern "C" fn unload(_loader: vpl::mfxLoader) {
        record(Call::Unload);
    }

    unsafe extern "C" fn create_config(_loader: vpl::mfxLoader) -> vpl::mfxConfig {
        handle()
    }

    unsafe extern "C" fn set_filter(
        _config: vpl::mfxConfig,
        name: *const vpl::mfxU8,
        value: vpl::mfxVariant,
    ) -> vpl::mfxStatus {
        // SAFETY: a NUL-terminated name and a U32 variant from the encoder.
        let (name, value) = unsafe {
            assert_eq!(value.Version.Version, VARIANT_VERSION);
            assert_eq!(value.Type, vpl::MFX_VARIANT_TYPE_U32);
            (CStr::from_ptr(name.cast()), value.Data.U32)
        };
        let name = name.to_string_lossy().into_owned();
        mock(|m| m.filters.push((name, value)));
        vpl::MFX_ERR_NONE
    }

    unsafe extern "C" fn create_session(
        _loader: vpl::mfxLoader,
        index: vpl::mfxU32,
        session: *mut vpl::mfxSession,
    ) -> vpl::mfxStatus {
        assert_eq!(index, 0);
        record(Call::CreateSession);
        let status = mock(|m| m.session_status);
        if status == vpl::MFX_ERR_NONE {
            // SAFETY: a valid pointer from the encoder.
            unsafe { *session = handle() };
        }
        status
    }

    unsafe extern "C" fn close_session(_session: vpl::mfxSession) -> vpl::mfxStatus {
        record(Call::CloseSession);
        vpl::MFX_ERR_NONE
    }

    /// Answers like a GPU with `max_layers` temporal layers: it lowers the count in `out`.
    unsafe extern "C" fn query(
        _session: vpl::mfxSession,
        par: *mut vpl::mfxVideoParam,
        out: *mut vpl::mfxVideoParam,
    ) -> vpl::mfxStatus {
        record(Call::Query);
        let max = mock(|m| m.max_layers);
        // SAFETY: valid parameters from the encoder.
        let wanted = unsafe { setup(par) }.layers.len();
        if wanted <= usize::from(max) {
            return vpl::MFX_ERR_NONE;
        }
        // SAFETY: `out` has the same temporal layer buffer as `par`.
        unsafe {
            let ext = *(*out).ExtParam;
            if (*ext).BufferId == value::EXT_VP9_LAYERS {
                let vp9 = &mut *ext.cast::<vpl::mfxExtVP9TemporalLayers>();
                for layer in &mut vp9.Layer[usize::from(max)..] {
                    layer.FrameRateScale = 0;
                }
            } else {
                (*ext.cast::<vpl::mfxExtTemporalLayers>()).NumLayers = u16::from(max);
            }
        }
        vpl::MFX_WRN_INCOMPATIBLE_VIDEO_PARAM
    }

    unsafe extern "C" fn init(
        _session: vpl::mfxSession,
        par: *mut vpl::mfxVideoParam,
    ) -> vpl::mfxStatus {
        record(Call::Init);
        // SAFETY: valid parameters from the encoder.
        let setup = unsafe { setup(par) };
        mock(|m| {
            let too_wide = setup.size.0 > m.max_width;
            m.av1 = setup.codec == value::CODEC_AV1;
            m.setups.push(setup);
            m.keyframe_due = true;
            if too_wide {
                vpl::MFX_ERR_UNSUPPORTED
            } else {
                m.init_status
            }
        })
    }

    unsafe extern "C" fn reset(
        _session: vpl::mfxSession,
        par: *mut vpl::mfxVideoParam,
    ) -> vpl::mfxStatus {
        record(Call::Reset);
        // SAFETY: valid parameters from the encoder.
        let setup = unsafe { setup(par) };
        mock(|m| {
            m.setups.push(setup);
            m.reset_status
        })
    }

    unsafe extern "C" fn close(_session: vpl::mfxSession) -> vpl::mfxStatus {
        record(Call::Close);
        vpl::MFX_ERR_NONE
    }

    unsafe extern "C" fn encode(
        _session: vpl::mfxSession,
        ctrl: *mut vpl::mfxEncodeCtrl,
        surface: *mut vpl::mfxFrameSurface1,
        bitstream: *mut vpl::mfxBitstream,
        sync: *mut vpl::mfxSyncPoint,
    ) -> vpl::mfxStatus {
        record(Call::Encode);
        mock(|m| {
            if m.busy > 0 {
                m.busy -= 1;
                return vpl::MFX_WRN_DEVICE_BUSY;
            }
            if m.encode_status != vpl::MFX_ERR_NONE {
                return m.encode_status;
            }
            // SAFETY: valid pointers from the encoder; the surface is an NV12 picture of the
            // size in its frame info.
            unsafe {
                let forced = !ctrl.is_null() && (*ctrl).FrameType & value::FRAME_IDR != 0;
                m.forced.push(forced);
                let surface = &*surface;
                let size = &surface.Info.__bindgen_anon_1.__bindgen_anon_1;
                let pitch = usize::from(surface.Data.__bindgen_anon_2.Pitch);
                let luma = pitch * usize::from(size.Height);
                m.input =
                    std::slice::from_raw_parts(surface.Data.__bindgen_anon_3.Y, luma).to_vec();
                m.input.extend_from_slice(std::slice::from_raw_parts(
                    surface.Data.__bindgen_anon_4.UV,
                    luma / 2,
                ));
                let keyframe = forced || m.keyframe_due;
                m.keyframe_due = false;
                let mut data = vec![m.frames; 8];
                if m.av1 {
                    // A frame OBU with a size field, and an extension header when the runtime
                    // writes temporal ids.
                    let id = (!m.obu_ids.is_empty())
                        .then(|| m.obu_ids[usize::from(m.frames) % m.obu_ids.len()]);
                    let mut obu = vec![(6 << 3) | 0x02];
                    if let Some(id) = id {
                        obu[0] |= 0x04;
                        obu.push(id << 5);
                    }
                    obu.push(8);
                    obu.extend_from_slice(&data);
                    data = obu;
                }
                let bitstream = &mut *bitstream;
                assert!(bitstream.MaxLength as usize >= data.len() + 2);
                ptr::copy_nonoverlapping(data.as_ptr(), bitstream.Data.add(2), data.len());
                bitstream.DataOffset = 2;
                bitstream.DataLength = data.len() as u32;
                bitstream.FrameType = if keyframe {
                    value::FRAME_I | value::FRAME_REF | value::FRAME_IDR
                } else {
                    value::FRAME_REF | FRAME_P
                };
                m.frames = m.frames.wrapping_add(1);
                *sync = handle();
            }
            vpl::MFX_ERR_NONE
        })
    }

    unsafe extern "C" fn sync(
        _session: vpl::mfxSession,
        _sync: vpl::mfxSyncPoint,
        wait: vpl::mfxU32,
    ) -> vpl::mfxStatus {
        assert_eq!(wait, SYNC_TIMEOUT_MS);
        record(Call::Sync);
        vpl::MFX_ERR_NONE
    }

    #[allow(clippy::unnecessary_cast)]
    const FRAME_P: u16 = vpl::MFX_FRAMETYPE_P as u16;

    static MOCK_VPL: Api = Api {
        load,
        unload,
        create_config,
        set_filter,
        create_session,
        close_session,
        query,
        init,
        reset,
        close,
        encode,
        sync,
    };

    fn config(width: u32, height: u32, fps: u32) -> EncoderConfig {
        EncoderConfig {
            width,
            height,
            fps,
            bitrate_kbps: 2400,
        }
    }

    fn open_mock(codec: Codec, config: EncoderConfig) -> Result<QsvEncoder> {
        QsvEncoder::new(&MOCK_VPL, codec, config)
    }

    fn last_setup() -> Setup {
        mock(|m| m.setups.last().cloned()).expect("no Init or Reset")
    }

    fn frame(width: u32, height: u32) -> I420Frame {
        let len = I420Frame::buffer_len(width, height);
        I420Frame::new(width, height, (0..len).map(|i| i as u8).collect()).expect("frame")
    }

    #[test]
    fn opens_a_low_latency_cbr_av1_encoder_with_three_layers_at_30_fps() {
        let encoder = open_mock(Codec::Av1, config(1366, 768, 30)).expect("encoder");
        assert_eq!(encoder.codec(), Codec::Av1);
        assert_eq!(encoder.temporal_layers(), 3);
        let filters = mock(|m| m.filters.clone());
        assert_eq!(
            filters,
            [
                ("mfxImplDescription.Impl".to_owned(), value::IMPL_HARDWARE),
                (
                    "mfxImplDescription.mfxEncoderDescription.encoder.CodecID".to_owned(),
                    value::CODEC_AV1
                ),
            ]
        );
        assert_eq!(
            last_setup(),
            Setup {
                codec: value::CODEC_AV1,
                size: (1376, 768),
                crop: (1366, 768),
                fps: (30, 1),
                fourcc: value::FOURCC_NV12,
                low_power: value::CODING_ON,
                target_usage: value::TARGET_BALANCED,
                rate_control: value::RATE_CBR,
                target_kbps: 2400,
                max_kbps: 2400,
                buffer_kb: 10,
                initial_delay_kb: 10,
                multiplier: 1,
                gop_size: u16::MAX,
                gop_ref_dist: 1,
                async_depth: 1,
                io_pattern: value::IN_SYSTEM_MEMORY,
                layers: vec![(1, 960), (2, 1440), (4, 2400)],
            }
        );
        assert_eq!(
            calls(),
            [Call::Load, Call::CreateSession, Call::Query, Call::Init]
        );
    }

    #[test]
    fn vp9_uses_four_layers_at_60_fps() {
        let encoder = open_mock(Codec::Vp9, config(1920, 1080, 60)).expect("encoder");
        assert_eq!(encoder.codec(), Codec::Vp9);
        assert_eq!(encoder.temporal_layers(), 4);
        assert_eq!(mock(|m| m.filters[1].1), value::CODEC_VP9);
        let setup = last_setup();
        assert_eq!(setup.codec, value::CODEC_VP9);
        assert_eq!(setup.size, (1920, 1088));
        assert_eq!(setup.layers, [(1, 600), (2, 960), (4, 1440), (8, 2400)]);
    }

    #[test]
    fn the_layer_count_falls_back_to_what_the_gpu_accepts() {
        mock(|m| m.max_layers = 2);
        let encoder = open_mock(Codec::Vp9, config(1280, 720, 60)).expect("encoder");
        assert_eq!(encoder.temporal_layers(), 2);
        assert_eq!(last_setup().layers, [(1, 1440), (2, 2400)]);
        assert_eq!(
            calls(),
            [
                Call::Load,
                Call::CreateSession,
                Call::Query,
                Call::Query,
                Call::Query,
                Call::Init
            ]
        );
    }

    #[test]
    fn a_gpu_without_temporal_layers_streams_a_single_layer() {
        mock(|m| m.max_layers = 1);
        let mut encoder = open_mock(Codec::Av1, config(1280, 720, 30)).expect("encoder");
        assert_eq!(encoder.temporal_layers(), 1);
        assert!(last_setup().layers.is_empty());
        for _ in 0..3 {
            let out = encoder
                .encode(&frame(1280, 720), 0, false)
                .expect("encoded");
            assert_eq!(out[0].temporal_layer, 0);
        }
    }

    #[test]
    fn no_intel_gpu_is_unavailable_and_releases_the_dispatcher() {
        mock(|m| m.session_status = vpl::MFX_ERR_NOT_FOUND);
        let Err(Error::HardwareUnavailable { encoder, .. }) =
            open_mock(Codec::Av1, config(1280, 720, 30))
        else {
            panic!("expected unavailable");
        };
        assert_eq!(encoder, HardwareEncoder::Qsv);
        assert_eq!(calls(), [Call::Load, Call::CreateSession, Call::Unload]);
    }

    #[test]
    fn a_missing_dispatcher_is_unavailable() {
        mock(|m| m.dispatcher = false);
        assert!(matches!(
            open_mock(Codec::Vp9, config(1280, 720, 30)),
            Err(Error::HardwareUnavailable { .. })
        ));
        assert_eq!(calls(), [Call::Load]);
    }

    #[test]
    fn a_refused_init_is_unavailable_and_closes_the_session() {
        mock(|m| m.init_status = vpl::MFX_ERR_UNSUPPORTED);
        assert!(matches!(
            open_mock(Codec::Av1, config(8192, 8192, 30)),
            Err(Error::HardwareUnavailable { .. })
        ));
        assert_eq!(
            calls()[calls().len() - 2..],
            [Call::CloseSession, Call::Unload]
        );
    }

    #[test]
    fn copies_the_i420_planes_into_padded_nv12() {
        let mut encoder = open_mock(Codec::Av1, config(3, 2, 30)).expect("encoder");
        // Y: 0..6, U: 6..8, V: 8..10.
        encoder.encode(&frame(3, 2), 0, false).expect("encoded");
        let input = mock(|m| m.input.clone());
        assert_eq!(input.len(), 16 * 16 * 3 / 2);
        assert_eq!(input[..3], [0, 1, 2]);
        assert_eq!(input[16..19], [3, 4, 5]);
        let chroma = 16 * 16;
        assert_eq!(input[chroma..chroma + 4], [6, 8, 7, 9]);
    }

    #[test]
    fn vp9_returns_keyframes_and_dyadic_temporal_layers() {
        let mut encoder = open_mock(Codec::Vp9, config(64, 64, 30)).expect("encoder");
        let mut frames = Vec::new();
        for pts in 0..6 {
            frames.extend(
                encoder
                    .encode(&frame(64, 64), pts, pts == 3)
                    .expect("encoded"),
            );
        }
        let layers: Vec<_> = frames
            .iter()
            .map(|f| (f.keyframe, f.temporal_layer))
            .collect();
        assert_eq!(
            layers,
            [
                (true, 0),
                (false, 2),
                (false, 1),
                (true, 0),
                (false, 2),
                (false, 1)
            ]
        );
        assert_eq!(
            mock(|m| m.forced.clone()),
            [false, false, false, true, false, false]
        );
        assert_eq!(frames[1].data, [1; 8]);
        assert_eq!(frames[5].pts, 5);
        assert_eq!(&calls()[4..6], [Call::Encode, Call::Sync]);
    }

    #[test]
    fn av1_reads_the_temporal_layer_from_the_obu_extension_header() {
        mock(|m| m.obu_ids = vec![0, 1, 1, 2]);
        let mut encoder = open_mock(Codec::Av1, config(64, 64, 30)).expect("encoder");
        let layers: Vec<_> = (0..4)
            .map(|pts| {
                encoder.encode(&frame(64, 64), pts, false).expect("encoded")[0].temporal_layer
            })
            .collect();
        assert_eq!(layers, [0, 1, 1, 2]);
    }

    #[test]
    fn av1_without_extension_headers_uses_the_dyadic_pattern() {
        let mut encoder = open_mock(Codec::Av1, config(64, 64, 30)).expect("encoder");
        let layers: Vec<_> = (0..4)
            .map(|pts| {
                encoder.encode(&frame(64, 64), pts, false).expect("encoded")[0].temporal_layer
            })
            .collect();
        assert_eq!(layers, [0, 2, 1, 2]);
    }

    #[test]
    fn an_av1_layer_beyond_the_configured_count_is_a_codec_error() {
        mock(|m| m.obu_ids = vec![0, 3]);
        let mut encoder = open_mock(Codec::Av1, config(64, 64, 30)).expect("encoder");
        encoder.encode(&frame(64, 64), 0, false).expect("encoded");
        assert!(matches!(
            encoder.encode(&frame(64, 64), 1, false),
            Err(Error::Codec { .. })
        ));
    }

    #[test]
    fn four_layers_repeat_every_eight_frames() {
        let ids: Vec<_> = (0..9).map(|i| layer_of(i, 4)).collect();
        assert_eq!(ids, [0, 3, 2, 3, 1, 3, 2, 3, 0]);
    }

    #[test]
    fn a_busy_gpu_is_asked_again() {
        let mut encoder = open_mock(Codec::Av1, config(64, 64, 30)).expect("encoder");
        mock(|m| m.busy = 2);
        assert_eq!(
            encoder
                .encode(&frame(64, 64), 0, false)
                .expect("encoded")
                .len(),
            1
        );
        let encodes = calls().iter().filter(|&&c| c == Call::Encode).count();
        assert_eq!(encodes, 3);
    }

    #[test]
    fn a_failure_while_encoding_is_a_codec_error() {
        let mut encoder = open_mock(Codec::Vp9, config(64, 64, 30)).expect("encoder");
        mock(|m| m.encode_status = vpl::MFX_ERR_DEVICE_LOST);
        assert!(matches!(
            encoder.encode(&frame(64, 64), 0, false),
            Err(Error::Codec { .. })
        ));
    }

    #[test]
    fn rejects_a_frame_of_the_wrong_size() {
        let mut encoder = open_mock(Codec::Av1, config(64, 64, 30)).expect("encoder");
        assert!(matches!(
            encoder.encode(&frame(32, 32), 0, false),
            Err(Error::InvalidFrame(_))
        ));
    }

    #[test]
    fn set_bitrate_resets_the_running_encoder() {
        let mut encoder = open_mock(Codec::Vp9, config(1280, 720, 30)).expect("encoder");
        encoder.set_bitrate(1000).expect("changed");
        assert_eq!(calls().last(), Some(&Call::Reset));
        let setup = last_setup();
        assert_eq!(setup.target_kbps, 1000);
        assert_eq!(setup.layers, [(1, 400), (2, 600), (4, 1000)]);
        assert_eq!(encoder.config().bitrate_kbps, 1000);
    }

    #[test]
    fn a_refused_reset_keeps_the_previous_settings() {
        let mut encoder = open_mock(Codec::Av1, config(1280, 720, 30)).expect("encoder");
        mock(|m| m.reset_status = vpl::MFX_ERR_INVALID_VIDEO_PARAM);
        assert!(matches!(
            encoder.set_bitrate(1000),
            Err(Error::Codec { .. })
        ));
        assert_eq!(encoder.config().bitrate_kbps, 2400);
    }

    #[test]
    fn bitrates_beyond_16_bits_use_the_multiplier() {
        let config = EncoderConfig {
            bitrate_kbps: 100_000,
            ..config(1920, 1080, 30)
        };
        open_mock(Codec::Vp9, config).expect("encoder");
        let setup = last_setup();
        assert_eq!((setup.multiplier, setup.target_kbps), (2, 50_000));
        assert_eq!(setup.layers[2], (4, 50_000));
    }

    #[test]
    fn going_to_60_fps_restarts_with_four_layers_and_a_keyframe() {
        let mut encoder = open_mock(Codec::Av1, config(1280, 720, 30)).expect("encoder");
        encoder
            .encode(&frame(1280, 720), 0, false)
            .expect("encoded");
        encoder
            .encode(&frame(1280, 720), 1, false)
            .expect("encoded");
        encoder.set_fps(60).expect("changed");
        assert_eq!(encoder.temporal_layers(), 4);
        assert_eq!(last_setup().fps, (60, 1));
        let out = encoder
            .encode(&frame(1280, 720), 2, false)
            .expect("encoded");
        assert!(out[0].keyframe);
        // Within the same layer count the frame rate changes in place.
        encoder.set_fps(50).expect("changed");
        assert_eq!(calls().last(), Some(&Call::Reset));
        assert_eq!(last_setup().fps, (50, 1));
    }

    #[test]
    fn set_resolution_restarts_with_the_new_size() {
        let mut encoder = open_mock(Codec::Vp9, config(1280, 720, 30)).expect("encoder");
        encoder.set_resolution(1920, 1080).expect("changed");
        assert_eq!(last_setup().crop, (1920, 1080));
        assert!(calls().ends_with(&[Call::Close, Call::Query, Call::Init]));
        let out = encoder
            .encode(&frame(1920, 1080), 0, false)
            .expect("encoded");
        assert!(out[0].keyframe);
        assert_eq!(mock(|m| m.input.len()), 1920 * 1088 * 3 / 2);
    }

    #[test]
    fn a_refused_size_restores_the_previous_encoder() {
        let mut encoder = open_mock(Codec::Av1, config(1280, 720, 30)).expect("encoder");
        assert!(matches!(
            encoder.set_resolution(7680, 4320),
            Err(Error::HardwareUnavailable { .. })
        ));
        assert_eq!(encoder.config().width, 1280);
        assert_eq!(last_setup().crop, (1280, 720));
        let out = encoder
            .encode(&frame(1280, 720), 0, false)
            .expect("encoded");
        assert!(out[0].keyframe);
    }

    #[test]
    fn dropping_closes_the_encoder_then_the_session_then_the_dispatcher() {
        drop(open_mock(Codec::Av1, config(64, 64, 30)).expect("encoder"));
        assert!(calls().ends_with(&[Call::Close, Call::CloseSession, Call::Unload]));
    }

    #[test]
    fn invalid_settings_are_rejected_before_touching_the_gpu() {
        assert!(matches!(
            open_mock(Codec::Av1, config(0, 720, 30)),
            Err(Error::InvalidConfig(_))
        ));
        assert!(matches!(
            open_mock(Codec::Av1, config(1280, 720, 0)),
            Err(Error::InvalidConfig(_))
        ));
        assert!(calls().is_empty());
    }

    #[cfg(not(windows))]
    #[test]
    fn linux_uses_va_api_instead() {
        assert!(matches!(
            open(Codec::Av1, config(1280, 720, 30)),
            Err(Error::HardwareUnavailable { .. })
        ));
    }
}
