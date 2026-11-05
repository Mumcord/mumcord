//! The own camera's picture for the preview dialog, the Settings preview and the self-view
//! (FR-8.11, FR-8.30): the frame after the background effect, scaled down to RGBA.
//!
//! The encode thread hands frames over with [`PreviewTap::offer`] (real-time path: buffers are
//! allocated up front and passed through lock-free queues); a delivery thread passes each
//! finished picture to the app's [`PreviewSink`]. Nothing is converted while no sink is set.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError, Weak};
use std::thread::{self, Thread};
use std::time::Duration;

use crossbeam_queue::ArrayQueue;
use mumcord_camera::CameraFrame;

/// Largest preview picture; the dialog and the tiles show the camera smaller than this.
pub const MAX_WIDTH: u32 = 640;
pub const MAX_HEIGHT: u32 = 360;
/// Pictures in flight between the encode thread and the delivery thread.
const PICTURES: usize = 3;
/// How long the delivery thread sleeps when it is not woken.
const IDLE_WAIT: Duration = Duration::from_millis(200);

/// One preview picture: packed, opaque RGBA, `width × height × 4` bytes.
pub struct PreviewPicture<'a> {
    pub width: u32,
    pub height: u32,
    pub rgba: &'a [u8],
}

/// Receives the preview pictures on the delivery thread (not a real-time thread).
pub type PreviewSink = Box<dyn FnMut(PreviewPicture<'_>) + Send>;

struct Picture {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

/// Shared by every camera session of the engine; one sink at a time.
pub(crate) struct PreviewTap {
    active: AtomicBool,
    free: ArrayQueue<Picture>,
    ready: ArrayQueue<Picture>,
    sink: Mutex<Option<PreviewSink>>,
    delivery: OnceLock<Thread>,
}

impl Default for PreviewTap {
    fn default() -> Self {
        let free = ArrayQueue::new(PICTURES);
        for _ in 0..PICTURES {
            let rgba = Vec::with_capacity((MAX_WIDTH * MAX_HEIGHT * 4) as usize);
            // Cannot fail: the queue holds exactly PICTURES.
            let _ = free.push(Picture {
                width: 0,
                height: 0,
                rgba,
            });
        }
        Self {
            active: AtomicBool::new(false),
            free,
            ready: ArrayQueue::new(PICTURES),
            sink: Mutex::new(None),
            delivery: OnceLock::new(),
        }
    }
}

impl PreviewTap {
    /// Sends the pictures to `sink` from now on (replacing an earlier one), or stops with `None`.
    pub(crate) fn set_sink(self: &Arc<Self>, sink: Option<PreviewSink>) {
        let active = sink.is_some();
        *self.sink.lock().unwrap_or_else(PoisonError::into_inner) = sink;
        self.active.store(active, Ordering::Relaxed);
        if active && self.delivery.get().is_none() {
            let weak = Arc::downgrade(self);
            match thread::Builder::new()
                .name("camera-preview".into())
                .spawn(move || deliver(&weak))
            {
                Ok(handle) => {
                    let _ = self.delivery.set(handle.thread().clone());
                }
                Err(e) => tracing::warn!(error = %e, "camera preview thread not started"),
            }
        }
    }

    /// Converts `frame` for the preview when a sink is set and a buffer is free; otherwise the
    /// frame is skipped. Runs on the encode thread: no allocation, lock, I/O or log.
    pub(crate) fn offer(&self, frame: &CameraFrame) {
        if !self.active.load(Ordering::Relaxed) {
            return;
        }
        let Some(mut picture) = self.free.pop() else {
            return;
        };
        let (width, height) = preview_size(frame.width, frame.height);
        picture.width = width;
        picture.height = height;
        nv12_to_rgba(
            &frame.data,
            (frame.width, frame.height),
            &mut picture.rgba,
            (width, height),
        );
        if let Err(picture) = self.ready.push(picture) {
            let _ = self.free.push(picture);
        }
        if let Some(delivery) = self.delivery.get() {
            delivery.unpark();
        }
    }
}

fn deliver(tap: &Weak<PreviewTap>) {
    loop {
        let Some(tap) = tap.upgrade() else {
            return;
        };
        while let Some(picture) = tap.ready.pop() {
            if let Some(sink) = tap
                .sink
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .as_mut()
            {
                sink(PreviewPicture {
                    width: picture.width,
                    height: picture.height,
                    rgba: &picture.rgba,
                });
            }
            let _ = tap.free.push(picture);
        }
        drop(tap);
        thread::park_timeout(IDLE_WAIT);
    }
}

/// The preview size for a `width × height` camera picture: the same aspect, at most
/// [`MAX_WIDTH`] × [`MAX_HEIGHT`], never larger than the picture, even sides.
pub fn preview_size(width: u32, height: u32) -> (u32, u32) {
    if width == 0 || height == 0 {
        return (0, 0);
    }
    let scale = (f64::from(MAX_WIDTH) / f64::from(width))
        .min(f64::from(MAX_HEIGHT) / f64::from(height))
        .min(1.0);
    let even = |v: f64| ((v.round() as u32) & !1).max(2);
    (
        even(f64::from(width) * scale),
        even(f64::from(height) * scale),
    )
}

/// Scales an NV12 picture to `dst_size` (nearest neighbour) as opaque RGBA, BT.601 limited
/// range like the rest of the camera pipeline. `dst` keeps its capacity: no allocation as long
/// as `dst_size` fits [`MAX_WIDTH`] × [`MAX_HEIGHT`].
pub fn nv12_to_rgba(src: &[u8], src_size: (u32, u32), dst: &mut Vec<u8>, dst_size: (u32, u32)) {
    let (sw, sh) = (src_size.0 as usize, src_size.1 as usize);
    let (dw, dh) = (dst_size.0 as usize, dst_size.1 as usize);
    dst.clear();
    dst.resize(dw * dh * 4, 0);
    if sw == 0 || sh == 0 || dw == 0 || dh == 0 || src.len() < sw * sh + sw * sh / 2 {
        return;
    }
    let (luma, chroma) = src.split_at(sw * sh);
    for (row, out_row) in dst.chunks_exact_mut(dw * 4).enumerate() {
        let sy = row * sh / dh;
        let chroma_row = sy / 2 * sw;
        for (col, px) in out_row.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let sx = col * sw / dw;
            let uv = chroma_row + sx / 2 * 2;
            let c = i32::from(luma[sy * sw + sx]) - 16;
            let d = i32::from(chroma[uv]) - 128;
            let e = i32::from(chroma[uv + 1]) - 128;
            px[0] = clamp((298 * c + 409 * e + 128) >> 8);
            px[1] = clamp((298 * c - 100 * d - 208 * e + 128) >> 8);
            px[2] = clamp((298 * c + 516 * d + 128) >> 8);
            px[3] = u8::MAX;
        }
    }
}

fn clamp(value: i32) -> u8 {
    value.clamp(0, 255) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_keeps_the_aspect_and_never_grows() {
        assert_eq!(preview_size(1280, 720), (640, 360));
        assert_eq!(preview_size(1920, 1080), (640, 360));
        assert_eq!(preview_size(640, 480), (480, 360));
        assert_eq!(preview_size(320, 240), (320, 240));
        assert_eq!(preview_size(0, 720), (0, 0));
    }

    #[test]
    fn grey_nv12_becomes_grey_rgba_at_the_preview_size() {
        let (w, h) = (8, 4);
        let mut src = vec![126_u8; w * h];
        src.extend(std::iter::repeat_n(128_u8, w * h / 2));
        let mut dst = Vec::with_capacity(4 * 2 * 4);
        let capacity = dst.capacity();
        nv12_to_rgba(&src, (8, 4), &mut dst, (4, 2));
        assert_eq!(dst.len(), 4 * 2 * 4);
        assert_eq!(dst.capacity(), capacity);
        assert!(
            dst.as_chunks::<4>()
                .0
                .iter()
                .all(|px| *px == [128, 128, 128, 255])
        );
    }

    #[test]
    fn limited_range_maps_to_full_range() {
        let mut src = vec![16_u8, 16, 235, 235];
        src.extend([128, 128]);
        let mut dst = Vec::new();
        nv12_to_rgba(&src, (2, 2), &mut dst, (2, 2));
        assert_eq!(&dst[..4], [0, 0, 0, 255]);
        assert_eq!(&dst[8..12], [255, 255, 255, 255]);
    }

    #[test]
    fn a_short_source_gives_a_black_picture_instead_of_a_panic() {
        let mut dst = Vec::new();
        nv12_to_rgba(&[0; 3], (8, 4), &mut dst, (4, 2));
        assert_eq!(dst.len(), 32);
    }
}
