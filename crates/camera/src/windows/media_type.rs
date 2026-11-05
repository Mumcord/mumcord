//! The plain-Rust side of Media Foundation: reading media type attributes, laying out locked
//! buffers and turning HRESULTs into [`CameraError`]s. Kept free of COM so it is unit tested
//! on every platform.

use crate::CameraError;
use crate::mode::{Mode, PixelFormat, Rate};
use crate::pump::Raw;

/// `MFVideoFormat_NV12`, `MFVideoFormat_YUY2` and `MFVideoFormat_MJPG`: FourCC subtypes.
const SUBTYPE_NV12: u128 = 0x3231564e_0000_0010_8000_00aa00389b71;
const SUBTYPE_YUY2: u128 = 0x32595559_0000_0010_8000_00aa00389b71;
const SUBTYPE_MJPG: u128 = 0x47504a4d_0000_0010_8000_00aa00389b71;

/// `E_ACCESSDENIED`: the camera privacy setting blocks this app (or all desktop apps).
const E_ACCESSDENIED: u32 = 0x8007_0005;
/// `MF_E_VIDEO_RECORDING_DEVICE_INVALIDATED`: the camera went away while capturing.
const MF_E_VIDEO_RECORDING_DEVICE_INVALIDATED: u32 = 0xC00D_3EA2;
/// `HRESULT_FROM_WIN32(ERROR_DEVICE_REMOVED)`, `ERROR_DEVICE_NOT_CONNECTED` and
/// `ERROR_FILE_NOT_FOUND` (the symbolic link no longer resolves).
const DEVICE_REMOVED: u32 = 0x8007_0651;
const DEVICE_NOT_CONNECTED: u32 = 0x8007_048F;
const FILE_NOT_FOUND: u32 = 0x8007_0002;
/// `MF_E_VIDEO_RECORDING_DEVICE_PREEMPTED` and `MF_E_HW_MFT_FAILED_START_STREAMING`: another
/// app holds the camera.
const DEVICE_PREEMPTED: u32 = 0xC00D_3EA3;
const FAILED_START_STREAMING: u32 = 0xC00D_3704;

/// The pixel format of a media subtype, if Mumcord reads it.
pub fn pixel_format(subtype: u128) -> Option<PixelFormat> {
    match subtype {
        SUBTYPE_NV12 => Some(PixelFormat::Nv12),
        SUBTYPE_YUY2 => Some(PixelFormat::Yuy2),
        SUBTYPE_MJPG => Some(PixelFormat::Mjpeg),
        _ => None,
    }
}

/// `MF_MT_FRAME_SIZE` and `MF_MT_FRAME_RATE` pack two `u32`s into one `u64`, the first value in
/// the high half.
fn unpack(value: u64) -> (u32, u32) {
    ((value >> 32) as u32, value as u32)
}

/// The [`Mode`] of one native media type, from its subtype, `MF_MT_FRAME_SIZE` and
/// `MF_MT_FRAME_RATE`. `None` for formats Mumcord does not read and for nonsense sizes or rates.
pub fn mode(subtype: u128, frame_size: u64, frame_rate: u64) -> Option<Mode> {
    let format = pixel_format(subtype)?;
    let (width, height) = unpack(frame_size);
    let (num, den) = unpack(frame_rate);
    if width == 0 || height == 0 || num == 0 || den == 0 {
        return None;
    }
    Some(Mode {
        format,
        width,
        height,
        rate: Rate { num, den },
    })
}

/// Bytes per row of an uncompressed picture without padding.
pub fn tight_stride(format: PixelFormat, width: u32) -> usize {
    match format {
        PixelFormat::Yuy2 => width.div_ceil(2) as usize * 4,
        PixelFormat::Nv12 | PixelFormat::Mjpeg => width as usize,
    }
}

/// Splits a locked buffer into the planes of one picture. `stride` is the pitch of the first
/// plane; an NV12 chroma plane follows the `height` luma rows at the same pitch, which is how
/// Media Foundation lays out both 2D and contiguous NV12 buffers. MJPEG passes the bytes on.
pub fn raw(format: PixelFormat, data: &[u8], stride: usize, height: u32) -> Option<Raw<'_>> {
    match format {
        PixelFormat::Mjpeg => Some(Raw::Packed { data, stride: 0 }),
        PixelFormat::Yuy2 => Some(Raw::Packed { data, stride }),
        PixelFormat::Nv12 => {
            let luma = stride.checked_mul(height as usize)?;
            if luma > data.len() {
                return None;
            }
            let (y, uv) = data.split_at(luma);
            Some(Raw::Nv12 {
                y,
                y_stride: stride,
                uv,
                uv_stride: stride,
            })
        }
    }
}

/// Maps a failed Media Foundation call to what the UI shows (FR-8.25). The text of other
/// errors carries only the code, never device names or ids (FR-8.28).
pub fn camera_error(hresult: i32) -> CameraError {
    match hresult as u32 {
        E_ACCESSDENIED => CameraError::PermissionDenied,
        MF_E_VIDEO_RECORDING_DEVICE_INVALIDATED
        | DEVICE_REMOVED
        | DEVICE_NOT_CONNECTED
        | FILE_NOT_FOUND => CameraError::DeviceGone,
        DEVICE_PREEMPTED | FAILED_START_STREAMING => {
            CameraError::Backend("another app is using the camera".to_owned())
        }
        code => CameraError::Backend(format!("Media Foundation error 0x{code:08X}")),
    }
}

/// The stable key of a camera for [`crate::DeviceId::from_os_key`]: its symbolic link, which
/// Windows keeps for the same camera on the same port. Lower-cased because Windows APIs differ
/// in the case they report it in.
pub fn device_key(symbolic_link: &str) -> String {
    symbolic_link.to_lowercase()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    const fn pack(high: u32, low: u32) -> u64 {
        ((high as u64) << 32) | low as u64
    }

    #[test]
    fn media_types_become_modes() {
        let yuy2 = mode(SUBTYPE_YUY2, pack(1280, 720), pack(30, 1)).unwrap();
        assert_eq!(
            yuy2,
            Mode {
                format: PixelFormat::Yuy2,
                width: 1280,
                height: 720,
                rate: Rate::fps(30)
            }
        );
        let mjpeg = mode(SUBTYPE_MJPG, pack(640, 480), pack(30000, 1001)).unwrap();
        assert_eq!(
            (mjpeg.format, mjpeg.rate.rounded()),
            (PixelFormat::Mjpeg, 30)
        );
        assert_eq!(
            mode(SUBTYPE_NV12, pack(1920, 1080), pack(15, 1))
                .unwrap()
                .format,
            PixelFormat::Nv12
        );
    }

    #[test]
    fn unknown_formats_and_empty_values_are_skipped() {
        // MFVideoFormat_RGB24 and H.264: not read by Mumcord.
        const RGB24: u128 = 0x00000014_0000_0010_8000_00aa00389b71;
        const H264: u128 = 0x34363248_0000_0010_8000_00aa00389b71;
        assert_eq!(mode(RGB24, pack(1280, 720), pack(30, 1)), None);
        assert_eq!(mode(H264, pack(1280, 720), pack(30, 1)), None);
        assert_eq!(mode(SUBTYPE_YUY2, pack(0, 720), pack(30, 1)), None);
        assert_eq!(mode(SUBTYPE_YUY2, pack(1280, 720), pack(30, 0)), None);
    }

    #[test]
    fn a_usb2_camera_picks_mjpeg_for_720p30() {
        // What a typical USB 2 webcam lists: YUY2 720p only at 10 fps, MJPEG at 30.
        let types = [
            (SUBTYPE_YUY2, pack(640, 480), pack(30, 1)),
            (SUBTYPE_YUY2, pack(1280, 720), pack(10, 1)),
            (SUBTYPE_MJPG, pack(1280, 720), pack(30, 1)),
            (SUBTYPE_MJPG, pack(1920, 1080), pack(30, 1)),
        ];
        let modes: Vec<Mode> = types
            .iter()
            .filter_map(|&(subtype, size, rate)| mode(subtype, size, rate))
            .collect();
        let chosen = crate::mode::choose(&modes, &crate::CaptureRequest::default()).unwrap();
        assert_eq!(chosen, modes[2]);
    }

    #[test]
    fn nv12_is_taken_directly_when_offered() {
        let types = [
            (SUBTYPE_MJPG, pack(1280, 720), pack(30, 1)),
            (SUBTYPE_YUY2, pack(1280, 720), pack(30, 1)),
            (SUBTYPE_NV12, pack(1280, 720), pack(30, 1)),
        ];
        let modes: Vec<Mode> = types
            .iter()
            .filter_map(|&(subtype, size, rate)| mode(subtype, size, rate))
            .collect();
        let chosen = crate::mode::choose(&modes, &crate::CaptureRequest::default()).unwrap();
        assert_eq!(chosen.format, PixelFormat::Nv12);
    }

    #[test]
    fn nv12_buffers_split_into_planes_at_the_pitch() {
        let data = [0u8; 16 * 4 + 16 * 2];
        let Some(Raw::Nv12 {
            y,
            y_stride,
            uv,
            uv_stride,
        }) = raw(PixelFormat::Nv12, &data, 16, 4)
        else {
            panic!("NV12 planes expected");
        };
        assert_eq!((y.len(), y_stride, uv.len(), uv_stride), (64, 16, 32, 16));
        assert!(raw(PixelFormat::Nv12, &data[..60], 16, 4).is_none());
        assert!(matches!(
            raw(PixelFormat::Yuy2, &data, 32, 4),
            Some(Raw::Packed { stride: 32, .. })
        ));
        assert_eq!(tight_stride(PixelFormat::Yuy2, 1280), 2560);
        assert_eq!(tight_stride(PixelFormat::Nv12, 1280), 1280);
    }

    #[test]
    fn privacy_refusal_and_unplugging_map_to_their_errors() {
        assert_eq!(
            camera_error(E_ACCESSDENIED as i32),
            CameraError::PermissionDenied
        );
        assert_eq!(
            camera_error(MF_E_VIDEO_RECORDING_DEVICE_INVALIDATED as i32),
            CameraError::DeviceGone
        );
        assert_eq!(camera_error(DEVICE_REMOVED as i32), CameraError::DeviceGone);
        assert_eq!(
            camera_error(0x8000_4005_u32 as i32),
            CameraError::Backend("Media Foundation error 0x80004005".to_owned())
        );
    }

    #[test]
    fn device_keys_ignore_case() {
        assert_eq!(
            device_key(r"\\?\USB#VID_046D&PID_0825#ABC#{e5323777-f976-4f5b-9b55-b94699c46e44}"),
            device_key(r"\\?\usb#vid_046d&pid_0825#abc#{E5323777-F976-4F5B-9B55-B94699C46E44}")
        );
    }
}
