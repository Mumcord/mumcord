//! The plain-Rust side of DirectShow (#631): modes from video headers, the order in which pins
//! and formats are tried, the picture size a virtual camera is asked for, which DirectShow
//! cameras Media Foundation lists already, and the graph events that end a capture. Kept free
//! of COM so it is unit tested on Linux too.

use super::media_type::{api_error, device_key, pixel_format};
use crate::mode::{self, Geometry, Mode, PixelFormat, Rate};
use crate::{CameraError, CaptureRequest};

/// DirectShow counts frame intervals in 100 ns units.
const INTERVALS_PER_SECOND: u32 = 10_000_000;

/// Pin categories (`PIN_CATEGORY_…`) that tell a camera filter's output pins apart.
const PIN_CATEGORY_CAPTURE: u128 = 0xfb6c4281_0353_11d1_905f_0000c0cc16ba;
const PIN_CATEGORY_PREVIEW: u128 = 0xfb6c4282_0353_11d1_905f_0000c0cc16ba;
const PIN_CATEGORY_STILL: u128 = 0xfb6c428a_0353_11d1_905f_0000c0cc16ba;

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

/// The usable modes in the order to try them: [`mode::choose`]'s pick first, then its pick
/// among the rest, and so on. A camera that refuses its best format still gets the next one,
/// for example YUY2 after MJPEG. Modes [`mode::choose`] never picks are left out.
pub fn preference_order(modes: &[Mode], request: &CaptureRequest) -> Vec<usize> {
    let mut left: Vec<usize> = (0..modes.len()).collect();
    let mut order = Vec::with_capacity(modes.len());
    loop {
        let candidates: Vec<Mode> = left.iter().map(|&index| modes[index]).collect();
        let Some(best) = mode::choose(&candidates, request) else {
            return order;
        };
        let Some(at) = candidates.iter().position(|&candidate| candidate == best) else {
            return order;
        };
        order.push(left.remove(at));
    }
}

/// What a camera filter's output pin is for, from its pin category.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PinRole {
    Capture,
    /// The pin names no category, or one of no meaning here.
    Unknown,
    Preview,
    Still,
}

/// The role of a pin whose category is `category` (`None` when the pin does not say).
pub fn pin_role(category: Option<u128>) -> PinRole {
    match category {
        Some(PIN_CATEGORY_CAPTURE) => PinRole::Capture,
        Some(PIN_CATEGORY_PREVIEW) => PinRole::Preview,
        Some(PIN_CATEGORY_STILL) => PinRole::Still,
        _ => PinRole::Unknown,
    }
}

/// The order in which a camera's output pins are tried: capture pins, then pins without a
/// category, then preview pins, each group in the filter's order. Still-image pins are never
/// tried: they deliver single pictures on request, not video.
pub fn pin_order(roles: &[PinRole]) -> Vec<usize> {
    let rank = |role: PinRole| match role {
        PinRole::Capture => Some(0u8),
        PinRole::Unknown => Some(1),
        PinRole::Preview => Some(2),
        PinRole::Still => None,
    };
    let mut order: Vec<(u8, usize)> = roles
        .iter()
        .enumerate()
        .filter_map(|(index, &role)| rank(role).map(|rank| (rank, index)))
        .collect();
    order.sort_unstable();
    order.into_iter().map(|(_, index)| index).collect()
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

/// Which DirectShow cameras Media Foundation does not list already, as one flag per entry of
/// `dshow` (`true`: list it). `dshow` holds each DirectShow camera's name and device path, `mf`
/// each Media Foundation camera's name and symbolic link.
///
/// Real cameras show up in both APIs and carry a device path, so a camera with a path counts as
/// listed only when Media Foundation has the same device; names never decide for it. A camera
/// without a path (a software filter, such as a virtual camera that registered with both APIs)
/// is matched by its name, but only when that is unambiguous: exactly one Media Foundation
/// camera has the name and no device path matched it, and exactly one camera without a path has
/// the name here. Otherwise it stays: a camera listed twice is better than one that is missing.
pub fn only_in_directshow(dshow: &[(&str, Option<&str>)], mf: &[(&str, &str)]) -> Vec<bool> {
    let mf_devices: Vec<String> = mf.iter().map(|&(_, link)| device_part(link)).collect();
    let devices: Vec<Option<String>> = dshow
        .iter()
        .map(|&(_, path)| path.map(device_part))
        .collect();
    let matched: Vec<bool> = mf_devices
        .iter()
        .map(|mf_device| devices.iter().flatten().any(|device| device == mf_device))
        .collect();
    dshow
        .iter()
        .zip(&devices)
        .map(|(&(name, _), device)| match device {
            Some(device) => !mf_devices.contains(device),
            None => {
                let mut same_name = mf
                    .iter()
                    .zip(&matched)
                    .filter(|&(&(mf_name, _), _)| mf_name == name);
                let one_free = matches!(
                    (same_name.next(), same_name.next()),
                    (Some((_, false)), None)
                );
                let twins = dshow
                    .iter()
                    .filter(|&&(other, path)| path.is_none() && other == name)
                    .count();
                !(one_free && twins == 1)
            }
        })
        .collect()
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

    fn mode(format: PixelFormat, width: u32, height: u32, fps: u32) -> Mode {
        Mode {
            format,
            width,
            height,
            rate: Rate::fps(fps),
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
            scaled_size(&mode(PixelFormat::Nv12, 1920, 1080, 30), &request),
            Some((1280, 720))
        );
        assert_eq!(
            scaled_size(&mode(PixelFormat::Yuy2, 1600, 900, 30), &request),
            Some((1280, 720))
        );
        let small = CaptureRequest {
            max_width: 640,
            max_height: 360,
            ..request
        };
        assert_eq!(
            scaled_size(&mode(PixelFormat::Nv12, 1600, 900, 30), &small),
            Some((640, 360))
        );
        // A third of 1920×1080 is 640×360 already.
        assert_eq!(
            scaled_size(&mode(PixelFormat::Nv12, 1920, 1080, 30), &small),
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
                scaled_size(&mode(format, width, height, 30), &request),
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

    /// A webcam as Media Foundation (video camera class) and DirectShow (capture class) name
    /// its device interface.
    const WEBCAM_MF: &str = r"\\?\usb#vid_046d&pid_0825&mi_00#7&2b6ddfab&0&0000#{e5323777-f976-4f5b-9b55-b94699c46e44}\global";
    const WEBCAM_DSHOW: &str = r"\\?\USB#VID_046D&PID_0825&MI_00#7&2B6DDFAB&0&0000#{65e8773d-8f56-11d0-a3b9-00a0c9223196}\global";
    /// Another camera, which DirectShow lists but Media Foundation does not.
    const CARD_DSHOW: &str =
        r"\\?\pci#ven_1cd7&dev_0010#4&1234&0&00e4#{65e8773d-8f56-11d0-a3b9-00a0c9223196}\global";

    #[test]
    fn real_cameras_media_foundation_lists_are_matched_by_device() {
        let mf = [("HD Webcam", WEBCAM_MF)];
        // Same device under another name: listed already. Another device under the same name:
        // kept.
        let dshow = [
            ("Webcam (DirectShow name)", Some(WEBCAM_DSHOW)),
            ("HD Webcam", Some(CARD_DSHOW)),
        ];
        assert_eq!(only_in_directshow(&dshow, &mf), [false, true]);
    }

    #[test]
    fn two_cameras_named_alike_both_stay() {
        // Three "USB Camera"s: the one Media Foundation has is matched by its device, so neither
        // a second real one nor a software filter of that name disappears.
        let mf = [("USB Camera", WEBCAM_MF)];
        let dshow = [
            ("USB Camera", Some(WEBCAM_DSHOW)),
            ("USB Camera", Some(CARD_DSHOW)),
            ("USB Camera", None),
        ];
        assert_eq!(only_in_directshow(&dshow, &mf), [false, true, true]);
    }

    #[test]
    fn a_virtual_camera_registered_with_both_apis_shows_once() {
        let mf = [
            ("HD Webcam", WEBCAM_MF),
            (
                "OBS Virtual Camera",
                r"\\?\SWD#VCAMDEVAPI#{2b6f1a87-6cd3-4f5c-8a4b-1f0e7a6a2f10}#{e5323777-f976-4f5b-9b55-b94699c46e44}\{}",
            ),
        ];
        let dshow = [
            ("HD Webcam", Some(WEBCAM_DSHOW)),
            ("OBS Virtual Camera", None),
        ];
        assert_eq!(only_in_directshow(&dshow, &mf), [false, false]);
    }

    #[test]
    fn names_decide_only_when_unambiguous() {
        // Two Media Foundation cameras share the name: which one would it be? Kept.
        let mf = [("Virtual Cam", "mf-a"), ("Virtual Cam", "mf-b")];
        assert_eq!(only_in_directshow(&[("Virtual Cam", None)], &mf), [true]);
        // Two path-less DirectShow cameras share the name of one Media Foundation camera.
        let mf = [("Virtual Cam", "mf-a")];
        let dshow = [("Virtual Cam", None), ("Virtual Cam", None)];
        assert_eq!(only_in_directshow(&dshow, &mf), [true, true]);
        // Only DirectShow knows it: kept.
        let mf = [("HD Webcam", WEBCAM_MF)];
        assert_eq!(
            only_in_directshow(&[("OBS Virtual Camera", None)], &mf),
            [true]
        );
        assert_eq!(
            only_in_directshow(&[("OBS Virtual Camera", None)], &[]),
            [true]
        );
    }

    #[test]
    fn formats_are_tried_best_first_and_all_usable_ones_come_after() {
        let request = CaptureRequest::default();
        let modes = [
            mode(PixelFormat::Yuy2, 1280, 720, 10),
            mode(PixelFormat::Mjpeg, 1280, 720, 30),
            // Too small to give a frame: never tried.
            mode(PixelFormat::Nv12, 1, 1, 30),
            mode(PixelFormat::Yuy2, 640, 480, 30),
        ];
        // MJPEG reaches 720p30 where YUY2 does not; after it, the smooth YUY2 mode wins over the
        // bigger slow one.
        assert_eq!(preference_order(&modes, &request), [1, 3, 0]);
        // The same mode listed twice (e.g. once per video header kind) is tried twice.
        let twice = [mode(PixelFormat::Nv12, 1280, 720, 30); 2];
        assert_eq!(preference_order(&twice, &request), [0, 1]);
        assert!(preference_order(&[], &request).is_empty());
    }

    #[test]
    fn capture_pins_come_first_and_still_pins_never() {
        let roles = [
            pin_role(Some(PIN_CATEGORY_PREVIEW)),
            pin_role(Some(PIN_CATEGORY_STILL)),
            pin_role(None),
            pin_role(Some(PIN_CATEGORY_CAPTURE)),
            // A category of no meaning here: closed captions.
            pin_role(Some(0xfb6c4289_0353_11d1_905f_0000c0cc16ba)),
        ];
        assert_eq!(
            roles,
            [
                PinRole::Preview,
                PinRole::Still,
                PinRole::Unknown,
                PinRole::Capture,
                PinRole::Unknown
            ]
        );
        assert_eq!(pin_order(&roles), [3, 2, 4, 0]);
        assert!(pin_order(&[PinRole::Still]).is_empty());
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

    /// The values above are the SDK's: the event codes, the pin categories, and the subtype GUIDs
    /// DirectShow and Media Foundation share.
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
        assert_eq!(PIN_CATEGORY_CAPTURE, mf::PIN_CATEGORY_CAPTURE.to_u128());
        assert_eq!(PIN_CATEGORY_PREVIEW, mf::PIN_CATEGORY_PREVIEW.to_u128());
        assert_eq!(PIN_CATEGORY_STILL, mf::PIN_CATEGORY_STILL.to_u128());
    }
}
