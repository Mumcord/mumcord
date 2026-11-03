//! AMD AMF AV1 encoder (Radeon RX 7000 and newer) behind [`VideoEncoder`].
//!
//! The AMF runtime ships with the AMD driver and is loaded at runtime. The encoder runs on a
//! Direct3D 11 context on the default adapter and takes NV12 frames from host memory; AMF uploads
//! them itself. Only Windows uses it: on Linux, AMD GPUs encode through VA-API.
//!
//! AMF reports whether a picture is a keyframe but not its temporal layer, so the layer comes
//! from the AV1 OBU extension headers in the bitstream ([`super::obu`]).

use std::ffi::c_void;
use std::time::{Duration, Instant};
use std::{ptr, thread};

use libloading::Library;

use super::{HardwareEncoder, dl, ffi::amf, obu};
use crate::{Codec, EncodedFrame, EncoderConfig, Error, I420Frame, Result, VideoEncoder};

const ENCODER: HardwareEncoder = HardwareEncoder::Amf;

/// The AMF version of the headers (`AMF_FULL_VERSION`, a function-like macro bindgen can't
/// translate). The runtime refuses versions newer than itself.
const AMF_VERSION: u64 = ((amf::AMF_VERSION_MAJOR as u64) << 48)
    | ((amf::AMF_VERSION_MINOR as u64) << 32)
    | ((amf::AMF_VERSION_RELEASE as u64) << 16)
    | amf::AMF_VERSION_BUILD_NUM as u64;

const AV1_COMPONENT: &str = "AMFVideoEncoderHW_AV1";

/// `IID_AMFBuffer` from `core/Buffer.h`.
const BUFFER_IID: amf::AMFGuid = amf::AMFGuid {
    data1: 0xb04b_7248,
    data2: 0xb6f0,
    data3: 0x4321,
    data41: 0xb6,
    data42: 0x91,
    data43: 0xba,
    data44: 0xa4,
    data45: 0x74,
    data46: 0x0f,
    data47: 0x9f,
    data48: 0xcb,
};

/// Property names from `components/VideoEncoderAV1.h`. They are wide-string macros there, which
/// bindgen can't translate.
mod name {
    pub const USAGE: &str = "Av1Usage";
    pub const FRAME_SIZE: &str = "Av1FrameSize";
    pub const FRAME_RATE: &str = "Av1FrameRate";
    pub const ALIGNMENT_MODE: &str = "Av1AlignmentMode";
    pub const LATENCY_MODE: &str = "Av1EncodingLatencyMode";
    pub const QUERY_TIMEOUT: &str = "Av1QueryTimeout";
    pub const RATE_CONTROL: &str = "Av1RateControlMethod";
    pub const TARGET_BITRATE: &str = "Av1TargetBitrate";
    pub const PEAK_BITRATE: &str = "Av1PeakBitrate";
    pub const VBV_BUFFER_SIZE: &str = "Av1VBVBufferSize";
    pub const INITIAL_VBV_FULLNESS: &str = "Av1InitialVBVBufferFullness";
    pub const FILLER_DATA: &str = "Av1FillerData";
    pub const GOP_SIZE: &str = "Av1GOPSize";
    pub const HEADER_INSERTION: &str = "Av1HeaderInsertionMode";
    pub const CAP_MAX_TEMPORAL_LAYERS: &str = "Av1CapMaxNumTemporalLayers";
    pub const MAX_TEMPORAL_LAYERS: &str = "Av1MaxNumOfTemporalLayers";
    pub const TEMPORAL_LAYERS: &str = "Av1NumTemporalLayers";
    pub const FORCE_FRAME_TYPE: &str = "Av1ForceFrameType";
    pub const FORCE_SEQUENCE_HEADER: &str = "Av1ForceInsertSequenceHeader";
    pub const OUTPUT_FRAME_TYPE: &str = "Av1OutputFrameType";
}

/// The most temporal layers any share uses (60 fps down to 7.5 fps).
const MAX_LAYERS: u8 = 4;

/// Temporal layers for `fps`: 3 below 60 fps and 4 from 60 fps (FR-7.37, like VA-API), so a
/// viewer of a 60 fps share can still drop to 30, 15 or 7.5 fps.
fn wanted_layers(fps: u32) -> u8 {
    if fps >= 60 { MAX_LAYERS } else { 3 }
}

/// Highest frame rate accepted; screen shares stop at 60 fps.
const MAX_FPS: u32 = 240;

/// How long one encoded picture may take before the encoder counts as stuck.
const OUTPUT_TIMEOUT: Duration = Duration::from_secs(1);

/// AMF's own wait inside `QueryOutput`, in milliseconds; below [`OUTPUT_TIMEOUT`] so a slow
/// picture is polled again rather than given up on.
const QUERY_TIMEOUT_MS: i64 = 100;

/// Calls the method `$method` through the C vtable of the AMF object `$obj`, or evaluates to
/// `$missing` when the runtime leaves that slot empty.
///
/// The caller wraps it in `unsafe`: `$obj` must be a live object of the matching interface.
macro_rules! vcall {
    ($obj:expr, $method:ident($($arg:expr),* $(,)?) or $missing:expr) => {{
        let obj = $obj;
        match (*(*obj).pVtbl).$method {
            Some(method) => method(obj $(, $arg)*),
            None => $missing,
        }
    }};
    ($obj:expr, $method:ident($($arg:expr),* $(,)?)) => {
        vcall!($obj, $method($($arg),*) or amf::AMF_NOT_SUPPORTED)
    };
}

type Status = amf::AMF_RESULT;

fn codec_error(operation: &'static str, status: Status) -> Error {
    Error::Codec {
        library: "AMF",
        operation,
        message: format!("AMF_RESULT {status}"),
    }
}

fn check(status: Status, operation: &'static str) -> Result<()> {
    if status == amf::AMF_OK {
        Ok(())
    } else {
        Err(codec_error(operation, status))
    }
}

/// Like [`check`], but for the steps of opening an encoder: any failure there means this GPU or
/// driver can't provide it, and the caller falls back to software.
fn check_open(status: Status, step: &str) -> Result<()> {
    if status == amf::AMF_OK {
        Ok(())
    } else {
        Err(ENCODER.unavailable(format!("{step} failed (AMF_RESULT {status})")))
    }
}

pub(super) fn open(codec: Codec, config: EncoderConfig) -> Result<Box<dyn VideoEncoder>> {
    if codec != Codec::Av1 {
        return Err(ENCODER.unavailable("AMF has no VP9 encoder"));
    }
    if !cfg!(windows) {
        return Err(ENCODER.unavailable("AMF is only used on Windows; Linux uses VA-API"));
    }
    validate(&config)?;
    Ok(Box::new(AmfEncoder::new(Api::load()?, config)?))
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

/// `name` as a NUL-terminated wide string. All AMF names are ASCII.
fn wide(name: &str) -> Vec<amf::wchar_t> {
    name.bytes()
        .map(amf::wchar_t::from)
        .chain(std::iter::once(0))
        .collect()
}

fn variant(type_: amf::AMF_VARIANT_TYPE) -> amf::AMFVariantStruct {
    // SAFETY: a plain C struct of an enum and a union of plain values; all zeroes is "empty".
    let mut value: amf::AMFVariantStruct = unsafe { std::mem::zeroed() };
    value.type_ = type_;
    value
}

fn int(number: i64) -> amf::AMFVariantStruct {
    let mut value = variant(amf::AMF_VARIANT_INT64);
    value.__bindgen_anon_1.int64Value = number;
    value
}

fn boolean(flag: bool) -> amf::AMFVariantStruct {
    let mut value = variant(amf::AMF_VARIANT_BOOL);
    value.__bindgen_anon_1.boolValue = flag.into();
    value
}

fn size(width: u32, height: u32) -> Result<amf::AMFVariantStruct> {
    let too_large = || Error::InvalidConfig(format!("size {width}x{height}"));
    let mut value = variant(amf::AMF_VARIANT_SIZE);
    value.__bindgen_anon_1.sizeValue = amf::AMFSize {
        width: i32::try_from(width).map_err(|_| too_large())?,
        height: i32::try_from(height).map_err(|_| too_large())?,
    };
    Ok(value)
}

fn rate(fps: u32) -> amf::AMFVariantStruct {
    let mut value = variant(amf::AMF_VARIANT_RATE);
    value.__bindgen_anon_1.rateValue = amf::AMFRate { num: fps, den: 1 };
    value
}

/// The loaded AMF runtime.
struct Api {
    factory: *mut amf::AMFFactory,
    /// Keeps the runtime loaded while the factory is in use; `None` for the test mock.
    _library: Option<Library>,
}

impl Api {
    fn load() -> Result<Self> {
        let library = dl::open(ENCODER, dl::AMF)?;
        // SAFETY: AMFInit_Fn is the AMF header's type for the AMFInit entry point.
        let init = unsafe { dl::symbol::<amf::AMFInit_Fn>(ENCODER, &library, "AMFInit") }?
            .ok_or_else(|| ENCODER.unavailable("the runtime's AMFInit is null"))?;
        let mut factory = ptr::null_mut();
        // SAFETY: AMFInit writes the runtime's factory, which lives as long as the library.
        let status = unsafe { init(AMF_VERSION, &mut factory) };
        check_open(
            status,
            &format!(
                "initialising AMF {}.{}.{} (driver too old?)",
                amf::AMF_VERSION_MAJOR,
                amf::AMF_VERSION_MINOR,
                amf::AMF_VERSION_RELEASE
            ),
        )?;
        Self::new(factory, Some(library))
    }

    fn new(factory: *mut amf::AMFFactory, library: Option<Library>) -> Result<Self> {
        if factory.is_null() {
            return Err(ENCODER.unavailable("the runtime returned no factory"));
        }
        Ok(Self {
            factory,
            _library: library,
        })
    }
}

/// An AMF context on the default Direct3D 11 adapter.
struct Context(*mut amf::AMFContext);

impl Context {
    fn open(api: &Api) -> Result<Self> {
        let mut context = ptr::null_mut();
        // SAFETY: the factory is live while `api` is.
        check_open(
            unsafe { vcall!(api.factory, CreateContext(&mut context)) },
            "creating the AMF context",
        )?;
        if context.is_null() {
            return Err(ENCODER.unavailable("the runtime returned no context"));
        }
        let context = Self(context);
        // SAFETY: a null device asks AMF to create one on the default adapter.
        check_open(
            unsafe { vcall!(context.0, InitDX11(ptr::null_mut(), amf::AMF_DX11_1)) },
            "initialising Direct3D 11",
        )?;
        Ok(context)
    }
}

impl Drop for Context {
    fn drop(&mut self) {
        // SAFETY: the context is live and released exactly once here. Teardown failures leave
        // nothing to recover.
        unsafe {
            vcall!(self.0, Terminate());
            vcall!(self.0, Release() or 0);
        }
    }
}

/// How the encoder pads frames: newer GPUs encode any size, older RDNA 3 firmware only sizes
/// aligned to 64x16 (with an exception for 1080 lines).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Alignment {
    Any,
    Align64x16Or1080p,
    Align64x16,
}

impl Alignment {
    fn fits(self, width: u32, height: u32) -> bool {
        let aligned = width.is_multiple_of(64) && height.is_multiple_of(16);
        match self {
            Self::Any => true,
            Self::Align64x16Or1080p => aligned || (width.is_multiple_of(64) && height == 1080),
            Self::Align64x16 => aligned,
        }
    }
}

/// One initialised AV1 encoder component.
struct Component {
    ptr: *mut amf::AMFComponent,
    /// Temporal layers the GPU allows for this stream (1 without temporal SVC).
    max_layers: u8,
}

impl Component {
    fn open(api: &Api, context: &Context, config: &EncoderConfig, layers: u8) -> Result<Self> {
        let mut component = ptr::null_mut();
        // SAFETY: factory and context are live; the name is NUL-terminated.
        check_open(
            unsafe {
                vcall!(
                    api.factory,
                    CreateComponent(context.0, wide(AV1_COMPONENT).as_ptr(), &mut component)
                )
            },
            "creating the AV1 encoder (no AV1 encoding on this GPU?)",
        )?;
        if component.is_null() {
            return Err(ENCODER.unavailable("the runtime returned no AV1 encoder"));
        }
        let mut component = Self {
            ptr: component,
            max_layers: 1,
        };
        component.configure(config, layers)?;
        let width = i32::try_from(config.width).unwrap_or(i32::MAX);
        let height = i32::try_from(config.height).unwrap_or(i32::MAX);
        // SAFETY: the component is live.
        check_open(
            unsafe { vcall!(component.ptr, Init(amf::AMF_SURFACE_NV12, width, height)) },
            "initialising the AV1 encoder",
        )?;
        Ok(component)
    }

    /// Sets everything that must be in place before `Init`.
    fn configure(&mut self, config: &EncoderConfig, layers: u8) -> Result<()> {
        // The usage comes first: it resets every other property to its defaults.
        self.require(
            name::USAGE,
            int(amf::AMF_VIDEO_ENCODER_AV1_USAGE_LOW_LATENCY.into()),
        )?;
        let alignment = self.alignment();
        if !alignment.fits(config.width, config.height) {
            return Err(ENCODER.unavailable(format!(
                "this GPU only encodes AV1 sizes aligned to 64x16, not {}x{}",
                config.width, config.height
            )));
        }
        self.require(name::FRAME_SIZE, size(config.width, config.height)?)?;
        self.require(
            name::RATE_CONTROL,
            int(amf::AMF_VIDEO_ENCODER_AV1_RATE_CONTROL_METHOD_CBR.into()),
        )?;
        self.require(name::INITIAL_VBV_FULLNESS, int(64))?;
        // Endless GOP: keyframes only on the first frame and when requested.
        self.require(name::GOP_SIZE, int(0))?;
        self.require(
            name::HEADER_INSERTION,
            int(amf::AMF_VIDEO_ENCODER_AV1_HEADER_INSERTION_MODE_KEY_FRAME_ALIGNED.into()),
        )?;
        // Nice to have, missing in older runtimes: padding would waste upload, the latency
        // mode and AMF's own wait only make the encoder faster.
        self.set(name::FILLER_DATA, boolean(false));
        self.set(
            name::LATENCY_MODE,
            int(amf::AMF_VIDEO_ENCODER_AV1_ENCODING_LATENCY_MODE_LOWEST_LATENCY.into()),
        );
        self.set(name::QUERY_TIMEOUT, int(QUERY_TIMEOUT_MS));

        let cap = self.get_int(name::CAP_MAX_TEMPORAL_LAYERS).unwrap_or(1);
        self.max_layers = u8::try_from(cap.clamp(1, MAX_LAYERS.into())).unwrap_or(1);
        if self.max_layers > 1 {
            self.require(name::MAX_TEMPORAL_LAYERS, int(self.max_layers.into()))?;
        }
        self.apply_rate(config, layers.min(self.max_layers))
            .map_err(|err| ENCODER.unavailable(format!("rate settings refused: {err}")))
    }

    /// Picks the least restrictive alignment the GPU accepts.
    fn alignment(&self) -> Alignment {
        let modes = [
            (
                Alignment::Any,
                amf::AMF_VIDEO_ENCODER_AV1_ALIGNMENT_MODE_NO_RESTRICTIONS,
            ),
            (
                Alignment::Align64x16Or1080p,
                amf::AMF_VIDEO_ENCODER_AV1_ALIGNMENT_MODE_64X16_1080P_CODED_1082,
            ),
        ];
        modes
            .into_iter()
            .find(|(_, mode)| self.set(name::ALIGNMENT_MODE, int((*mode).into())) == amf::AMF_OK)
            .map_or(Alignment::Align64x16, |(alignment, _)| alignment)
    }

    /// Sets the frame rate, the CBR rate and the layer count; all of them may change while
    /// encoding. A new layer count takes effect at the next layer-0 frame.
    fn apply_rate(&self, config: &EncoderConfig, layers: u8) -> Result<()> {
        let bitrate = i64::from(config.bitrate_kbps) * 1000;
        let one_frame = bitrate / i64::from(config.fps);
        self.check_set(name::FRAME_RATE, rate(config.fps), "set frame rate")?;
        self.check_set(name::TARGET_BITRATE, int(bitrate), "set bitrate")?;
        self.check_set(name::PEAK_BITRATE, int(bitrate), "set peak bitrate")?;
        self.check_set(name::VBV_BUFFER_SIZE, int(one_frame), "set VBV buffer")?;
        if self.max_layers > 1 {
            self.check_set(
                name::TEMPORAL_LAYERS,
                int(layers.into()),
                "set temporal layers",
            )?;
        }
        Ok(())
    }

    fn set(&self, name: &str, value: amf::AMFVariantStruct) -> Status {
        // SAFETY: the component is live; the name is NUL-terminated and only read in the call.
        unsafe { vcall!(self.ptr, SetProperty(wide(name).as_ptr(), value)) }
    }

    fn check_set(
        &self,
        name: &str,
        value: amf::AMFVariantStruct,
        operation: &'static str,
    ) -> Result<()> {
        check(self.set(name, value), operation)
    }

    fn require(&self, name: &str, value: amf::AMFVariantStruct) -> Result<()> {
        check_open(self.set(name, value), &format!("setting {name}"))
    }

    fn get_int(&self, name: &str) -> Option<i64> {
        let mut value = variant(amf::AMF_VARIANT_EMPTY);
        // SAFETY: the component is live; it writes the value into `value`.
        let status = unsafe { vcall!(self.ptr, GetProperty(wide(name).as_ptr(), &mut value)) };
        if status == amf::AMF_OK && value.type_ == amf::AMF_VARIANT_INT64 {
            // SAFETY: the type says which union member is set.
            Some(unsafe { value.__bindgen_anon_1.int64Value })
        } else {
            None
        }
    }
}

impl Drop for Component {
    fn drop(&mut self) {
        // SAFETY: the component is live and released exactly once here. Teardown failures leave
        // nothing to recover.
        unsafe {
            vcall!(self.ptr, Terminate());
            vcall!(self.ptr, Release() or 0);
        }
    }
}

/// An AMF object reference that is released when dropped.
struct Owned<T: Release>(*mut T);

trait Release {
    /// # Safety
    /// `this` must be a live object holding a reference this call gives up.
    unsafe fn release(this: *mut Self);
}

macro_rules! release {
    ($($interface:ty),*) => {$(
        impl Release for $interface {
            unsafe fn release(this: *mut Self) {
                // SAFETY: see the trait.
                unsafe { vcall!(this, Release() or 0) };
            }
        }
    )*};
}

release!(amf::AMFSurface, amf::AMFData, amf::AMFBuffer);

impl<T: Release> Drop for Owned<T> {
    fn drop(&mut self) {
        // SAFETY: the reference was handed to this owner, which gives it up once.
        unsafe { T::release(self.0) }
    }
}

/// The AMF AV1 encoder.
struct AmfEncoder {
    // Field order is drop order: the component before its context, both before the runtime.
    component: Component,
    context: Context,
    api: Api,
    config: EncoderConfig,
    /// Layers of the frames coming out now.
    layers: u8,
    /// Layers asked of AMF, which switches to them at the next layer-0 frame.
    target_layers: u8,
}

// SAFETY: AMF objects may be used from any thread, one call at a time, which `&mut self`
// guarantees.
unsafe impl Send for AmfEncoder {}

impl AmfEncoder {
    fn new(api: Api, config: EncoderConfig) -> Result<Self> {
        let context = Context::open(&api)?;
        let component = Component::open(&api, &context, &config, wanted_layers(config.fps))?;
        let layers = wanted_layers(config.fps).min(component.max_layers);
        Ok(Self {
            component,
            context,
            api,
            config,
            layers,
            target_layers: layers,
        })
    }

    /// Copies `frame` into a new host-memory NV12 surface.
    fn upload(&self, frame: &I420Frame, pts: i64) -> Result<Owned<amf::AMFSurface>> {
        let mut surface = ptr::null_mut();
        // SAFETY: the context is live; the size is the encoder's (validated to fit i32).
        check(
            unsafe {
                vcall!(
                    self.context.0,
                    AllocSurface(
                        amf::AMF_MEMORY_HOST,
                        amf::AMF_SURFACE_NV12,
                        self.config.width as i32,
                        self.config.height as i32,
                        &mut surface,
                    )
                )
            },
            "allocate a surface",
        )?;
        if surface.is_null() {
            return Err(codec_error("allocate a surface", amf::AMF_OUT_OF_MEMORY));
        }
        let surface = Owned(surface);
        // SAFETY: an NV12 surface has the luma plane at 0 and the chroma plane at 1; the planes
        // live as long as the surface.
        let (luma, chroma) = unsafe {
            (
                plane(vcall!(surface.0, GetPlaneAt(0) or ptr::null_mut())),
                plane(vcall!(surface.0, GetPlaneAt(1) or ptr::null_mut())),
            )
        };
        copy_nv12(frame, luma, chroma)?;
        // SAFETY: the surface is live. AMF counts time in 100 ns units.
        unsafe { vcall!(surface.0, SetPts(pts.saturating_mul(10_000)) or ()) };
        Ok(surface)
    }

    /// Waits for the encoded picture of the frame just submitted.
    fn query_output(&self) -> Result<Owned<amf::AMFData>> {
        let deadline = Instant::now() + OUTPUT_TIMEOUT;
        loop {
            let mut data = ptr::null_mut();
            // SAFETY: the component is live; a returned picture is ours to release.
            let status = unsafe { vcall!(self.component.ptr, QueryOutput(&mut data)) };
            if status == amf::AMF_OK && !data.is_null() {
                return Ok(Owned(data));
            }
            if !matches!(status, amf::AMF_OK | amf::AMF_REPEAT) {
                return Err(codec_error("query output", status));
            }
            if Instant::now() >= deadline {
                return Err(Error::Codec {
                    library: "AMF",
                    operation: "query output",
                    message: format!("no picture after {OUTPUT_TIMEOUT:?}"),
                });
            }
            thread::sleep(Duration::from_millis(1));
        }
    }

    /// Reads an encoded picture out of `data`.
    fn download(&mut self, data: &Owned<amf::AMFData>, pts: i64) -> Result<EncodedFrame> {
        let mut buffer = ptr::null_mut::<c_void>();
        // SAFETY: the picture is live; a returned buffer is ours to release.
        check(
            unsafe { vcall!(data.0, QueryInterface(&BUFFER_IID, &mut buffer)) },
            "read output buffer",
        )?;
        if buffer.is_null() {
            return Err(codec_error("read output buffer", amf::AMF_NO_INTERFACE));
        }
        let buffer = Owned(buffer.cast::<amf::AMFBuffer>());
        let mut frame_type = variant(amf::AMF_VARIANT_EMPTY);
        // SAFETY: the buffer is live; the property is an int64 enum.
        let status = unsafe {
            vcall!(
                buffer.0,
                GetProperty(wide(name::OUTPUT_FRAME_TYPE).as_ptr(), &mut frame_type)
            )
        };
        check(status, "read frame type")?;
        // SAFETY: an int64 property, checked by its type tag.
        let keyframe = frame_type.type_ == amf::AMF_VARIANT_INT64
            && unsafe { frame_type.__bindgen_anon_1.int64Value }
                == i64::from(amf::AMF_VIDEO_ENCODER_AV1_OUTPUT_FRAME_TYPE_KEY);
        // SAFETY: the buffer holds `GetSize` bytes at `GetNative` while it is live.
        let bytes = unsafe {
            let native = vcall!(buffer.0, GetNative() or ptr::null_mut()).cast::<u8>();
            let len = vcall!(buffer.0, GetSize() or 0);
            if native.is_null() || len == 0 {
                return Err(codec_error("read output buffer", amf::AMF_INVALID_POINTER));
            }
            std::slice::from_raw_parts(native, len).to_vec()
        };
        let temporal_layer = self.temporal_layer(&bytes, keyframe)?;
        Ok(EncodedFrame {
            data: bytes,
            keyframe,
            temporal_layer,
            pts,
        })
    }

    /// The layer of the picture `bytes`. A layer-0 picture is where AMF switches to a new layer
    /// count, so the reported count follows there.
    fn temporal_layer(&mut self, bytes: &[u8], keyframe: bool) -> Result<u8> {
        let id = obu::temporal_id(bytes).map_err(|message| Error::Codec {
            library: "AMF",
            operation: "read temporal layer id",
            message: message.into(),
        })?;
        // Without extension headers every picture is a base picture, which is always safe.
        let layer = if keyframe { 0 } else { id.unwrap_or(0) };
        if layer == 0 {
            self.layers = self.target_layers;
        }
        if layer >= self.layers {
            return Err(Error::Codec {
                library: "AMF",
                operation: "read temporal layer id",
                message: format!("layer id {layer} in a {}-layer stream", self.layers),
            });
        }
        Ok(layer)
    }

    /// Changes the stored configuration with `change` and applies the rate settings to the
    /// running encoder, or restores the previous settings if the driver refuses.
    fn change(&mut self, change: impl FnOnce(&mut EncoderConfig), layers: u8) -> Result<()> {
        let mut config = self.config;
        change(&mut config);
        if let Err(err) = self.component.apply_rate(&config, layers) {
            if let Err(restore) = self.component.apply_rate(&self.config, self.target_layers) {
                return Err(Error::Codec {
                    library: "AMF",
                    operation: "restore rate settings",
                    message: format!("{err}; then {restore}"),
                });
            }
            return Err(err);
        }
        self.config = config;
        self.target_layers = layers;
        Ok(())
    }
}

/// The base address and row pitch of an NV12 plane.
///
/// # Safety
/// `plane` must be null or a live plane.
unsafe fn plane(plane: *mut amf::AMFPlane) -> (*mut u8, usize) {
    if plane.is_null() {
        return (ptr::null_mut(), 0);
    }
    // SAFETY: see the function's contract.
    unsafe {
        (
            vcall!(plane, GetNative() or ptr::null_mut()).cast(),
            usize::try_from(vcall!(plane, GetHPitch() or 0)).unwrap_or(0),
        )
    }
}

/// Copies `frame` into an NV12 surface: the Y plane as is, U and V interleaved in the chroma
/// plane.
fn copy_nv12(frame: &I420Frame, luma: (*mut u8, usize), chroma: (*mut u8, usize)) -> Result<()> {
    let (width, height) = (frame.width() as usize, frame.height() as usize);
    let (chroma_width, chroma_height) = (width.div_ceil(2), height.div_ceil(2));
    let ((luma, luma_pitch), (chroma, chroma_pitch)) = (luma, chroma);
    if luma.is_null() || chroma.is_null() || luma_pitch < width || chroma_pitch < chroma_width * 2 {
        return Err(Error::Codec {
            library: "AMF",
            operation: "map the surface",
            message: format!(
                "unusable planes (pitches {luma_pitch}/{chroma_pitch} for width {width})"
            ),
        });
    }
    for (row, source) in frame.y().chunks_exact(width).take(height).enumerate() {
        // SAFETY: a surface of this size holds `height` luma rows of `luma_pitch` bytes, and
        // `width` fits in the pitch.
        unsafe {
            ptr::copy_nonoverlapping(source.as_ptr(), luma.add(row * luma_pitch), width);
        }
    }
    let rows = frame
        .u()
        .chunks_exact(chroma_width)
        .zip(frame.v().chunks_exact(chroma_width))
        .take(chroma_height);
    for (row, (u, v)) in rows.enumerate() {
        // SAFETY: the chroma plane holds `chroma_height` rows of `chroma_pitch` bytes, and the
        // interleaved row of `2 * chroma_width` bytes fits in the pitch.
        let target = unsafe {
            std::slice::from_raw_parts_mut(chroma.add(row * chroma_pitch), chroma_width * 2)
        };
        for ((pair, u), v) in target.as_chunks_mut::<2>().0.iter_mut().zip(u).zip(v) {
            *pair = [*u, *v];
        }
    }
    Ok(())
}

impl VideoEncoder for AmfEncoder {
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
        let surface = self.upload(frame, pts)?;
        if force_keyframe {
            let key = int(amf::AMF_VIDEO_ENCODER_AV1_FORCE_FRAME_TYPE_KEY.into());
            // SAFETY: the surface is live; the names are only read during the calls.
            unsafe {
                check(
                    vcall!(
                        surface.0,
                        SetProperty(wide(name::FORCE_FRAME_TYPE).as_ptr(), key)
                    ),
                    "force a keyframe",
                )?;
                check(
                    vcall!(
                        surface.0,
                        SetProperty(wide(name::FORCE_SEQUENCE_HEADER).as_ptr(), boolean(true))
                    ),
                    "force a sequence header",
                )?;
            }
        }
        // SAFETY: a surface is an AMFData; the encoder takes its own reference.
        let status = unsafe {
            vcall!(
                self.component.ptr,
                SubmitInput(surface.0.cast::<amf::AMFData>())
            )
        };
        check(status, "submit a frame")?;
        drop(surface);
        let data = self.query_output()?;
        Ok(vec![self.download(&data, pts)?])
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
        // A new encoder (its first picture is a keyframe); the old one stays until it exists.
        let component = Component::open(&self.api, &self.context, &config, self.target_layers)?;
        self.component = component;
        self.config = config;
        self.layers = self.target_layers.min(self.component.max_layers);
        self.target_layers = self.layers;
        Ok(())
    }

    fn set_bitrate(&mut self, bitrate_kbps: u32) -> Result<()> {
        validate_bitrate(bitrate_kbps)?;
        self.change(
            |config| config.bitrate_kbps = bitrate_kbps,
            self.target_layers,
        )
    }

    fn set_fps(&mut self, fps: u32) -> Result<()> {
        validate_fps(fps)?;
        let layers = wanted_layers(fps).min(self.component.max_layers);
        self.change(|config| config.fps = fps, layers)
    }
}

/// No AMD GPU runs in CI, so these tests drive the encoder through mocked AMF objects that
/// behave like the runtime: they record what the encoder asks for and hand back pictures with
/// AMF's keyframe and temporal-layer pattern, layer ids in the OBU extension headers.
#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    #[derive(Clone, Copy, Debug, PartialEq)]
    enum Value {
        Int(i64),
        Bool(bool),
        Size(i32, i32),
        Rate(u32, u32),
    }

    type Props = Vec<(String, Value)>;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Call {
        CreateContext,
        InitDx11,
        CreateComponent,
        Init(i32, i32),
        Submit,
        TerminateComponent,
        TerminateContext,
    }

    #[derive(Default)]
    struct Mock {
        calls: Vec<Call>,
        /// Every property set on an encoder component, in order.
        props: Vec<(String, Value)>,
        /// Property names the encoder refuses.
        refused: Vec<&'static str>,
        cap_layers: Option<i64>,
        fail_init: bool,
        /// `QueryOutput` answers "repeat" this often before each picture.
        repeats: u32,
        /// Objects handed out and not yet released.
        live: i32,
        /// The NV12 bytes of the surface being filled, and its pitch.
        surface: Vec<u8>,
        pitch: usize,
        height: usize,
        /// Per submitted frame: its NV12 bytes, pts and the properties set on it.
        submitted: Vec<(Vec<u8>, i64, Props)>,
        /// Current surface's properties.
        surface_props: Vec<(String, Value)>,
        /// Layers asked for and in use (switched at a layer-0 picture).
        target_layers: i64,
        layers: i64,
        position: u32,
        pending: Option<(Vec<u8>, bool)>,
        output: Vec<u8>,
        output_key: bool,
    }

    thread_local! {
        static MOCK: RefCell<Mock> = RefCell::new(Mock { layers: 1, target_layers: 1, ..Mock::default() });
    }

    fn mock<R>(f: impl FnOnce(&mut Mock) -> R) -> R {
        MOCK.with(|mock| f(&mut mock.borrow_mut()))
    }

    fn calls() -> Vec<Call> {
        mock(|m| m.calls.clone())
    }

    fn props() -> Vec<(String, Value)> {
        mock(|m| m.props.clone())
    }

    fn prop(name: &str) -> Option<Value> {
        props()
            .into_iter()
            .rev()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v)
    }

    /// # Safety
    /// `name` must be a NUL-terminated wide string.
    unsafe fn read_name(name: *const amf::wchar_t) -> String {
        let mut out = String::new();
        for i in 0.. {
            // SAFETY: NUL-terminated.
            let c = i64::from(unsafe { *name.add(i) });
            if c == 0 {
                break;
            }
            out.push(char::from_u32(u32::try_from(c).expect("char")).expect("char"));
        }
        out
    }

    fn value(v: &amf::AMFVariantStruct) -> Value {
        // SAFETY: the tag says which member is set.
        unsafe {
            match v.type_ {
                amf::AMF_VARIANT_INT64 => Value::Int(v.__bindgen_anon_1.int64Value),
                amf::AMF_VARIANT_BOOL => Value::Bool(v.__bindgen_anon_1.boolValue != 0),
                amf::AMF_VARIANT_SIZE => {
                    let s = v.__bindgen_anon_1.sizeValue;
                    Value::Size(s.width, s.height)
                }
                amf::AMF_VARIANT_RATE => {
                    let r = v.__bindgen_anon_1.rateValue;
                    Value::Rate(r.num, r.den)
                }
                other => panic!("unexpected variant type {other}"),
            }
        }
    }

    fn new_object<T>(object: T) -> *mut T {
        mock(|m| m.live += 1);
        Box::into_raw(Box::new(object))
    }

    /// # Safety
    /// `this` must come from [`new_object`] and not be released yet.
    unsafe fn free<T>(this: *mut T) -> amf::amf_long {
        mock(|m| m.live -= 1);
        // SAFETY: see above.
        drop(unsafe { Box::from_raw(this) });
        0
    }

    // ---- factory ----

    unsafe extern "C" fn create_context(
        _: *mut amf::AMFFactory,
        context: *mut *mut amf::AMFContext,
    ) -> Status {
        mock(|m| m.calls.push(Call::CreateContext));
        // SAFETY: valid out pointer.
        unsafe {
            *context = new_object(amf::AMFContext {
                pVtbl: &CONTEXT_VTBL,
            })
        };
        amf::AMF_OK
    }

    unsafe extern "C" fn create_component(
        _: *mut amf::AMFFactory,
        context: *mut amf::AMFContext,
        id: *const amf::wchar_t,
        component: *mut *mut amf::AMFComponent,
    ) -> Status {
        assert!(!context.is_null());
        // SAFETY: NUL-terminated.
        assert_eq!(unsafe { read_name(id) }, AV1_COMPONENT);
        mock(|m| m.calls.push(Call::CreateComponent));
        // SAFETY: valid out pointer.
        unsafe {
            *component = new_object(amf::AMFComponent {
                pVtbl: &COMPONENT_VTBL,
            })
        };
        amf::AMF_OK
    }

    static FACTORY_VTBL: amf::AMFFactoryVtbl = amf::AMFFactoryVtbl {
        CreateContext: Some(create_context),
        CreateComponent: Some(create_component),
        // SAFETY: empty slots.
        ..unsafe { std::mem::zeroed() }
    };

    // ---- context ----

    unsafe extern "C" fn context_release(this: *mut amf::AMFContext) -> amf::amf_long {
        // SAFETY: from create_context.
        unsafe { free(this) }
    }

    unsafe extern "C" fn context_terminate(_: *mut amf::AMFContext) -> Status {
        mock(|m| m.calls.push(Call::TerminateContext));
        amf::AMF_OK
    }

    unsafe extern "C" fn init_dx11(
        _: *mut amf::AMFContext,
        device: *mut c_void,
        version: amf::AMF_DX_VERSION,
    ) -> Status {
        assert!(device.is_null());
        assert_eq!(version, amf::AMF_DX11_1);
        mock(|m| m.calls.push(Call::InitDx11));
        amf::AMF_OK
    }

    #[repr(C)]
    struct MockSurface {
        surface: amf::AMFSurface,
        planes: [*mut MockPlane; 2],
    }

    #[repr(C)]
    struct MockPlane {
        plane: amf::AMFPlane,
        offset: usize,
    }

    unsafe extern "C" fn alloc_surface(
        _: *mut amf::AMFContext,
        memory: amf::AMF_MEMORY_TYPE,
        format: amf::AMF_SURFACE_FORMAT,
        width: i32,
        height: i32,
        surface: *mut *mut amf::AMFSurface,
    ) -> Status {
        assert_eq!(memory, amf::AMF_MEMORY_HOST);
        assert_eq!(format, amf::AMF_SURFACE_NV12);
        // A padded pitch, as GPUs use.
        let pitch = (width as usize).next_multiple_of(256);
        let height = height as usize;
        mock(|m| {
            m.pitch = pitch;
            m.height = height;
            m.surface = vec![0; pitch * (height + height.div_ceil(2))];
            m.surface_props.clear();
        });
        let planes = [0, pitch * height].map(|offset| {
            new_object(MockPlane {
                plane: amf::AMFPlane { pVtbl: &PLANE_VTBL },
                offset,
            })
        });
        let object = new_object(MockSurface {
            surface: amf::AMFSurface {
                pVtbl: &SURFACE_VTBL,
            },
            planes,
        });
        // SAFETY: valid out pointer; MockSurface starts with the AMFSurface.
        unsafe { *surface = object.cast() };
        amf::AMF_OK
    }

    static CONTEXT_VTBL: amf::AMFContextVtbl = amf::AMFContextVtbl {
        Release: Some(context_release),
        Terminate: Some(context_terminate),
        InitDX11: Some(init_dx11),
        AllocSurface: Some(alloc_surface),
        // SAFETY: empty slots.
        ..unsafe { std::mem::zeroed() }
    };

    // ---- surface and planes ----

    unsafe extern "C" fn surface_release(this: *mut amf::AMFSurface) -> amf::amf_long {
        let this = this.cast::<MockSurface>();
        // SAFETY: from alloc_surface.
        unsafe {
            for plane in (*this).planes {
                free(plane);
            }
            free(this)
        }
    }

    unsafe extern "C" fn surface_set_property(
        _: *mut amf::AMFSurface,
        name: *const amf::wchar_t,
        v: amf::AMFVariantStruct,
    ) -> Status {
        // SAFETY: NUL-terminated.
        let name = unsafe { read_name(name) };
        mock(|m| m.surface_props.push((name, value(&v))));
        amf::AMF_OK
    }

    unsafe extern "C" fn surface_set_pts(_: *mut amf::AMFSurface, pts: amf::amf_pts) {
        mock(|m| m.surface_props.push(("pts".into(), Value::Int(pts))));
    }

    unsafe extern "C" fn get_plane_at(
        this: *mut amf::AMFSurface,
        index: usize,
    ) -> *mut amf::AMFPlane {
        // SAFETY: from alloc_surface.
        unsafe { (*this.cast::<MockSurface>()).planes[index].cast() }
    }

    static SURFACE_VTBL: amf::AMFSurfaceVtbl = amf::AMFSurfaceVtbl {
        Release: Some(surface_release),
        SetProperty: Some(surface_set_property),
        SetPts: Some(surface_set_pts),
        GetPlaneAt: Some(get_plane_at),
        // SAFETY: empty slots.
        ..unsafe { std::mem::zeroed() }
    };

    unsafe extern "C" fn plane_native(this: *mut amf::AMFPlane) -> *mut c_void {
        // SAFETY: from alloc_surface.
        let offset = unsafe { (*this.cast::<MockPlane>()).offset };
        // The vector stays put until the next surface is allocated.
        mock(|m| m.surface[offset..].as_mut_ptr().cast())
    }

    unsafe extern "C" fn plane_pitch(_: *mut amf::AMFPlane) -> amf::amf_int {
        mock(|m| m.pitch as amf::amf_int)
    }

    static PLANE_VTBL: amf::AMFPlaneVtbl = amf::AMFPlaneVtbl {
        GetNative: Some(plane_native),
        GetHPitch: Some(plane_pitch),
        // SAFETY: empty slots.
        ..unsafe { std::mem::zeroed() }
    };

    // ---- encoder component ----

    unsafe extern "C" fn component_release(this: *mut amf::AMFComponent) -> amf::amf_long {
        // SAFETY: from create_component.
        unsafe { free(this) }
    }

    unsafe extern "C" fn component_terminate(_: *mut amf::AMFComponent) -> Status {
        mock(|m| m.calls.push(Call::TerminateComponent));
        amf::AMF_OK
    }

    unsafe extern "C" fn set_property(
        _: *mut amf::AMFComponent,
        name: *const amf::wchar_t,
        v: amf::AMFVariantStruct,
    ) -> Status {
        // SAFETY: NUL-terminated.
        let name = unsafe { read_name(name) };
        let v = value(&v);
        mock(|m| {
            if m.refused.contains(&name.as_str()) {
                return amf::AMF_NOT_SUPPORTED;
            }
            if name == name::TEMPORAL_LAYERS
                && let Value::Int(layers) = v
            {
                m.target_layers = layers;
            }
            m.props.push((name, v));
            amf::AMF_OK
        })
    }

    unsafe extern "C" fn get_property(
        _: *mut amf::AMFComponent,
        name: *const amf::wchar_t,
        out: *mut amf::AMFVariantStruct,
    ) -> Status {
        // SAFETY: NUL-terminated.
        let name = unsafe { read_name(name) };
        assert_eq!(name, name::CAP_MAX_TEMPORAL_LAYERS);
        match mock(|m| m.cap_layers) {
            // SAFETY: valid out pointer.
            Some(cap) => unsafe { *out = int(cap) },
            None => return amf::AMF_NOT_FOUND,
        }
        amf::AMF_OK
    }

    unsafe extern "C" fn init(
        _: *mut amf::AMFComponent,
        format: amf::AMF_SURFACE_FORMAT,
        width: i32,
        height: i32,
    ) -> Status {
        assert_eq!(format, amf::AMF_SURFACE_NV12);
        mock(|m| {
            m.calls.push(Call::Init(width, height));
            m.layers = m.target_layers;
            m.position = 0;
            if m.fail_init {
                amf::AMF_NOT_SUPPORTED
            } else {
                amf::AMF_OK
            }
        })
    }

    const TEMPORAL_DELIMITER: u8 = 2 << 3;
    const SEQUENCE_HEADER: u8 = 1 << 3;
    const FRAME: u8 = 6 << 3;

    /// The AV1 temporal-layer pattern: with 3 layers 0,2,1,2, with 4 layers 0,3,2,3,1,3,2,3.
    fn layer_at(position: u32, layers: i64) -> i64 {
        let period = 1u32 << (layers - 1);
        let step = position % period;
        if step == 0 {
            0
        } else {
            layers - 1 - i64::from(step.trailing_zeros())
        }
    }

    unsafe extern "C" fn submit(_: *mut amf::AMFComponent, data: *mut amf::AMFData) -> Status {
        assert!(!data.is_null());
        mock(|m| {
            m.calls.push(Call::Submit);
            let props = std::mem::take(&mut m.surface_props);
            let pts = props.iter().find_map(|(n, v)| (n == "pts").then_some(*v));
            let Some(Value::Int(pts)) = pts else {
                panic!("no pts")
            };
            let forced = props.iter().any(|(n, v)| {
                n == name::FORCE_FRAME_TYPE
                    && *v == Value::Int(amf::AMF_VIDEO_ENCODER_AV1_FORCE_FRAME_TYPE_KEY.into())
            });
            m.submitted.push((m.surface.clone(), pts, props));
            let key = forced || m.position == 0;
            if key {
                m.position = 0;
            }
            let layer = layer_at(m.position, m.layers);
            if layer == 0 && m.layers != m.target_layers {
                // A new layer count starts its pattern at this base picture.
                m.layers = m.target_layers;
                m.position = 0;
            }
            m.position += 1;
            let mut bytes = vec![TEMPORAL_DELIMITER | 0x02, 0];
            if key {
                bytes.extend([SEQUENCE_HEADER | 0x02, 2, 0xaa, 0xbb]);
            }
            if m.layers > 1 {
                bytes.extend([FRAME | 0x06, (layer as u8) << 5, 3, 1, 2, 3]);
            } else {
                bytes.extend([FRAME | 0x02, 3, 1, 2, 3]);
            }
            m.pending = Some((bytes, key));
            amf::AMF_OK
        })
    }

    #[repr(C)]
    struct MockData {
        data: amf::AMFData,
    }

    unsafe extern "C" fn query_output(
        _: *mut amf::AMFComponent,
        out: *mut *mut amf::AMFData,
    ) -> Status {
        let ready = mock(|m| {
            if m.repeats > 0 {
                m.repeats -= 1;
                return None;
            }
            m.pending.take()
        });
        let Some((bytes, key)) = ready else {
            return amf::AMF_REPEAT;
        };
        mock(|m| {
            m.output = bytes;
            m.output_key = key;
        });
        // SAFETY: valid out pointer.
        unsafe {
            *out = new_object(MockData {
                data: amf::AMFData { pVtbl: &DATA_VTBL },
            })
            .cast()
        };
        amf::AMF_OK
    }

    static COMPONENT_VTBL: amf::AMFComponentVtbl = amf::AMFComponentVtbl {
        Release: Some(component_release),
        Terminate: Some(component_terminate),
        SetProperty: Some(set_property),
        GetProperty: Some(get_property),
        Init: Some(init),
        SubmitInput: Some(submit),
        QueryOutput: Some(query_output),
        // SAFETY: empty slots.
        ..unsafe { std::mem::zeroed() }
    };

    // ---- output picture ----

    unsafe extern "C" fn data_release(this: *mut amf::AMFData) -> amf::amf_long {
        // SAFETY: from query_output.
        unsafe { free(this.cast::<MockData>()) }
    }

    unsafe extern "C" fn data_query_interface(
        _: *mut amf::AMFData,
        iid: *const amf::AMFGuid,
        out: *mut *mut c_void,
    ) -> Status {
        // SAFETY: valid pointers.
        unsafe {
            assert_eq!((*iid).data1, BUFFER_IID.data1);
            *out = new_object(amf::AMFBuffer {
                pVtbl: &BUFFER_VTBL,
            })
            .cast();
        }
        amf::AMF_OK
    }

    static DATA_VTBL: amf::AMFDataVtbl = amf::AMFDataVtbl {
        Release: Some(data_release),
        QueryInterface: Some(data_query_interface),
        // SAFETY: empty slots.
        ..unsafe { std::mem::zeroed() }
    };

    unsafe extern "C" fn buffer_release(this: *mut amf::AMFBuffer) -> amf::amf_long {
        // SAFETY: from data_query_interface.
        unsafe { free(this) }
    }

    unsafe extern "C" fn buffer_get_property(
        _: *mut amf::AMFBuffer,
        name: *const amf::wchar_t,
        out: *mut amf::AMFVariantStruct,
    ) -> Status {
        // SAFETY: NUL-terminated.
        assert_eq!(unsafe { read_name(name) }, name::OUTPUT_FRAME_TYPE);
        let kind = if mock(|m| m.output_key) {
            amf::AMF_VIDEO_ENCODER_AV1_OUTPUT_FRAME_TYPE_KEY
        } else {
            amf::AMF_VIDEO_ENCODER_AV1_OUTPUT_FRAME_TYPE_INTER
        };
        // SAFETY: valid out pointer.
        unsafe { *out = int(kind.into()) };
        amf::AMF_OK
    }

    unsafe extern "C" fn buffer_native(_: *mut amf::AMFBuffer) -> *mut c_void {
        mock(|m| m.output.as_mut_ptr().cast())
    }

    unsafe extern "C" fn buffer_size(_: *mut amf::AMFBuffer) -> amf::amf_size {
        mock(|m| m.output.len())
    }

    static BUFFER_VTBL: amf::AMFBufferVtbl = amf::AMFBufferVtbl {
        Release: Some(buffer_release),
        GetProperty: Some(buffer_get_property),
        GetNative: Some(buffer_native),
        GetSize: Some(buffer_size),
        // SAFETY: empty slots.
        ..unsafe { std::mem::zeroed() }
    };

    // ---- helpers ----

    fn config(width: u32, height: u32, fps: u32) -> EncoderConfig {
        EncoderConfig {
            width,
            height,
            fps,
            bitrate_kbps: 3000,
        }
    }

    fn open_mock(config: EncoderConfig) -> Result<AmfEncoder> {
        let factory = Box::leak(Box::new(amf::AMFFactory {
            pVtbl: &FACTORY_VTBL,
        }));
        AmfEncoder::new(Api::new(factory, None)?, config)
    }

    /// A frame whose every byte tells where it came from: Y counts up, U is 200, V is 250.
    fn frame(width: u32, height: u32) -> I420Frame {
        let luma = (width * height) as usize;
        let chroma = (I420Frame::buffer_len(width, height) - luma) / 2;
        let mut data: Vec<u8> = (0..luma).map(|i| (i % 199) as u8).collect();
        data.extend(std::iter::repeat_n(200, chroma));
        data.extend(std::iter::repeat_n(250, chroma));
        I420Frame::new(width, height, data).expect("frame")
    }

    fn encode_layers(encoder: &mut AmfEncoder, frames: usize) -> Vec<(bool, u8)> {
        let input = frame(encoder.config.width, encoder.config.height);
        (0..frames)
            .map(|i| {
                let out = encoder
                    .encode(&input, i as i64 * 33, false)
                    .expect("encode");
                assert_eq!(out.len(), 1);
                (out[0].keyframe, out[0].temporal_layer)
            })
            .collect()
    }

    #[test]
    fn opens_a_low_latency_cbr_encoder_with_three_layers_at_30_fps() {
        mock(|m| m.cap_layers = Some(4));
        let encoder = open_mock(config(1920, 1080, 30)).expect("encoder");
        assert_eq!(encoder.codec(), Codec::Av1);
        assert_eq!(encoder.temporal_layers(), 3);
        let int_enum = |v: i64| Some(Value::Int(v));
        assert_eq!(
            props().first().map(|(n, _)| n.as_str()),
            Some(name::USAGE),
            "usage must come first"
        );
        assert_eq!(
            prop(name::USAGE),
            int_enum(amf::AMF_VIDEO_ENCODER_AV1_USAGE_LOW_LATENCY.into())
        );
        assert_eq!(
            prop(name::ALIGNMENT_MODE),
            int_enum(amf::AMF_VIDEO_ENCODER_AV1_ALIGNMENT_MODE_NO_RESTRICTIONS.into())
        );
        assert_eq!(prop(name::FRAME_SIZE), Some(Value::Size(1920, 1080)));
        assert_eq!(prop(name::FRAME_RATE), Some(Value::Rate(30, 1)));
        assert_eq!(
            prop(name::RATE_CONTROL),
            int_enum(amf::AMF_VIDEO_ENCODER_AV1_RATE_CONTROL_METHOD_CBR.into())
        );
        assert_eq!(prop(name::TARGET_BITRATE), int_enum(3_000_000));
        assert_eq!(prop(name::PEAK_BITRATE), int_enum(3_000_000));
        assert_eq!(prop(name::VBV_BUFFER_SIZE), int_enum(100_000));
        assert_eq!(prop(name::GOP_SIZE), int_enum(0));
        assert_eq!(
            prop(name::HEADER_INSERTION),
            int_enum(amf::AMF_VIDEO_ENCODER_AV1_HEADER_INSERTION_MODE_KEY_FRAME_ALIGNED.into())
        );
        assert_eq!(prop(name::FILLER_DATA), Some(Value::Bool(false)));
        assert_eq!(prop(name::MAX_TEMPORAL_LAYERS), int_enum(4));
        assert_eq!(prop(name::TEMPORAL_LAYERS), int_enum(3));
        assert_eq!(
            calls(),
            [
                Call::CreateContext,
                Call::InitDx11,
                Call::CreateComponent,
                Call::Init(1920, 1080)
            ]
        );
    }

    #[test]
    fn four_layers_only_from_60_fps() {
        assert_eq!(
            [15, 30, 45, 59, 60].map(wanted_layers),
            [3, 3, 3, 3, MAX_LAYERS]
        );
    }

    #[test]
    fn uses_four_layers_at_60_fps_and_tags_every_picture() {
        mock(|m| m.cap_layers = Some(4));
        let mut encoder = open_mock(config(1280, 720, 60)).expect("encoder");
        assert_eq!(encoder.temporal_layers(), 4);
        let layers: Vec<_> = encode_layers(&mut encoder, 9)
            .into_iter()
            .map(|(_, layer)| layer)
            .collect();
        assert_eq!(layers, [0, 3, 2, 3, 1, 3, 2, 3, 0]);
    }

    #[test]
    fn a_gpu_with_fewer_layers_reports_its_count() {
        mock(|m| m.cap_layers = Some(2));
        let mut encoder = open_mock(config(1280, 720, 60)).expect("encoder");
        assert_eq!(encoder.temporal_layers(), 2);
        assert_eq!(prop(name::MAX_TEMPORAL_LAYERS), Some(Value::Int(2)));
        assert_eq!(prop(name::TEMPORAL_LAYERS), Some(Value::Int(2)));
        let layers: Vec<_> = encode_layers(&mut encoder, 4).iter().map(|f| f.1).collect();
        assert_eq!(layers, [0, 1, 0, 1]);
    }

    #[test]
    fn a_gpu_without_temporal_svc_streams_a_single_layer() {
        let mut encoder = open_mock(config(1280, 720, 30)).expect("encoder");
        assert_eq!(encoder.temporal_layers(), 1);
        assert_eq!(prop(name::MAX_TEMPORAL_LAYERS), None);
        assert_eq!(prop(name::TEMPORAL_LAYERS), None);
        let frames = encode_layers(&mut encoder, 3);
        assert_eq!(frames, [(true, 0), (false, 0), (false, 0)]);
    }

    #[test]
    fn keyframes_only_on_the_first_frame_and_on_request() {
        mock(|m| m.cap_layers = Some(4));
        let mut encoder = open_mock(config(640, 360, 30)).expect("encoder");
        let input = frame(640, 360);
        let mut keys = Vec::new();
        for i in 0..6 {
            let out = encoder.encode(&input, i * 33, i == 3).expect("encode");
            keys.push((out[0].keyframe, out[0].temporal_layer, out[0].pts));
        }
        assert_eq!(
            keys,
            [
                (true, 0, 0),
                (false, 2, 33),
                (false, 1, 66),
                (true, 0, 99),
                (false, 2, 132),
                (false, 1, 165)
            ]
        );
        let submitted = mock(|m| m.submitted.clone());
        let forced = &submitted[3].2;
        assert!(forced.contains(&(
            name::FORCE_FRAME_TYPE.into(),
            Value::Int(amf::AMF_VIDEO_ENCODER_AV1_FORCE_FRAME_TYPE_KEY.into())
        )));
        assert!(forced.contains(&(name::FORCE_SEQUENCE_HEADER.into(), Value::Bool(true))));
        assert!(
            submitted[2]
                .2
                .iter()
                .all(|(n, _)| n != name::FORCE_FRAME_TYPE)
        );
        assert_eq!(submitted[1].1, 33 * 10_000, "pts in 100 ns units");
    }

    #[test]
    fn copies_i420_into_nv12_with_the_surface_pitch() {
        let mut encoder = open_mock(config(6, 4, 30)).expect("encoder");
        let mut data: Vec<u8> = (0..24).collect();
        data.extend([100, 101, 102, 103, 104, 105]);
        data.extend([200, 201, 202, 203, 204, 205]);
        let input = I420Frame::new(6, 4, data).expect("frame");
        encoder.encode(&input, 0, false).expect("encode");
        let (pitch, height) = mock(|m| (m.pitch, m.height));
        let (nv12, _, _) = mock(|m| m.submitted[0].clone());
        for row in 0..4 {
            let start = row * pitch;
            let expected: Vec<u8> = (row as u8 * 6..row as u8 * 6 + 6).collect();
            assert_eq!(nv12[start..start + 6], expected);
        }
        let uv = pitch * height;
        assert_eq!(nv12[uv..uv + 6], [100, 200, 101, 201, 102, 202]);
        assert_eq!(
            nv12[uv + pitch..uv + pitch + 6],
            [103, 203, 104, 204, 105, 205]
        );
    }

    #[test]
    fn waits_for_the_picture_while_amf_repeats() {
        let mut encoder = open_mock(config(640, 360, 30)).expect("encoder");
        mock(|m| m.repeats = 3);
        let out = encoder.encode(&frame(640, 360), 0, false).expect("encode");
        assert!(out[0].keyframe);
    }

    #[test]
    fn a_stuck_encoder_times_out() {
        let mut encoder = open_mock(config(640, 360, 30)).expect("encoder");
        mock(|m| m.repeats = u32::MAX);
        let err = encoder
            .encode(&frame(640, 360), 0, false)
            .expect_err("stuck");
        assert!(matches!(err, Error::Codec { library: "AMF", .. }), "{err}");
    }

    #[test]
    fn changes_bitrate_live() {
        let mut encoder = open_mock(config(1280, 720, 30)).expect("encoder");
        encoder.set_bitrate(6000).expect("bitrate");
        assert_eq!(encoder.config().bitrate_kbps, 6000);
        assert_eq!(prop(name::TARGET_BITRATE), Some(Value::Int(6_000_000)));
        assert_eq!(prop(name::PEAK_BITRATE), Some(Value::Int(6_000_000)));
        assert_eq!(prop(name::VBV_BUFFER_SIZE), Some(Value::Int(200_000)));
        assert!(!calls().contains(&Call::TerminateComponent));
        assert!(matches!(
            encoder.set_bitrate(0),
            Err(Error::InvalidConfig(_))
        ));
    }

    #[test]
    fn a_refused_change_restores_the_previous_settings() {
        let mut encoder = open_mock(config(1280, 720, 30)).expect("encoder");
        mock(|m| m.refused.push(name::PEAK_BITRATE));
        assert!(encoder.set_bitrate(6000).is_err());
        assert_eq!(encoder.config().bitrate_kbps, 3000);
        assert_eq!(prop(name::TARGET_BITRATE), Some(Value::Int(3_000_000)));
    }

    #[test]
    fn a_frame_rate_change_switches_layers_at_the_next_base_picture() {
        mock(|m| m.cap_layers = Some(4));
        let mut encoder = open_mock(config(1280, 720, 30)).expect("encoder");
        assert_eq!(encode_layers(&mut encoder, 2), [(true, 0), (false, 2)]);
        encoder.set_fps(60).expect("fps");
        assert_eq!(prop(name::FRAME_RATE), Some(Value::Rate(60, 1)));
        assert_eq!(prop(name::TEMPORAL_LAYERS), Some(Value::Int(4)));
        // Still the 3-layer stream until its next layer-0 picture.
        assert_eq!(encoder.temporal_layers(), 3);
        let layers: Vec<_> = encode_layers(&mut encoder, 6).iter().map(|f| f.1).collect();
        assert_eq!(layers, [1, 2, 0, 3, 2, 3]);
        assert_eq!(encoder.temporal_layers(), 4);
        encoder.set_fps(30).expect("fps");
        assert_eq!(prop(name::TEMPORAL_LAYERS), Some(Value::Int(3)));
        assert_eq!(encoder.temporal_layers(), 4);
        let layers: Vec<_> = encode_layers(&mut encoder, 4).iter().map(|f| f.1).collect();
        assert_eq!(layers, [1, 3, 2, 3]);
        assert_eq!(encoder.temporal_layers(), 4);
        let layers: Vec<_> = encode_layers(&mut encoder, 2).iter().map(|f| f.1).collect();
        assert_eq!(layers, [0, 2]);
        assert_eq!(encoder.temporal_layers(), 3);
    }

    #[test]
    fn a_resize_opens_a_new_encoder_that_starts_with_a_keyframe() {
        mock(|m| m.cap_layers = Some(4));
        let mut encoder = open_mock(config(1280, 720, 30)).expect("encoder");
        encode_layers(&mut encoder, 2);
        encoder.set_resolution(1920, 1080).expect("resize");
        assert_eq!(
            calls()[calls().len() - 3..],
            [
                Call::CreateComponent,
                Call::Init(1920, 1080),
                Call::TerminateComponent
            ]
        );
        assert_eq!(encoder.temporal_layers(), 3);
        assert!(encoder.encode(&frame(1280, 720), 99, false).is_err());
        assert_eq!(encode_layers(&mut encoder, 1), [(true, 0)]);
    }

    #[test]
    fn falls_back_to_64x16_alignment_and_refuses_other_sizes() {
        mock(|m| {
            m.refused.push(name::ALIGNMENT_MODE);
        });
        assert!(open_mock(config(1280, 720, 30)).is_ok());
        let err = open_mock(config(1366, 768, 30)).err().expect("unaligned");
        assert!(
            matches!(
                err,
                Error::HardwareUnavailable {
                    encoder: HardwareEncoder::Amf,
                    ..
                }
            ),
            "{err}"
        );
    }

    #[test]
    fn the_1080p_alignment_mode_accepts_1080_lines() {
        assert!(Alignment::Align64x16Or1080p.fits(1920, 1080));
        assert!(!Alignment::Align64x16.fits(1920, 1080));
        assert!(!Alignment::Align64x16Or1080p.fits(1366, 768));
        assert!(Alignment::Any.fits(1366, 767));
    }

    #[test]
    fn an_encoder_the_gpu_refuses_is_unavailable() {
        mock(|m| m.fail_init = true);
        let err = open_mock(config(1280, 720, 30)).err().expect("refused");
        assert!(matches!(err, Error::HardwareUnavailable { .. }), "{err}");
        mock(|m| {
            m.fail_init = false;
            m.refused.push(name::USAGE);
        });
        let err = open_mock(config(1280, 720, 30)).err().expect("refused");
        assert!(matches!(err, Error::HardwareUnavailable { .. }), "{err}");
    }

    #[test]
    fn releases_every_object() {
        mock(|m| m.cap_layers = Some(4));
        {
            let mut encoder = open_mock(config(640, 360, 30)).expect("encoder");
            encode_layers(&mut encoder, 3);
            encoder.set_resolution(320, 180).expect("resize");
            encode_layers(&mut encoder, 2);
        }
        mock(|m| m.fail_init = true);
        assert!(open_mock(config(640, 360, 30)).is_err());
        assert_eq!(mock(|m| m.live), 0);
        assert_eq!(calls().last(), Some(&Call::TerminateContext));
    }

    #[test]
    fn rejects_invalid_settings() {
        assert!(matches!(
            validate(&config(0, 720, 30)),
            Err(Error::InvalidConfig(_))
        ));
        assert!(matches!(validate_fps(0), Err(Error::InvalidConfig(_))));
        assert!(matches!(validate_fps(241), Err(Error::InvalidConfig(_))));
        assert!(matches!(
            open(Codec::Vp9, config(1280, 720, 30)),
            Err(Error::HardwareUnavailable { .. })
        ));
    }

    #[test]
    fn wide_names_are_nul_terminated() {
        let name = wide("Av1Usage");
        assert_eq!(name.len(), 9);
        assert_eq!(name.last(), Some(&0));
        // SAFETY: NUL-terminated.
        assert_eq!(unsafe { read_name(name.as_ptr()) }, "Av1Usage");
    }
}
