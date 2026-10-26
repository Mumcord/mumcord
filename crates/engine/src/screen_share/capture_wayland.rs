//! Window or screen capture on Wayland (FR-2.42 – FR-2.46).
//!
//! Wayland apps cannot read the screen themselves: the ScreenCast desktop portal shows the
//! system picker (window or screen) and hands over a PipeWire stream of the choice. Frames
//! arrive on a PipeWire thread in shared memory (no DMA-BUF: without announcing modifiers the
//! compositor falls back to memory the CPU can read), are copied into one "latest picture" slot
//! as tight BGRX, and the encode thread scales and converts the newest one when it asks for it.
//! No sound: the portal does not reveal which app owns the shared window (FR-2.42). Nothing is
//! remembered between shares, so the picker shows every time.

use std::io::Cursor;
use std::os::fd::OwnedFd;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use ashpd::desktop::screencast::{
    CursorMode, OpenPipeWireRemoteOptions, Screencast, SelectSourcesOptions, SourceType,
    StartCastOptions,
};
use ashpd::desktop::{CreateSessionOptions, PersistMode, ResponseError};
use futures_util::StreamExt;
use mumcord_screenshare::I420Frame;
use pipewire as pw;
use pw::spa;
use pw::spa::param::ParamType;
use pw::spa::param::format::{FormatProperties, MediaSubtype, MediaType};
use pw::spa::param::video::{VideoFormat, VideoInfoRaw};
use pw::spa::pod::serialize::PodSerializer;
use pw::spa::pod::{Object, Pod, Property, PropertyFlags, Value};
use pw::spa::utils::{Choice, ChoiceEnum, ChoiceFlags, Fraction, Rectangle, SpaTypes};
use pw::stream::{StreamFlags, StreamState};
use tokio::sync::oneshot;

use super::capture::CaptureSource;
use super::convert::bgrx_to_i420;
use crate::session::lock;

/// Why a Wayland share could not start.
#[derive(Debug, thiserror::Error)]
pub enum WaylandCaptureError {
    /// The user closed the picker without choosing; not an error to show.
    #[error("the user cancelled the screen picker")]
    Cancelled,
    #[error("the screen capture portal failed: {0}")]
    Portal(ashpd::Error),
    #[error("the screen capture portal returned no stream")]
    NoStream,
    #[error("cannot start the capture thread: {0}")]
    Thread(std::io::Error),
}

impl From<ashpd::Error> for WaylandCaptureError {
    fn from(error: ashpd::Error) -> Self {
        match error {
            ashpd::Error::Response(ResponseError::Cancelled) => Self::Cancelled,
            other => Self::Portal(other),
        }
    }
}

/// The newest picture PipeWire delivered, as tight BGRX.
#[derive(Default)]
struct Latest {
    data: Vec<u8>,
    width: u32,
    height: u32,
    fresh: bool,
}

#[derive(Default)]
struct Shared {
    latest: Mutex<Latest>,
    ended: AtomicBool,
}

/// A running portal share. Dropping it stops the PipeWire stream and closes the portal session,
/// which also removes the desktop's "screen is being shared" indicator.
pub struct WaylandCapture {
    shared: Arc<Shared>,
    /// Picture buffer swapped with the slot, so neither side allocates per frame.
    spare: Vec<u8>,
    quit: pw::channel::Sender<()>,
    thread: Option<JoinHandle<()>>,
    /// Dropped to close the portal session.
    _close: oneshot::Sender<()>,
}

impl WaylandCapture {
    /// Shows the system picker and starts capturing the chosen window or screen. Must run inside
    /// the Tokio runtime: the portal session is watched and closed by a task there.
    pub async fn open() -> Result<Self, WaylandCaptureError> {
        let portal = Screencast::new().await?;
        let wanted = SourceType::Monitor | SourceType::Window;
        let sources = portal.available_source_types().await? & wanted;
        let sources = if sources.is_empty() { wanted } else { sources };
        let cursor = if portal
            .available_cursor_modes()
            .await?
            .contains(CursorMode::Embedded)
        {
            CursorMode::Embedded
        } else {
            CursorMode::Hidden
        };
        let session = portal
            .create_session(CreateSessionOptions::default())
            .await?;
        let streams = async {
            portal
                .select_sources(
                    &session,
                    SelectSourcesOptions::default()
                        .set_cursor_mode(cursor)
                        .set_sources(sources)
                        .set_multiple(false)
                        .set_persist_mode(PersistMode::DoNot),
                )
                .await?
                .response()?;
            let streams = portal
                .start(&session, None, StartCastOptions::default())
                .await?
                .response()?;
            let node = streams
                .streams()
                .first()
                .map(|s| s.pipe_wire_node_id())
                .ok_or(WaylandCaptureError::NoStream)?;
            let fd = portal
                .open_pipe_wire_remote(&session, OpenPipeWireRemoteOptions::default())
                .await?;
            Ok::<_, WaylandCaptureError>((node, fd))
        }
        .await;
        let (node, fd) = match streams {
            Ok(started) => started,
            Err(error) => {
                if let Err(close) = session.close().await {
                    tracing::debug!(%close, "closing the screen cast session failed");
                }
                return Err(error);
            }
        };

        let shared = Arc::new(Shared::default());
        let (quit, quit_rx) = pw::channel::channel();
        let thread = {
            let shared = shared.clone();
            thread::Builder::new()
                .name("screen-capture".into())
                .spawn(move || {
                    if let Err(error) = run_stream(fd, node, &shared, quit_rx) {
                        tracing::warn!(%error, "screen capture stream failed");
                    }
                    shared.ended.store(true, Ordering::Relaxed);
                })
                .map_err(WaylandCaptureError::Thread)?
        };
        let (close, close_rx) = oneshot::channel();
        tokio::spawn(watch_session(session, close_rx, shared.clone()));
        Ok(Self {
            shared,
            spare: Vec::new(),
            quit,
            thread: Some(thread),
            _close: close,
        })
    }

    /// Whether the share ended on its own: the user stopped it through the desktop, the window
    /// closed or the stream failed. No more pictures come; the share should be stopped.
    pub fn ended(&self) -> bool {
        self.shared.ended.load(Ordering::Relaxed)
    }
}

impl CaptureSource for WaylandCapture {
    fn ended(&self) -> bool {
        WaylandCapture::ended(self)
    }

    fn frame(&mut self, max_width: u32, max_height: u32) -> Option<I420Frame> {
        let (width, height) = {
            let mut latest = lock(&self.shared.latest);
            if !latest.fresh {
                return None;
            }
            latest.fresh = false;
            std::mem::swap(&mut latest.data, &mut self.spare);
            (latest.width, latest.height)
        };
        bgrx_to_i420(&self.spare, width, height, max_width, max_height)
    }
}

impl Drop for WaylandCapture {
    fn drop(&mut self) {
        // Fails only when the loop already stopped on its own.
        let _ = self.quit.send(());
        if let Some(thread) = self.thread.take()
            && thread.join().is_err()
        {
            tracing::warn!("screen capture thread panicked");
        }
    }
}

/// Keeps the portal session open until the share is dropped, and notices when the desktop ends
/// it (e.g. through its own "stop sharing" button).
async fn watch_session(
    session: ashpd::desktop::Session<Screencast>,
    close: oneshot::Receiver<()>,
    shared: Arc<Shared>,
) {
    match session.receive_closed().await {
        Ok(closed) => {
            let mut closed = std::pin::pin!(closed);
            tokio::select! {
                _ = close => {}
                _ = closed.next() => {
                    tracing::info!("the desktop ended the screen share");
                    shared.ended.store(true, Ordering::Relaxed);
                    return;
                }
            }
        }
        Err(error) => {
            tracing::debug!(%error, "cannot watch the screen cast session");
            let _ = close.await;
        }
    }
    if let Err(error) = session.close().await {
        tracing::debug!(%error, "closing the screen cast session failed");
    }
}

/// Byte positions of red, green and blue within one 4-byte pixel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Layout {
    r: usize,
    g: usize,
    b: usize,
}

/// The packed 8-bit RGB formats compositors hand out; BGRx (xRGB8888) is the common one.
const OFFERED: [VideoFormat; 8] = [
    VideoFormat::BGRx,
    VideoFormat::BGRA,
    VideoFormat::RGBx,
    VideoFormat::RGBA,
    VideoFormat::xRGB,
    VideoFormat::ARGB,
    VideoFormat::xBGR,
    VideoFormat::ABGR,
];

/// Frame rate asked for: the encoder never sends more than 30 (FR-2.44).
const MAX_FPS: u32 = 30;

fn layout(format: VideoFormat) -> Option<Layout> {
    let (r, g, b) = match format {
        VideoFormat::BGRx | VideoFormat::BGRA => (2, 1, 0),
        VideoFormat::RGBx | VideoFormat::RGBA => (0, 1, 2),
        VideoFormat::xRGB | VideoFormat::ARGB => (1, 2, 3),
        VideoFormat::xBGR | VideoFormat::ABGR => (3, 2, 1),
        _ => return None,
    };
    Some(Layout { r, g, b })
}

/// The format PipeWire settled on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Negotiated {
    layout: Layout,
    width: u32,
    height: u32,
}

/// Reads a `Format` param; `None` for anything but a packed RGB format.
fn negotiated(param: &Pod) -> Option<Negotiated> {
    let (media_type, subtype) = spa::param::format_utils::parse_format(param).ok()?;
    if media_type != MediaType::Video || subtype != MediaSubtype::Raw {
        return None;
    }
    let mut info = VideoInfoRaw::new();
    info.parse(param).ok()?;
    let size = info.size();
    if size.width == 0 || size.height == 0 {
        return None;
    }
    Some(Negotiated {
        layout: layout(info.format())?,
        width: size.width,
        height: size.height,
    })
}

fn serialize(object: Object) -> Option<Vec<u8>> {
    PodSerializer::serialize(Cursor::new(Vec::new()), &Value::Object(object))
        .ok()
        .map(|(cursor, _)| cursor.into_inner())
}

fn property(key: FormatProperties, value: Value) -> Property {
    Property {
        key: key.as_raw(),
        flags: PropertyFlags::empty(),
        value,
    }
}

/// The formats the stream accepts: packed RGB at any size, up to [`MAX_FPS`].
fn enum_format() -> Option<Vec<u8>> {
    let formats = OFFERED.iter().map(|f| spa::utils::Id(f.as_raw())).collect();
    serialize(Object {
        type_: SpaTypes::ObjectParamFormat.as_raw(),
        id: ParamType::EnumFormat.as_raw(),
        properties: vec![
            property(
                FormatProperties::MediaType,
                Value::Id(spa::utils::Id(MediaType::Video.as_raw())),
            ),
            property(
                FormatProperties::MediaSubtype,
                Value::Id(spa::utils::Id(MediaSubtype::Raw.as_raw())),
            ),
            property(
                FormatProperties::VideoFormat,
                Value::Choice(spa::pod::ChoiceValue::Id(Choice(
                    ChoiceFlags::empty(),
                    ChoiceEnum::Enum {
                        default: spa::utils::Id(OFFERED[0].as_raw()),
                        alternatives: formats,
                    },
                ))),
            ),
            property(
                FormatProperties::VideoSize,
                Value::Choice(spa::pod::ChoiceValue::Rectangle(Choice(
                    ChoiceFlags::empty(),
                    ChoiceEnum::Range {
                        default: Rectangle {
                            width: 1920,
                            height: 1080,
                        },
                        min: Rectangle {
                            width: 1,
                            height: 1,
                        },
                        max: Rectangle {
                            width: 8192,
                            height: 8192,
                        },
                    },
                ))),
            ),
            property(
                FormatProperties::VideoFramerate,
                Value::Choice(spa::pod::ChoiceValue::Fraction(Choice(
                    ChoiceFlags::empty(),
                    ChoiceEnum::Range {
                        default: Fraction {
                            num: MAX_FPS,
                            denom: 1,
                        },
                        min: Fraction { num: 0, denom: 1 },
                        max: Fraction {
                            num: MAX_FPS,
                            denom: 1,
                        },
                    },
                ))),
            ),
        ],
    })
}

/// Buffer types the stream takes: plain shared memory only, mapped by PipeWire.
fn buffers() -> Option<Vec<u8>> {
    let types = (1 << spa::buffer::DataType::MemPtr.as_raw())
        | (1 << spa::buffer::DataType::MemFd.as_raw());
    serialize(Object {
        type_: SpaTypes::ObjectParamBuffers.as_raw(),
        id: ParamType::Buffers.as_raw(),
        properties: vec![Property {
            key: spa::sys::SPA_PARAM_BUFFERS_dataType,
            flags: PropertyFlags::empty(),
            value: Value::Choice(spa::pod::ChoiceValue::Int(Choice(
                ChoiceFlags::empty(),
                ChoiceEnum::Flags {
                    default: types,
                    flags: vec![],
                },
            ))),
        }],
    })
}

/// Copies one picture into `out` as tight BGRX. `stride` 0 means rows are packed. `false` when
/// `src` is too short for the picture.
fn to_bgrx(
    src: &[u8],
    stride: usize,
    width: u32,
    height: u32,
    layout: Layout,
    out: &mut Vec<u8>,
) -> bool {
    let row_len = width as usize * 4;
    let stride = if stride == 0 { row_len } else { stride };
    let rows = height as usize;
    if stride < row_len || rows == 0 || src.len() < stride * (rows - 1) + row_len {
        return false;
    }
    out.clear();
    out.reserve(row_len * rows);
    let bgrx = Layout { r: 2, g: 1, b: 0 };
    for row in src.chunks(stride).take(rows) {
        let row = &row[..row_len];
        if layout == bgrx {
            out.extend_from_slice(row);
        } else {
            for px in row.as_chunks::<4>().0 {
                out.extend_from_slice(&[px[layout.b], px[layout.g], px[layout.r], 255]);
            }
        }
    }
    true
}

/// Runs the PipeWire stream of the portal node until `quit` fires or the stream ends.
fn run_stream(
    fd: OwnedFd,
    node: u32,
    shared: &Arc<Shared>,
    quit: pw::channel::Receiver<()>,
) -> Result<(), pw::Error> {
    pw::init();
    let mainloop = pw::main_loop::MainLoopRc::new(None)?;
    let context = pw::context::ContextRc::new(&mainloop, None)?;
    let core = context.connect_fd_rc(fd, None)?;
    let _quit = quit.attach(mainloop.loop_(), {
        let mainloop = mainloop.clone();
        move |()| mainloop.quit()
    });
    let stream = pw::stream::StreamBox::new(
        &core,
        "mumcord-screen-share",
        pw::properties::properties! {
            *pw::keys::MEDIA_TYPE => "Video",
            *pw::keys::MEDIA_CATEGORY => "Capture",
            *pw::keys::MEDIA_ROLE => "Screen",
        },
    )?;
    let process_shared = shared.clone();
    let _listener = stream
        .add_local_listener_with_user_data(None::<Negotiated>)
        .state_changed({
            let mainloop = mainloop.clone();
            move |_, _, old, new| match new {
                StreamState::Error(error) => {
                    tracing::warn!(%error, "screen capture stream error");
                    mainloop.quit();
                }
                StreamState::Unconnected if old != StreamState::Connecting => mainloop.quit(),
                _ => {}
            }
        })
        .param_changed(|stream, format, id, param| {
            if id != ParamType::Format.as_raw() {
                return;
            }
            let Some(param) = param else {
                *format = None;
                return;
            };
            *format = negotiated(param);
            match *format {
                Some(f) => tracing::debug!(f.width, f.height, "screen capture format"),
                None => tracing::warn!("screen capture offered no usable format"),
            }
            let Some(bytes) = buffers() else { return };
            let Some(pod) = Pod::from_bytes(&bytes) else {
                return;
            };
            if let Err(error) = stream.update_params(&mut [pod]) {
                tracing::warn!(%error, "cannot set the screen capture buffer type");
            }
        })
        .process(move |stream, format| {
            let Some(mut buffer) = stream.dequeue_buffer() else {
                return;
            };
            let Some(f) = *format else { return };
            let Some(data) = buffer.datas_mut().first_mut() else {
                return;
            };
            let chunk = data.chunk();
            let (offset, size) = (chunk.offset() as usize, chunk.size() as usize);
            let stride = usize::try_from(chunk.stride()).unwrap_or(0);
            if size == 0 {
                // An update without a new picture (e.g. only the cursor moved).
                return;
            }
            let Some(src) = data.data().and_then(|d| d.get(offset..offset + size)) else {
                return;
            };
            let mut latest = lock(&process_shared.latest);
            let latest = &mut *latest;
            if to_bgrx(src, stride, f.width, f.height, f.layout, &mut latest.data) {
                latest.width = f.width;
                latest.height = f.height;
                latest.fresh = true;
            }
        })
        .register()?;
    let format = enum_format().ok_or(pw::Error::CreationFailed)?;
    let pod = Pod::from_bytes(&format).ok_or(pw::Error::CreationFailed)?;
    stream.connect(
        spa::utils::Direction::Input,
        Some(node),
        StreamFlags::AUTOCONNECT | StreamFlags::MAP_BUFFERS,
        &mut [pod],
    )?;
    mainloop.run();
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn fixed_format(format: VideoFormat, width: u32, height: u32) -> Vec<u8> {
        serialize(Object {
            type_: SpaTypes::ObjectParamFormat.as_raw(),
            id: ParamType::Format.as_raw(),
            properties: vec![
                property(
                    FormatProperties::MediaType,
                    Value::Id(spa::utils::Id(MediaType::Video.as_raw())),
                ),
                property(
                    FormatProperties::MediaSubtype,
                    Value::Id(spa::utils::Id(MediaSubtype::Raw.as_raw())),
                ),
                property(
                    FormatProperties::VideoFormat,
                    Value::Id(spa::utils::Id(format.as_raw())),
                ),
                property(
                    FormatProperties::VideoSize,
                    Value::Rectangle(Rectangle { width, height }),
                ),
            ],
        })
        .unwrap()
    }

    #[test]
    fn every_offered_format_has_a_layout() {
        for format in OFFERED {
            assert!(layout(format).is_some(), "{format:?}");
        }
        assert_eq!(layout(VideoFormat::I420), None);
    }

    #[test]
    fn the_offer_is_a_raw_video_format() {
        let bytes = enum_format().unwrap();
        let pod = Pod::from_bytes(&bytes).unwrap();
        let (media_type, subtype) = spa::param::format_utils::parse_format(pod).unwrap();
        assert_eq!((media_type, subtype), (MediaType::Video, MediaSubtype::Raw));
        assert!(Pod::from_bytes(&buffers().unwrap()).is_some());
    }

    #[test]
    fn the_chosen_format_is_read() {
        let bytes = fixed_format(VideoFormat::RGBx, 64, 32);
        let got = negotiated(Pod::from_bytes(&bytes).unwrap()).unwrap();
        assert_eq!(
            got,
            Negotiated {
                layout: Layout { r: 0, g: 1, b: 2 },
                width: 64,
                height: 32
            }
        );
        let yuv = fixed_format(VideoFormat::I420, 64, 32);
        assert_eq!(negotiated(Pod::from_bytes(&yuv).unwrap()), None);
    }

    #[test]
    fn padded_rows_and_other_orders_become_tight_bgrx() {
        // 2×2 RGBA with 4 padding bytes per row: red, green / blue, white.
        let src = [
            255, 0, 0, 9, 0, 255, 0, 9, 7, 7, 7, 7, //
            0, 0, 255, 9, 255, 255, 255, 9,
        ];
        let mut out = Vec::new();
        let rgba = layout(VideoFormat::RGBA).unwrap();
        assert!(to_bgrx(&src, 12, 2, 2, rgba, &mut out));
        assert_eq!(
            out,
            [
                0, 0, 255, 255, 0, 255, 0, 255, //
                255, 0, 0, 255, 255, 255, 255, 255
            ]
        );
        let xbgr = layout(VideoFormat::xBGR).unwrap();
        assert!(to_bgrx(&[0, 1, 2, 3], 0, 1, 1, xbgr, &mut out));
        assert_eq!(out, [1, 2, 3, 255]);
    }

    #[test]
    fn bgrx_is_copied_as_is_and_short_buffers_are_refused() {
        let bgrx = layout(VideoFormat::BGRx).unwrap();
        let mut out = Vec::new();
        assert!(to_bgrx(&[1, 2, 3, 4, 5, 6, 7, 8], 0, 2, 1, bgrx, &mut out));
        assert_eq!(out, [1, 2, 3, 4, 5, 6, 7, 8]);
        assert!(!to_bgrx(&[0; 12], 8, 2, 2, bgrx, &mut out));
        assert!(!to_bgrx(&[0; 16], 4, 2, 2, bgrx, &mut out));
    }

    #[test]
    fn the_newest_picture_is_converted_once() {
        let (quit, _rx) = pw::channel::channel();
        let (close, _close_rx) = oneshot::channel();
        let shared = Arc::new(Shared::default());
        let mut capture = WaylandCapture {
            shared: shared.clone(),
            spare: Vec::new(),
            quit,
            thread: None,
            _close: close,
        };
        assert!(capture.frame(1280, 720).is_none());
        {
            let mut latest = lock(&shared.latest);
            latest.data = vec![0; 8 * 4 * 4];
            (latest.width, latest.height, latest.fresh) = (8, 4, true);
        }
        let frame = capture.frame(4, 4).unwrap();
        assert_eq!((frame.width(), frame.height()), (4, 2));
        assert!(capture.frame(4, 4).is_none(), "nothing new");
        assert!(!capture.ended());
    }
}
