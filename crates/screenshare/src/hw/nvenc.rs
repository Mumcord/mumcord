//! NVIDIA NVENC AV1 encoder (GeForce RTX 40 and newer) behind [`VideoEncoder`].
//!
//! The NVENC and CUDA driver libraries are both loaded at runtime. The encoder runs on a CUDA
//! context on the first NVIDIA GPU and takes I420 frames from host memory, so it needs neither
//! Direct3D nor the CUDA toolkit, and works the same on Windows and Linux.
//!
//! It is built against NVENC API 13.0, the first one with temporal layers for AV1. Drivers older
//! than 570 don't offer that API, so NVENC reports itself unavailable there and screen sharing
//! falls back to the software encoders.

use std::ffi::{c_int, c_uint, c_void};
use std::ptr;

use libloading::Library;

use super::{HardwareEncoder, dl, ffi::nvenc as nv};
use crate::{Codec, EncodedFrame, EncoderConfig, Error, I420Frame, Result, VideoEncoder};

const ENCODER: HardwareEncoder = HardwareEncoder::Nvenc;

#[cfg(windows)]
const CUDA: &[&str] = &["nvcuda.dll"];
#[cfg(not(windows))]
const CUDA: &[&str] = &["libcuda.so.1"];

/// The most temporal layers any share uses (60 fps down to 7.5 fps).
const MAX_LAYERS: u8 = 4;

/// Temporal layers for `fps`: like the software encoders 3 up to 30 fps, and 4 above, so a
/// viewer of a 60 fps share can still drop to 30, 15 or 7.5 fps.
fn wanted_layers(fps: u32) -> u8 {
    if fps > 30 { MAX_LAYERS } else { 3 }
}

/// Highest frame rate accepted; screen shares stop at 60 fps.
const MAX_FPS: u32 = 240;

// The struct versions are function-like macros in nvEncodeAPI.h, which bindgen can't translate;
// these mirror the 13.0 header exactly.
const fn struct_version(version: u32) -> u32 {
    nv::NVENCAPI_VERSION | (version << 16) | (0x7 << 28)
}
const CAPS_PARAM_VER: u32 = struct_version(1);
const CREATE_INPUT_BUFFER_VER: u32 = struct_version(2);
const CREATE_BITSTREAM_BUFFER_VER: u32 = struct_version(1);
const CONFIG_VER: u32 = struct_version(9) | (1 << 31);
const INITIALIZE_PARAMS_VER: u32 = struct_version(7) | (1 << 31);
const RECONFIGURE_PARAMS_VER: u32 = struct_version(2) | (1 << 31);
const PRESET_CONFIG_VER: u32 = struct_version(5) | (1 << 31);
const PIC_PARAMS_VER: u32 = struct_version(7) | (1 << 31);
const LOCK_BITSTREAM_VER: u32 = struct_version(2) | (1 << 31);
const LOCK_INPUT_BUFFER_VER: u32 = struct_version(1);
const OPEN_SESSION_VER: u32 = struct_version(1);
const FUNCTION_LIST_VER: u32 = struct_version(2);

// The GUIDs are `static const` in the header, so they are not exported by the driver either.
const AV1_GUID: nv::GUID = guid(
    0x0a35_2289,
    0x0aa7,
    0x4759,
    [0x86, 0x2d, 0x5d, 0x15, 0xcd, 0x16, 0xd2, 0x54],
);
/// P4 is NVIDIA's balanced preset; with ultra-low-latency tuning it keeps a 1440p60 share well
/// within one frame time on every AV1-capable GPU.
const PRESET_P4_GUID: nv::GUID = guid(
    0x90a7_b826,
    0xdf06,
    0x4862,
    [0xb9, 0xd2, 0xcd, 0x6d, 0x73, 0xa0, 0x86, 0x81],
);

const fn guid(data1: u32, data2: u16, data3: u16, data4: [u8; 8]) -> nv::GUID {
    nv::GUID {
        Data1: data1,
        Data2: data2,
        Data3: data3,
        Data4: data4,
    }
}

fn same_guid(a: &nv::GUID, b: &nv::GUID) -> bool {
    (a.Data1, a.Data2, a.Data3, a.Data4) == (b.Data1, b.Data2, b.Data3, b.Data4)
}

type Status = nv::NVENCSTATUS;

/// Picture flags for a requested keyframe, with the sequence header in front.
// The flag enum is signed on MSVC and unsigned elsewhere, while `encodePicFlags` is a u32 on both.
#[allow(clippy::unnecessary_cast)]
const FORCE_KEYFRAME: u32 =
    (nv::NV_ENC_PIC_FLAG_FORCEIDR | nv::NV_ENC_PIC_FLAG_OUTPUT_SPSPPS) as u32;

/// Calls the NVENC function `$name` from the function list `$fns`. Every function used here is
/// checked when the list is loaded ([`Api::new`]), so the `None` arm is unreachable in practice.
macro_rules! call {
    ($fns:expr, $name:ident($($arg:expr),* $(,)?)) => {
        match $fns.$name {
            Some(function) => function($($arg),*),
            None => nv::NV_ENC_ERR_UNIMPLEMENTED,
        }
    };
}

/// Returns a C struct of the NVENC bindings with every field zero.
///
/// # Safety
/// `T` must be a plain C struct (integers, pointers, optional function pointers), for which all
/// zero bytes is valid; NVENC documents zero as "not set" for every field.
unsafe fn zeroed<T>() -> T {
    // SAFETY: see the function's contract.
    unsafe { std::mem::zeroed() }
}

fn codec_error(operation: &'static str, status: Status) -> Error {
    Error::Codec {
        library: "NVENC",
        operation,
        message: format!("NVENCSTATUS {status}"),
    }
}

fn check(status: Status, operation: &'static str) -> Result<()> {
    if status == nv::NV_ENC_SUCCESS {
        Ok(())
    } else {
        Err(codec_error(operation, status))
    }
}

/// Like [`check`], but for the steps of opening an encoder: any failure there means this GPU or
/// driver can't provide it, and the caller falls back to software.
fn check_open(status: Status, step: &str) -> Result<()> {
    if status == nv::NV_ENC_SUCCESS {
        Ok(())
    } else {
        Err(ENCODER.unavailable(format!("{step} failed (NVENCSTATUS {status})")))
    }
}

pub(super) fn open(codec: Codec, config: EncoderConfig) -> Result<Box<dyn VideoEncoder>> {
    if codec != Codec::Av1 {
        return Err(ENCODER.unavailable("NVENC has no VP9 encoder"));
    }
    validate(&config)?;
    let api = Api::load()?;
    let device = Device::Cuda(Cuda::open()?);
    Ok(Box::new(NvencEncoder::new(api, device, config)?))
}

fn validate(config: &EncoderConfig) -> Result<()> {
    if config.width == 0 || config.height == 0 {
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

/// Fails unless the driver offers the NVENC API these bindings were generated from.
fn check_driver_version(status: Status, version: u32) -> Result<()> {
    check_open(status, "reading the NVENC API version")?;
    let required = (nv::NVENCAPI_MAJOR_VERSION << 4) | nv::NVENCAPI_MINOR_VERSION;
    if version < required {
        return Err(ENCODER.unavailable(format!(
            "the driver offers NVENC API {}.{}; AV1 temporal layers need {}.{} (driver 570 or newer)",
            version >> 4,
            version & 0xf,
            nv::NVENCAPI_MAJOR_VERSION,
            nv::NVENCAPI_MINOR_VERSION
        )));
    }
    Ok(())
}

/// The NVENC function list, and the driver library its functions live in.
struct Api {
    fns: nv::NV_ENCODE_API_FUNCTION_LIST,
    _library: Option<Library>,
}

impl Api {
    fn load() -> Result<Self> {
        type GetMaxSupportedVersion = unsafe extern "C" fn(*mut u32) -> Status;
        type CreateInstance = unsafe extern "C" fn(*mut nv::NV_ENCODE_API_FUNCTION_LIST) -> Status;

        let library = dl::open(ENCODER, dl::NVENC)?;
        // SAFETY: both types match the declarations in nvEncodeAPI.h.
        let (max_version, create_instance) = unsafe {
            (
                dl::symbol::<GetMaxSupportedVersion>(
                    ENCODER,
                    &library,
                    "NvEncodeAPIGetMaxSupportedVersion",
                )?,
                dl::symbol::<CreateInstance>(ENCODER, &library, "NvEncodeAPICreateInstance")?,
            )
        };
        let mut version = 0;
        // SAFETY: `version` is a valid out pointer.
        let status = unsafe { max_version(&mut version) };
        check_driver_version(status, version)?;
        // SAFETY: plain C struct.
        let mut fns: nv::NV_ENCODE_API_FUNCTION_LIST = unsafe { zeroed() };
        fns.version = FUNCTION_LIST_VER;
        // SAFETY: `fns` is a zeroed function list of the version the driver was just asked for.
        check_open(
            unsafe { create_instance(&mut fns) },
            "NvEncodeAPICreateInstance",
        )?;
        Self::new(fns, Some(library))
    }

    fn new(fns: nv::NV_ENCODE_API_FUNCTION_LIST, library: Option<Library>) -> Result<Self> {
        let required = [
            (
                "nvEncOpenEncodeSessionEx",
                fns.nvEncOpenEncodeSessionEx.is_some(),
            ),
            (
                "nvEncGetEncodeGUIDCount",
                fns.nvEncGetEncodeGUIDCount.is_some(),
            ),
            ("nvEncGetEncodeGUIDs", fns.nvEncGetEncodeGUIDs.is_some()),
            ("nvEncGetEncodeCaps", fns.nvEncGetEncodeCaps.is_some()),
            (
                "nvEncGetEncodePresetConfigEx",
                fns.nvEncGetEncodePresetConfigEx.is_some(),
            ),
            (
                "nvEncInitializeEncoder",
                fns.nvEncInitializeEncoder.is_some(),
            ),
            (
                "nvEncReconfigureEncoder",
                fns.nvEncReconfigureEncoder.is_some(),
            ),
            (
                "nvEncCreateInputBuffer",
                fns.nvEncCreateInputBuffer.is_some(),
            ),
            (
                "nvEncDestroyInputBuffer",
                fns.nvEncDestroyInputBuffer.is_some(),
            ),
            ("nvEncLockInputBuffer", fns.nvEncLockInputBuffer.is_some()),
            (
                "nvEncUnlockInputBuffer",
                fns.nvEncUnlockInputBuffer.is_some(),
            ),
            (
                "nvEncCreateBitstreamBuffer",
                fns.nvEncCreateBitstreamBuffer.is_some(),
            ),
            (
                "nvEncDestroyBitstreamBuffer",
                fns.nvEncDestroyBitstreamBuffer.is_some(),
            ),
            ("nvEncEncodePicture", fns.nvEncEncodePicture.is_some()),
            ("nvEncLockBitstream", fns.nvEncLockBitstream.is_some()),
            ("nvEncUnlockBitstream", fns.nvEncUnlockBitstream.is_some()),
            ("nvEncDestroyEncoder", fns.nvEncDestroyEncoder.is_some()),
        ];
        if let Some((name, _)) = required.iter().find(|(_, present)| !present) {
            return Err(ENCODER.unavailable(format!("driver lacks {name}")));
        }
        Ok(Self {
            fns,
            _library: library,
        })
    }
}

type CuResult = c_int;
type CuDevice = c_int;
type CuContext = *mut c_void;

/// A CUDA context on the first NVIDIA GPU, the device NVENC encodes on.
struct Cuda {
    context: CuContext,
    push: unsafe extern "C" fn(CuContext) -> CuResult,
    pop: unsafe extern "C" fn(*mut CuContext) -> CuResult,
    destroy: unsafe extern "C" fn(CuContext) -> CuResult,
    _library: Library,
}

impl Cuda {
    fn open() -> Result<Self> {
        let library = dl::open(ENCODER, CUDA)?;
        // SAFETY: every type matches the declaration in cuda.h; the `_v2` names are the ABI that
        // cuda.h maps the plain names to.
        let (init, device_get, create, destroy, push, pop) = unsafe {
            (
                dl::symbol::<unsafe extern "C" fn(c_uint) -> CuResult>(
                    ENCODER, &library, "cuInit",
                )?,
                dl::symbol::<unsafe extern "C" fn(*mut CuDevice, c_int) -> CuResult>(
                    ENCODER,
                    &library,
                    "cuDeviceGet",
                )?,
                dl::symbol::<unsafe extern "C" fn(*mut CuContext, c_uint, CuDevice) -> CuResult>(
                    ENCODER,
                    &library,
                    "cuCtxCreate_v2",
                )?,
                dl::symbol::<unsafe extern "C" fn(CuContext) -> CuResult>(
                    ENCODER,
                    &library,
                    "cuCtxDestroy_v2",
                )?,
                dl::symbol::<unsafe extern "C" fn(CuContext) -> CuResult>(
                    ENCODER,
                    &library,
                    "cuCtxPushCurrent_v2",
                )?,
                dl::symbol::<unsafe extern "C" fn(*mut CuContext) -> CuResult>(
                    ENCODER,
                    &library,
                    "cuCtxPopCurrent_v2",
                )?,
            )
        };
        let cuda_check = |result: CuResult, step: &str| {
            if result == 0 {
                Ok(())
            } else {
                // 100 is CUDA_ERROR_NO_DEVICE: a driver without an NVIDIA GPU.
                Err(ENCODER.unavailable(format!("CUDA {step} failed (CUresult {result})")))
            }
        };
        let mut device = 0;
        let mut context = ptr::null_mut();
        // SAFETY: valid out pointers; cuInit must precede every other call.
        unsafe {
            cuda_check(init(0), "cuInit")?;
            cuda_check(device_get(&mut device, 0), "cuDeviceGet")?;
            cuda_check(create(&mut context, 0, device), "cuCtxCreate")?;
        }
        let cuda = Self {
            context,
            push,
            pop,
            destroy,
            _library: library,
        };
        // A new context is current on the creating thread; it is only pushed around each use
        // (`Device::enter`), so the encoder can move between threads.
        let mut popped = ptr::null_mut();
        // SAFETY: the context was just made current on this thread.
        cuda_check(unsafe { (cuda.pop)(&mut popped) }, "cuCtxPopCurrent")?;
        Ok(cuda)
    }
}

impl Drop for Cuda {
    fn drop(&mut self) {
        // SAFETY: the context is owned by `self` and no longer current anywhere; the encoder
        // session using it was destroyed first. A failure here leaves nothing to clean up.
        unsafe { (self.destroy)(self.context) };
    }
}

enum Device {
    Cuda(Cuda),
    /// The driver mock of the unit tests; it has no context to make current.
    #[cfg(test)]
    Mock,
}

impl Device {
    /// The device handle NVENC opens its session on.
    fn handle(&self) -> *mut c_void {
        match self {
            Self::Cuda(cuda) => cuda.context,
            #[cfg(test)]
            Self::Mock => ptr::NonNull::<u8>::dangling().as_ptr().cast(),
        }
    }

    fn cuda(&self) -> Option<&Cuda> {
        match self {
            Self::Cuda(cuda) => Some(cuda),
            #[cfg(test)]
            Self::Mock => None,
        }
    }

    /// Makes the device's context current on this thread until the guard is dropped.
    fn enter(&self) -> Result<Current<'_>> {
        if let Some(cuda) = self.cuda() {
            // SAFETY: the context lives as long as `self`.
            let result = unsafe { (cuda.push)(cuda.context) };
            if result != 0 {
                return Err(Error::Codec {
                    library: "CUDA",
                    operation: "make the context current",
                    message: format!("CUresult {result}"),
                });
            }
        }
        Ok(Current(self))
    }
}

struct Current<'a>(&'a Device);

impl Drop for Current<'_> {
    fn drop(&mut self) {
        if let Some(cuda) = self.0.cuda() {
            let mut popped = ptr::null_mut();
            // SAFETY: `enter` pushed this context on this thread. Popping only fails when the
            // context isn't current, and then there is nothing to undo.
            unsafe { (cuda.pop)(&mut popped) };
        }
    }
}

/// One NVENC encoder session with its input and output buffer.
struct Session {
    encoder: *mut c_void,
    input: nv::NV_ENC_INPUT_PTR,
    output: nv::NV_ENC_OUTPUT_PTR,
    /// The session's configuration; `init.encodeConfig` points into `config`.
    config: Box<nv::NV_ENC_CONFIG>,
    init: Box<nv::NV_ENC_INITIALIZE_PARAMS>,
    /// Temporal layers the GPU can produce (1 when it has no temporal SVC).
    max_layers: u8,
}

impl Session {
    /// Opens a session for `config` with `layers` temporal layers (or fewer, if that's all the
    /// GPU can do). The device's context must be current.
    fn open(api: &Api, device: &Device, config: &EncoderConfig, layers: u8) -> Result<Self> {
        // SAFETY: plain C struct.
        let mut params: nv::NV_ENC_OPEN_ENCODE_SESSION_EX_PARAMS = unsafe { zeroed() };
        params.version = OPEN_SESSION_VER;
        params.deviceType = nv::NV_ENC_DEVICE_TYPE_CUDA;
        params.device = device.handle();
        params.apiVersion = nv::NVENCAPI_VERSION;
        let mut encoder = ptr::null_mut();
        // SAFETY: valid parameters and out pointer.
        let status = unsafe { call!(api.fns, nvEncOpenEncodeSessionEx(&mut params, &mut encoder)) };
        // SAFETY: plain C structs.
        let mut session = Self {
            encoder,
            input: ptr::null_mut(),
            output: ptr::null_mut(),
            config: Box::new(unsafe { zeroed() }),
            init: Box::new(unsafe { zeroed() }),
            max_layers: 1,
        };
        let opened = check_open(status, "opening an NVENC session")
            .and_then(|()| session.configure(api, config, layers));
        match opened {
            Ok(()) => Ok(session),
            Err(err) => {
                session.close(api);
                Err(err)
            }
        }
    }

    fn configure(&mut self, api: &Api, config: &EncoderConfig, layers: u8) -> Result<()> {
        self.check_av1(api)?;
        let max_width = self.caps(api, nv::NV_ENC_CAPS_WIDTH_MAX)?;
        let max_height = self.caps(api, nv::NV_ENC_CAPS_HEIGHT_MAX)?;
        if i64::from(config.width) > i64::from(max_width)
            || i64::from(config.height) > i64::from(max_height)
        {
            return Err(ENCODER.unavailable(format!(
                "{}x{} is larger than this GPU's AV1 limit of {max_width}x{max_height}",
                config.width, config.height
            )));
        }
        self.max_layers = if self.caps(api, nv::NV_ENC_CAPS_SUPPORT_TEMPORAL_SVC)? == 0 {
            1
        } else {
            let max = self.caps(api, nv::NV_ENC_CAPS_NUM_MAX_TEMPORAL_LAYERS)?;
            u8::try_from(max.clamp(1, i32::from(MAX_LAYERS))).unwrap_or(1)
        };

        // SAFETY: plain C struct.
        let mut preset: nv::NV_ENC_PRESET_CONFIG = unsafe { zeroed() };
        preset.version = PRESET_CONFIG_VER;
        preset.presetCfg.version = CONFIG_VER;
        // SAFETY: valid session and out pointer.
        check_open(
            unsafe {
                call!(
                    api.fns,
                    nvEncGetEncodePresetConfigEx(
                        self.encoder,
                        AV1_GUID,
                        PRESET_P4_GUID,
                        nv::NV_ENC_TUNING_INFO_ULTRA_LOW_LATENCY,
                        &mut preset,
                    )
                )
            },
            "reading the AV1 preset",
        )?;
        *self.config = preset.presetCfg;
        let cfg = &mut *self.config;
        cfg.version = CONFIG_VER;
        // Keyframes only on request (a new viewer or a lost packet), and no B-frames, which
        // would add latency.
        cfg.gopLength = nv::NVENC_INFINITE_GOPLENGTH;
        cfg.frameIntervalP = 1;
        cfg.rcParams.rateControlMode = nv::NV_ENC_PARAMS_RC_CBR;
        set_rate(cfg, config);
        // SAFETY: an AV1 session's codec config is the `av1Config` member.
        let av1 = unsafe { &mut cfg.encodeCodecConfig.av1Config };
        av1.idrPeriod = nv::NVENC_INFINITE_GOPLENGTH;
        // Every keyframe carries the sequence header, so a viewer can start at any of them.
        av1.set_repeatSeqHdr(1);
        av1.set_chromaFormatIDC(1);
        if self.max_layers > 1 {
            av1.set_enableTemporalSVC(1);
            // The maximum can't change later; the layer count below can, per picture.
            av1.maxTemporalLayersMinus1 = u32::from(self.max_layers - 1);
            av1.numTemporalLayers = u32::from(layers.min(self.max_layers));
        }

        let init = &mut *self.init;
        init.version = INITIALIZE_PARAMS_VER;
        init.encodeGUID = AV1_GUID;
        init.presetGUID = PRESET_P4_GUID;
        init.tuningInfo = nv::NV_ENC_TUNING_INFO_ULTRA_LOW_LATENCY;
        init.encodeWidth = config.width;
        init.encodeHeight = config.height;
        init.darWidth = config.width;
        init.darHeight = config.height;
        init.maxEncodeWidth = config.width;
        init.maxEncodeHeight = config.height;
        init.frameRateNum = config.fps;
        init.frameRateDen = 1;
        init.enableEncodeAsync = 0;
        init.enablePTD = 1;
        init.encodeConfig = &mut *self.config;
        // SAFETY: the parameters and the config they point to live in `self`.
        check_open(
            unsafe {
                call!(
                    api.fns,
                    nvEncInitializeEncoder(self.encoder, &mut *self.init)
                )
            },
            "initialising the AV1 encoder",
        )?;

        // SAFETY: plain C structs.
        let mut input: nv::NV_ENC_CREATE_INPUT_BUFFER = unsafe { zeroed() };
        input.version = CREATE_INPUT_BUFFER_VER;
        input.width = config.width;
        input.height = config.height;
        input.bufferFmt = nv::NV_ENC_BUFFER_FORMAT_IYUV;
        // SAFETY: valid session and parameters.
        check_open(
            unsafe { call!(api.fns, nvEncCreateInputBuffer(self.encoder, &mut input)) },
            "creating the input buffer",
        )?;
        self.input = input.inputBuffer;

        // SAFETY: plain C struct.
        let mut output: nv::NV_ENC_CREATE_BITSTREAM_BUFFER = unsafe { zeroed() };
        output.version = CREATE_BITSTREAM_BUFFER_VER;
        // SAFETY: valid session and parameters.
        check_open(
            unsafe {
                call!(
                    api.fns,
                    nvEncCreateBitstreamBuffer(self.encoder, &mut output)
                )
            },
            "creating the output buffer",
        )?;
        self.output = output.bitstreamBuffer;
        Ok(())
    }

    fn check_av1(&self, api: &Api) -> Result<()> {
        let mut count = 0;
        // SAFETY: valid session and out pointer.
        check_open(
            unsafe { call!(api.fns, nvEncGetEncodeGUIDCount(self.encoder, &mut count)) },
            "listing the codecs",
        )?;
        let mut guids = vec![guid(0, 0, 0, [0; 8]); count as usize];
        let mut written = 0;
        // SAFETY: `guids` holds `count` entries.
        check_open(
            unsafe {
                call!(
                    api.fns,
                    nvEncGetEncodeGUIDs(self.encoder, guids.as_mut_ptr(), count, &mut written)
                )
            },
            "listing the codecs",
        )?;
        guids.truncate(written as usize);
        if guids.iter().any(|g| same_guid(g, &AV1_GUID)) {
            Ok(())
        } else {
            Err(ENCODER.unavailable("this GPU has no AV1 encoder (GeForce RTX 40 or newer)"))
        }
    }

    fn caps(&self, api: &Api, cap: nv::NV_ENC_CAPS) -> Result<i32> {
        // SAFETY: plain C struct.
        let mut param: nv::NV_ENC_CAPS_PARAM = unsafe { zeroed() };
        param.version = CAPS_PARAM_VER;
        param.capsToQuery = cap;
        let mut value = 0;
        // SAFETY: valid session, parameters and out pointer.
        check_open(
            unsafe {
                call!(
                    api.fns,
                    nvEncGetEncodeCaps(self.encoder, AV1_GUID, &mut param, &mut value)
                )
            },
            "reading the AV1 capabilities",
        )?;
        Ok(value)
    }

    /// Destroys the buffers and the session. The device's context must be current.
    fn close(&mut self, api: &Api) {
        if self.encoder.is_null() {
            return;
        }
        // SAFETY: the buffers belong to this session, which is destroyed last. Teardown failures
        // can't be acted upon: destroying the session frees everything it still holds.
        unsafe {
            if !self.input.is_null() {
                call!(api.fns, nvEncDestroyInputBuffer(self.encoder, self.input));
            }
            if !self.output.is_null() {
                call!(
                    api.fns,
                    nvEncDestroyBitstreamBuffer(self.encoder, self.output)
                );
            }
            call!(api.fns, nvEncDestroyEncoder(self.encoder));
        }
        self.encoder = ptr::null_mut();
        self.input = ptr::null_mut();
        self.output = ptr::null_mut();
    }
}

/// Constant bitrate with a one-frame buffer, so a frame never queues behind the previous one.
fn set_rate(cfg: &mut nv::NV_ENC_CONFIG, config: &EncoderConfig) {
    let bitrate = config.bitrate_kbps.saturating_mul(1000);
    let rc = &mut cfg.rcParams;
    rc.averageBitRate = bitrate;
    rc.maxBitRate = bitrate;
    rc.vbvBufferSize = bitrate / config.fps;
    rc.vbvInitialDelay = rc.vbvBufferSize;
}

/// The NVENC AV1 encoder.
struct NvencEncoder {
    session: Session,
    config: EncoderConfig,
    layers: u8,
    /// The layer count changed (with the frame rate) and goes out with the next picture.
    layers_changed: bool,
    frame_index: u32,
    device: Device,
    api: Api,
}

// SAFETY: the NVENC session and the CUDA context may be used from any thread, one at a time
// (`&mut self`); the context is made current around every use.
unsafe impl Send for NvencEncoder {}

impl NvencEncoder {
    fn new(api: Api, device: Device, config: EncoderConfig) -> Result<Self> {
        let session = {
            let _current = device.enter()?;
            Session::open(&api, &device, &config, wanted_layers(config.fps))?
        };
        Ok(Self {
            layers: wanted_layers(config.fps).min(session.max_layers),
            session,
            config,
            layers_changed: false,
            frame_index: 0,
            device,
            api,
        })
    }

    /// Copies `frame` into the input buffer and returns the buffer's luma pitch.
    fn upload(&self, frame: &I420Frame) -> Result<u32> {
        // SAFETY: plain C struct.
        let mut lock: nv::NV_ENC_LOCK_INPUT_BUFFER = unsafe { zeroed() };
        lock.version = LOCK_INPUT_BUFFER_VER;
        lock.inputBuffer = self.session.input;
        // SAFETY: the input buffer belongs to the session.
        check(
            unsafe {
                call!(
                    self.api.fns,
                    nvEncLockInputBuffer(self.session.encoder, &mut lock)
                )
            },
            "lock the input buffer",
        )?;
        let copied = copy_iyuv(frame, lock.bufferDataPtr.cast(), lock.pitch);
        // SAFETY: the buffer was locked above.
        let unlocked = unsafe {
            call!(
                self.api.fns,
                nvEncUnlockInputBuffer(self.session.encoder, self.session.input)
            )
        };
        copied?;
        check(unlocked, "unlock the input buffer")?;
        Ok(lock.pitch)
    }

    /// Reads the encoded picture out of the output buffer.
    fn download(&self, pts: i64) -> Result<EncodedFrame> {
        // SAFETY: plain C struct.
        let mut lock: nv::NV_ENC_LOCK_BITSTREAM = unsafe { zeroed() };
        lock.version = LOCK_BITSTREAM_VER;
        lock.outputBitstream = self.session.output;
        // SAFETY: the output buffer belongs to the session; encoding is synchronous, so the
        // picture is complete.
        check(
            unsafe {
                call!(
                    self.api.fns,
                    nvEncLockBitstream(self.session.encoder, &mut lock)
                )
            },
            "lock the output buffer",
        )?;
        // SAFETY: while locked, the driver's pointer holds `bitstreamSizeInBytes` bytes.
        let data = unsafe {
            std::slice::from_raw_parts(
                lock.bitstreamBufferPtr.cast::<u8>(),
                lock.bitstreamSizeInBytes as usize,
            )
        }
        .to_vec();
        // SAFETY: the buffer was locked above.
        check(
            unsafe {
                call!(
                    self.api.fns,
                    nvEncUnlockBitstream(self.session.encoder, self.session.output)
                )
            },
            "unlock the output buffer",
        )?;
        let keyframe = matches!(
            lock.pictureType,
            nv::NV_ENC_PIC_TYPE_IDR | nv::NV_ENC_PIC_TYPE_I
        );
        let temporal_layer = if keyframe {
            0
        } else {
            u8::try_from(lock.temporalId)
                .ok()
                .filter(|layer| *layer < self.layers)
                .ok_or_else(|| Error::Codec {
                    library: "NVENC",
                    operation: "read temporal layer id",
                    message: format!("unexpected layer id {}", lock.temporalId),
                })?
        };
        Ok(EncodedFrame {
            data,
            keyframe,
            temporal_layer,
            pts,
        })
    }

    /// Applies the changed rate or frame rate to the running session.
    fn reconfigure(&mut self, operation: &'static str) -> Result<()> {
        let _current = self.device.enter()?;
        self.session.init.encodeConfig = &mut *self.session.config;
        // SAFETY: plain C struct.
        let mut params: nv::NV_ENC_RECONFIGURE_PARAMS = unsafe { zeroed() };
        params.version = RECONFIGURE_PARAMS_VER;
        params.reInitEncodeParams = *self.session.init;
        // SAFETY: the parameters point to the session's config, which outlives the call.
        check(
            unsafe {
                call!(
                    self.api.fns,
                    nvEncReconfigureEncoder(self.session.encoder, &mut params)
                )
            },
            operation,
        )
    }

    /// Changes the stored configuration with `change` and reconfigures the session, or restores
    /// the previous configuration if the driver refuses.
    fn change(
        &mut self,
        operation: &'static str,
        change: impl FnOnce(&mut EncoderConfig),
    ) -> Result<()> {
        let previous = (self.config, *self.session.config, *self.session.init);
        change(&mut self.config);
        self.session.init.frameRateNum = self.config.fps;
        set_rate(&mut self.session.config, &self.config);
        let result = self.reconfigure(operation);
        if result.is_err() {
            (self.config, *self.session.config, *self.session.init) = previous;
            self.session.init.encodeConfig = &mut *self.session.config;
        }
        result
    }
}

/// Copies `frame` into a locked IYUV buffer: the Y plane with `pitch`, then U and V with half
/// the pitch, as NVENC lays out planar 4:2:0.
fn copy_iyuv(frame: &I420Frame, buffer: *mut u8, pitch: u32) -> Result<()> {
    let (width, height) = (frame.width() as usize, frame.height() as usize);
    let pitch = pitch as usize;
    if buffer.is_null() || pitch < width {
        return Err(Error::Codec {
            library: "NVENC",
            operation: "lock the input buffer",
            message: format!("unusable buffer (pitch {pitch} for width {width})"),
        });
    }
    let (chroma_width, chroma_height) = (width.div_ceil(2), height.div_ceil(2));
    let chroma_pitch = pitch.div_ceil(2);
    let u_offset = pitch * height;
    let v_offset = u_offset + chroma_pitch * chroma_height;
    let planes = [
        (frame.y(), width, height, 0, pitch),
        (
            frame.u(),
            chroma_width,
            chroma_height,
            u_offset,
            chroma_pitch,
        ),
        (
            frame.v(),
            chroma_width,
            chroma_height,
            v_offset,
            chroma_pitch,
        ),
    ];
    for (plane, row_len, rows, offset, row_pitch) in planes {
        for (row, source) in plane.chunks_exact(row_len).take(rows).enumerate() {
            // SAFETY: an IYUV buffer locked for this size holds every plane at these offsets
            // and pitches; `row_len` never exceeds the pitch.
            unsafe {
                ptr::copy_nonoverlapping(
                    source.as_ptr(),
                    buffer.add(offset + row * row_pitch),
                    row_len,
                );
            }
        }
    }
    Ok(())
}

impl Drop for NvencEncoder {
    fn drop(&mut self) {
        // The session must go before the context (dropped with `device`). If the context can't
        // be made current, destroying the context below frees the session as well.
        if let Ok(_current) = self.device.enter() {
            self.session.close(&self.api);
        }
    }
}

impl VideoEncoder for NvencEncoder {
    fn codec(&self) -> Codec {
        Codec::Av1
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
        let _current = self.device.enter()?;
        let pitch = self.upload(frame)?;
        // SAFETY: plain C struct.
        let mut pic: nv::NV_ENC_PIC_PARAMS = unsafe { zeroed() };
        pic.version = PIC_PARAMS_VER;
        pic.inputWidth = self.config.width;
        pic.inputHeight = self.config.height;
        pic.inputPitch = pitch;
        pic.inputBuffer = self.session.input;
        pic.outputBitstream = self.session.output;
        pic.bufferFmt = nv::NV_ENC_BUFFER_FORMAT_IYUV;
        pic.pictureStruct = nv::NV_ENC_PIC_STRUCT_FRAME;
        pic.frameIdx = self.frame_index;
        pic.inputTimeStamp = u64::from(self.frame_index);
        if force_keyframe {
            pic.encodePicFlags = FORCE_KEYFRAME;
        }
        if self.layers_changed {
            // SAFETY: an AV1 session's picture parameters are the `av1PicParams` member.
            let av1 = unsafe { &mut pic.codecPicParams.av1PicParams };
            av1.set_temporalConfigUpdate(1);
            av1.numTemporalLayers = u32::from(self.layers);
        }
        // SAFETY: the parameters point to this session's buffers.
        let status = unsafe {
            call!(
                self.api.fns,
                nvEncEncodePicture(self.session.encoder, &mut pic)
            )
        };
        if status == nv::NV_ENC_ERR_NEED_MORE_INPUT {
            // Only happens with B-frames, which are off; nothing to send yet.
            return Ok(Vec::new());
        }
        check(status, "encode")?;
        self.layers_changed = false;
        self.frame_index = self.frame_index.wrapping_add(1);
        Ok(vec![self.download(pts)?])
    }

    fn set_resolution(&mut self, width: u32, height: u32) -> Result<()> {
        let config = EncoderConfig {
            width,
            height,
            ..self.config
        };
        validate(&config)?;
        if (width, height) == (self.config.width, self.config.height) {
            return Ok(());
        }
        // A new session (its first picture is a keyframe); the old one stays until it exists.
        {
            let _current = self.device.enter()?;
            let mut session = Session::open(&self.api, &self.device, &config, self.layers)?;
            std::mem::swap(&mut self.session, &mut session);
            session.close(&self.api);
        }
        self.config = config;
        self.frame_index = 0;
        self.layers_changed = false;
        Ok(())
    }

    fn set_bitrate(&mut self, bitrate_kbps: u32) -> Result<()> {
        validate_bitrate(bitrate_kbps)?;
        self.change("set bitrate", |config| config.bitrate_kbps = bitrate_kbps)
    }

    fn set_fps(&mut self, fps: u32) -> Result<()> {
        validate_fps(fps)?;
        self.change("set frame rate", |config| config.fps = fps)?;
        let layers = wanted_layers(fps).min(self.session.max_layers);
        if layers != self.layers {
            self.layers = layers;
            self.layers_changed = true;
        }
        Ok(())
    }
}

/// No NVIDIA GPU runs in CI, so these tests drive the encoder through a mocked NVENC function
/// list that behaves like the driver: it records what the encoder asks for and hands back
/// pictures with the driver's keyframe and temporal-layer pattern.
#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Call {
        Open,
        Init,
        Reconfigure,
        CreateInput,
        CreateOutput,
        Encode,
        DestroyInput,
        DestroyOutput,
        DestroyEncoder,
    }

    struct Picture {
        flags: u32,
        pitch: u32,
        layer_update: Option<u32>,
    }

    struct Mock {
        has_av1: bool,
        svc: i32,
        max_layers: i32,
        max_size: i32,
        fail_encode: bool,
        fail_reconfigure: bool,
        calls: Vec<Call>,
        /// Every configuration the session was initialised or reconfigured with.
        configs: Vec<(nv::NV_ENC_INITIALIZE_PARAMS, nv::NV_ENC_CONFIG)>,
        pitch: u32,
        input: Vec<u8>,
        pictures: Vec<Picture>,
        layers: u32,
        since_keyframe: u32,
        output: Vec<u8>,
        picture_type: nv::NV_ENC_PIC_TYPE,
        temporal_id: u32,
    }

    impl Default for Mock {
        fn default() -> Self {
            Self {
                has_av1: true,
                svc: 1,
                max_layers: 4,
                max_size: 4096,
                fail_encode: false,
                fail_reconfigure: false,
                calls: Vec::new(),
                configs: Vec::new(),
                pitch: 0,
                input: Vec::new(),
                pictures: Vec::new(),
                layers: 1,
                since_keyframe: 0,
                output: Vec::new(),
                picture_type: nv::NV_ENC_PIC_TYPE_P,
                temporal_id: 0,
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

    const INPUT: nv::NV_ENC_INPUT_PTR = 0x10 as nv::NV_ENC_INPUT_PTR;
    const OUTPUT: nv::NV_ENC_OUTPUT_PTR = 0x20 as nv::NV_ENC_OUTPUT_PTR;
    const H264_GUID: nv::GUID = guid(1, 2, 3, [4; 8]);

    unsafe extern "C" fn open_session(
        params: *mut nv::NV_ENC_OPEN_ENCODE_SESSION_EX_PARAMS,
        encoder: *mut *mut c_void,
    ) -> Status {
        // SAFETY: valid pointers from the encoder.
        let params = unsafe { &*params };
        assert_eq!(params.version, OPEN_SESSION_VER);
        assert_eq!(params.deviceType, nv::NV_ENC_DEVICE_TYPE_CUDA);
        assert_eq!(params.apiVersion, nv::NVENCAPI_VERSION);
        assert!(!params.device.is_null());
        // SAFETY: see above.
        unsafe { *encoder = ptr::NonNull::<u8>::dangling().as_ptr().cast() };
        mock(|m| m.calls.push(Call::Open));
        nv::NV_ENC_SUCCESS
    }

    unsafe extern "C" fn guid_count(_: *mut c_void, count: *mut u32) -> Status {
        // SAFETY: valid out pointer.
        unsafe { *count = if mock(|m| m.has_av1) { 2 } else { 1 } };
        nv::NV_ENC_SUCCESS
    }

    unsafe extern "C" fn guids(
        _: *mut c_void,
        guids: *mut nv::GUID,
        size: u32,
        written: *mut u32,
    ) -> Status {
        let list: &[nv::GUID] = if mock(|m| m.has_av1) {
            &[H264_GUID, AV1_GUID]
        } else {
            &[H264_GUID]
        };
        assert!(size as usize >= list.len());
        // SAFETY: `guids` holds `size` entries.
        unsafe {
            ptr::copy_nonoverlapping(list.as_ptr(), guids, list.len());
            *written = list.len() as u32;
        }
        nv::NV_ENC_SUCCESS
    }

    unsafe extern "C" fn caps(
        _: *mut c_void,
        codec: nv::GUID,
        param: *mut nv::NV_ENC_CAPS_PARAM,
        value: *mut c_int,
    ) -> Status {
        assert!(same_guid(&codec, &AV1_GUID));
        // SAFETY: valid pointers from the encoder.
        let param = unsafe { &*param };
        assert_eq!(param.version, CAPS_PARAM_VER);
        let answer = mock(|m| match param.capsToQuery {
            nv::NV_ENC_CAPS_WIDTH_MAX | nv::NV_ENC_CAPS_HEIGHT_MAX => m.max_size,
            nv::NV_ENC_CAPS_SUPPORT_TEMPORAL_SVC => m.svc,
            nv::NV_ENC_CAPS_NUM_MAX_TEMPORAL_LAYERS => m.max_layers,
            other => panic!("unexpected caps query {other}"),
        });
        // SAFETY: see above.
        unsafe { *value = answer };
        nv::NV_ENC_SUCCESS
    }

    unsafe extern "C" fn preset(
        _: *mut c_void,
        codec: nv::GUID,
        preset: nv::GUID,
        tuning: nv::NV_ENC_TUNING_INFO,
        config: *mut nv::NV_ENC_PRESET_CONFIG,
    ) -> Status {
        assert!(same_guid(&codec, &AV1_GUID));
        assert!(same_guid(&preset, &PRESET_P4_GUID));
        assert_eq!(tuning, nv::NV_ENC_TUNING_INFO_ULTRA_LOW_LATENCY);
        // SAFETY: valid pointer from the encoder.
        let config = unsafe { &mut *config };
        assert_eq!(config.version, PRESET_CONFIG_VER);
        assert_eq!(config.presetCfg.version, CONFIG_VER);
        // Defaults the encoder has to override.
        config.presetCfg.gopLength = 250;
        config.presetCfg.frameIntervalP = 3;
        nv::NV_ENC_SUCCESS
    }

    /// Records the configuration and returns the layer count it starts with.
    fn record(init: &nv::NV_ENC_INITIALIZE_PARAMS, m: &mut Mock) {
        // SAFETY: the encoder always passes its config.
        let config = unsafe { *init.encodeConfig };
        m.configs.push((*init, config));
    }

    unsafe extern "C" fn initialize(
        _: *mut c_void,
        init: *mut nv::NV_ENC_INITIALIZE_PARAMS,
    ) -> Status {
        // SAFETY: valid pointer from the encoder.
        let init = unsafe { &*init };
        assert_eq!(init.version, INITIALIZE_PARAMS_VER);
        // SAFETY: AV1 session.
        let av1 = unsafe { (*init.encodeConfig).encodeCodecConfig.av1Config };
        mock(|m| {
            record(init, m);
            m.calls.push(Call::Init);
            m.layers = av1.numTemporalLayers.max(1);
            m.since_keyframe = 0;
            m.pitch = init.encodeWidth.next_multiple_of(64);
            let rows = init.encodeHeight as usize;
            let pitch = m.pitch as usize;
            m.input = vec![0; pitch * rows + 2 * pitch.div_ceil(2) * rows.div_ceil(2)];
        });
        nv::NV_ENC_SUCCESS
    }

    unsafe extern "C" fn reconfigure(
        _: *mut c_void,
        params: *mut nv::NV_ENC_RECONFIGURE_PARAMS,
    ) -> Status {
        // SAFETY: valid pointer from the encoder.
        let params = unsafe { &*params };
        assert_eq!(params.version, RECONFIGURE_PARAMS_VER);
        assert_eq!(params.resetEncoder(), 0);
        assert_eq!(params.forceIDR(), 0);
        mock(|m| {
            m.calls.push(Call::Reconfigure);
            if m.fail_reconfigure {
                return nv::NV_ENC_ERR_INVALID_PARAM;
            }
            record(&params.reInitEncodeParams, m);
            nv::NV_ENC_SUCCESS
        })
    }

    unsafe extern "C" fn create_input(
        _: *mut c_void,
        params: *mut nv::NV_ENC_CREATE_INPUT_BUFFER,
    ) -> Status {
        // SAFETY: valid pointer from the encoder.
        let params = unsafe { &mut *params };
        assert_eq!(params.version, CREATE_INPUT_BUFFER_VER);
        assert_eq!(params.bufferFmt, nv::NV_ENC_BUFFER_FORMAT_IYUV);
        params.inputBuffer = INPUT;
        mock(|m| m.calls.push(Call::CreateInput));
        nv::NV_ENC_SUCCESS
    }

    unsafe extern "C" fn lock_input(
        _: *mut c_void,
        params: *mut nv::NV_ENC_LOCK_INPUT_BUFFER,
    ) -> Status {
        // SAFETY: valid pointer from the encoder.
        let params = unsafe { &mut *params };
        assert_eq!(params.version, LOCK_INPUT_BUFFER_VER);
        assert_eq!(params.inputBuffer, INPUT);
        mock(|m| {
            params.bufferDataPtr = m.input.as_mut_ptr().cast();
            params.pitch = m.pitch;
        });
        nv::NV_ENC_SUCCESS
    }

    unsafe extern "C" fn unlock_input(_: *mut c_void, input: nv::NV_ENC_INPUT_PTR) -> Status {
        assert_eq!(input, INPUT);
        nv::NV_ENC_SUCCESS
    }

    unsafe extern "C" fn create_output(
        _: *mut c_void,
        params: *mut nv::NV_ENC_CREATE_BITSTREAM_BUFFER,
    ) -> Status {
        // SAFETY: valid pointer from the encoder.
        let params = unsafe { &mut *params };
        assert_eq!(params.version, CREATE_BITSTREAM_BUFFER_VER);
        params.bitstreamBuffer = OUTPUT;
        mock(|m| m.calls.push(Call::CreateOutput));
        nv::NV_ENC_SUCCESS
    }

    /// Encodes like NVENC with temporal SVC: a keyframe on request and at the session start,
    /// then the usual dyadic layer pattern (0, 2, 1, 2 for three layers).
    unsafe extern "C" fn encode(_: *mut c_void, pic: *mut nv::NV_ENC_PIC_PARAMS) -> Status {
        // SAFETY: valid pointer from the encoder.
        let pic = unsafe { &*pic };
        assert_eq!(pic.version, PIC_PARAMS_VER);
        assert_eq!((pic.inputBuffer, pic.outputBitstream), (INPUT, OUTPUT));
        assert_eq!(pic.bufferFmt, nv::NV_ENC_BUFFER_FORMAT_IYUV);
        // SAFETY: AV1 session.
        let av1 = unsafe { pic.codecPicParams.av1PicParams };
        mock(|m| {
            m.calls.push(Call::Encode);
            if m.fail_encode {
                return nv::NV_ENC_ERR_GENERIC;
            }
            let layer_update = (av1.temporalConfigUpdate() == 1).then_some(av1.numTemporalLayers);
            if let Some(layers) = layer_update {
                m.layers = layers;
            }
            m.pictures.push(Picture {
                flags: pic.encodePicFlags,
                pitch: pic.inputPitch,
                layer_update,
            });
            if pic.encodePicFlags & FORCE_KEYFRAME != 0 {
                m.since_keyframe = 0;
            }
            let position = m.since_keyframe % (1 << (m.layers - 1));
            m.temporal_id = if position == 0 {
                0
            } else {
                m.layers - 1 - position.trailing_zeros()
            };
            m.picture_type = if m.since_keyframe == 0 {
                nv::NV_ENC_PIC_TYPE_IDR
            } else {
                nv::NV_ENC_PIC_TYPE_P
            };
            m.output = vec![pic.frameIdx as u8, m.temporal_id as u8];
            m.since_keyframe += 1;
            nv::NV_ENC_SUCCESS
        })
    }

    unsafe extern "C" fn lock_output(
        _: *mut c_void,
        lock: *mut nv::NV_ENC_LOCK_BITSTREAM,
    ) -> Status {
        // SAFETY: valid pointer from the encoder.
        let lock = unsafe { &mut *lock };
        assert_eq!(lock.version, LOCK_BITSTREAM_VER);
        assert_eq!(lock.outputBitstream, OUTPUT);
        mock(|m| {
            lock.bitstreamBufferPtr = m.output.as_mut_ptr().cast();
            lock.bitstreamSizeInBytes = m.output.len() as u32;
            lock.pictureType = m.picture_type;
            lock.temporalId = m.temporal_id;
        });
        nv::NV_ENC_SUCCESS
    }

    unsafe extern "C" fn unlock_output(_: *mut c_void, output: nv::NV_ENC_OUTPUT_PTR) -> Status {
        assert_eq!(output, OUTPUT);
        nv::NV_ENC_SUCCESS
    }

    unsafe extern "C" fn destroy_input(_: *mut c_void, input: nv::NV_ENC_INPUT_PTR) -> Status {
        assert_eq!(input, INPUT);
        mock(|m| m.calls.push(Call::DestroyInput));
        nv::NV_ENC_SUCCESS
    }

    unsafe extern "C" fn destroy_output(_: *mut c_void, output: nv::NV_ENC_OUTPUT_PTR) -> Status {
        assert_eq!(output, OUTPUT);
        mock(|m| m.calls.push(Call::DestroyOutput));
        nv::NV_ENC_SUCCESS
    }

    unsafe extern "C" fn destroy_encoder(_: *mut c_void) -> Status {
        mock(|m| m.calls.push(Call::DestroyEncoder));
        nv::NV_ENC_SUCCESS
    }

    fn function_list() -> nv::NV_ENCODE_API_FUNCTION_LIST {
        // SAFETY: plain C struct.
        let mut fns: nv::NV_ENCODE_API_FUNCTION_LIST = unsafe { zeroed() };
        fns.version = FUNCTION_LIST_VER;
        fns.nvEncOpenEncodeSessionEx = Some(open_session);
        fns.nvEncGetEncodeGUIDCount = Some(guid_count);
        fns.nvEncGetEncodeGUIDs = Some(guids);
        fns.nvEncGetEncodeCaps = Some(caps);
        fns.nvEncGetEncodePresetConfigEx = Some(preset);
        fns.nvEncInitializeEncoder = Some(initialize);
        fns.nvEncReconfigureEncoder = Some(reconfigure);
        fns.nvEncCreateInputBuffer = Some(create_input);
        fns.nvEncDestroyInputBuffer = Some(destroy_input);
        fns.nvEncLockInputBuffer = Some(lock_input);
        fns.nvEncUnlockInputBuffer = Some(unlock_input);
        fns.nvEncCreateBitstreamBuffer = Some(create_output);
        fns.nvEncDestroyBitstreamBuffer = Some(destroy_output);
        fns.nvEncEncodePicture = Some(encode);
        fns.nvEncLockBitstream = Some(lock_output);
        fns.nvEncUnlockBitstream = Some(unlock_output);
        fns.nvEncDestroyEncoder = Some(destroy_encoder);
        fns
    }

    fn config(width: u32, height: u32, fps: u32) -> EncoderConfig {
        EncoderConfig {
            width,
            height,
            fps,
            bitrate_kbps: 3000,
        }
    }

    fn open_mock(config: EncoderConfig) -> Result<NvencEncoder> {
        NvencEncoder::new(Api::new(function_list(), None)?, Device::Mock, config)
    }

    fn last_config() -> (nv::NV_ENC_INITIALIZE_PARAMS, nv::NV_ENC_CONFIG) {
        mock(|m| *m.configs.last().expect("a configuration"))
    }

    fn av1(config: &nv::NV_ENC_CONFIG) -> nv::NV_ENC_CONFIG_AV1 {
        // SAFETY: AV1 session.
        unsafe { config.encodeCodecConfig.av1Config }
    }

    /// A frame whose every byte tells where it came from: Y rows count up, U is 200, V is 250.
    fn frame(width: u32, height: u32) -> I420Frame {
        let luma = (width * height) as usize;
        let chroma = (I420Frame::buffer_len(width, height) - luma) / 2;
        let mut data: Vec<u8> = (0..luma).map(|i| (i % 199) as u8).collect();
        data.extend(std::iter::repeat_n(200, chroma));
        data.extend(std::iter::repeat_n(250, chroma));
        I420Frame::new(width, height, data).expect("frame")
    }

    #[test]
    fn opens_a_low_latency_cbr_av1_session_with_three_layers_at_30_fps() {
        let encoder = open_mock(config(1920, 1080, 30)).expect("encoder");
        assert_eq!(encoder.codec(), Codec::Av1);
        assert_eq!(encoder.temporal_layers(), 3);
        let (init, cfg) = last_config();
        assert!(same_guid(&init.encodeGUID, &AV1_GUID));
        assert!(same_guid(&init.presetGUID, &PRESET_P4_GUID));
        assert_eq!(init.tuningInfo, nv::NV_ENC_TUNING_INFO_ULTRA_LOW_LATENCY);
        assert_eq!((init.encodeWidth, init.encodeHeight), (1920, 1080));
        assert_eq!((init.frameRateNum, init.frameRateDen), (30, 1));
        assert_eq!((init.enablePTD, init.enableEncodeAsync), (1, 0));
        assert_eq!(cfg.version, CONFIG_VER);
        assert_eq!(cfg.gopLength, nv::NVENC_INFINITE_GOPLENGTH);
        assert_eq!(cfg.frameIntervalP, 1);
        assert_eq!(cfg.rcParams.rateControlMode, nv::NV_ENC_PARAMS_RC_CBR);
        assert_eq!(cfg.rcParams.averageBitRate, 3_000_000);
        assert_eq!(cfg.rcParams.maxBitRate, 3_000_000);
        assert_eq!(cfg.rcParams.vbvBufferSize, 100_000);
        let av1 = av1(&cfg);
        assert_eq!(av1.idrPeriod, nv::NVENC_INFINITE_GOPLENGTH);
        assert_eq!(av1.repeatSeqHdr(), 1);
        assert_eq!(av1.chromaFormatIDC(), 1);
        assert_eq!(av1.enableTemporalSVC(), 1);
        assert_eq!(av1.maxTemporalLayersMinus1, 3);
        assert_eq!(av1.numTemporalLayers, 3);
        assert_eq!(
            calls(),
            [
                Call::Open,
                Call::Init,
                Call::CreateInput,
                Call::CreateOutput
            ]
        );
    }

    #[test]
    fn uses_four_layers_at_60_fps() {
        let encoder = open_mock(config(1920, 1080, 60)).expect("encoder");
        assert_eq!(encoder.temporal_layers(), 4);
        assert_eq!(av1(&last_config().1).numTemporalLayers, 4);
    }

    #[test]
    fn a_gpu_without_temporal_svc_streams_a_single_layer() {
        mock(|m| m.svc = 0);
        let encoder = open_mock(config(1280, 720, 60)).expect("encoder");
        assert_eq!(encoder.temporal_layers(), 1);
        let av1 = av1(&last_config().1);
        assert_eq!(av1.enableTemporalSVC(), 0);
        assert_eq!(av1.numTemporalLayers, 0);
    }

    #[test]
    fn the_layer_count_is_capped_by_the_gpu() {
        mock(|m| m.max_layers = 2);
        let encoder = open_mock(config(1280, 720, 60)).expect("encoder");
        assert_eq!(encoder.temporal_layers(), 2);
        let av1 = av1(&last_config().1);
        assert_eq!((av1.maxTemporalLayersMinus1, av1.numTemporalLayers), (1, 2));
    }

    #[test]
    fn a_gpu_without_av1_is_unavailable_and_its_session_closed() {
        mock(|m| m.has_av1 = false);
        let err = open_mock(config(1280, 720, 30)).err().expect("no AV1");
        let Error::HardwareUnavailable { encoder, reason } = err else {
            panic!("unexpected error {err}");
        };
        assert_eq!(encoder, HardwareEncoder::Nvenc);
        assert!(reason.contains("no AV1 encoder"), "{reason}");
        assert_eq!(calls(), [Call::Open, Call::DestroyEncoder]);
    }

    #[test]
    fn a_size_beyond_the_gpu_limit_is_unavailable() {
        mock(|m| m.max_size = 1024);
        let err = open_mock(config(1920, 1080, 30)).err().expect("too large");
        assert!(matches!(err, Error::HardwareUnavailable { .. }), "{err}");
    }

    #[test]
    fn copies_the_i420_planes_with_the_buffer_pitch() {
        let mut encoder = open_mock(config(20, 10, 30)).expect("encoder");
        let frame = frame(20, 10);
        encoder.encode(&frame, 0, false).expect("encode");
        let (pitch, input, picture_pitch) =
            mock(|m| (m.pitch as usize, m.input.clone(), m.pictures[0].pitch));
        assert_eq!(pitch, 64);
        assert_eq!(picture_pitch, 64);
        for row in 0..10 {
            assert_eq!(
                &input[row * pitch..row * pitch + 20],
                &frame.y()[row * 20..][..20]
            );
        }
        let chroma_pitch = pitch / 2;
        let u = pitch * 10;
        let v = u + chroma_pitch * 5;
        for row in 0..5 {
            assert!(
                input[u + row * chroma_pitch..][..10]
                    .iter()
                    .all(|&b| b == 200)
            );
            assert!(
                input[v + row * chroma_pitch..][..10]
                    .iter()
                    .all(|&b| b == 250)
            );
        }
    }

    #[test]
    fn returns_keyframes_and_temporal_layers_from_the_driver() {
        let mut encoder = open_mock(config(64, 64, 30)).expect("encoder");
        let frame = frame(64, 64);
        let mut packets = Vec::new();
        for (pts, force) in [
            (0, false),
            (33, false),
            (67, false),
            (100, false),
            (133, true),
            (167, false),
        ] {
            let out = encoder.encode(&frame, pts, force).expect("encode");
            assert_eq!(out.len(), 1);
            packets.extend(out);
        }
        let summary: Vec<_> = packets
            .iter()
            .map(|p| (p.keyframe, p.temporal_layer, p.pts))
            .collect();
        assert_eq!(
            summary,
            [
                (true, 0, 0),
                (false, 2, 33),
                (false, 1, 67),
                (false, 2, 100),
                (true, 0, 133),
                (false, 2, 167)
            ]
        );
        assert_eq!(packets[1].data, [1, 2]);
        let forced = mock(|m| m.pictures[4].flags);
        assert_eq!(forced, FORCE_KEYFRAME);
        assert_eq!(mock(|m| m.pictures[3].flags), 0);
    }

    #[test]
    fn rejects_a_frame_of_the_wrong_size() {
        let mut encoder = open_mock(config(64, 64, 30)).expect("encoder");
        let err = encoder
            .encode(&frame(32, 32), 0, false)
            .expect_err("wrong size");
        assert!(matches!(err, Error::InvalidFrame(_)), "{err}");
    }

    #[test]
    fn a_driver_failure_while_encoding_is_a_codec_error() {
        let mut encoder = open_mock(config(64, 64, 30)).expect("encoder");
        mock(|m| m.fail_encode = true);
        let err = encoder
            .encode(&frame(64, 64), 0, false)
            .expect_err("failure");
        assert!(
            matches!(
                err,
                Error::Codec {
                    library: "NVENC",
                    ..
                }
            ),
            "{err}"
        );
    }

    #[test]
    fn set_bitrate_reconfigures_the_running_session() {
        let mut encoder = open_mock(config(1280, 720, 30)).expect("encoder");
        encoder.set_bitrate(6000).expect("bitrate");
        assert_eq!(encoder.config().bitrate_kbps, 6000);
        let (_, cfg) = last_config();
        assert_eq!(cfg.rcParams.averageBitRate, 6_000_000);
        assert_eq!(cfg.rcParams.vbvBufferSize, 200_000);
        assert_eq!(calls().last(), Some(&Call::Reconfigure));
        assert!(matches!(
            encoder.set_bitrate(0),
            Err(Error::InvalidConfig(_))
        ));
    }

    #[test]
    fn a_refused_reconfiguration_keeps_the_previous_settings() {
        let mut encoder = open_mock(config(1280, 720, 30)).expect("encoder");
        mock(|m| m.fail_reconfigure = true);
        encoder.set_bitrate(6000).expect_err("refused");
        assert_eq!(encoder.config().bitrate_kbps, 3000);
        assert_eq!(encoder.session.config.rcParams.averageBitRate, 3_000_000);
    }

    #[test]
    fn going_to_60_fps_adds_the_fourth_layer_with_the_next_picture() {
        let mut encoder = open_mock(config(64, 64, 30)).expect("encoder");
        let frame = frame(64, 64);
        encoder.encode(&frame, 0, false).expect("encode");
        encoder.set_fps(60).expect("fps");
        assert_eq!(encoder.temporal_layers(), 4);
        let (init, cfg) = last_config();
        assert_eq!(init.frameRateNum, 60);
        assert_eq!(cfg.rcParams.vbvBufferSize, 50_000);
        encoder.encode(&frame, 17, false).expect("encode");
        encoder.encode(&frame, 33, false).expect("encode");
        let updates: Vec<_> = mock(|m| m.pictures.iter().map(|p| p.layer_update).collect());
        assert_eq!(updates, [None, Some(4), None]);
        encoder.set_fps(30).expect("fps");
        assert_eq!(encoder.temporal_layers(), 3);
    }

    #[test]
    fn set_resolution_replaces_the_session() {
        let mut encoder = open_mock(config(64, 64, 30)).expect("encoder");
        encoder.encode(&frame(64, 64), 0, false).expect("encode");
        encoder.set_resolution(128, 96).expect("resize");
        assert_eq!((encoder.config().width, encoder.config().height), (128, 96));
        let (init, _) = last_config();
        assert_eq!((init.encodeWidth, init.encodeHeight), (128, 96));
        let out = encoder.encode(&frame(128, 96), 33, false).expect("encode");
        assert!(out[0].keyframe);
        assert_eq!(
            calls()[5..],
            [
                Call::Open,
                Call::Init,
                Call::CreateInput,
                Call::CreateOutput,
                Call::DestroyInput,
                Call::DestroyOutput,
                Call::DestroyEncoder,
                Call::Encode
            ]
        );
    }

    #[test]
    fn dropping_the_encoder_frees_buffers_before_the_session() {
        drop(open_mock(config(64, 64, 30)).expect("encoder"));
        assert_eq!(
            calls()[4..],
            [
                Call::DestroyInput,
                Call::DestroyOutput,
                Call::DestroyEncoder
            ]
        );
    }

    #[test]
    fn vp9_is_unavailable_without_loading_the_driver() {
        let err = open(Codec::Vp9, config(64, 64, 30)).err().expect("no VP9");
        assert!(matches!(err, Error::HardwareUnavailable { .. }), "{err}");
    }

    #[test]
    fn drivers_before_api_13_are_unavailable() {
        let err = check_driver_version(nv::NV_ENC_SUCCESS, (12 << 4) | 2).expect_err("too old");
        let Error::HardwareUnavailable { reason, .. } = err else {
            panic!("unexpected error {err}");
        };
        assert!(
            reason.contains("12.2") && reason.contains("570"),
            "{reason}"
        );
        check_driver_version(nv::NV_ENC_SUCCESS, 13 << 4).expect("13.0");
        check_driver_version(nv::NV_ENC_SUCCESS, (13 << 4) | 1).expect("13.1");
        assert!(check_driver_version(nv::NV_ENC_ERR_GENERIC, 13 << 4).is_err());
    }

    #[test]
    fn a_function_list_with_gaps_is_unavailable() {
        let mut fns = function_list();
        fns.nvEncReconfigureEncoder = None;
        let err = Api::new(fns, None).err().expect("incomplete");
        assert!(err.to_string().contains("nvEncReconfigureEncoder"), "{err}");
    }
}
