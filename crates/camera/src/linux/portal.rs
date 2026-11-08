//! xdg-desktop-portal Camera plus PipeWire (FR-8.25): the path inside Flatpak and on every
//! desktop whose portal has a camera backend.
//!
//! `AccessCamera` asks the user once (host apps are allowed without a dialog), then
//! `OpenPipeWireRemote` gives a PipeWire connection that shows only the camera nodes. One thread
//! per connection runs the PipeWire loop: it keeps the node list current from registry events
//! (that is the hot-plug notification), reads each camera's formats, and runs the one capture
//! stream. Pictures are converted on that thread and go straight to the sink.

use std::cell::RefCell;
use std::future::Future;
use std::io::Cursor;
use std::os::fd::OwnedFd;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

use ashpd::desktop::ResponseError;
use ashpd::desktop::camera::{Camera, CameraAccessOptions, OpenPipeWireRemoteOptions};
use pipewire as pw;
use pw::spa;
use pw::spa::param::ParamType;
use pw::spa::param::format::{FormatProperties, MediaSubtype, MediaType};
use pw::spa::param::video::VideoFormat;
use pw::spa::pod::deserialize::PodDeserializer;
use pw::spa::pod::serialize::PodSerializer;
use pw::spa::pod::{
    CanonicalFixedSizedPod, ChoiceValue, Object, Pod, Property, PropertyFlags, Value,
};
use pw::spa::utils::{Choice, ChoiceEnum, Fraction, Id, Rectangle, SpaTypes};
use pw::stream::{StreamFlags, StreamState};

use super::ranges;
use crate::mode::{self, Geometry, Mode, PixelFormat, Rate};
use crate::pump::{Pump, Raw};
use crate::{
    CameraDevice, CameraError, CaptureRequest, CaptureSession, DeviceId, FrameSink, HotplugCallback,
};

/// Backend name in [`DeviceId::from_os_key`].
const BACKEND: &str = "portal";

/// `IsCameraPresent` answers quickly; no answer means no usable portal.
const PROBE_TIMEOUT: Duration = Duration::from_secs(3);
/// `AccessCamera` may show a dialog; give the user time to answer it.
const ACCESS_TIMEOUT: Duration = Duration::from_secs(120);
/// Connecting, listing formats and starting a stream are local round trips.
const LOOP_TIMEOUT: Duration = Duration::from_secs(5);

/// Shared with [`super::LinuxCamera`]: the one hot-plug callback, if someone watches.
pub type HotplugSlot = Arc<Mutex<Option<HotplugCallback>>>;

/// Runs the portal's D-Bus calls. It lives for the whole process because `ashpd` keeps one
/// session-bus connection for every portal user in Mumcord, and that connection's tasks run on
/// the runtime that opened it.
fn portal_call<F>(future: F, timeout: Duration) -> Option<F::Output>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    static RUNTIME: OnceLock<Option<tokio::runtime::Runtime>> = OnceLock::new();
    let runtime = RUNTIME
        .get_or_init(|| {
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(1)
                .thread_name("camera-portal")
                .enable_all()
                .build()
                .inspect_err(|err| tracing::warn!(%err, "camera portal runtime not started"))
                .ok()
        })
        .as_ref()?;
    let (tx, rx) = mpsc::channel();
    runtime.spawn(async move {
        let _ = tx.send(future.await);
    });
    rx.recv_timeout(timeout).ok()
}

/// Whether the portal is there and reports a camera; `None` when there is no camera portal.
/// Never shows a dialog.
pub fn probe() -> Option<bool> {
    portal_call(
        async {
            let camera = Camera::new().await.ok()?;
            camera.is_present().await.ok()
        },
        PROBE_TIMEOUT,
    )
    .flatten()
}

/// Asks for camera access and opens the PipeWire remote with the camera nodes.
fn access() -> Result<OwnedFd, CameraError> {
    let result = portal_call(
        async {
            let camera = Camera::new().await?;
            camera
                .request_access(CameraAccessOptions::default())
                .await?
                .response()?;
            camera
                .open_pipe_wire_remote(OpenPipeWireRemoteOptions::default())
                .await
        },
        ACCESS_TIMEOUT,
    )
    .ok_or_else(|| CameraError::Backend("no answer from the camera portal".to_owned()))?;
    result.map_err(|err| match err {
        ashpd::Error::Response(ResponseError::Cancelled)
        | ashpd::Error::Portal(ashpd::PortalError::NotAllowed(_)) => CameraError::PermissionDenied,
        other => CameraError::Backend(format!("camera portal: {other}")),
    })
}

/// A camera node as the device list shows it.
#[derive(Clone)]
struct Listed {
    node: u32,
    id: DeviceId,
    name: String,
}

/// State shared between the PipeWire thread and the callers.
struct Shared {
    cameras: Mutex<Vec<Listed>>,
    hotplug: HotplugSlot,
    /// Cleared when the PipeWire loop ended (PipeWire restarted, portal revoked access).
    alive: AtomicBool,
}

impl Shared {
    fn notify(&self) {
        let callback = self
            .hotplug
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        if let Some(callback) = callback {
            callback();
        }
    }
}

struct OpenRequest {
    node: u32,
    request: CaptureRequest,
    sink: Box<dyn FrameSink>,
    reply: mpsc::Sender<Result<(), CameraError>>,
}

enum Command {
    Open(OpenRequest),
    /// Stops the stream; the reply comes after the sink was dropped.
    Close(mpsc::Sender<()>),
    Quit,
}

/// A live portal connection with its PipeWire thread.
pub struct Connection {
    shared: Arc<Shared>,
    commands: pw::channel::Sender<Command>,
    thread: Option<JoinHandle<()>>,
}

impl Connection {
    /// Asks for access (may show the portal dialog) and connects. Blocks until the first node
    /// list is known.
    pub fn open(hotplug: HotplugSlot) -> Result<Self, CameraError> {
        let fd = access()?;
        let shared = Arc::new(Shared {
            cameras: Mutex::new(Vec::new()),
            hotplug,
            alive: AtomicBool::new(true),
        });
        let (commands, commands_rx) = pw::channel::channel();
        let (ready, ready_rx) = mpsc::channel();
        let thread = {
            let shared = Arc::clone(&shared);
            std::thread::Builder::new()
                .name("camera-pipewire".to_owned())
                .spawn(move || {
                    if let Err(err) = run(fd, &shared, commands_rx, ready) {
                        tracing::warn!(%err, "camera PipeWire connection failed");
                    }
                    shared.alive.store(false, Ordering::Release);
                    // Lets a watcher list again, which reconnects.
                    shared.notify();
                })
                .map_err(|err| CameraError::Backend(format!("camera thread: {err}")))?
        };
        let connection = Self {
            shared,
            commands,
            thread: Some(thread),
        };
        match ready_rx.recv_timeout(LOOP_TIMEOUT) {
            Ok(()) => Ok(connection),
            Err(_) => Err(CameraError::Backend(
                "the camera PipeWire connection did not start".to_owned(),
            )),
        }
    }

    pub fn alive(&self) -> bool {
        self.shared.alive.load(Ordering::Acquire)
    }

    pub fn devices(&self) -> Vec<CameraDevice> {
        self.shared
            .cameras
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .enumerate()
            .map(|(index, camera)| CameraDevice {
                id: camera.id.clone(),
                name: camera.name.clone(),
                is_default: index == 0,
            })
            .collect()
    }

    pub fn capture(
        &self,
        device: &DeviceId,
        request: CaptureRequest,
        sink: Box<dyn FrameSink>,
    ) -> Result<Box<dyn CaptureSession>, CameraError> {
        let node = self
            .shared
            .cameras
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .find(|camera| &camera.id == device)
            .map(|camera| camera.node)
            .ok_or(CameraError::NoCamera)?;
        let (reply, reply_rx) = mpsc::channel();
        let open = OpenRequest {
            node,
            request,
            sink,
            reply,
        };
        if self.commands.send(Command::Open(open)).is_err() {
            return Err(CameraError::Backend(
                "the camera PipeWire connection ended".to_owned(),
            ));
        }
        let session = PortalSession {
            commands: self.commands.clone(),
        };
        match reply_rx.recv_timeout(LOOP_TIMEOUT) {
            Ok(Ok(())) => Ok(Box::new(session)),
            Ok(Err(err)) => Err(err),
            // Dropping `session` stops whatever may still start.
            Err(_) => Err(CameraError::Backend(
                "the camera stream did not start".to_owned(),
            )),
        }
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Quit);
        if let Some(thread) = self.thread.take()
            && thread.join().is_err()
        {
            tracing::error!("the camera PipeWire thread panicked");
        }
    }
}

struct PortalSession {
    commands: pw::channel::Sender<Command>,
}

impl CaptureSession for PortalSession {}

impl Drop for PortalSession {
    fn drop(&mut self) {
        let (done, done_rx) = mpsc::channel();
        if self.commands.send(Command::Close(done)).is_ok() {
            // Either the stream is gone or the loop ended (which also drops the sink).
            let _ = done_rx.recv_timeout(LOOP_TIMEOUT);
        }
    }
}

/// A camera node the loop knows, with the formats it offers.
struct Node {
    id: u32,
    _proxy: pw::node::Node,
    _listener: pw::node::NodeListener,
    modes: Rc<RefCell<Vec<Mode>>>,
}

/// The capture stream.
struct Active {
    node: u32,
    pump: Rc<RefCell<Pump>>,
    _listener: pw::stream::StreamListener<()>,
    _stream: pw::stream::StreamRc,
}

impl Active {
    /// Ends the stream without telling the sink: its owner stopped it.
    fn stop(self) {
        self.pump.borrow_mut().stop();
    }
}

struct Pending {
    seq: i32,
    open: OpenRequest,
}

#[derive(Default)]
struct LoopState {
    nodes: Vec<Node>,
    listed: Vec<Listed>,
    active: Option<Active>,
    pending: Option<Pending>,
}

fn run(
    fd: OwnedFd,
    shared: &Arc<Shared>,
    commands: pw::channel::Receiver<Command>,
    ready: mpsc::Sender<()>,
) -> Result<(), pw::Error> {
    pw::init();
    let mainloop = pw::main_loop::MainLoopRc::new(None)?;
    let context = pw::context::ContextRc::new(&mainloop, None)?;
    let core = context.connect_fd_rc(fd, None)?;
    let registry = core.get_registry_rc()?;
    let state = Rc::new(RefCell::new(LoopState::default()));
    let initial = Rc::new(RefCell::new(None::<i32>));

    let _registry_listener = registry
        .add_listener_local()
        .global({
            let registry = registry.clone();
            let state = Rc::clone(&state);
            let shared = Arc::clone(shared);
            let initial = Rc::clone(&initial);
            move |global| {
                if global.type_ != pw::types::ObjectType::Node {
                    return;
                }
                let Some(props) = global.props else { return };
                if props.get("media.role") != Some("Camera") {
                    return;
                }
                let proxy: pw::node::Node = match registry.bind(global) {
                    Ok(proxy) => proxy,
                    Err(err) => {
                        tracing::warn!(%err, "camera node not bound");
                        return;
                    }
                };
                let modes = Rc::new(RefCell::new(Vec::new()));
                let listener = proxy
                    .add_listener_local()
                    .param({
                        let modes = Rc::clone(&modes);
                        move |_, id, _, _, param| {
                            if id == ParamType::EnumFormat
                                && let Some(param) = param
                            {
                                modes.borrow_mut().extend(modes_of(param));
                            }
                        }
                    })
                    .register();
                proxy.enum_params(0, Some(ParamType::EnumFormat), 0, u32::MAX);
                // `node.name` comes from the device path or libcamera id, so it is the same for
                // the same camera across restarts; the object serial is not.
                let key = props
                    .get("node.name")
                    .or_else(|| props.get("object.path"))
                    .unwrap_or_default();
                let name = props
                    .get("node.description")
                    .or_else(|| props.get("node.nick"))
                    .unwrap_or("Camera")
                    .to_owned();
                let mut state = state.borrow_mut();
                state.nodes.push(Node {
                    id: global.id,
                    _proxy: proxy,
                    _listener: listener,
                    modes,
                });
                state.listed.push(Listed {
                    node: global.id,
                    id: DeviceId::from_os_key(BACKEND, key),
                    name,
                });
                publish(&shared, &state.listed);
                drop(state);
                if initial.borrow().is_none() {
                    shared.notify();
                }
            }
        })
        .global_remove({
            let state = Rc::clone(&state);
            let shared = Arc::clone(shared);
            move |id| {
                let mut state = state.borrow_mut();
                if !state.nodes.iter().any(|node| node.id == id) {
                    return;
                }
                state.nodes.retain(|node| node.id != id);
                state.listed.retain(|listed| listed.node != id);
                publish(&shared, &state.listed);
                let gone = state.active.as_ref().is_some_and(|a| a.node == id);
                let active = if gone { state.active.take() } else { None };
                drop(state);
                if let Some(active) = active {
                    active.pump.borrow_mut().end(CameraError::DeviceGone);
                    active.stop();
                }
                shared.notify();
            }
        })
        .register();

    let _core_listener = core
        .add_listener_local()
        .done({
            let state = Rc::clone(&state);
            let core = core.clone();
            let initial = Rc::clone(&initial);
            move |id, seq| {
                if id != pw::core::PW_ID_CORE {
                    return;
                }
                if *initial.borrow() == Some(seq.seq()) {
                    *initial.borrow_mut() = None;
                    let _ = ready.send(());
                }
                let pending = {
                    let mut state = state.borrow_mut();
                    match &state.pending {
                        Some(p) if p.seq == seq.seq() => state.pending.take(),
                        _ => None,
                    }
                };
                if let Some(Pending { open, .. }) = pending {
                    start(&core, &state, open);
                }
            }
        })
        .error({
            let mainloop = mainloop.clone();
            move |id, _, res, message| {
                if id == pw::core::PW_ID_CORE {
                    tracing::warn!(res, detail = message, "camera PipeWire connection lost");
                    mainloop.quit();
                }
            }
        })
        .register();

    let _commands = commands.attach(mainloop.loop_(), {
        let mainloop = mainloop.clone();
        let state = Rc::clone(&state);
        let core = core.clone();
        move |command| match command {
            Command::Open(open) => {
                let busy = {
                    let state = state.borrow();
                    state.active.is_some() || state.pending.is_some()
                };
                if busy {
                    let _ = open.reply.send(Err(CameraError::Backend(
                        "the camera is already on".to_owned(),
                    )));
                    return;
                }
                // The formats asked for when the node appeared arrive before this sync is done.
                match core.sync(0) {
                    Ok(seq) => {
                        state.borrow_mut().pending = Some(Pending {
                            seq: seq.seq(),
                            open,
                        });
                    }
                    Err(err) => {
                        let _ = open
                            .reply
                            .send(Err(CameraError::Backend(format!("camera PipeWire: {err}"))));
                    }
                }
            }
            Command::Close(done) => {
                let (active, pending) = {
                    let mut state = state.borrow_mut();
                    (state.active.take(), state.pending.take())
                };
                if let Some(active) = active {
                    active.stop();
                }
                drop(pending);
                let _ = done.send(());
            }
            Command::Quit => mainloop.quit(),
        }
    });

    *initial.borrow_mut() = Some(core.sync(0)?.seq());
    mainloop.run();

    // Drop the stream (and with it the sink) before the loop goes away.
    let active = state.borrow_mut().active.take();
    if let Some(active) = active {
        active.pump.borrow_mut().end(CameraError::Backend(
            "the camera PipeWire connection ended".to_owned(),
        ));
        active.stop();
    }
    shared
        .cameras
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clear();
    Ok(())
}

fn publish(shared: &Shared, listed: &[Listed]) {
    *shared
        .cameras
        .lock()
        .unwrap_or_else(PoisonError::into_inner) = listed.to_vec();
}

/// Picks the mode for `open` and starts its stream; answers `open.reply`.
fn start(core: &pw::core::CoreRc, state: &Rc<RefCell<LoopState>>, open: OpenRequest) {
    let OpenRequest {
        node,
        request,
        sink,
        reply,
    } = open;
    let modes = {
        let state = state.borrow();
        let Some(found) = state.nodes.iter().find(|n| n.id == node) else {
            let _ = reply.send(Err(CameraError::NoCamera));
            return;
        };
        found.modes.borrow().clone()
    };
    let Some(mode) = mode::choose(&modes, &request) else {
        let _ = reply.send(Err(CameraError::Backend(
            "the camera offers no usable picture format".to_owned(),
        )));
        return;
    };
    let Some(geometry) = Geometry::new(
        mode.width,
        mode.height,
        request.max_width,
        request.max_height,
    ) else {
        let _ = reply.send(Err(CameraError::Backend(
            "the camera picture is too small".to_owned(),
        )));
        return;
    };
    tracing::debug!(?mode, "portal camera mode");
    let pump = Rc::new(RefCell::new(Pump::new(mode, geometry, sink)));
    let limits = (request.max_width, request.max_height);
    match stream(core, node, mode, limits, &pump) {
        Ok((stream, listener)) => {
            state.borrow_mut().active = Some(Active {
                node,
                pump,
                _listener: listener,
                _stream: stream,
            });
            let _ = reply.send(Ok(()));
        }
        Err(err) => {
            pump.borrow_mut().stop();
            let _ = reply.send(Err(CameraError::Backend(format!("camera stream: {err}"))));
        }
    }
}

type StreamParts = (pw::stream::StreamRc, pw::stream::StreamListener<()>);

fn stream(
    core: &pw::core::CoreRc,
    node: u32,
    mode: Mode,
    limits: (u32, u32),
    pump: &Rc<RefCell<Pump>>,
) -> Result<StreamParts, pw::Error> {
    let stream = pw::stream::StreamRc::new(
        core.clone(),
        "mumcord-camera",
        pw::properties::properties! {
            *pw::keys::MEDIA_TYPE => "Video",
            *pw::keys::MEDIA_CATEGORY => "Capture",
            *pw::keys::MEDIA_ROLE => "Camera",
        },
    )?;
    let listener = stream
        .add_local_listener_with_user_data(())
        .state_changed({
            let pump = Rc::clone(pump);
            move |_, _, _, new| match new {
                StreamState::Error(_) => pump
                    .borrow_mut()
                    .end(CameraError::Backend("the camera stream failed".to_owned())),
                // The node went away under the stream (unplugged, access revoked).
                StreamState::Unconnected => pump.borrow_mut().end(CameraError::DeviceGone),
                _ => {}
            }
        })
        .param_changed({
            let pump = Rc::clone(pump);
            move |stream, _, id, param| {
                if id != ParamType::Format.as_raw() {
                    return;
                }
                let Some(param) = param else { return };
                if !adopt(&mut pump.borrow_mut(), param, mode, limits) {
                    return;
                }
                let Some(bytes) = buffers() else { return };
                let Some(pod) = Pod::from_bytes(&bytes) else {
                    return;
                };
                if let Err(err) = stream.update_params(&mut [pod]) {
                    tracing::warn!(%err, "camera buffer type not set");
                }
            }
        })
        .process({
            let pump = Rc::clone(pump);
            let mut layout_logged = false;
            move |stream, _| {
                let Some(mut buffer) = stream.dequeue_buffer() else {
                    return;
                };
                let Some((first, rest)) = buffer.datas_mut().split_first_mut() else {
                    return;
                };
                let mode = pump.borrow().mode();
                if !layout_logged {
                    // Producers lay out buffers differently; the first buffer's layout and the
                    // negotiated mode explain an "unreadable pictures" stop without a debugger.
                    layout_logged = true;
                    tracing::info!(
                        mode = ?mode,
                        planes = 1 + rest.len(),
                        first = ?layout(first),
                        second = ?rest.first_mut().map(layout),
                        "camera buffer layout"
                    );
                }
                let Some(first) = plane(first) else {
                    return;
                };
                let raw = match mode.format {
                    PixelFormat::Nv12 => {
                        let uv = rest.first_mut().and_then(plane);
                        let Some(raw) =
                            nv12_raw(first, uv, mode.width as usize, mode.height as usize)
                        else {
                            return;
                        };
                        raw
                    }
                    PixelFormat::Yuy2 | PixelFormat::Mjpeg => {
                        if first.size == 0 {
                            return;
                        }
                        let row = mode.width as usize * 2;
                        Raw::Packed {
                            data: first.filled(),
                            stride: first.stride_or(row),
                        }
                    }
                };
                pump.borrow_mut().push(raw);
            }
        })
        .register()?;
    let formats = offered_formats(mode).ok_or(pw::Error::CreationFailed)?;
    let mut pods = formats
        .iter()
        .map(|format| Pod::from_bytes(format))
        .collect::<Option<Vec<&Pod>>>()
        .ok_or(pw::Error::CreationFailed)?;
    stream.connect(
        spa::utils::Direction::Input,
        Some(node),
        StreamFlags::AUTOCONNECT | StreamFlags::MAP_BUFFERS,
        &mut pods,
    )?;
    Ok((stream, listener))
}

/// Reads the pictures in the format the stream negotiated instead of the requested one.
/// False (and the session ended) when that format cannot be read.
fn adopt(pump: &mut Pump, format: &Pod, requested: Mode, limits: (u32, u32)) -> bool {
    let Some(negotiated) = negotiated_mode(format, requested) else {
        tracing::warn!(
            ?requested,
            "the camera negotiated a picture format Mumcord cannot read"
        );
        pump.end(CameraError::Backend(
            "the camera offers no usable picture format".to_owned(),
        ));
        return false;
    };
    let Some(geometry) = Geometry::new(negotiated.width, negotiated.height, limits.0, limits.1)
    else {
        tracing::warn!(?negotiated, "the negotiated camera picture is too small");
        pump.end(CameraError::Backend(
            "the camera picture is too small".to_owned(),
        ));
        return false;
    };
    if negotiated != pump.mode() {
        tracing::info!(
            ?requested,
            ?negotiated,
            "camera delivers another mode than requested"
        );
        pump.renegotiate(negotiated, geometry);
    }
    true
}

/// One mapped buffer plane and the part of it PipeWire says it filled.
struct PlaneView<'a> {
    mapped: &'a [u8],
    offset: usize,
    size: usize,
    /// 0 when the producer leaves it unset; the callers then use the tight row length.
    stride: usize,
}

impl<'a> PlaneView<'a> {
    /// The filled bytes, or everything after the offset when the chunk size is unset.
    fn filled(&self) -> &'a [u8] {
        let start = self.offset.min(self.mapped.len());
        let end = if self.size == 0 {
            self.mapped.len()
        } else {
            start.saturating_add(self.size).min(self.mapped.len())
        };
        &self.mapped[start..end]
    }

    /// Everything after the offset, whatever the chunk size says.
    fn rest(&self) -> &'a [u8] {
        &self.mapped[self.offset.min(self.mapped.len())..]
    }

    fn stride_or(&self, tight: usize) -> usize {
        if self.stride == 0 { tight } else { self.stride }
    }
}

/// Offset, size and stride of a plane's chunk and the length of its mapping, for the log.
fn layout(data: &mut spa::buffer::Data) -> (u32, u32, i32, Option<usize>) {
    let chunk = data.chunk();
    let (offset, size, stride) = (chunk.offset(), chunk.size(), chunk.stride());
    (offset, size, stride, data.data().map(|mapped| mapped.len()))
}

/// A buffer plane with mapped memory. `None` when it has none (no picture).
fn plane(data: &mut spa::buffer::Data) -> Option<PlaneView<'_>> {
    let chunk = data.chunk();
    let (offset, size) = (chunk.offset() as usize, chunk.size() as usize);
    let stride = usize::try_from(chunk.stride()).unwrap_or(0);
    let mapped: &[u8] = data.data()?;
    if mapped.is_empty() {
        return None;
    }
    Some(PlaneView {
        mapped,
        offset,
        size,
        stride,
    })
}

/// NV12 from one plane (Y followed by UV) or two (Y, UV). PipeWire producers differ: some leave
/// the stride at 0 and some fill only the first chunk's size, so both fall back to the tight
/// layout instead of reporting every picture as unreadable. `None` when the first plane has no
/// picture.
fn nv12_raw<'a>(
    y: PlaneView<'a>,
    uv: Option<PlaneView<'a>>,
    width: usize,
    height: usize,
) -> Option<Raw<'a>> {
    if y.size == 0 && uv.is_none() {
        return None;
    }
    let y_stride = y.stride_or(width);
    let luma = y.filled();
    Some(match uv {
        Some(uv) => {
            let uv_stride = uv.stride_or(y_stride);
            let chroma = uv.filled();
            let chroma = &chroma[..chroma.len().min(uv_stride * height.div_ceil(2))];
            Raw::Nv12 {
                y: luma,
                y_stride,
                uv: chroma,
                uv_stride,
            }
        }
        None => {
            // Some producers set the chunk size to the Y plane alone although the chroma
            // follows it in the same buffer; then the chroma lies beyond the size.
            let whole = y_stride * height + y_stride * height.div_ceil(2);
            let luma = if luma.len() < whole { y.rest() } else { luma };
            let split = (y_stride * height).min(luma.len());
            let (y, uv) = luma.split_at(split);
            Raw::Nv12 {
                y,
                y_stride,
                uv,
                uv_stride: y_stride,
            }
        }
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

/// The stream's `EnumFormat` params: every format Mumcord can read, the chosen one first, each at
/// any size and rate but preferring the chosen ones. PipeWire tries the params in order and
/// fixates a choice to its default, so it settles on the chosen mode when the camera can do it
/// and otherwise on any readable mode (another app may hold the camera in its own mode). The
/// pictures are read in whatever was negotiated, see [`negotiated_mode`]. PipeWire files a
/// connect param under its object id, so only `EnumFormat` restricts the negotiation.
fn offered_formats(mode: Mode) -> Option<Vec<Vec<u8>>> {
    let others = [PixelFormat::Nv12, PixelFormat::Yuy2, PixelFormat::Mjpeg]
        .into_iter()
        .filter(|&format| format != mode.format);
    std::iter::once(mode.format)
        .chain(others)
        .map(|format| offered_format(format, mode))
        .collect()
}

/// One `EnumFormat` param: `format` at any size and rate, preferring those of `preferred`.
fn offered_format(format: PixelFormat, preferred: Mode) -> Option<Vec<u8>> {
    let subtype = match format {
        PixelFormat::Mjpeg => MediaSubtype::Mjpg,
        PixelFormat::Nv12 | PixelFormat::Yuy2 => MediaSubtype::Raw,
    };
    let mut properties = vec![
        property(
            FormatProperties::MediaType,
            Value::Id(Id(MediaType::Video.as_raw())),
        ),
        property(
            FormatProperties::MediaSubtype,
            Value::Id(Id(subtype.as_raw())),
        ),
    ];
    let raw_format = match format {
        PixelFormat::Nv12 => Some(VideoFormat::NV12),
        PixelFormat::Yuy2 => Some(VideoFormat::YUY2),
        PixelFormat::Mjpeg => None,
    };
    if let Some(format) = raw_format {
        properties.push(property(
            FormatProperties::VideoFormat,
            Value::Id(Id(format.as_raw())),
        ));
    }
    properties.push(property(
        FormatProperties::VideoSize,
        Value::Choice(ChoiceValue::Rectangle(Choice(
            spa::utils::ChoiceFlags::empty(),
            ChoiceEnum::Range {
                default: Rectangle {
                    width: preferred.width,
                    height: preferred.height,
                },
                min: Rectangle {
                    width: 1,
                    height: 1,
                },
                max: Rectangle {
                    width: MAX_SIDE,
                    height: MAX_SIDE,
                },
            },
        ))),
    ));
    properties.push(property(
        FormatProperties::VideoFramerate,
        Value::Choice(ChoiceValue::Fraction(Choice(
            spa::utils::ChoiceFlags::empty(),
            ChoiceEnum::Range {
                default: Fraction {
                    num: preferred.rate.num,
                    denom: preferred.rate.den,
                },
                // 0/1 is how PipeWire describes a variable frame rate.
                min: Fraction { num: 0, denom: 1 },
                max: Fraction {
                    num: MAX_OFFERED_FPS,
                    denom: 1,
                },
            },
        ))),
    ));
    serialize(Object {
        type_: SpaTypes::ObjectParamFormat.as_raw(),
        id: ParamType::EnumFormat.as_raw(),
        properties,
    })
}

/// Bounds of the "any size and rate" ranges: far beyond any camera, so they never exclude one.
const MAX_SIDE: u32 = 16384;
const MAX_OFFERED_FPS: u32 = 1000;

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
            value: Value::Choice(ChoiceValue::Int(Choice(
                spa::utils::ChoiceFlags::empty(),
                ChoiceEnum::Flags {
                    default: types,
                    flags: vec![],
                },
            ))),
        }],
    })
}

/// The modes one `EnumFormat` param of a camera node describes.
fn modes_of(param: &Pod) -> Vec<Mode> {
    let Some((formats, sizes, rates)) = format_parts(param) else {
        return Vec::new();
    };
    let mut modes = Vec::new();
    for &format in &formats {
        for &(width, height) in &sizes {
            for &rate in &rates {
                modes.push(Mode {
                    format,
                    width,
                    height,
                    rate,
                });
            }
        }
    }
    modes
}

/// The mode the stream settled on, from its negotiated `Format` param. That is what the camera
/// sends, whatever was asked for: a camera that is busy elsewhere or ignores the request
/// delivers its own format and size. A format without a frame rate keeps the requested rate;
/// the rate does not change how a picture is read. `None` when the format is not one Mumcord
/// can read.
fn negotiated_mode(param: &Pod, requested: Mode) -> Option<Mode> {
    let (formats, sizes, rates) = format_parts(param)?;
    let (&format, &(width, height)) = (formats.first()?, sizes.first()?);
    Some(Mode {
        format,
        width,
        height,
        rate: rates.first().copied().unwrap_or(requested.rate),
    })
}

/// Pixel formats Mumcord can read, picture sizes and frame rates of a format param.
type FormatParts = (Vec<PixelFormat>, Vec<(u32, u32)>, Vec<Rate>);

fn format_parts(param: &Pod) -> Option<FormatParts> {
    let Ok((_, Value::Object(object))) = PodDeserializer::deserialize_any_from(param.as_bytes())
    else {
        return None;
    };
    let find = |key: FormatProperties| {
        object
            .properties
            .iter()
            .find(|p| p.key == key.as_raw())
            .map(|p| &p.value)
    };
    if find(FormatProperties::MediaType) != Some(&Value::Id(Id(MediaType::Video.as_raw()))) {
        return None;
    }
    let formats: Vec<PixelFormat> = match find(FormatProperties::MediaSubtype) {
        Some(Value::Id(Id(subtype))) if *subtype == MediaSubtype::Mjpg.as_raw() => {
            vec![PixelFormat::Mjpeg]
        }
        Some(Value::Id(Id(subtype))) if *subtype == MediaSubtype::Raw.as_raw() => {
            choices(find(FormatProperties::VideoFormat), |v| match v {
                Value::Id(id) => Some(*id),
                _ => None,
            })
            .into_iter()
            .filter_map(|Id(raw)| {
                if raw == VideoFormat::NV12.as_raw() {
                    Some(PixelFormat::Nv12)
                } else if raw == VideoFormat::YUY2.as_raw() {
                    Some(PixelFormat::Yuy2)
                } else {
                    None
                }
            })
            .collect()
        }
        _ => return None,
    };
    Some((
        formats,
        sizes(find(FormatProperties::VideoSize)),
        rates(find(FormatProperties::VideoFramerate)),
    ))
}

/// The values of a fixed property or of an enum choice; ranges are handled by the callers.
fn choices<T: CanonicalFixedSizedPod + Clone>(
    value: Option<&Value>,
    single: impl Fn(&Value) -> Option<T>,
) -> Vec<T>
where
    ChoiceValue: ChoiceOf<T>,
{
    match value {
        Some(Value::Choice(choice)) => match ChoiceValue::choice(choice) {
            Some(ChoiceEnum::None(v)) => vec![v.clone()],
            Some(ChoiceEnum::Enum {
                default,
                alternatives,
            }) => {
                if alternatives.is_empty() {
                    vec![default.clone()]
                } else {
                    alternatives.clone()
                }
            }
            _ => Vec::new(),
        },
        Some(other) => single(other).into_iter().collect(),
        None => Vec::new(),
    }
}

/// Reads the typed choice out of a [`ChoiceValue`].
trait ChoiceOf<T: CanonicalFixedSizedPod> {
    fn choice(&self) -> Option<&ChoiceEnum<T>>;
}

impl ChoiceOf<Id> for ChoiceValue {
    fn choice(&self) -> Option<&ChoiceEnum<Id>> {
        match self {
            ChoiceValue::Id(Choice(_, choice)) => Some(choice),
            _ => None,
        }
    }
}

impl ChoiceOf<Rectangle> for ChoiceValue {
    fn choice(&self) -> Option<&ChoiceEnum<Rectangle>> {
        match self {
            ChoiceValue::Rectangle(Choice(_, choice)) => Some(choice),
            _ => None,
        }
    }
}

impl ChoiceOf<Fraction> for ChoiceValue {
    fn choice(&self) -> Option<&ChoiceEnum<Fraction>> {
        match self {
            ChoiceValue::Fraction(Choice(_, choice)) => Some(choice),
            _ => None,
        }
    }
}

fn sizes(value: Option<&Value>) -> Vec<(u32, u32)> {
    let range = |min: &Rectangle, max: &Rectangle, step: (u32, u32)| {
        ranges::sizes_in_range((min.width, min.height), (max.width, max.height), step).collect()
    };
    if let Some(Value::Choice(choice)) = value {
        match ChoiceOf::<Rectangle>::choice(choice) {
            Some(ChoiceEnum::Range { min, max, .. }) => return range(min, max, (1, 1)),
            Some(ChoiceEnum::Step { min, max, step, .. }) => {
                return range(min, max, (step.width, step.height));
            }
            _ => {}
        }
    }
    choices(value, |v| match v {
        Value::Rectangle(r) => Some(*r),
        _ => None,
    })
    .into_iter()
    .map(|r| (r.width, r.height))
    .collect()
}

fn rates(value: Option<&Value>) -> Vec<Rate> {
    let rate = |f: &Fraction| Rate {
        num: f.num,
        den: f.denom,
    };
    if let Some(Value::Choice(choice)) = value
        && let Some(ChoiceEnum::Range { min, max, .. } | ChoiceEnum::Step { min, max, .. }) =
            ChoiceOf::<Fraction>::choice(choice)
    {
        return vec![ranges::rate_in_range(rate(min), rate(max), crate::MAX_FPS)];
    }
    choices(value, |v| match v {
        Value::Fraction(f) => Some(*f),
        _ => None,
    })
    .iter()
    .map(rate)
    .collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn view(mapped: &[u8], offset: usize, size: usize, stride: usize) -> PlaneView<'_> {
        PlaneView {
            mapped,
            offset,
            size,
            stride,
        }
    }

    fn nv12_parts<'a>(raw: Raw<'a>) -> (&'a [u8], usize, &'a [u8], usize) {
        match raw {
            Raw::Nv12 {
                y,
                y_stride,
                uv,
                uv_stride,
            } => (y, y_stride, uv, uv_stride),
            Raw::Packed { .. } => panic!("expected NV12"),
        }
    }

    // 8×4 NV12: 32 bytes of Y, then 16 bytes of interleaved UV.
    const W: usize = 8;
    const H: usize = 4;

    #[test]
    fn one_plane_without_a_stride_uses_the_tight_row() {
        let frame = [7u8; W * H * 3 / 2];
        let raw = nv12_raw(view(&frame, 0, frame.len(), 0), None, W, H).unwrap();
        let (y, y_stride, uv, uv_stride) = nv12_parts(raw);
        assert_eq!(
            (y.len(), y_stride, uv.len(), uv_stride),
            (W * H, W, W * H / 2, W)
        );
    }

    #[test]
    fn one_plane_whose_size_covers_only_the_luma_still_gives_the_chroma() {
        let mut frame = [1u8; W * H * 3 / 2];
        frame[W * H..].fill(2);
        let raw = nv12_raw(view(&frame, 0, W * H, W), None, W, H).unwrap();
        let (y, _, uv, uv_stride) = nv12_parts(raw);
        assert_eq!((y.len(), uv.len(), uv_stride), (W * H, W * H / 2, W));
        assert!(uv.iter().all(|&b| b == 2));
    }

    #[test]
    fn a_second_plane_without_a_chunk_size_still_gives_the_chroma() {
        let luma = [1u8; W * H];
        let chroma = [2u8; W * H / 2];
        let raw = nv12_raw(
            view(&luma, 0, luma.len(), W),
            Some(view(&chroma, 0, 0, 0)),
            W,
            H,
        )
        .unwrap();
        let (y, y_stride, uv, uv_stride) = nv12_parts(raw);
        assert_eq!(
            (y.len(), y_stride, uv.len(), uv_stride),
            (W * H, W, W * H / 2, W)
        );
        assert!(uv.iter().all(|&b| b == 2));
    }

    #[test]
    fn a_padded_two_plane_frame_keeps_its_strides() {
        let stride = 16;
        let luma = [1u8; 16 * H];
        let chroma = [2u8; 16 * H / 2];
        let raw = nv12_raw(
            view(&luma, 0, luma.len(), stride),
            Some(view(&chroma, 0, chroma.len(), stride)),
            W,
            H,
        )
        .unwrap();
        let (_, y_stride, uv, uv_stride) = nv12_parts(raw);
        assert_eq!(
            (y_stride, uv_stride, uv.len()),
            (stride, stride, 16 * H / 2)
        );
    }

    #[test]
    fn an_empty_single_plane_is_no_picture() {
        let frame = [0u8; W * H * 3 / 2];
        assert!(nv12_raw(view(&frame, 0, 0, W), None, W, H).is_none());
    }

    fn enum_format(subtype: MediaSubtype, extra: Vec<Property>) -> Vec<u8> {
        let mut properties = vec![
            property(
                FormatProperties::MediaType,
                Value::Id(Id(MediaType::Video.as_raw())),
            ),
            property(
                FormatProperties::MediaSubtype,
                Value::Id(Id(subtype.as_raw())),
            ),
        ];
        properties.extend(extra);
        serialize(Object {
            type_: SpaTypes::ObjectParamFormat.as_raw(),
            id: ParamType::EnumFormat.as_raw(),
            properties,
        })
        .unwrap()
    }

    fn modes(bytes: &[u8]) -> Vec<Mode> {
        modes_of(Pod::from_bytes(bytes).unwrap())
    }

    #[test]
    fn a_v4l2_style_raw_format_lists_its_modes() {
        // What the PipeWire V4L2 plugin offers for a UVC camera: one size, enumerated rates.
        let bytes = enum_format(
            MediaSubtype::Raw,
            vec![
                property(
                    FormatProperties::VideoFormat,
                    Value::Id(Id(VideoFormat::YUY2.as_raw())),
                ),
                property(
                    FormatProperties::VideoSize,
                    Value::Rectangle(Rectangle {
                        width: 1280,
                        height: 720,
                    }),
                ),
                property(
                    FormatProperties::VideoFramerate,
                    Value::Choice(ChoiceValue::Fraction(Choice(
                        spa::utils::ChoiceFlags::empty(),
                        ChoiceEnum::Enum {
                            default: Fraction { num: 10, denom: 1 },
                            alternatives: vec![
                                Fraction { num: 10, denom: 1 },
                                Fraction { num: 5, denom: 1 },
                            ],
                        },
                    ))),
                ),
            ],
        );
        let rate = |num| Rate { num, den: 1 };
        assert_eq!(
            modes(&bytes),
            [10, 5].map(|fps| Mode {
                format: PixelFormat::Yuy2,
                width: 1280,
                height: 720,
                rate: rate(fps),
            })
        );
    }

    #[test]
    fn mjpeg_and_ranges_are_read_and_unknown_formats_skipped() {
        let mjpeg = enum_format(
            MediaSubtype::Mjpg,
            vec![
                property(
                    FormatProperties::VideoSize,
                    Value::Choice(ChoiceValue::Rectangle(Choice(
                        spa::utils::ChoiceFlags::empty(),
                        ChoiceEnum::Range {
                            default: Rectangle {
                                width: 640,
                                height: 480,
                            },
                            min: Rectangle {
                                width: 160,
                                height: 120,
                            },
                            max: Rectangle {
                                width: 1280,
                                height: 720,
                            },
                        },
                    ))),
                ),
                property(
                    FormatProperties::VideoFramerate,
                    Value::Choice(ChoiceValue::Fraction(Choice(
                        spa::utils::ChoiceFlags::empty(),
                        ChoiceEnum::Range {
                            default: Fraction { num: 30, denom: 1 },
                            min: Fraction { num: 1, denom: 1 },
                            max: Fraction { num: 60, denom: 1 },
                        },
                    ))),
                ),
            ],
        );
        let found = modes(&mjpeg);
        assert!(found.iter().all(|m| m.format == PixelFormat::Mjpeg));
        assert!(found.contains(&Mode {
            format: PixelFormat::Mjpeg,
            width: 1280,
            height: 720,
            rate: Rate::fps(30),
        }));

        let rgb = enum_format(
            MediaSubtype::Raw,
            vec![property(
                FormatProperties::VideoFormat,
                Value::Id(Id(VideoFormat::RGB.as_raw())),
            )],
        );
        assert!(modes(&rgb).is_empty());
    }

    /// The preferred value of a fixed property or of a range choice.
    fn preferred<T: CanonicalFixedSizedPod + Clone>(
        object: &Object,
        key: FormatProperties,
        single: impl Fn(&Value) -> Option<T>,
    ) -> Option<T>
    where
        ChoiceValue: ChoiceOf<T>,
    {
        let value = &object
            .properties
            .iter()
            .find(|p| p.key == key.as_raw())?
            .value;
        match value {
            Value::Choice(choice) => match ChoiceValue::choice(choice)? {
                ChoiceEnum::Range { default, .. } => Some(default.clone()),
                _ => None,
            },
            other => single(other),
        }
    }

    /// The mode one offered `EnumFormat` prefers: its format, default size and default rate.
    fn preferred_mode(bytes: &[u8]) -> Mode {
        let Ok((_, Value::Object(object))) = PodDeserializer::deserialize_any_from(bytes) else {
            panic!("not an object");
        };
        assert_eq!(object.id, ParamType::EnumFormat.as_raw());
        assert_eq!(object.type_, SpaTypes::ObjectParamFormat.as_raw());
        let subtype = preferred(&object, FormatProperties::MediaSubtype, |v| match v {
            Value::Id(id) => Some(*id),
            _ => None,
        })
        .unwrap();
        let format = if subtype == Id(MediaSubtype::Mjpg.as_raw()) {
            PixelFormat::Mjpeg
        } else {
            let Id(raw) = preferred(&object, FormatProperties::VideoFormat, |v| match v {
                Value::Id(id) => Some(*id),
                _ => None,
            })
            .unwrap();
            if raw == VideoFormat::NV12.as_raw() {
                PixelFormat::Nv12
            } else if raw == VideoFormat::YUY2.as_raw() {
                PixelFormat::Yuy2
            } else {
                panic!("unreadable format offered")
            }
        };
        let size = preferred(&object, FormatProperties::VideoSize, |v| match v {
            Value::Rectangle(r) => Some(*r),
            _ => None,
        })
        .unwrap();
        let rate = preferred(&object, FormatProperties::VideoFramerate, |v| match v {
            Value::Fraction(f) => Some(*f),
            _ => None,
        })
        .unwrap();
        Mode {
            format,
            width: size.width,
            height: size.height,
            rate: Rate {
                num: rate.num,
                den: rate.denom,
            },
        }
    }

    const REQUESTED: Mode = Mode {
        format: PixelFormat::Yuy2,
        width: 1280,
        height: 720,
        rate: Rate {
            num: 30000,
            den: 1001,
        },
    };

    #[test]
    fn the_requested_mode_is_offered_first_and_preferred() {
        for format in [PixelFormat::Nv12, PixelFormat::Yuy2, PixelFormat::Mjpeg] {
            let mode = Mode {
                format,
                ..REQUESTED
            };
            let offers = offered_formats(mode).unwrap();
            assert_eq!(preferred_mode(&offers[0]), mode);
        }
    }

    #[test]
    fn every_readable_format_is_offered_once_preferring_the_requested_size_and_rate() {
        // A camera another app holds in a different mode must still negotiate something.
        let offers = offered_formats(REQUESTED).unwrap();
        let preferred: Vec<Mode> = offers.iter().map(|o| preferred_mode(o)).collect();
        assert_eq!(
            preferred,
            [PixelFormat::Yuy2, PixelFormat::Nv12, PixelFormat::Mjpeg].map(|format| Mode {
                format,
                ..REQUESTED
            })
        );
    }

    #[test]
    fn the_offers_accept_any_size_and_rate() {
        for offer in offered_formats(REQUESTED).unwrap() {
            let found = modes(&offer);
            assert!(found.iter().any(|m| (m.width, m.height) == (640, 480)));
            assert!(found.iter().any(|m| (m.width, m.height) == (320, 240)));
        }
    }

    #[test]
    fn the_negotiated_format_wins_over_the_requested_mode() {
        // The log of issue: NV12 1280x720 asked for, YUYV 640x480 delivered.
        let negotiated = enum_format(
            MediaSubtype::Raw,
            vec![
                property(
                    FormatProperties::VideoFormat,
                    Value::Id(Id(VideoFormat::YUY2.as_raw())),
                ),
                property(
                    FormatProperties::VideoSize,
                    Value::Rectangle(Rectangle {
                        width: 640,
                        height: 480,
                    }),
                ),
                property(
                    FormatProperties::VideoFramerate,
                    Value::Fraction(Fraction { num: 30, denom: 1 }),
                ),
            ],
        );
        let requested = Mode {
            format: PixelFormat::Nv12,
            width: 1280,
            height: 720,
            rate: Rate { num: 30, den: 1 },
        };
        let mode = negotiated_mode(Pod::from_bytes(&negotiated).unwrap(), requested).unwrap();
        assert_eq!(
            mode,
            Mode {
                format: PixelFormat::Yuy2,
                width: 640,
                height: 480,
                rate: Rate { num: 30, den: 1 },
            }
        );
    }

    #[test]
    fn a_negotiated_format_without_a_frame_rate_keeps_the_requested_rate() {
        let negotiated = enum_format(
            MediaSubtype::Raw,
            vec![
                property(
                    FormatProperties::VideoFormat,
                    Value::Id(Id(VideoFormat::NV12.as_raw())),
                ),
                property(
                    FormatProperties::VideoSize,
                    Value::Rectangle(Rectangle {
                        width: 1280,
                        height: 720,
                    }),
                ),
            ],
        );
        let requested = Mode {
            format: PixelFormat::Nv12,
            width: 1280,
            height: 720,
            rate: Rate { num: 30, den: 1 },
        };
        let mode = negotiated_mode(Pod::from_bytes(&negotiated).unwrap(), requested);
        assert_eq!(mode, Some(requested));
    }

    #[test]
    fn an_unusable_negotiated_format_gives_no_mode() {
        let negotiated = enum_format(
            MediaSubtype::Raw,
            vec![property(
                FormatProperties::VideoFormat,
                Value::Id(Id(VideoFormat::RGB.as_raw())),
            )],
        );
        let requested = Mode {
            format: PixelFormat::Nv12,
            width: 1280,
            height: 720,
            rate: Rate { num: 30, den: 1 },
        };
        assert_eq!(
            negotiated_mode(Pod::from_bytes(&negotiated).unwrap(), requested),
            None
        );
    }
}
