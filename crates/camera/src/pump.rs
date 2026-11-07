//! Turns the camera pictures of one session into [`CameraFrame`]s for the sink. Shared by the
//! Linux portal and V4L2 paths and by Windows Media Foundation.

use std::time::Instant;

use crate::convert::{Picture, to_nv12};
use crate::mjpeg::{MjpegDecoder, UnreadableRun};
use crate::mode::{Geometry, Mode, PixelFormat};
use crate::{CameraError, CameraFrame, FramePool, FrameSink, POOL_BUFFERS, nv12_len};

/// One camera picture as the OS hands it over.
pub enum Raw<'a> {
    /// YUY2 rows or one MJPEG frame (stride unused).
    Packed { data: &'a [u8], stride: usize },
    Nv12 {
        y: &'a [u8],
        y_stride: usize,
        uv: &'a [u8],
        uv_stride: usize,
    },
}

pub struct Pump {
    mode: Mode,
    geometry: Geometry,
    pool: FramePool,
    mjpeg: Option<MjpegDecoder>,
    unreadable: UnreadableRun,
    start: Instant,
    sink: Box<dyn FrameSink>,
    ended: bool,
}

impl Pump {
    /// Allocates everything the session needs up front, so [`Pump::push`] never allocates.
    pub fn new(mode: Mode, geometry: Geometry, sink: Box<dyn FrameSink>) -> Self {
        Self {
            mode,
            geometry,
            pool: FramePool::new(POOL_BUFFERS, nv12_len(geometry.width, geometry.height)),
            mjpeg: (mode.format == PixelFormat::Mjpeg)
                .then(|| MjpegDecoder::new(mode.width, mode.height)),
            unreadable: UnreadableRun::default(),
            start: Instant::now(),
            sink,
            ended: false,
        }
    }

    /// The V4L2 backend's mode fallback (Linux) reads it.
    #[cfg(target_os = "linux")]
    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// Gives the sink back, e.g. to start over with another mode before any picture arrived.
    #[cfg(target_os = "linux")]
    pub fn into_sink(self) -> Box<dyn FrameSink> {
        self.sink
    }

    pub fn ended(&self) -> bool {
        self.ended
    }

    /// Converts one picture and hands it to the sink. Capture-thread path: no allocation, lock,
    /// I/O or logging. A picture is dropped when the consumer still holds every buffer.
    pub fn push(&mut self, raw: Raw<'_>) {
        if self.ended {
            return;
        }
        let Some(mut data) = self.pool.acquire() else {
            return;
        };
        let (width, height) = (self.mode.width, self.mode.height);
        let picture = match (raw, self.mode.format, self.mjpeg.as_mut()) {
            (Raw::Packed { data, .. }, PixelFormat::Mjpeg, Some(decoder)) => {
                decoder.decode(data).map(|ycbcr| Picture::Ycbcr {
                    data: ycbcr,
                    stride: width as usize * 3,
                })
            }
            (Raw::Packed { data, stride }, PixelFormat::Yuy2, _) => {
                Some(Picture::Yuy2 { data, stride })
            }
            (
                Raw::Nv12 {
                    y,
                    y_stride,
                    uv,
                    uv_stride,
                },
                PixelFormat::Nv12,
                _,
            ) => Some(Picture::Nv12 {
                y,
                y_stride,
                uv,
                uv_stride,
            }),
            _ => None,
        };
        let readable =
            picture.is_some_and(|p| to_nv12(p, width, height, &self.geometry, &mut data));
        if self.unreadable.record(readable) {
            self.end(CameraError::Unreadable);
            return;
        }
        if readable {
            self.sink.frame(CameraFrame {
                width: self.geometry.width,
                height: self.geometry.height,
                timestamp: self.start.elapsed(),
                data,
            });
        }
    }

    /// The owner stopped the session: no more frames, and the sink is not told.
    pub fn stop(&mut self) {
        self.ended = true;
    }

    /// Ends the session; the sink hears about it once.
    pub fn end(&mut self, reason: CameraError) {
        if !self.ended {
            self.ended = true;
            self.sink.ended(reason);
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::sync::mpsc;

    use super::*;
    use crate::mode::Rate;

    type Event = Result<(u32, u32, Vec<u8>), CameraError>;

    struct Sink(mpsc::Sender<Event>);

    impl FrameSink for Sink {
        fn frame(&mut self, frame: CameraFrame) {
            let _ = self
                .0
                .send(Ok((frame.width, frame.height, frame.data.to_vec())));
        }
        fn ended(&mut self, reason: CameraError) {
            let _ = self.0.send(Err(reason));
        }
    }

    fn pump(format: PixelFormat, width: u32, height: u32) -> (Pump, mpsc::Receiver<Event>) {
        let (tx, rx) = mpsc::channel();
        let mode = Mode {
            format,
            width,
            height,
            rate: Rate::fps(30),
        };
        let geometry = Geometry::new(width, height, 1280, 720).unwrap();
        (Pump::new(mode, geometry, Box::new(Sink(tx))), rx)
    }

    #[test]
    fn a_4_3_yuy2_picture_arrives_as_16_9_nv12() {
        let (mut pump, rx) = pump(PixelFormat::Yuy2, 64, 48);
        // Luma rises by one per row, so the crop's first row tells where it starts.
        let mut data = vec![128u8; 64 * 2 * 48];
        for (y, row) in data.as_chunks_mut::<128>().0.iter_mut().enumerate() {
            for luma in row.iter_mut().step_by(2) {
                *luma = y as u8;
            }
        }
        pump.push(Raw::Packed {
            data: &data,
            stride: 128,
        });
        let (w, h, nv12) = rx.try_recv().unwrap().unwrap();
        assert_eq!((w, h), (64, 36));
        assert_eq!(nv12.len(), nv12_len(64, 36));
        assert_eq!(nv12[0], 6, "six rows cut at the top");
        assert_eq!(nv12[64 * 35], 41, "and six at the bottom");
    }

    #[test]
    fn mjpeg_frames_decode_and_garbage_stops_the_camera() {
        let frame = include_bytes!("../tests/fixtures/mjpeg-64x48-no-dht.jpg");
        let (mut pump, rx) = pump(PixelFormat::Mjpeg, 64, 48);
        pump.push(Raw::Packed {
            data: frame,
            stride: 0,
        });
        let (w, h, nv12) = rx.try_recv().unwrap().unwrap();
        assert_eq!((w, h), (64, 36));
        assert!(nv12[63].abs_diff(255) <= 4, "right half is white");

        for _ in 0..=crate::mjpeg::MAX_UNREADABLE_RUN {
            pump.push(Raw::Packed {
                data: b"garbage",
                stride: 0,
            });
        }
        assert_eq!(rx.try_recv().unwrap(), Err(CameraError::Unreadable));
        assert!(pump.ended());
        pump.push(Raw::Packed {
            data: frame,
            stride: 0,
        });
        assert!(rx.try_recv().is_err(), "nothing after the end");
    }
}
