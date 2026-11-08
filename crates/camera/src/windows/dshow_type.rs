//! The plain-Rust side of DirectShow (#631): modes from video headers, the picture size a
//! virtual camera is asked for, which DirectShow cameras Media Foundation lists already, and
//! the graph events that end a capture. Kept free of COM so it is unit tested on Linux too.

use super::media_type::{api_error, device_key, pixel_format};
use crate::mode::{Geometry, Mode, PixelFormat, Rate};
use crate::{CameraError, CaptureRequest};

/// DirectShow counts frame intervals in 100 ns units.
const INTERVALS_PER_SECOND: u32 = 10_000_000;

/// Graph events (`EC_…`) that end a capture: the stream ended, an error stopped the graph, or
/// the device went away.
const EC_COMPLETE: i32 = 0x01;
const EC_ERRORABORT: i32 = 0x03;
const EC_STREAM_ERROR_STOPPED: i32 = 0x06;
const EC_DEVICE_LOST: i32 = 0x1F;
const EC_ERRORABORTEX: i32 = 0x45;

/// The [`Mode`] of one video media type, from its subtype and the `biWidth`, `biHeight` and
/// frame interval of its video header. `None` for formats Mumcord does not read and for
/// nonsense sizes or intervals. YUV pictures are top-down whatever the sign of `biHeight`, so
/// only its size counts.
pub fn video_mode(subtype: u128, width: i32, height: i32, interval: i64) -> Option<Mode> {
    let format = pixel_format(subtype)?;
    let width = u32::try_from(width).ok().filter(|&width| width > 0)?;
    let height = Some(height.unsigned_abs()).filter(|&height| height > 0)?;
    let den = u32::try_from(interval).ok().filter(|&den| den > 0)?;
    Some(Mode {
        format,
        width,
        height,
        rate: Rate {
            num: INTERVALS_PER_SECOND,
            den,
        },
    })
}

/// The size to ask a DirectShow camera for instead of `mode`'s own, or `None` to keep it.
///
/// Virtual cameras such as OBS's list only their output size but scale to any size they are
/// asked for. A 16:9 picture that does not shrink to the requested size by a whole factor
/// would otherwise arrive smaller than it could (1920×1080 becomes 960×540, a size the
/// background effects can't use), so such a camera is asked for the requested size itself.
/// MJPEG and other aspect ratios keep their size: a scaler would stretch them.
pub fn scaled_size(mode: &Mode, request: &CaptureRequest) -> Option<(u32, u32)> {
    if mode.format == PixelFormat::Mjpeg || u64::from(mode.width) * 9 != u64::from(mode.height) * 16
    {
        return None;
    }
    let wanted = Geometry::new(
        request.max_width,
        request.max_height,
        request.max_width,
        request.max_height,
    )?;
    let shrunk = Geometry::new(
        mode.width,
        mode.height,
        request.max_width,
        request.max_height,
    )?;
    (mode.width > wanted.width && shrunk.width < wanted.width)
        .then_some((wanted.width, wanted.height))
}

/// Bytes of one uncompressed picture (`biSizeImage`); `None` for MJPEG.
pub fn image_size(format: PixelFormat, width: u32, height: u32) -> Option<u32> {
    let pixels = width.checked_mul(height)?;
    match format {
        PixelFormat::Nv12 => pixels.checked_mul(3).map(|bytes| bytes / 2),
        PixelFormat::Yuy2 => pixels.checked_mul(2),
        PixelFormat::Mjpeg => None,
    }
}

/// The device part of a device interface path: everything before the interface class
/// (`#{…}`), lower-cased. Media Foundation and DirectShow name the same camera through
/// different interface classes, so only this part matches.
fn device_part(path: &str) -> String {
    let path = device_key(path);
    match path.rfind("#{") {
        Some(class) => path[..class].to_owned(),
        None => path,
    }
}

/// Whether Media Foundation lists this DirectShow camera already (`mf` holds its cameras' names
/// and symbolic links): real cameras show up in both with the same device, and a virtual camera
/// registered with both APIs has the same name.
pub fn listed_by_mf<'a>(
    name: &str,
    device_path: Option<&str>,
    mf: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> bool {
    let device = device_path.map(device_part);
    mf.into_iter().any(|(mf_name, symbolic_link)| {
        mf_name == name || device.as_deref() == Some(device_part(symbolic_link).as_str())
    })
}

/// Maps a failed DirectShow call to what the UI shows; see [`api_error`].
pub fn dshow_error(hresult: i32) -> CameraError {
    api_error("DirectShow", hresult)
}

/// Why a graph event ends the capture, or `None` when it does not. `EC_DEVICE_LOST` also
/// reports a device coming back (second parameter 1), which ends nothing.
pub fn event_end(code: i32, param1: isize, param2: isize) -> Option<CameraError> {
    match code {
        EC_DEVICE_LOST if param2 == 0 => Some(CameraError::DeviceGone),
        EC_COMPLETE => Some(CameraError::DeviceGone),
        // The first parameter carries the HRESULT; only its low 32 bits are meaningful.
        EC_ERRORABORT | EC_ERRORABORTEX | EC_STREAM_ERROR_STOPPED => {
            Some(dshow_error(param1 as i32))
        }
        _ => None,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    const NV12: u128 = 0x3231564e_0000_0010_8000_00aa00389b71;
    const YUY2: u128 = 0x32595559_0000_0010_8000_00aa00389b71;
    const MJPG: u128 = 0x47504a4d_0000_0010_8000_00aa00389b71;
    const RGB24: u128 = 0xe436eb7d_524f_11ce_9f53_0020af0ba770;
    /// 30 fps as OBS lists it.
    const OBS_INTERVAL: i64 = 333_333;

    fn mode(format: PixelFormat, width: u32, height: u32) -> Mode {
        Mode {
            format,
            width,
            height,
            rate: Rate::fps(30),
        }
    }

    #[test]
    fn video_headers_become_modes() {
        let obs = video_mode(NV12, 1920, 1080, OBS_INTERVAL).unwrap();
        assert_eq!(
            (obs.format, obs.width, obs.height, obs.rate.rounded()),
            (PixelFormat::Nv12, 1920, 1080, 30)
        );
        assert_eq!(
            video_mode(YUY2, 1280, 720, 166_667).unwrap().rate.rounded(),
            60
        );
        assert_eq!(
            video_mode(MJPG, 640, 480, OBS_INTERVAL).unwrap().format,
            PixelFormat::Mjpeg
        );
        // A negative height still describes a top-down YUV picture.
        assert_eq!(
            video_mode(YUY2, 640, -480, OBS_INTERVAL).unwrap().height,
            480
        );
    }

    #[test]
    fn unread_formats_and_nonsense_headers_are_skipped() {
        assert_eq!(video_mode(RGB24, 1280, 720, OBS_INTERVAL), None);
        assert_eq!(video_mode(NV12, 0, 720, OBS_INTERVAL), None);
        assert_eq!(video_mode(NV12, -1280, 720, OBS_INTERVAL), None);
        assert_eq!(video_mode(NV12, 1280, 0, OBS_INTERVAL), None);
        assert_eq!(video_mode(NV12, 1280, 720, 0), None);
        assert_eq!(video_mode(NV12, 1280, 720, i64::MAX), None);
    }

    #[test]
    fn obs_at_1080p_is_asked_for_720p() {
        let request = CaptureRequest::default();
        assert_eq!(
            scaled_size(&mode(PixelFormat::Nv12, 1920, 1080), &request),
            Some((1280, 720))
        );
        assert_eq!(
            scaled_size(&mode(PixelFormat::Yuy2, 1600, 900), &request),
            Some((1280, 720))
        );
        let small = CaptureRequest {
            max_width: 640,
            max_height: 360,
            ..request
        };
        assert_eq!(
            scaled_size(&mode(PixelFormat::Nv12, 1600, 900), &small),
            Some((640, 360))
        );
        // A third of 1920×1080 is 640×360 already.
        assert_eq!(
            scaled_size(&mode(PixelFormat::Nv12, 1920, 1080), &small),
            None
        );
    }

    #[test]
    fn sizes_that_shrink_well_or_would_stretch_keep_their_size() {
        let request = CaptureRequest::default();
        for (format, width, height) in [
            // Already the requested size, or a whole multiple of it.
            (PixelFormat::Nv12, 1280, 720),
            (PixelFormat::Nv12, 2560, 1440),
            (PixelFormat::Nv12, 3840, 2160),
            // Smaller than requested: never scaled up.
            (PixelFormat::Nv12, 640, 360),
            // Not 16:9: a scaler would stretch the picture.
            (PixelFormat::Nv12, 1440, 1080),
            (PixelFormat::Nv12, 1920, 1200),
            // Compressed.
            (PixelFormat::Mjpeg, 1920, 1080),
        ] {
            assert_eq!(
                scaled_size(&mode(format, width, height), &request),
                None,
                "{format:?} {width}x{height}"
            );
        }
    }

    #[test]
    fn image_sizes_follow_the_pixel_format() {
        assert_eq!(image_size(PixelFormat::Nv12, 1280, 720), Some(1_382_400));
        assert_eq!(image_size(PixelFormat::Yuy2, 1280, 720), Some(1_843_200));
        assert_eq!(image_size(PixelFormat::Mjpeg, 1280, 720), None);
        assert_eq!(image_size(PixelFormat::Yuy2, u32::MAX, 2), None);
    }

    #[test]
    fn real_cameras_media_foundation_lists_are_skipped() {
        // The same webcam as Media Foundation (video camera class) and DirectShow (capture
        // class) name it.
        let mf = [(
            "HD Webcam",
            r"\\?\usb#vid_046d&pid_0825&mi_00#7&2b6ddfab&0&0000#{e5323777-f976-4f5b-9b55-b94699c46e44}\global",
        )];
        let dshow = r"\\?\USB#VID_046D&PID_0825&MI_00#7&2B6DDFAB&0&0000#{65e8773d-8f56-11d0-a3b9-00a0c9223196}\global";
        assert!(listed_by_mf("Webcam (DirectShow name)", Some(dshow), mf));
        // A virtual camera registered with both APIs under one name.
        assert!(listed_by_mf("HD Webcam", None, mf));
    }

    #[test]
    fn virtual_cameras_only_directshow_lists_are_kept() {
        let mf = [(
            "HD Webcam",
            r"\\?\usb#vid_046d&pid_0825&mi_00#7&2b6ddfab&0&0000#{e5323777-f976-4f5b-9b55-b94699c46e44}\global",
        )];
        assert!(!listed_by_mf("OBS Virtual Camera", None, mf));
        assert!(!listed_by_mf(
            "Capture card",
            Some(
                r"\\?\pci#ven_1cd7&dev_0010#4&1234&0&00e4#{65e8773d-8f56-11d0-a3b9-00a0c9223196}\global"
            ),
            mf
        ));
        assert!(!listed_by_mf("OBS Virtual Camera", None, []));
    }

    #[test]
    fn graph_events_that_end_the_capture() {
        assert_eq!(
            event_end(EC_DEVICE_LOST, 0, 0),
            Some(CameraError::DeviceGone)
        );
        assert_eq!(
            event_end(EC_DEVICE_LOST, 0, 1),
            None,
            "the device came back"
        );
        assert_eq!(event_end(EC_COMPLETE, 0, 0), Some(CameraError::DeviceGone));
        assert_eq!(
            event_end(EC_ERRORABORT, 0x8007_0005_u32 as i32 as isize, 0),
            Some(CameraError::PermissionDenied)
        );
        assert_eq!(
            event_end(EC_ERRORABORTEX, 0x8000_4005_u32 as i32 as isize, 0),
            Some(CameraError::Backend(
                "DirectShow error 0x80004005".to_owned()
            ))
        );
        assert_eq!(
            event_end(EC_STREAM_ERROR_STOPPED, 0x8007_0651_u32 as i32 as isize, 0),
            Some(CameraError::DeviceGone)
        );
        // EC_REPAINT and the like change nothing.
        assert_eq!(event_end(0x05, 0, 0), None);
    }

    /// The values above are the SDK's: the event codes, and the subtype GUIDs DirectShow and
    /// Media Foundation share.
    #[cfg(windows)]
    #[test]
    fn constants_match_the_windows_sdk() {
        use ::windows::Win32::Media::DirectShow as ds;
        use ::windows::Win32::Media::MediaFoundation as mf;
        assert_eq!(EC_COMPLETE, ds::EC_COMPLETE as i32);
        assert_eq!(EC_ERRORABORT, ds::EC_ERRORABORT as i32);
        assert_eq!(EC_STREAM_ERROR_STOPPED, ds::EC_STREAM_ERROR_STOPPED as i32);
        assert_eq!(EC_DEVICE_LOST, ds::EC_DEVICE_LOST as i32);
        assert_eq!(EC_ERRORABORTEX, ds::EC_ERRORABORTEX as i32);
        assert_eq!(NV12, mf::MEDIASUBTYPE_NV12.to_u128());
        assert_eq!(YUY2, mf::MEDIASUBTYPE_YUY2.to_u128());
        assert_eq!(MJPG, mf::MEDIASUBTYPE_MJPG.to_u128());
        assert_eq!(RGB24, mf::MEDIASUBTYPE_RGB24.to_u128());
    }
}
