//! VA-API AV1 and VP9 encoder for Intel and AMD GPUs on Linux (task N3-va).
//!
//! libva and libva-drm are loaded at runtime ([`dl`]) and talk to the first render node
//! (`/dev/dri/renderD*`) whose driver can encode the codec. Every frame is uploaded to an NV12
//! surface, encoded synchronously and read back from the coded buffer.
//!
//! The application decides every frame's references, which is how the temporal layers are made
//! (the drivers have no temporal SVC of their own): each frame predicts from one reference slot
//! that only ever holds a frame of a lower layer (layer 0 from the previous layer 0), and frames
//! of the top layer are never stored. Dropping all frames above some layer therefore leaves a
//! stream that still decodes. Three layers (0-2-1-2) up to 59 fps, four (0-3-2-3-1-3-2-3) from
//! 60 fps; one reference per frame is all any encoding driver offers, so four layers work
//! wherever the encoder does.
//!
//! AV1 drivers take the sequence and frame headers from the application ([`av1_obu`]) and patch
//! the rate-control fields in place; VP9 drivers write their own headers.

mod av1_obu;

use std::ffi::CStr;
use std::fs::{File, OpenOptions};
use std::mem::size_of;
use std::os::fd::AsRawFd;
use std::os::raw::{c_int, c_uint, c_void};
use std::ptr;

use self::av1_obu::FrameRefs;
use super::ffi::{va, va_drm};
use super::{HardwareEncoder, dl};
use crate::{Codec, EncodedFrame, EncoderConfig, Error, I420Frame, Result, VideoEncoder};

const ENCODER: HardwareEncoder = HardwareEncoder::Vaapi;

/// Render nodes probed in order; the kernel numbers them from 128.
const RENDER_NODES: std::ops::Range<u32> = 128..136;

/// Reconstructed-frame surfaces: up to three layer slots, the last key frame (which fills every
/// slot), the frame being encoded and one spare.
const POOL_SIZE: usize = 6;

/// Input surfaces are padded to whole 16×16 blocks (see [`i420_to_nv12`]).
const INPUT_ALIGN: u32 = 16;

/// AV1 reconstructed frames are padded to whole 64×64 superblocks.
const RECON_ALIGN: u32 = 64;

const THREE_LAYERS: [u8; 4] = [0, 2, 1, 2];
const FOUR_LAYERS: [u8; 8] = [0, 3, 2, 3, 1, 3, 2, 3];

/// The layer count for a frame rate: four layers from 60 fps, so the lowest layer still has
/// 15 fps, otherwise three.
fn layers_for(fps: u32) -> u8 {
    if fps >= 60 { 4 } else { 3 }
}

fn pattern(layers: u8) -> &'static [u8] {
    if layers == 4 {
        &FOUR_LAYERS
    } else {
        &THREE_LAYERS
    }
}

pub(super) fn open(codec: Codec, config: EncoderConfig) -> Result<Box<dyn VideoEncoder>> {
    validate(&config)?;
    Ok(Box::new(VaapiEncoder::new(codec, config)?))
}

fn validate(config: &EncoderConfig) -> Result<()> {
    if config.width < 16 || config.height < 16 {
        return Err(Error::InvalidConfig(format!(
            "size {}x{} (VA-API needs at least 16x16)",
            config.width, config.height
        )));
    }
    if config.width > av1_obu::MAX_WIDTH
        || config.width * config.height > av1_obu::MAX_AREA
        || config.height > av1_obu::MAX_WIDTH
    {
        return Err(Error::InvalidConfig(format!(
            "size {}x{} is larger than 4096x2304",
            config.width, config.height
        )));
    }
    if config.fps == 0 || config.fps > 240 {
        return Err(Error::InvalidConfig(format!("fps {}", config.fps)));
    }
    if config.bitrate_kbps == 0 {
        return Err(Error::InvalidConfig("bitrate 0".into()));
    }
    Ok(())
}

/// All-zero value of a libva parameter struct.
fn zeroed<T: Copy>() -> T {
    // SAFETY: only used for libva's C parameter structs (integers, arrays, unions and
    // bitfields of them), for which all-zero is a valid value.
    unsafe { std::mem::zeroed() }
}

/// An initialised VA display on one render node.
struct Display {
    api: va::LibVa,
    handle: va::VADisplay,
    // libva calls back into libva-drm until vaTerminate, and the display uses the device's file
    // descriptor; both are dropped after `Drop::drop` terminated the display.
    _drm: va_drm::LibVaDrm,
    _device: File,
}

impl Display {
    /// Opens the first render node whose driver has an encode entrypoint for `profile`.
    fn open(profile: va::VAProfile) -> Result<(Self, va::VAEntrypoint)> {
        let mut failures = Vec::new();
        for node in RENDER_NODES {
            let path = format!("/dev/dri/renderD{node}");
            let device = match OpenOptions::new().read(true).write(true).open(&path) {
                Ok(device) => device,
                // Nodes are numbered without gaps, so the first missing one ends the search.
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => break,
                Err(err) => {
                    failures.push(format!("{path}: {err}"));
                    continue;
                }
            };
            match Self::initialise(device).and_then(|display| {
                let entrypoint = display.encode_entrypoint(profile)?;
                Ok((display, entrypoint))
            }) {
                Ok(found) => return Ok(found),
                Err(Error::HardwareUnavailable { reason, .. }) => {
                    failures.push(format!("{path}: {reason}"));
                }
                Err(err) => return Err(err),
            }
        }
        if failures.is_empty() {
            failures.push("no render node in /dev/dri".into());
        }
        Err(ENCODER.unavailable(failures.join("; ")))
    }

    fn initialise(device: File) -> Result<Self> {
        let va_library = dl::open(ENCODER, dl::VA)?;
        let drm_library = dl::open(ENCODER, dl::VA_DRM)?;
        // SAFETY: the generated loaders only look up libva's functions with their declared
        // signatures.
        let api = unsafe { va::LibVa::from_library(va_library) }
            .map_err(|err| ENCODER.unavailable(format!("libva unusable: {err}")))?;
        // SAFETY: as above.
        let drm = unsafe { va_drm::LibVaDrm::from_library(drm_library) }
            .map_err(|err| ENCODER.unavailable(format!("libva-drm unusable: {err}")))?;
        require_functions(&api, &drm)?;
        // SAFETY: the descriptor is an open render node and stays open as long as the display.
        let handle = unsafe { drm.vaGetDisplayDRM(device.as_raw_fd()) };
        if handle.is_null() {
            return Err(ENCODER.unavailable("vaGetDisplayDRM returned no display"));
        }
        // libva prints its own errors and info lines to stdout/stderr; every failure is
        // reported through the returned status instead.
        // SAFETY: valid display; a null callback disables the messages.
        unsafe {
            api.vaSetErrorCallback(handle, None, ptr::null_mut());
            api.vaSetInfoCallback(handle, None, ptr::null_mut());
        }
        let (mut major, mut minor) = (0, 0);
        // SAFETY: valid display and out pointers.
        let status = unsafe { api.vaInitialize(handle, &mut major, &mut minor) };
        let display = Self {
            api,
            handle,
            _drm: drm,
            _device: device,
        };
        if status != va::VA_STATUS_SUCCESS as c_int {
            // vaTerminate also releases a display whose initialisation failed.
            return Err(ENCODER.unavailable(format!(
                "vaInitialize failed: {}",
                display.error_text(status)
            )));
        }
        Ok(display)
    }

    fn encode_entrypoint(&self, profile: va::VAProfile) -> Result<va::VAEntrypoint> {
        // SAFETY: valid display.
        let max = unsafe { self.api.vaMaxNumEntrypoints(self.handle) }.max(1);
        let mut entrypoints = vec![0; max as usize];
        let mut count = 0;
        // SAFETY: the list has room for vaMaxNumEntrypoints entries.
        let status = unsafe {
            self.api.vaQueryConfigEntrypoints(
                self.handle,
                profile,
                entrypoints.as_mut_ptr(),
                &mut count,
            )
        };
        if status != va::VA_STATUS_SUCCESS as c_int {
            return Err(ENCODER.unavailable(format!(
                "driver has no {}: {}",
                profile_name(profile),
                self.error_text(status)
            )));
        }
        entrypoints.truncate(count.max(0) as usize);
        // The full encoder (EncSlice) where present, else Intel's low-power one.
        [va::VAEntrypointEncSlice, va::VAEntrypointEncSliceLP]
            .into_iter()
            .find(|entrypoint| entrypoints.contains(entrypoint))
            .ok_or_else(|| {
                ENCODER.unavailable(format!("driver cannot encode {}", profile_name(profile)))
            })
    }

    fn error_text(&self, status: va::VAStatus) -> String {
        // SAFETY: vaErrorStr returns a static string for every status.
        let text = unsafe { CStr::from_ptr(self.api.vaErrorStr(status)) };
        format!("{} ({status})", text.to_string_lossy())
    }

    /// Turns a libva status into a codec error.
    fn check(&self, status: va::VAStatus, operation: &'static str) -> Result<()> {
        if status == va::VA_STATUS_SUCCESS as c_int {
            Ok(())
        } else {
            Err(Error::Codec {
                library: "VA-API",
                operation,
                message: self.error_text(status),
            })
        }
    }
}

impl Drop for Display {
    fn drop(&mut self) {
        // SAFETY: the display was returned by vaGetDisplayDRM and is terminated once.
        unsafe { self.api.vaTerminate(self.handle) };
    }
}

/// Fails with "unavailable" when the loaded libva lacks a function the encoder calls, so the
/// generated wrappers (which panic on a missing symbol) are never reached with one.
fn require_functions(api: &va::LibVa, drm: &va_drm::LibVaDrm) -> Result<()> {
    let functions = [
        ("vaErrorStr", api.vaErrorStr.is_ok()),
        ("vaSetErrorCallback", api.vaSetErrorCallback.is_ok()),
        ("vaSetInfoCallback", api.vaSetInfoCallback.is_ok()),
        ("vaInitialize", api.vaInitialize.is_ok()),
        ("vaTerminate", api.vaTerminate.is_ok()),
        ("vaMaxNumEntrypoints", api.vaMaxNumEntrypoints.is_ok()),
        (
            "vaQueryConfigEntrypoints",
            api.vaQueryConfigEntrypoints.is_ok(),
        ),
        ("vaGetConfigAttributes", api.vaGetConfigAttributes.is_ok()),
        ("vaCreateConfig", api.vaCreateConfig.is_ok()),
        ("vaDestroyConfig", api.vaDestroyConfig.is_ok()),
        ("vaCreateSurfaces", api.vaCreateSurfaces.is_ok()),
        ("vaDestroySurfaces", api.vaDestroySurfaces.is_ok()),
        ("vaCreateContext", api.vaCreateContext.is_ok()),
        ("vaDestroyContext", api.vaDestroyContext.is_ok()),
        ("vaCreateBuffer", api.vaCreateBuffer.is_ok()),
        ("vaDestroyBuffer", api.vaDestroyBuffer.is_ok()),
        ("vaMapBuffer", api.vaMapBuffer.is_ok()),
        ("vaUnmapBuffer", api.vaUnmapBuffer.is_ok()),
        ("vaBeginPicture", api.vaBeginPicture.is_ok()),
        ("vaRenderPicture", api.vaRenderPicture.is_ok()),
        ("vaEndPicture", api.vaEndPicture.is_ok()),
        ("vaSyncSurface", api.vaSyncSurface.is_ok()),
        ("vaDeriveImage", api.vaDeriveImage.is_ok()),
        ("vaCreateImage", api.vaCreateImage.is_ok()),
        ("vaDestroyImage", api.vaDestroyImage.is_ok()),
        ("vaPutImage", api.vaPutImage.is_ok()),
        ("vaGetDisplayDRM", drm.vaGetDisplayDRM.is_ok()),
    ];
    match functions.iter().find(|(_, present)| !present) {
        Some((name, _)) => Err(ENCODER.unavailable(format!("libva lacks {name}"))),
        None => Ok(()),
    }
}

fn profile_name(profile: va::VAProfile) -> &'static str {
    match profile {
        va::VAProfileAV1Profile0 => "AV1 profile 0",
        va::VAProfileVP9Profile0 => "VP9 profile 0",
        _ => "this profile",
    }
}

/// What the driver's encoder configuration supports.
struct Capabilities {
    rate_control: u32,
    /// Length of the obu_size field the driver patches in AV1 frame headers.
    obu_size_bytes: usize,
}

/// Creates the encoder configuration for `profile`, or reports why the driver can't.
fn create_config(
    display: &Display,
    profile: va::VAProfile,
    entrypoint: va::VAEntrypoint,
    codec: Codec,
) -> Result<(va::VAConfigID, Capabilities)> {
    let mut attributes = [
        va::VAConfigAttribRTFormat,
        va::VAConfigAttribRateControl,
        va::VAConfigAttribEncPackedHeaders,
        va::VAConfigAttribEncMaxRefFrames,
        va::VAConfigAttribEncAV1Ext2,
    ]
    .map(|type_| va::VAConfigAttrib { type_, value: 0 });
    // SAFETY: valid display and attribute list of the given length.
    let status = unsafe {
        display.api.vaGetConfigAttributes(
            display.handle,
            profile,
            entrypoint,
            attributes.as_mut_ptr(),
            attributes.len() as c_int,
        )
    };
    if status != va::VA_STATUS_SUCCESS as c_int {
        return Err(ENCODER.unavailable(format!(
            "vaGetConfigAttributes: {}",
            display.error_text(status)
        )));
    }
    let supported = |value: u32| value != va::VA_ATTRIB_NOT_SUPPORTED;
    let [rt_format, rate_control, packed, max_refs, av1_ext2] = attributes.map(|a| a.value);
    if !supported(rt_format) || rt_format & va::VA_RT_FORMAT_YUV420 == 0 {
        return Err(ENCODER.unavailable("driver cannot encode 8-bit 4:2:0"));
    }
    let rate_control = if !supported(rate_control) {
        return Err(ENCODER.unavailable("driver has no rate control"));
    } else if rate_control & va::VA_RC_CBR != 0 {
        va::VA_RC_CBR
    } else if rate_control & va::VA_RC_VBR != 0 {
        va::VA_RC_VBR
    } else {
        return Err(ENCODER.unavailable("driver has neither CBR nor VBR rate control"));
    };
    // The low 16 bits are the forward references a P frame may use; one is all this needs.
    if supported(max_refs) && max_refs & 0xffff == 0 {
        return Err(ENCODER.unavailable("driver encodes no inter frames"));
    }
    let mut chosen = vec![
        va::VAConfigAttrib {
            type_: va::VAConfigAttribRTFormat,
            value: va::VA_RT_FORMAT_YUV420,
        },
        va::VAConfigAttrib {
            type_: va::VAConfigAttribRateControl,
            value: rate_control,
        },
    ];
    let mut obu_size_bytes = 4;
    if codec == Codec::Av1 {
        let needed = va::VA_ENC_PACKED_HEADER_SEQUENCE | va::VA_ENC_PACKED_HEADER_PICTURE;
        if !supported(packed) || packed & needed != needed {
            return Err(ENCODER.unavailable("driver takes no AV1 sequence and frame headers"));
        }
        chosen.push(va::VAConfigAttrib {
            type_: va::VAConfigAttribEncPackedHeaders,
            value: needed,
        });
        if supported(av1_ext2) {
            // obu_size_bytes_minus1 sits in bits 2..4 of VAConfigAttribValEncAV1Ext2.
            obu_size_bytes = ((av1_ext2 >> 2) & 0b11) as usize + 1;
        }
    }
    let mut config = 0;
    // SAFETY: valid display, attribute list and out pointer.
    let status = unsafe {
        display.api.vaCreateConfig(
            display.handle,
            profile,
            entrypoint,
            chosen.as_mut_ptr(),
            chosen.len() as c_int,
            &mut config,
        )
    };
    if status != va::VA_STATUS_SUCCESS as c_int {
        return Err(ENCODER.unavailable(format!("vaCreateConfig: {}", display.error_text(status))));
    }
    Ok((
        config,
        Capabilities {
            rate_control,
            obu_size_bytes,
        },
    ))
}

/// How frames get into the input surface.
enum Upload {
    /// Map the surface itself (vaDeriveImage), the cheap path most drivers offer.
    Derive,
    /// Write into a separate image and copy it over (vaPutImage).
    Copy(va::VAImage),
}

/// Everything that depends on the frame size: the context and its surfaces.
struct Session {
    width: u32,
    height: u32,
    context: va::VAContextID,
    input: va::VASurfaceID,
    pool: [va::VASurfaceID; POOL_SIZE],
    coded: va::VABufferID,
    upload: Upload,
}

impl Session {
    fn create(display: &Display, config: va::VAConfigID, width: u32, height: u32) -> Result<Self> {
        let mut session = Self {
            width,
            height,
            context: va::VA_INVALID_ID,
            input: va::VA_INVALID_SURFACE,
            pool: [va::VA_INVALID_SURFACE; POOL_SIZE],
            coded: va::VA_INVALID_ID,
            upload: Upload::Derive,
        };
        // On failure, dropping the half-built session through `destroy` frees what exists.
        match session.build(display, config) {
            Ok(()) => Ok(session),
            Err(err) => {
                session.destroy(display);
                Err(err)
            }
        }
    }

    fn build(&mut self, display: &Display, config: va::VAConfigID) -> Result<()> {
        let api = &display.api;
        // Padded like the reconstructed frames' blocks, for drivers that encode a padded frame.
        let (padded_w, padded_h) = (
            self.width.next_multiple_of(INPUT_ALIGN),
            self.height.next_multiple_of(INPUT_ALIGN),
        );
        // SAFETY (all calls below): valid display, config and out pointers; arrays have the
        // lengths passed.
        let status = unsafe {
            api.vaCreateSurfaces(
                display.handle,
                va::VA_RT_FORMAT_YUV420,
                padded_w,
                padded_h,
                &mut self.input,
                1,
                ptr::null_mut(),
                0,
            )
        };
        display.check(status, "create input surface")?;
        let status = unsafe {
            api.vaCreateSurfaces(
                display.handle,
                va::VA_RT_FORMAT_YUV420,
                self.width.next_multiple_of(RECON_ALIGN),
                self.height.next_multiple_of(RECON_ALIGN),
                self.pool.as_mut_ptr(),
                POOL_SIZE as c_uint,
                ptr::null_mut(),
                0,
            )
        };
        display.check(status, "create reference surfaces")?;
        let mut targets = self.pool.to_vec();
        targets.push(self.input);
        let status = unsafe {
            api.vaCreateContext(
                display.handle,
                config,
                self.width as c_int,
                self.height as c_int,
                va::VA_PROGRESSIVE as c_int,
                targets.as_mut_ptr(),
                targets.len() as c_int,
                &mut self.context,
            )
        };
        display.check(status, "create context")?;
        // A compressed frame is far smaller than the raw one; twice the raw size leaves room
        // for any key frame at any bitrate.
        let coded_size = (padded_w * padded_h * 3).max(1 << 20);
        let status = unsafe {
            api.vaCreateBuffer(
                display.handle,
                self.context,
                va::VAEncCodedBufferType,
                coded_size,
                1,
                ptr::null_mut(),
                &mut self.coded,
            )
        };
        display.check(status, "create coded buffer")?;

        let mut image: va::VAImage = zeroed();
        let derived = unsafe { api.vaDeriveImage(display.handle, self.input, &mut image) };
        if derived == va::VA_STATUS_SUCCESS as c_int && image.format.fourcc == va::VA_FOURCC_NV12 {
            unsafe { api.vaDestroyImage(display.handle, image.image_id) };
        } else {
            if derived == va::VA_STATUS_SUCCESS as c_int {
                unsafe { api.vaDestroyImage(display.handle, image.image_id) };
            }
            let mut format: va::VAImageFormat = zeroed();
            format.fourcc = va::VA_FOURCC_NV12;
            format.byte_order = va::VA_LSB_FIRST;
            format.bits_per_pixel = 12;
            let status = unsafe {
                api.vaCreateImage(
                    display.handle,
                    &mut format,
                    padded_w as c_int,
                    padded_h as c_int,
                    &mut image,
                )
            };
            display.check(status, "create upload image")?;
            self.upload = Upload::Copy(image);
        }
        Ok(())
    }

    /// Copies `frame` (I420) into the input surface as NV12.
    fn upload(&self, display: &Display, frame: &I420Frame) -> Result<()> {
        let api = &display.api;
        let image = match &self.upload {
            Upload::Copy(image) => *image,
            Upload::Derive => {
                let mut image: va::VAImage = zeroed();
                // SAFETY: valid display, surface and out pointer.
                let status = unsafe { api.vaDeriveImage(display.handle, self.input, &mut image) };
                display.check(status, "map input surface")?;
                image
            }
        };
        let written = self.write_image(display, &image, frame);
        // SAFETY (all calls below): the image belongs to this display; a derived one is
        // destroyed after every frame, a copy image only with the session.
        let copied = written.and_then(|()| match self.upload {
            Upload::Copy(_) => {
                let (w, h) = (image.width.into(), image.height.into());
                let status = unsafe {
                    api.vaPutImage(
                        display.handle,
                        self.input,
                        image.image_id,
                        0,
                        0,
                        w,
                        h,
                        0,
                        0,
                        w,
                        h,
                    )
                };
                display.check(status, "copy frame to surface")
            }
            Upload::Derive => Ok(()),
        });
        if let Upload::Derive = self.upload {
            unsafe { api.vaDestroyImage(display.handle, image.image_id) };
        }
        copied
    }

    fn write_image(&self, display: &Display, image: &va::VAImage, frame: &I420Frame) -> Result<()> {
        let mut base: *mut c_void = ptr::null_mut();
        // SAFETY: the image buffer belongs to this display.
        let status = unsafe {
            display
                .api
                .vaMapBuffer(display.handle, image.buf, &mut base)
        };
        display.check(status, "map image")?;
        // SAFETY: the mapping covers data_size bytes until vaUnmapBuffer.
        let data =
            unsafe { std::slice::from_raw_parts_mut(base.cast::<u8>(), image.data_size as usize) };
        let copied = i420_to_nv12(
            frame,
            data,
            [image.offsets[0], image.offsets[1]].map(|o| o as usize),
            [image.pitches[0], image.pitches[1]].map(|p| p as usize),
            (usize::from(image.width), usize::from(image.height)),
        );
        // SAFETY: mapped above.
        let status = unsafe { display.api.vaUnmapBuffer(display.handle, image.buf) };
        copied?;
        display.check(status, "unmap image")
    }

    fn destroy(&mut self, display: &Display) {
        let api = &display.api;
        // SAFETY (all calls): every id was created on this display and is released once; an
        // id still invalid was never created.
        unsafe {
            if let Upload::Copy(image) = self.upload {
                api.vaDestroyImage(display.handle, image.image_id);
                self.upload = Upload::Derive;
            }
            if self.coded != va::VA_INVALID_ID {
                api.vaDestroyBuffer(display.handle, self.coded);
            }
            if self.context != va::VA_INVALID_ID {
                api.vaDestroyContext(display.handle, self.context);
            }
            if self.pool[0] != va::VA_INVALID_SURFACE {
                api.vaDestroySurfaces(display.handle, self.pool.as_mut_ptr(), POOL_SIZE as c_int);
            }
            if self.input != va::VA_INVALID_SURFACE {
                api.vaDestroySurfaces(display.handle, &mut self.input, 1);
            }
        }
        self.coded = va::VA_INVALID_ID;
        self.context = va::VA_INVALID_ID;
        self.pool = [va::VA_INVALID_SURFACE; POOL_SIZE];
        self.input = va::VA_INVALID_SURFACE;
    }
}

/// Copies an I420 frame into the top left of an NV12 image and fills the rest of the image by
/// repeating the last column and row. Some drivers (AMD) encode a frame padded to a multiple of
/// 8 pixels; the padding then continues the picture instead of showing stale memory.
fn i420_to_nv12(
    frame: &I420Frame,
    data: &mut [u8],
    offsets: [usize; 2],
    pitches: [usize; 2],
    image_size: (usize, usize),
) -> Result<()> {
    let (w, h) = (frame.width() as usize, frame.height() as usize);
    let (cw, ch) = (w.div_ceil(2), h.div_ceil(2));
    let (iw, ih) = (image_size.0 & !1, image_size.1 & !1);
    let fits = iw >= w
        && ih >= h
        && pitches[0] >= iw
        && pitches[1] >= iw
        && offsets[0] + pitches[0] * (ih - 1) + iw <= data.len()
        && offsets[1] + pitches[1] * (ih / 2 - 1) + iw <= data.len();
    if !fits {
        return Err(Error::Codec {
            library: "VA-API",
            operation: "upload frame",
            message: format!("driver image {iw}x{ih} does not fit a {w}x{h} frame"),
        });
    }
    let (y, u, v) = (frame.y(), frame.u(), frame.v());
    for row in 0..ih {
        let src = &y[row.min(h - 1) * w..][..w];
        let dst = &mut data[offsets[0] + row * pitches[0]..][..iw];
        dst[..w].copy_from_slice(src);
        dst[w..].fill(src[w - 1]);
    }
    for row in 0..ih / 2 {
        let src = row.min(ch - 1) * cw;
        let dst = &mut data[offsets[1] + row * pitches[1]..][..iw];
        for (col, pair) in dst.as_chunks_mut::<2>().0.iter_mut().enumerate() {
            let col = col.min(cw - 1);
            pair[0] = u[src + col];
            pair[1] = v[src + col];
        }
    }
    Ok(())
}

/// The reference structure of one frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Plan {
    layers: u8,
    position: usize,
    layer: u8,
    refs: FrameRefs,
    /// Pool index of the surface the frame is reconstructed into.
    recon: usize,
    order_hint: u8,
}

/// Decides layers and references, and tracks which surface each of the eight reference slots
/// holds. Slot `k` holds the latest frame of layer `k`; a key frame fills every slot.
struct References {
    layers: u8,
    next_layers: u8,
    position: usize,
    slots: [Option<usize>; 8],
    /// Frame number stored in each slot, to find the newest lower-layer frame.
    stored: [u64; 8],
    frame: u64,
    since_key: u64,
}

impl References {
    fn new(fps: u32) -> Self {
        Self {
            layers: layers_for(fps),
            next_layers: layers_for(fps),
            position: 0,
            slots: [None; 8],
            stored: [0; 8],
            frame: 0,
            since_key: 0,
        }
    }

    /// The layer count changes when the next layer-0 frame starts a new pattern.
    fn set_fps(&mut self, fps: u32) {
        self.next_layers = layers_for(fps);
    }

    fn plan(&self, keyframe: bool) -> Plan {
        let keyframe = keyframe || self.slots[0].is_none();
        let position = if keyframe { 0 } else { self.position };
        let layers = if position == 0 {
            self.next_layers
        } else {
            self.layers
        };
        let layer = pattern(layers)[position];
        let refs = if keyframe {
            FrameRefs::Key
        } else {
            // Layer 0 predicts from the previous layer 0, every other layer from the newest
            // frame of a lower layer.
            let reference = (0..usize::from(layer.max(1)))
                .filter(|&slot| self.slots[slot].is_some())
                // On a tie (all slots hold the key frame) the lowest slot wins.
                .rev()
                .max_by_key(|&slot| self.stored[slot])
                .unwrap_or(0) as u8;
            let refresh = if layer + 1 < layers { 1 << layer } else { 0 };
            FrameRefs::Inter { reference, refresh }
        };
        let recon = (0..POOL_SIZE)
            .find(|index| !self.slots.contains(&Some(*index)))
            .expect("the pool has more surfaces than distinct slots");
        Plan {
            layers,
            position,
            layer,
            refs,
            recon,
            order_hint: if keyframe { 0 } else { self.since_key as u8 },
        }
    }

    fn commit(&mut self, plan: &Plan) {
        self.layers = plan.layers;
        self.position = (plan.position + 1) % pattern(plan.layers).len();
        let refresh = match plan.refs {
            FrameRefs::Key => {
                self.since_key = 0;
                0xff
            }
            FrameRefs::Inter { refresh, .. } => refresh,
        };
        for slot in 0..8 {
            if refresh & (1 << slot) != 0 {
                self.slots[slot] = Some(plan.recon);
                self.stored[slot] = self.frame;
            }
        }
        self.frame += 1;
        self.since_key += 1;
    }

    fn surfaces(&self, pool: &[va::VASurfaceID; POOL_SIZE]) -> [va::VASurfaceID; 8] {
        self.slots
            .map(|slot| slot.map_or(va::VA_INVALID_SURFACE, |index| pool[index]))
    }

    /// Forgets every reference; the next frame becomes a key frame.
    fn reset(&mut self) {
        self.slots = [None; 8];
    }
}

/// Parameter buffers of one frame, destroyed after the frame is submitted.
struct Buffers<'a> {
    display: &'a Display,
    context: va::VAContextID,
    ids: Vec<va::VABufferID>,
}

impl<'a> Buffers<'a> {
    fn new(display: &'a Display, context: va::VAContextID) -> Self {
        Self {
            display,
            context,
            ids: Vec::new(),
        }
    }

    fn add_bytes(&mut self, type_: va::VABufferType, bytes: &[u8]) -> Result<()> {
        let mut id = va::VA_INVALID_ID;
        // SAFETY: libva copies `bytes` into the new buffer.
        let status = unsafe {
            self.display.api.vaCreateBuffer(
                self.display.handle,
                self.context,
                type_,
                bytes.len() as c_uint,
                1,
                bytes.as_ptr().cast_mut().cast(),
                &mut id,
            )
        };
        self.display.check(status, "create parameter buffer")?;
        self.ids.push(id);
        Ok(())
    }

    fn add<T: Copy>(&mut self, type_: va::VABufferType, value: &T) -> Result<()> {
        self.add_bytes(type_, as_bytes(value))
    }

    /// A VAEncMiscParameterBuffer: the parameter type followed by its struct.
    fn add_misc<T: Copy>(&mut self, misc: va::VAEncMiscParameterType, value: &T) -> Result<()> {
        let mut bytes = misc.to_ne_bytes().to_vec();
        bytes.extend_from_slice(as_bytes(value));
        self.add_bytes(va::VAEncMiscParameterBufferType, &bytes)
    }

    fn add_packed_header(&mut self, type_: u32, data: &[u8]) -> Result<()> {
        let mut parameter: va::VAEncPackedHeaderParameterBuffer = zeroed();
        parameter.type_ = type_;
        parameter.bit_length = 8 * data.len() as u32;
        // AV1 has no emulation prevention bytes, so the data is final as it is.
        parameter.has_emulation_bytes = 1;
        self.add(va::VAEncPackedHeaderParameterBufferType, &parameter)?;
        self.add_bytes(va::VAEncPackedHeaderDataBufferType, data)
    }
}

impl Drop for Buffers<'_> {
    fn drop(&mut self) {
        for id in &self.ids {
            // SAFETY: created on this display and destroyed once.
            unsafe { self.display.api.vaDestroyBuffer(self.display.handle, *id) };
        }
    }
}

fn as_bytes<T: Copy>(value: &T) -> &[u8] {
    // SAFETY: libva's parameter structs are plain C data; reading their bytes (padding
    // included, which libva ignores) is how they are passed to the driver anyway.
    unsafe { std::slice::from_raw_parts((value as *const T).cast::<u8>(), size_of::<T>()) }
}

struct VaapiEncoder {
    codec: Codec,
    config: EncoderConfig,
    capabilities: Capabilities,
    refs: References,
    /// Rate control parameters go to the driver with the next frame.
    pending_rate: bool,
    va_config: va::VAConfigID,
    session: Session,
    // Dropped last, after `Drop::drop` released everything created on it.
    display: Display,
}

// SAFETY: the encoder owns its VA display exclusively, and every libva call goes through
// `&mut self` or `&self` of the one owner; libva displays may be used from any thread as long
// as calls on them don't overlap.
unsafe impl Send for VaapiEncoder {}

impl VaapiEncoder {
    fn new(codec: Codec, config: EncoderConfig) -> Result<Self> {
        let profile = match codec {
            Codec::Av1 => va::VAProfileAV1Profile0,
            Codec::Vp9 => va::VAProfileVP9Profile0,
        };
        let (display, entrypoint) = Display::open(profile)?;
        let (va_config, capabilities) = create_config(&display, profile, entrypoint, codec)?;
        let session = match Session::create(&display, va_config, config.width, config.height) {
            Ok(session) => session,
            Err(err) => {
                // SAFETY: created above on this display.
                unsafe { display.api.vaDestroyConfig(display.handle, va_config) };
                // A driver that lists the encoder but fails to set it up can't be used either.
                return Err(ENCODER.unavailable(err.to_string()));
            }
        };
        Ok(Self {
            codec,
            config,
            capabilities,
            refs: References::new(config.fps),
            pending_rate: true,
            va_config,
            session,
            display,
        })
    }

    fn encode_frame(
        &mut self,
        frame: &I420Frame,
        pts: i64,
        force_keyframe: bool,
    ) -> Result<EncodedFrame> {
        if (frame.width(), frame.height()) != (self.config.width, self.config.height) {
            return Err(Error::InvalidFrame(format!(
                "frame is {}x{}, encoder is set to {}x{}",
                frame.width(),
                frame.height(),
                self.config.width,
                self.config.height
            )));
        }
        let plan = self.refs.plan(force_keyframe);
        self.session.upload(&self.display, frame)?;
        let data = match self.submit(&plan) {
            Ok(data) => data,
            Err(err) => {
                // The driver's reference state is unknown now; start over with a key frame.
                self.refs.reset();
                return Err(err);
            }
        };
        self.refs.commit(&plan);
        Ok(EncodedFrame {
            data,
            keyframe: plan.refs == FrameRefs::Key,
            temporal_layer: plan.layer,
            pts,
        })
    }

    /// Encodes the uploaded input surface with `plan` and returns the compressed frame.
    fn submit(&mut self, plan: &Plan) -> Result<Vec<u8>> {
        let display = &self.display;
        let session = &self.session;
        let key = plan.refs == FrameRefs::Key;
        let mut buffers = Buffers::new(display, session.context);
        let mut prefix_len = 0;
        match self.codec {
            Codec::Av1 => {
                if key {
                    let level = av1_obu::level_idx(
                        self.config.width,
                        self.config.height,
                        self.config.fps,
                        self.config.bitrate_kbps,
                    );
                    buffers.add(
                        va::VAEncSequenceParameterBufferType,
                        &av1_sequence(&self.config, level),
                    )?;
                    let header =
                        av1_obu::sequence_header(self.config.width, self.config.height, level);
                    prefix_len = header.len();
                    buffers.add_packed_header(va::VAEncPackedHeaderSequence, &header)?;
                }
            }
            Codec::Vp9 => {
                if key {
                    buffers.add(
                        va::VAEncSequenceParameterBufferType,
                        &vp9_sequence(&self.config),
                    )?;
                }
            }
        }
        if key || self.pending_rate {
            self.add_rate_control(&mut buffers, !key)?;
        }
        let surfaces = self.refs.surfaces(&session.pool);
        let recon = session.pool[plan.recon];
        match self.codec {
            Codec::Av1 => {
                let header = av1_obu::frame_header(
                    plan.refs,
                    plan.order_hint,
                    self.config.width,
                    self.config.height,
                    self.capabilities.obu_size_bytes,
                );
                let picture = av1_picture(
                    &self.config,
                    plan,
                    &header,
                    prefix_len,
                    recon,
                    session.coded,
                    surfaces,
                );
                buffers.add(va::VAEncPictureParameterBufferType, &picture)?;
                buffers.add_packed_header(va::VAEncPackedHeaderPicture, &header.data)?;
                let tile_group: va::VAEncTileGroupBufferAV1 = zeroed();
                buffers.add(va::VAEncSliceParameterBufferType, &tile_group)?;
            }
            Codec::Vp9 => {
                let picture = vp9_picture(&self.config, plan, recon, session.coded, surfaces);
                buffers.add(va::VAEncPictureParameterBufferType, &picture)?;
            }
        }

        let api = &display.api;
        // SAFETY (all calls below): context, surfaces and buffers belong to this display.
        let status = unsafe { api.vaBeginPicture(display.handle, session.context, session.input) };
        display.check(status, "begin picture")?;
        let status = unsafe {
            api.vaRenderPicture(
                display.handle,
                session.context,
                buffers.ids.as_mut_ptr(),
                buffers.ids.len() as c_int,
            )
        };
        let end = unsafe { api.vaEndPicture(display.handle, session.context) };
        display.check(status, "render picture")?;
        display.check(end, "end picture")?;
        drop(buffers);
        let status = unsafe { api.vaSyncSurface(display.handle, session.input) };
        display.check(status, "wait for encoder")?;
        let mut data = self.read_coded()?;
        if self.codec == Codec::Av1 && !av1_obu::starts_with_temporal_delimiter(&data) {
            data.splice(0..0, av1_obu::TEMPORAL_DELIMITER);
        }
        self.pending_rate = false;
        Ok(data)
    }

    fn add_rate_control(&self, buffers: &mut Buffers<'_>, reset: bool) -> Result<()> {
        let bits_per_second = self.config.bitrate_kbps.saturating_mul(1000);
        let mut rate: va::VAEncMiscParameterRateControl = zeroed();
        rate.bits_per_second = bits_per_second;
        rate.target_percentage = if self.capabilities.rate_control == va::VA_RC_CBR {
            100
        } else {
            90
        };
        rate.window_size = 1000;
        // qindex range for both AV1 and VP9.
        rate.min_qp = 1;
        rate.max_qp = 255;
        let mut flags: va::_VAEncMiscParameterRateControl__bindgen_ty_1__bindgen_ty_1 = zeroed();
        flags.set_reset(u32::from(reset));
        // Every captured frame must come out; a skipped frame would break the layer pattern.
        flags.set_disable_frame_skip(1);
        rate.rc_flags.bits = flags;
        buffers.add_misc(va::VAEncMiscParameterTypeRateControl, &rate)?;

        let mut hrd: va::VAEncMiscParameterHRD = zeroed();
        // A one-second buffer: short enough for interactive latency, long enough for key frames.
        hrd.buffer_size = bits_per_second;
        hrd.initial_buffer_fullness = bits_per_second / 4 * 3;
        buffers.add_misc(va::VAEncMiscParameterTypeHRD, &hrd)?;

        let mut frame_rate: va::VAEncMiscParameterFrameRate = zeroed();
        // Numerator in the low 16 bits, denominator in the high 16 bits.
        frame_rate.framerate = self.config.fps | (1 << 16);
        buffers.add_misc(va::VAEncMiscParameterTypeFrameRate, &frame_rate)
    }

    fn read_coded(&self) -> Result<Vec<u8>> {
        let display = &self.display;
        let mut segment: *mut c_void = ptr::null_mut();
        // SAFETY: the coded buffer belongs to this display.
        let status = unsafe {
            display
                .api
                .vaMapBuffer(display.handle, self.session.coded, &mut segment)
        };
        display.check(status, "map coded buffer")?;
        let mut data = Vec::new();
        let mut overflow = false;
        let mut next = segment.cast::<va::VACodedBufferSegment>();
        while !next.is_null() {
            // SAFETY: a mapped coded buffer is a linked list of segments, each pointing at
            // `size` bytes, valid until the buffer is unmapped.
            let current = unsafe { &*next };
            overflow |= current.status & va::VA_CODED_BUF_STATUS_SLICE_OVERFLOW_MASK != 0;
            if current.size > 0 && !current.buf.is_null() {
                data.extend_from_slice(unsafe {
                    std::slice::from_raw_parts(current.buf.cast::<u8>(), current.size as usize)
                });
            }
            next = current.next.cast();
        }
        // SAFETY: mapped above.
        let status = unsafe {
            display
                .api
                .vaUnmapBuffer(display.handle, self.session.coded)
        };
        display.check(status, "unmap coded buffer")?;
        if overflow || data.is_empty() {
            return Err(Error::Codec {
                library: "VA-API",
                operation: "read coded frame",
                message: if overflow {
                    "the frame did not fit the coded buffer".into()
                } else {
                    "the driver returned an empty frame".into()
                },
            });
        }
        Ok(data)
    }
}

fn av1_sequence(config: &EncoderConfig, level: u8) -> va::VAEncSequenceParameterBufferAV1 {
    let mut sequence: va::VAEncSequenceParameterBufferAV1 = zeroed();
    sequence.seq_profile = 0;
    sequence.seq_level_idx = level;
    // Key frames come only on request; the period just has to be long.
    sequence.intra_period = config.fps.saturating_mul(3600);
    sequence.ip_period = 1;
    sequence.bits_per_second = config.bitrate_kbps.saturating_mul(1000);
    let mut fields: va::_VAEncSequenceParameterBufferAV1__bindgen_ty_1__bindgen_ty_1 = zeroed();
    fields.set_enable_order_hint(1);
    fields.set_enable_cdef(1);
    sequence.seq_fields.bits = fields;
    sequence.order_hint_bits_minus_1 = (av1_obu::ORDER_HINT_BITS - 1) as u8;
    sequence
}

fn av1_picture(
    config: &EncoderConfig,
    plan: &Plan,
    header: &av1_obu::FrameHeader,
    sequence_header_len: usize,
    recon: va::VASurfaceID,
    coded: va::VABufferID,
    surfaces: [va::VASurfaceID; 8],
) -> va::VAEncPictureParameterBufferAV1 {
    let mut picture: va::VAEncPictureParameterBufferAV1 = zeroed();
    picture.frame_width_minus_1 = (config.width - 1) as u16;
    picture.frame_height_minus_1 = (config.height - 1) as u16;
    picture.reconstructed_frame = recon;
    picture.coded_buf = coded;
    picture.order_hint = plan.order_hint;
    let mut flags: va::_VAEncPictureParameterBufferAV1__bindgen_ty_1__bindgen_ty_1 = zeroed();
    match plan.refs {
        FrameRefs::Key => {
            picture.reference_frames = [va::VA_INVALID_SURFACE; 8];
            picture.primary_ref_frame = av1_obu::PRIMARY_REF_NONE;
            picture.refresh_frame_flags = 0xff;
            flags.set_frame_type(0);
            flags.set_error_resilient_mode(1);
        }
        FrameRefs::Inter { reference, refresh } => {
            picture.reference_frames = surfaces;
            picture.ref_frame_idx = [reference; 7];
            picture.primary_ref_frame = 0;
            picture.refresh_frame_flags = refresh;
            let mut search: va::VARefFrameCtrlAV1__bindgen_ty_1 = zeroed();
            // Search only LAST_FRAME (1), the frame's one reference.
            search.set_search_idx0(1);
            picture.ref_frame_ctrl_l0.fields = search;
            flags.set_frame_type(1);
        }
    }
    picture.picture_flags.bits = flags;
    picture.temporal_id = 0;
    picture.base_qindex = av1_obu::BASE_Q_IDX;
    picture.min_base_qindex = 1;
    picture.max_base_qindex = 255;
    let mut mode: va::_VAEncPictureParameterBufferAV1__bindgen_ty_4__bindgen_ty_1 = zeroed();
    mode.set_tx_mode(2); // TX_MODE_SELECT, as in the frame header
    picture.mode_control_flags.bits = mode;
    let (sb_cols, sb_rows) = av1_obu::superblocks(config.width, config.height);
    picture.tile_cols = 1;
    picture.tile_rows = 1;
    picture.width_in_sbs_minus_1[0] = (sb_cols - 1) as u16;
    picture.height_in_sbs_minus_1[0] = (sb_rows - 1) as u16;
    picture.bit_offset_qindex = header.bit_offset_qindex;
    picture.bit_offset_segmentation = header.bit_offset_segmentation;
    picture.bit_offset_loopfilter_params = header.bit_offset_loopfilter_params;
    picture.bit_offset_cdef_params = header.bit_offset_cdef_params;
    picture.size_in_bits_cdef_params = header.size_in_bits_cdef_params;
    // The driver writes the sequence header in front of the frame header on key frames and
    // counts the obu_size position from the start of both.
    picture.byte_offset_frame_hdr_obu_size =
        header.byte_offset_obu_size + sequence_header_len as u32;
    picture.size_in_bits_frame_hdr_obu = 8 * header.data.len() as u32;
    let mut obu: va::_VAEncPictureParameterBufferAV1__bindgen_ty_6__bindgen_ty_1 = zeroed();
    obu.set_obu_has_size_field(1);
    picture.tile_group_obu_hdr_info.bits = obu;
    picture
}

fn vp9_sequence(config: &EncoderConfig) -> va::VAEncSequenceParameterBufferVP9 {
    let mut sequence: va::VAEncSequenceParameterBufferVP9 = zeroed();
    sequence.max_frame_width = config.width;
    sequence.max_frame_height = config.height;
    // Key frames come only on request.
    sequence.kf_auto = 0;
    sequence.kf_min_dist = 1;
    sequence.kf_max_dist = config.fps.saturating_mul(3600);
    sequence.intra_period = config.fps.saturating_mul(3600);
    sequence.bits_per_second = config.bitrate_kbps.saturating_mul(1000);
    sequence
}

fn vp9_picture(
    config: &EncoderConfig,
    plan: &Plan,
    recon: va::VASurfaceID,
    coded: va::VABufferID,
    surfaces: [va::VASurfaceID; 8],
) -> va::VAEncPictureParameterBufferVP9 {
    let mut picture: va::VAEncPictureParameterBufferVP9 = zeroed();
    picture.frame_width_src = config.width;
    picture.frame_height_src = config.height;
    picture.frame_width_dst = config.width;
    picture.frame_height_dst = config.height;
    picture.reconstructed_frame = recon;
    picture.coded_buf = coded;
    let mut refs: va::_VAEncPictureParameterBufferVP9__bindgen_ty_1__bindgen_ty_1 = zeroed();
    let mut flags: va::_VAEncPictureParameterBufferVP9__bindgen_ty_2__bindgen_ty_1 = zeroed();
    flags.set_show_frame(1);
    flags.set_frame_context_idx(0);
    match plan.refs {
        FrameRefs::Key => {
            picture.reference_frames = [va::VA_INVALID_SURFACE; 8];
            picture.refresh_frame_flags = 0xff;
            flags.set_frame_type(0);
            flags.set_refresh_frame_context(1);
        }
        FrameRefs::Inter { reference, refresh } => {
            picture.reference_frames = surfaces;
            picture.refresh_frame_flags = refresh;
            // LAST only (bit 0), stored in `reference`; golden and altref point there too.
            refs.set_ref_frame_ctrl_l0(1);
            refs.set_ref_last_idx(u32::from(reference));
            refs.set_ref_gf_idx(u32::from(reference));
            refs.set_ref_arf_idx(u32::from(reference));
            flags.set_frame_type(1);
            // Only layer 0 updates the probability context, so frames of higher layers can be
            // dropped without the remaining ones decoding with the wrong probabilities.
            flags.set_refresh_frame_context(u32::from(plan.layer == 0));
        }
    }
    picture.ref_flags.bits = refs;
    picture.pic_flags.bits = flags;
    // Starting values; the driver's rate control sets the real ones.
    picture.luma_ac_qindex = 60;
    picture.filter_level = 10;
    picture
}

impl VideoEncoder for VaapiEncoder {
    fn codec(&self) -> Codec {
        self.codec
    }

    fn config(&self) -> EncoderConfig {
        self.config
    }

    fn temporal_layers(&self) -> u8 {
        self.refs.layers
    }

    fn encode(
        &mut self,
        frame: &I420Frame,
        pts: i64,
        force_keyframe: bool,
    ) -> Result<Vec<EncodedFrame>> {
        self.encode_frame(frame, pts, force_keyframe)
            .map(|frame| vec![frame])
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
        // The new surfaces are created first so a failure leaves the old ones working.
        let session = Session::create(&self.display, self.va_config, width, height)?;
        let mut old = std::mem::replace(&mut self.session, session);
        old.destroy(&self.display);
        self.config = config;
        self.refs.reset();
        Ok(())
    }

    fn set_bitrate(&mut self, bitrate_kbps: u32) -> Result<()> {
        let config = EncoderConfig {
            bitrate_kbps,
            ..self.config
        };
        validate(&config)?;
        self.config = config;
        self.pending_rate = true;
        Ok(())
    }

    fn set_fps(&mut self, fps: u32) -> Result<()> {
        let config = EncoderConfig { fps, ..self.config };
        validate(&config)?;
        self.config = config;
        self.refs.set_fps(fps);
        self.pending_rate = true;
        Ok(())
    }
}

impl Drop for VaapiEncoder {
    fn drop(&mut self) {
        self.session.destroy(&self.display);
        // SAFETY: created on this display and destroyed once; the display outlives this call.
        unsafe {
            self.display
                .api
                .vaDestroyConfig(self.display.handle, self.va_config)
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(refs: &mut References, frames: usize, key_at: &[usize]) -> Vec<Plan> {
        (0..frames)
            .map(|index| {
                let plan = refs.plan(key_at.contains(&index));
                refs.commit(&plan);
                plan
            })
            .collect()
    }

    #[test]
    fn three_layers_below_60_fps() {
        let mut refs = References::new(30);
        let plans = run(&mut refs, 9, &[]);
        let layers: Vec<u8> = plans.iter().map(|p| p.layer).collect();
        assert_eq!(layers, [0, 2, 1, 2, 0, 2, 1, 2, 0]);
        assert_eq!(plans[0].refs, FrameRefs::Key);
        let inter = |reference, refresh| FrameRefs::Inter { reference, refresh };
        // Layer 2 never refreshes a slot; layer 1 stores into slot 1; layer 2 after a layer 1
        // predicts from it; the next layer 0 from slot 0.
        assert_eq!(plans[1].refs, inter(0, 0));
        assert_eq!(plans[2].refs, inter(0, 0b10));
        assert_eq!(plans[3].refs, inter(1, 0));
        assert_eq!(plans[4].refs, inter(0, 0b01));
        assert_eq!(plans[5].refs, inter(0, 0));
        assert_eq!(refs.layers, 3);
    }

    #[test]
    fn four_layers_from_60_fps() {
        let mut refs = References::new(60);
        let plans = run(&mut refs, 8, &[]);
        let layers: Vec<u8> = plans.iter().map(|p| p.layer).collect();
        assert_eq!(layers, [0, 3, 2, 3, 1, 3, 2, 3]);
        let inter = |reference, refresh| FrameRefs::Inter { reference, refresh };
        assert_eq!(plans[2].refs, inter(0, 0b100));
        assert_eq!(plans[3].refs, inter(2, 0));
        assert_eq!(plans[4].refs, inter(0, 0b010));
        assert_eq!(plans[5].refs, inter(1, 0));
        // Layer 2 after a layer 1 predicts from that layer 1.
        assert_eq!(plans[6].refs, inter(1, 0b100));
        assert_eq!(plans[7].refs, inter(2, 0));
    }

    /// Decoding only the frames up to `max_layer` must never need a dropped frame.
    #[test]
    fn every_reference_is_from_a_kept_lower_layer() {
        for fps in [30, 60] {
            let mut refs = References::new(fps);
            let plans = run(&mut refs, 64, &[21]);
            for max_layer in 0..refs.layers {
                // Which frame each slot holds when only layers ≤ max_layer are decoded.
                let mut slots: [Option<usize>; 8] = [None; 8];
                for (index, plan) in plans.iter().enumerate() {
                    if plan.layer > max_layer {
                        continue;
                    }
                    let refresh = match plan.refs {
                        FrameRefs::Key => 0xff,
                        FrameRefs::Inter { reference, refresh } => {
                            let held = slots[usize::from(reference)].expect("slot filled");
                            let ok = plans[held].layer < plan.layer
                                || (plan.layer == 0 && plans[held].layer == 0);
                            assert!(ok, "fps {fps} frame {index}: bad reference {held}");
                            refresh
                        }
                    };
                    for (slot, held) in slots.iter_mut().enumerate() {
                        if refresh & (1 << slot) != 0 {
                            *held = Some(index);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn forced_key_frame_restarts_the_pattern() {
        let mut refs = References::new(30);
        let plans = run(&mut refs, 8, &[3]);
        let layers: Vec<u8> = plans.iter().map(|p| p.layer).collect();
        assert_eq!(layers, [0, 2, 1, 0, 2, 1, 2, 0]);
        assert_eq!(plans[3].refs, FrameRefs::Key);
        assert_eq!(plans[3].order_hint, 0);
        assert_eq!(plans[4].order_hint, 1);
    }

    #[test]
    fn layer_count_changes_at_the_next_layer_0() {
        let mut refs = References::new(30);
        run(&mut refs, 2, &[]);
        refs.set_fps(60);
        let layers: Vec<u8> = run(&mut refs, 10, &[]).iter().map(|p| p.layer).collect();
        assert_eq!(layers, [1, 2, 0, 3, 2, 3, 1, 3, 2, 3]);
        assert_eq!(refs.layers, 4);
    }

    #[test]
    fn reconstruction_never_overwrites_a_held_reference() {
        let mut refs = References::new(60);
        for index in 0..200 {
            let plan = refs.plan(index % 37 == 0);
            assert!(!refs.slots.contains(&Some(plan.recon)), "frame {index}");
            refs.commit(&plan);
        }
    }

    #[test]
    fn reset_makes_the_next_frame_a_key_frame() {
        let mut refs = References::new(30);
        run(&mut refs, 5, &[]);
        refs.reset();
        assert_eq!(refs.plan(false).refs, FrameRefs::Key);
    }

    #[test]
    fn nv12_upload_pads_odd_sizes() {
        // 3x3 frame: Y 0..9, U 100.., V 200..
        let mut data: Vec<u8> = (0..9).collect();
        data.extend([100, 101, 102, 103]);
        data.extend([200, 201, 202, 203]);
        let frame = I420Frame::new(3, 3, data).expect("frame");
        let mut image = vec![0u8; 8 * 6 + 8 * 3];
        i420_to_nv12(&frame, &mut image, [0, 48], [8, 8], (6, 6)).expect("fits");
        assert_eq!(&image[0..6], &[0, 1, 2, 2, 2, 2], "last column repeated");
        assert_eq!(&image[16..22], &[6, 7, 8, 8, 8, 8]);
        assert_eq!(&image[40..46], &[6, 7, 8, 8, 8, 8], "last row repeated");
        assert_eq!(&image[48..54], &[100, 200, 101, 201, 101, 201]);
        assert_eq!(&image[56..62], &[102, 202, 103, 203, 103, 203]);
        assert_eq!(&image[64..70], &[102, 202, 103, 203, 103, 203]);
        assert!(i420_to_nv12(&frame, &mut image, [0, 48], [8, 8], (2, 2)).is_err());
    }
}
