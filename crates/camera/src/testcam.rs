//! `MUMCORD_TEST_CAMERA`: a virtual camera for E2E tests (FR-8.31, spec section 10.3).
//!
//! It draws a stylised person (head, neck and body in fixed colours) in front of eight colour
//! bars, 1280×720 NV12 at up to 30 fps. The person sways sideways so frames differ over time.
//! The picture is computed, not loaded from a file, so the person's outline is known exactly for
//! every frame ([`TestCamera::person_span`]) and tests can check pixels without a fixture image.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::{
    CameraBackend, CameraDevice, CameraError, CameraFrame, CaptureRequest, CaptureSession,
    DeviceId, FramePool, FrameSink, MAX_FPS, POOL_BUFFERS, nv12_len,
};

/// Name of the virtual camera in the pickers.
pub const TEST_CAMERA_NAME: &str = "Test camera";

const WIDTH: u32 = 1280;
const HEIGHT: u32 = 720;

/// Behaviour of the virtual camera, from the `MUMCORD_TEST_CAMERA` value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TestCameraMode {
    /// `1`: one working camera.
    Present,
    /// `absent`: no camera at all.
    Absent,
    /// `denied`: the camera is listed, but opening it is refused.
    Denied,
    /// `unplug_after=5s` (or `=500ms`): the camera disappears this long after it is first opened.
    UnplugAfter(Duration),
}

impl TestCameraMode {
    /// Reads `MUMCORD_TEST_CAMERA`. Always `None` in release builds, where the hook is inert.
    pub fn from_env() -> Option<Self> {
        if !cfg!(debug_assertions) {
            return None;
        }
        let value = std::env::var("MUMCORD_TEST_CAMERA").ok()?;
        if value.is_empty() || value == "0" {
            return None;
        }
        let mode = Self::parse(&value);
        if mode.is_none() {
            tracing::warn!("MUMCORD_TEST_CAMERA has an unknown value; the test camera stays off");
        }
        mode
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "1" => Some(Self::Present),
            "absent" => Some(Self::Absent),
            "denied" => Some(Self::Denied),
            _ => {
                let delay = value.strip_prefix("unplug_after=")?;
                let duration = if let Some(ms) = delay.strip_suffix("ms") {
                    Duration::from_millis(ms.parse().ok()?)
                } else {
                    Duration::from_secs(delay.strip_suffix('s')?.parse().ok()?)
                };
                Some(Self::UnplugAfter(duration))
            }
        }
    }
}

/// The virtual camera backend. It replaces the OS backend when the hook is set.
pub struct TestCamera {
    mode: TestCameraMode,
    /// When the camera vanishes (`UnplugAfter`); fixed on the first open.
    unplug_at: OnceLock<Instant>,
}

impl TestCamera {
    pub fn new(mode: TestCameraMode) -> Self {
        Self {
            mode,
            unplug_at: OnceLock::new(),
        }
    }

    pub fn device_id() -> DeviceId {
        DeviceId::from_os_key("test", "test-camera")
    }

    fn unplugged(&self) -> bool {
        self.unplug_at.get().is_some_and(|at| Instant::now() >= *at)
    }

    /// Horizontal offset of the person in `frame`: a triangle wave of ±24 px over 4 s at 30 fps.
    fn sway(frame: u64) -> i64 {
        let phase = (frame % 120) as i64;
        (phase - 60).abs() * 4 / 5 - 24
    }

    /// The pixels `[start, end)` of row `y` that belong to the person in `frame`, if any.
    pub fn person_span(y: u32, frame: u64) -> Option<(u32, u32)> {
        let y = i64::from(y);
        let half_width = if (150..350).contains(&y) {
            // Head: a circle of radius 100 around (centre, 250).
            let dy = y - 250;
            (100 * 100 - dy * dy).isqrt()
        } else if (350..380).contains(&y) {
            40
        } else if (380..i64::from(HEIGHT)).contains(&y) {
            // Shoulders widen from 180 to 240 px towards the bottom edge.
            180 + (y - 380) * 60 / 340
        } else {
            return None;
        };
        let centre = i64::from(WIDTH / 2) + Self::sway(frame);
        let start = (centre - half_width).clamp(0, i64::from(WIDTH));
        let end = (centre + half_width).clamp(0, i64::from(WIDTH));
        (start < end).then_some((start as u32, end as u32))
    }

    /// Draws frame number `frame` into `nv12` (1280×720 NV12, [`nv12_len`] bytes). Does not
    /// allocate.
    ///
    /// # Panics
    ///
    /// When `nv12` has the wrong length.
    pub fn render(frame: u64, nv12: &mut [u8]) {
        assert_eq!(
            nv12.len(),
            nv12_len(WIDTH, HEIGHT),
            "test camera buffer size"
        );
        let width = WIDTH as usize;
        let (luma, chroma) = nv12.split_at_mut(width * HEIGHT as usize);
        let bar_width = width / BARS.len();

        for (y, row) in luma.chunks_exact_mut(width).enumerate() {
            for (bar, colour) in row.chunks_exact_mut(bar_width).zip(BARS) {
                bar.fill(colour.0);
            }
            if let Some((start, end)) = Self::person_span(y as u32, frame) {
                row[start as usize..end as usize].fill(person_colour(y as u32).0);
            }
        }

        for (cy, row) in chroma.chunks_exact_mut(width).enumerate() {
            for (bar, colour) in row.chunks_exact_mut(bar_width).zip(BARS) {
                fill_uv(bar, colour);
            }
            let y = cy as u32 * 2;
            if let Some((start, end)) = Self::person_span(y, frame) {
                let (start, end) = (start as usize & !1, (end as usize + 1) & !1);
                fill_uv(&mut row[start..end.min(width)], person_colour(y));
            }
        }
    }
}

/// Y, U, V in BT.601 limited range.
type Yuv = (u8, u8, u8);

/// White, yellow, cyan, green, magenta, red, blue, black.
const BARS: [Yuv; 8] = [
    (235, 128, 128),
    (210, 16, 146),
    (170, 166, 16),
    (145, 54, 34),
    (106, 202, 222),
    (81, 90, 240),
    (41, 240, 110),
    (16, 128, 128),
];
const SKIN: Yuv = (160, 110, 155);
const SHIRT: Yuv = (70, 150, 110);

fn person_colour(y: u32) -> Yuv {
    if y < 380 { SKIN } else { SHIRT }
}

fn fill_uv(row: &mut [u8], colour: Yuv) {
    for pair in row.as_chunks_mut::<2>().0 {
        *pair = [colour.1, colour.2];
    }
}

impl CameraBackend for TestCamera {
    fn devices(&self) -> Result<Vec<CameraDevice>, CameraError> {
        if self.mode == TestCameraMode::Absent || self.unplugged() {
            return Ok(Vec::new());
        }
        Ok(vec![CameraDevice {
            id: Self::device_id(),
            name: TEST_CAMERA_NAME.to_owned(),
            is_default: true,
        }])
    }

    fn open(
        &self,
        device: &DeviceId,
        request: CaptureRequest,
        mut sink: Box<dyn FrameSink>,
    ) -> Result<Box<dyn CaptureSession>, CameraError> {
        if self.mode == TestCameraMode::Absent || *device != Self::device_id() {
            return Err(CameraError::NoCamera);
        }
        if self.mode == TestCameraMode::Denied {
            return Err(CameraError::PermissionDenied);
        }
        if let TestCameraMode::UnplugAfter(delay) = self.mode {
            self.unplug_at.get_or_init(|| Instant::now() + delay);
        }
        if self.unplugged() {
            return Err(CameraError::NoCamera);
        }

        let unplug_at = self.unplug_at.get().copied();
        let interval = Duration::from_secs(1) / request.max_fps.clamp(1, MAX_FPS);
        let pool = FramePool::new(POOL_BUFFERS, nv12_len(WIDTH, HEIGHT));
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let thread = std::thread::Builder::new()
            .name("test-camera".to_owned())
            .spawn(move || {
                let start = Instant::now();
                let mut frame = 0u64;
                while !thread_stop.load(Ordering::Acquire) {
                    if unplug_at.is_some_and(|at| Instant::now() >= at) {
                        sink.ended(CameraError::DeviceGone);
                        return;
                    }
                    // No free buffer means the consumer is behind: skip this frame.
                    if let Some(mut data) = pool.acquire() {
                        Self::render(frame, &mut data);
                        sink.frame(CameraFrame {
                            width: WIDTH,
                            height: HEIGHT,
                            timestamp: start.elapsed(),
                            data,
                        });
                    }
                    frame += 1;
                    let next = start + interval * frame as u32;
                    std::thread::sleep(next.saturating_duration_since(Instant::now()));
                }
            })
            .map_err(|err| CameraError::Backend(format!("test camera thread: {err}")))?;
        Ok(Box::new(TestSession {
            stop,
            thread: Some(thread),
        }))
    }
}

struct TestSession {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl CaptureSession for TestSession {}

impl Drop for TestSession {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take()
            && thread.join().is_err()
        {
            tracing::error!("the test camera thread panicked");
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use super::*;

    enum Event {
        Frame {
            width: u32,
            height: u32,
            timestamp: Duration,
            len: usize,
        },
        Ended(CameraError),
    }

    struct ChannelSink(mpsc::Sender<Event>);

    impl FrameSink for ChannelSink {
        fn frame(&mut self, frame: CameraFrame) {
            let _ = self.0.send(Event::Frame {
                width: frame.width,
                height: frame.height,
                timestamp: frame.timestamp,
                len: frame.data.len(),
            });
        }

        fn ended(&mut self, reason: CameraError) {
            let _ = self.0.send(Event::Ended(reason));
        }
    }

    fn sink() -> (Box<dyn FrameSink>, mpsc::Receiver<Event>) {
        let (tx, rx) = mpsc::channel();
        (Box::new(ChannelSink(tx)), rx)
    }

    fn rendered(frame: u64) -> Vec<u8> {
        let mut buffer = vec![0; nv12_len(WIDTH, HEIGHT)];
        TestCamera::render(frame, &mut buffer);
        buffer
    }

    fn pixel(nv12: &[u8], x: usize, y: usize) -> Yuv {
        let width = WIDTH as usize;
        let uv = width * HEIGHT as usize + (y / 2) * width + (x & !1);
        (nv12[y * width + x], nv12[uv], nv12[uv + 1])
    }

    #[test]
    fn parses_the_hook_values() {
        assert_eq!(TestCameraMode::parse("1"), Some(TestCameraMode::Present));
        assert_eq!(
            TestCameraMode::parse("absent"),
            Some(TestCameraMode::Absent)
        );
        assert_eq!(
            TestCameraMode::parse("denied"),
            Some(TestCameraMode::Denied)
        );
        assert_eq!(
            TestCameraMode::parse("unplug_after=5s"),
            Some(TestCameraMode::UnplugAfter(Duration::from_secs(5)))
        );
        assert_eq!(
            TestCameraMode::parse("unplug_after=250ms"),
            Some(TestCameraMode::UnplugAfter(Duration::from_millis(250)))
        );
        for bad in ["yes", "unplug_after=", "unplug_after=5", "unplug_after=xs"] {
            assert_eq!(TestCameraMode::parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn draws_the_person_in_front_of_the_colour_bars() {
        let frame = rendered(0);
        // Top-left: the white bar; top-right: the black bar.
        assert_eq!(pixel(&frame, 10, 10), BARS[0]);
        assert_eq!(pixel(&frame, 1270, 10), BARS[7]);
        // Centre of the head and of the body.
        let centre = (640 + TestCamera::sway(0)) as usize;
        assert_eq!(pixel(&frame, centre, 250), SKIN);
        assert_eq!(pixel(&frame, centre, 600), SHIRT);
        // Every pixel matches the published outline.
        for y in [160u32, 250, 360, 400, 719] {
            let (start, end) = TestCamera::person_span(y, 0).expect("person on this row");
            assert_eq!(
                pixel(&frame, start as usize, y as usize).0,
                person_colour(y).0
            );
            assert_eq!(
                pixel(&frame, end as usize - 1, y as usize).0,
                person_colour(y).0
            );
            assert_ne!(
                pixel(&frame, end as usize, y as usize).0,
                person_colour(y).0
            );
        }
        assert_eq!(TestCamera::person_span(100, 0), None);
    }

    #[test]
    fn the_person_moves_between_frames() {
        assert_ne!(rendered(0), rendered(15));
        assert_ne!(
            TestCamera::person_span(600, 0),
            TestCamera::person_span(600, 30)
        );
        // The movement stays slight and loops every 4 s.
        for frame in 0..120 {
            assert!(TestCamera::sway(frame).abs() <= 24);
        }
        assert_eq!(rendered(7), rendered(127));
    }

    #[test]
    fn lists_one_default_camera_and_streams_720p_frames() {
        let camera = TestCamera::new(TestCameraMode::Present);
        let devices = camera.devices().expect("devices");
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].name, TEST_CAMERA_NAME);
        assert!(devices[0].is_default);

        let (sink, events) = sink();
        let session = camera
            .open(&devices[0].id, CaptureRequest::default(), sink)
            .expect("open");
        let mut last = None;
        for _ in 0..3 {
            match events.recv_timeout(Duration::from_secs(2)).expect("frame") {
                Event::Frame {
                    width,
                    height,
                    timestamp,
                    len,
                } => {
                    assert_eq!((width, height, len), (1280, 720, nv12_len(1280, 720)));
                    assert!(last.is_none_or(|last| timestamp > last));
                    last = Some(timestamp);
                }
                Event::Ended(reason) => panic!("ended early: {reason}"),
            }
        }
        drop(session);
        // The thread is joined: nothing arrives after the drop.
        while events.try_recv().is_ok() {}
        assert!(events.recv_timeout(Duration::from_millis(100)).is_err());
    }

    #[test]
    fn absent_and_denied_modes() {
        let absent = TestCamera::new(TestCameraMode::Absent);
        assert!(absent.devices().expect("devices").is_empty());
        let (sink_a, _) = sink();
        assert_eq!(
            absent
                .open(&TestCamera::device_id(), CaptureRequest::default(), sink_a)
                .err(),
            Some(CameraError::NoCamera)
        );

        let denied = TestCamera::new(TestCameraMode::Denied);
        assert_eq!(denied.devices().expect("devices").len(), 1);
        let (sink_d, _) = sink();
        assert_eq!(
            denied
                .open(&TestCamera::device_id(), CaptureRequest::default(), sink_d)
                .err(),
            Some(CameraError::PermissionDenied)
        );
    }

    #[test]
    fn unplug_ends_the_session_and_empties_the_device_list() {
        let camera = TestCamera::new(TestCameraMode::UnplugAfter(Duration::from_millis(150)));
        assert_eq!(camera.devices().expect("devices").len(), 1);
        let (sink, events) = sink();
        let _session = camera
            .open(&TestCamera::device_id(), CaptureRequest::default(), sink)
            .expect("open");
        let reason = loop {
            match events.recv_timeout(Duration::from_secs(2)).expect("event") {
                Event::Frame { .. } => {}
                Event::Ended(reason) => break reason,
            }
        };
        assert_eq!(reason, CameraError::DeviceGone);
        assert!(camera.devices().expect("devices").is_empty());
    }
}
