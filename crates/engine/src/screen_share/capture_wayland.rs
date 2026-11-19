//! Window or screen capture on Wayland (FR-2.42 – FR-2.46).
//!
//! Wayland apps cannot read the screen themselves: the ScreenCast desktop portal shows the
//! system picker (window or screen) and hands over a PipeWire stream of the choice. Frames
//! arrive on a PipeWire thread in shared memory (no DMA-BUF: without announcing modifiers the
//! compositor falls back to memory the CPU can read), are copied into one "latest picture" slot
//! as tight BGRX, and the encode thread scales and converts the newest one when it asks for it.
//! Nothing is remembered between shares, so the picker shows every time.
//!
//! Sound comes from PipeWire (FR-7.81, FR-7.82): a whole screen shares the desktop sound. For a
//! window the portal does not say which app owns it, and no common compositor (GNOME, KDE,
//! wlroots) puts that on the screen cast either, so the matcher gets no hint and the user picks
//! the app in "Which app's sound?" (FR-7.83). The compositor's own name on the screen cast node
//! is deliberately not used as a hint: it would match the desktop's own sounds.

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
use hwcodec::I420Frame;
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
use super::convert::{bgra_to_i420, bgra_to_i420_strided};
use super::signal::ShareKind;
use super::sound::{SoundOutcome, SoundRequest, pick};
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

/// The newest picture PipeWire delivered, ready for the encoder.
///
/// The callback converts into the box the encode thread asked for and holds one picture at a
/// time: converting one the loop has not taken yet would only throw it away, and a frame is
/// 81 MiB at 6144×3456.
#[derive(Default)]
struct Shared {
    latest: Mutex<Option<I420Frame>>,
    /// The box [`CaptureSource::frame`] last asked for; `0` before the loop's first poll.
    target: Mutex<(u32, u32)>,
    /// Where the layouts other than BGRX are packed before converting.
    packing: Mutex<Vec<u8>>,
    ended: AtomicBool,
}

/// A running portal share. Dropping it stops the PipeWire stream and closes the portal session,
/// which also removes the desktop's "screen is being shared" indicator.
pub struct WaylandCapture {
    shared: Arc<Shared>,
    quit: pw::channel::Sender<()>,
    thread: Option<JoinHandle<()>>,
    /// What the user picked in the system picker; `None` when the portal did not say.
    kind: Option<ShareKind>,
    /// Dropped to close the portal session.
    _close: oneshot::Sender<()>,
}

impl WaylandCapture {
    /// Shows the system picker and starts capturing the chosen window or screen. Must run inside
    /// the Tokio runtime: the portal session is watched and closed by a task there.
    pub async fn open() -> Result<Self, WaylandCaptureError> {
        let portal = Screencast::new().await?;
        let wanted = SourceType::MONITOR | SourceType::WINDOW;
        let sources = portal.available_source_types().await? & wanted;
        let sources = if sources.is_empty() { wanted } else { sources };
        let cursor = if portal
            .available_cursor_modes()
            .await?
            .contains(CursorMode::EMBEDDED)
        {
            CursorMode::EMBEDDED
        } else {
            CursorMode::HIDDEN
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
            let (node, kind) = streams
                .streams()
                .first()
                .map(|s| (s.pipe_wire_node_id(), share_kind(s.source_type())))
                .ok_or(WaylandCaptureError::NoStream)?;
            let fd = portal
                .open_pipe_wire_remote(&session, OpenPipeWireRemoteOptions::default())
                .await?;
            Ok::<_, WaylandCaptureError>((node, kind, fd))
        }
        .await;
        let (node, kind, fd) = match streams {
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
            quit,
            thread: Some(thread),
            kind,
            _close: close,
        })
    }

    /// Whether the share ended on its own: the user stopped it through the desktop, the window
    /// closed or the stream failed. No more pictures come; the share should be stopped.
    pub fn ended(&self) -> bool {
        self.shared.ended.load(Ordering::Relaxed)
    }
}

/// The share kind of the portal's source type. A virtual monitor is a whole screen.
fn share_kind(source: Option<SourceType>) -> Option<ShareKind> {
    let source = source?;
    if source.is_empty() {
        return None;
    }
    // The portal reports the picked source as a flag set, one flag per stream. Anything that is
    // not a single window counts as a whole screen, so desktop sound is shared with it.
    if source == SourceType::WINDOW {
        Some(ShareKind::Window)
    } else {
        Some(ShareKind::Screen)
    }
}

impl CaptureSource for WaylandCapture {
    fn ended(&self) -> bool {
        WaylandCapture::ended(self)
    }

    fn kind(&self) -> Option<ShareKind> {
        self.kind
    }

    fn take_sound(&mut self, request: SoundRequest) -> SoundOutcome {
        // The portal tells nothing about the window's app (see the module docs).
        pick::take(request, &[])
    }

    fn frame(&mut self, max_width: u32, max_height: u32) -> Option<I420Frame> {
        *lock(&self.shared.target) = (max_width, max_height);
        let mut latest = lock(&self.shared.latest);
        match latest.take() {
            // A picture converted for an earlier, bigger box (the quality step just changed) does
            // not fit any more; the source keeps sending, so the next conversion uses the new box.
            Some(picture)
                if hwcodec::convert::fit(
                    picture.width(),
                    picture.height(),
                    max_width,
                    max_height,
                ) == Some((picture.width(), picture.height())) =>
            {
                Some(picture)
            }
            _ => None,
        }
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

/// BGRX (xRGB8888), the layout compositors hand out unless asked otherwise.
const BGRX: Layout = Layout { r: 2, g: 1, b: 0 };

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
    for row in src.chunks(stride).take(rows) {
        let row = &row[..row_len];
        if layout == BGRX {
            out.extend_from_slice(row);
        } else {
            for px in row.as_chunks::<4>().0 {
                out.extend_from_slice(&[px[layout.b], px[layout.g], px[layout.r], 255]);
            }
        }
    }
    true
}

/// Offers one delivered picture: converts it into the box the encode loop asked for, unless the
/// slot still holds one. The loop takes at most one picture per poll, so converting another one
/// before that would only throw it away.
fn offer(shared: &Shared, src: &[u8], stride: usize, format: Negotiated) {
    let mut latest = lock(&shared.latest);
    if latest.is_some() {
        return;
    }
    let (max_width, max_height) = *lock(&shared.target);
    if max_width < 2 || max_height < 2 {
        // No picture was asked for yet; the encode loop sets the box before its first poll.
        return;
    }
    let row = usize::try_from(format.width).map_or(0, |width| width * 4);
    let stride = if stride == 0 { row } else { stride };
    let picture = if format.layout == BGRX {
        bgra_to_i420_strided(
            src,
            stride,
            format.width,
            format.height,
            max_width,
            max_height,
        )
    } else {
        // The other packed layouts are rare; the reference conversion takes them packed.
        let mut packing = lock(&shared.packing);
        if to_bgrx(
            src,
            stride,
            format.width,
            format.height,
            format.layout,
            &mut packing,
        ) {
            bgra_to_i420(&packing, format.width, format.height, max_width, max_height)
        } else {
            None
        }
    };
    *latest = picture;
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
            offer(&process_shared, src, stride, f);
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

    /// A BGRX picture whose pixels differ from the next one, so a stale slot is visible.
    fn bgrx(width: u32, height: u32, seed: u32) -> Vec<u8> {
        let mut state = seed;
        (0..width * height * 4)
            .map(|_| {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (state >> 24) as u8
            })
            .collect()
    }

    fn settled(layout: Layout, width: u32, height: u32) -> Negotiated {
        Negotiated {
            layout,
            width,
            height,
        }
    }

    fn asking_for(box_side: (u32, u32)) -> Shared {
        let shared = Shared::default();
        *lock(&shared.target) = box_side;
        shared
    }

    #[test]
    fn a_picture_waits_for_the_loop_and_the_next_one_is_skipped() {
        let shared = asking_for((64, 48));
        let first = bgrx(64, 48, 0x1234_5678);
        let second = bgrx(64, 48, 0x9abc_def0);
        offer(&shared, &first, 0, settled(BGRX, 64, 48));
        let want = bgra_to_i420(&first, 64, 48, 64, 48);
        assert_eq!(*lock(&shared.latest), want);

        // The loop has not taken the first one: the second is not converted, not even kept.
        offer(&shared, &second, 0, settled(BGRX, 64, 48));
        assert_eq!(*lock(&shared.latest), want);

        let taken = lock(&shared.latest).take().expect("a picture");
        assert_eq!(Some(taken), want);
        assert!(lock(&shared.latest).is_none());
        offer(&shared, &second, 0, settled(BGRX, 64, 48));
        assert_eq!(
            *lock(&shared.latest),
            bgra_to_i420(&second, 64, 48, 64, 48),
            "the newer picture replaces nothing while the slot is empty"
        );
    }

    #[test]
    fn a_padded_picture_converts_as_delivered() {
        let shared = asking_for((64, 48));
        let packed = bgrx(64, 48, 7);
        let stride = 64 * 4 + 40;
        let mut padded = vec![0xEEu8; stride * 48];
        for y in 0..48 {
            padded[y * stride..y * stride + 64 * 4]
                .copy_from_slice(&packed[y * 64 * 4..(y + 1) * 64 * 4]);
        }
        offer(&shared, &padded, stride, settled(BGRX, 64, 48));
        assert_eq!(*lock(&shared.latest), bgra_to_i420(&packed, 64, 48, 64, 48));
    }

    #[test]
    fn nothing_is_converted_before_the_loop_asks_for_a_box() {
        let shared = Shared::default();
        offer(&shared, &bgrx(64, 48, 1), 0, settled(BGRX, 64, 48));
        assert!(lock(&shared.latest).is_none());
    }

    #[test]
    fn the_other_layouts_land_as_the_same_picture() {
        let bgrx_picture = bgrx(32, 18, 42);
        let rgba: Vec<u8> = bgrx_picture
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|px| [px[2], px[1], px[0], px[3]])
            .collect();
        let from_bgrx = asking_for((32, 18));
        let from_rgba = asking_for((32, 18));
        offer(&from_bgrx, &bgrx_picture, 0, settled(BGRX, 32, 18));
        offer(
            &from_rgba,
            &rgba,
            0,
            settled(Layout { r: 0, g: 1, b: 2 }, 32, 18),
        );
        assert!(lock(&from_bgrx.latest).is_some());
        assert_eq!(*lock(&from_bgrx.latest), *lock(&from_rgba.latest));
    }

    #[test]
    fn the_portal_pick_decides_the_kind() {
        assert_eq!(
            share_kind(Some(SourceType::MONITOR)),
            Some(ShareKind::Screen)
        );
        assert_eq!(
            share_kind(Some(SourceType::VIRTUAL)),
            Some(ShareKind::Screen)
        );
        assert_eq!(
            share_kind(Some(SourceType::WINDOW)),
            Some(ShareKind::Window)
        );
        assert_eq!(
            share_kind(Some(SourceType::empty())),
            None,
            "a portal that names no source"
        );
        assert_eq!(share_kind(None), None, "a portal that does not say");
    }

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
    fn the_loop_takes_one_picture_per_poll() {
        let (quit, _rx) = pw::channel::channel();
        let (close, _close_rx) = oneshot::channel();
        let shared = Arc::new(Shared::default());
        let mut capture = WaylandCapture {
            shared: shared.clone(),
            quit,
            thread: None,
            kind: None,
            _close: close,
        };
        // The first poll asks for a box; until then the callback converts nothing.
        assert!(capture.frame(1280, 720).is_none());
        assert_eq!(*lock(&shared.target), (1280, 720));
        offer(&shared, &bgrx(640, 360, 3), 0, settled(BGRX, 640, 360));
        let frame = capture.frame(1280, 720).expect("the offered picture");
        assert_eq!((frame.width(), frame.height()), (640, 360));
        assert!(capture.frame(1280, 720).is_none(), "nothing new");
        // A step down leaves the picture converted for the old, bigger box behind.
        offer(&shared, &bgrx(640, 360, 4), 0, settled(BGRX, 640, 360));
        assert!(
            capture.frame(320, 180).is_none(),
            "the bigger picture does not fit"
        );
        offer(&shared, &bgrx(640, 360, 5), 0, settled(BGRX, 640, 360));
        let small = capture
            .frame(320, 180)
            .expect("converted for the smaller box");
        assert_eq!((small.width(), small.height()), (320, 180));
        assert!(!capture.ended());
    }
}
