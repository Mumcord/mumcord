//! The share's sound on Linux through PipeWire (FR-7.81, FR-7.82, FR-7.86), on Wayland and X11.
//!
//! Mumcord opens one capture stream of its own that the session manager never connects, and
//! links the output ports of the wanted playback streams straight into it. PipeWire mixes
//! everything linked to one input port, so the desktop mix needs no mixer of Mumcord's and the
//! streams stay in step. The registry is watched for the whole share: a playback stream that
//! starts is linked as soon as its ports appear, and the links of a stream that ends go away
//! with it.
//!
//! Device monitors are never used, so Mumcord's own output (voices, sounds, watched shares)
//! cannot reach the mix by that route; Mumcord's own playback streams are recognised by the
//! process ids of its whole process tree, which includes the web view's processes, never by
//! name. A stream without any process id is left out too: it could be Mumcord's.
//!
//! The PipeWire loop runs on its own thread. Its process callback only turns samples into
//! 10 ms mono frames on a lock-free queue, which the share's sound thread takes.
//!
//! Logs carry reason codes only, never app names, binaries or process ids (FR-7.87).

use mumcord_audio::{FRAME_SAMPLES, Frame};

use super::matcher::{MatchedApp, SoundStream};

#[cfg(target_os = "linux")]
pub use pipewire_side::{PipeWireSound, playback_streams, running};

/// What a Linux share's sound is made of.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SoundTarget {
    /// Every playback stream except Mumcord's (whole-screen share, FR-7.81).
    Desktop,
    /// Only this app's streams, followed by process id and binary, so streams the app opens
    /// during the share are included too (window share, FR-7.82).
    App(MatchedApp),
}

impl SoundTarget {
    /// Whether `stream` belongs in the share's sound; never one of `own` (Mumcord's processes).
    fn wants(&self, stream: &Playback, own: &[u32]) -> bool {
        if stream.is_own(own) {
            return false;
        }
        match self {
            Self::Desktop => true,
            Self::App(app) => {
                stream.pid().is_some_and(|pid| app.pids.contains(&pid))
                    || app
                        .binary
                        .as_deref()
                        .zip(stream.binary.as_deref())
                        .is_some_and(|(app, stream)| app.eq_ignore_ascii_case(stream))
            }
        }
    }
}

/// What PipeWire tells about one playback stream.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Playback {
    app_name: Option<String>,
    binary: Option<String>,
    desktop_id: Option<String>,
    /// The freedesktop icon name the app gives its streams.
    icon_name: Option<String>,
    /// The process id the app reports for itself.
    app_pid: Option<u32>,
    /// The process id PipeWire read from the connection. For apps that play through the
    /// PulseAudio layer this is the layer's own process, so it only stands in for a missing
    /// `app_pid`.
    socket_pid: Option<u32>,
}

impl Playback {
    fn from_props<'a>(prop: impl Fn(&str) -> Option<&'a str>) -> Self {
        let text = |key: &str| {
            prop(key)
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(String::from)
        };
        let pid = |key: &str| prop(key).and_then(|v| v.trim().parse().ok());
        Self {
            app_name: text("application.name"),
            binary: text("application.process.binary"),
            desktop_id: text("application.id").or_else(|| text("pipewire.access.portal.app_id")),
            icon_name: text("application.icon-name"),
            app_pid: pid("application.process.id"),
            socket_pid: pid("pipewire.sec.pid"),
        }
    }

    fn pid(&self) -> Option<u32> {
        self.app_pid.or(self.socket_pid)
    }

    /// Fills what the stream itself does not say from its client's details.
    fn or_client(self, client: Option<&Playback>) -> Self {
        let Some(client) = client else { return self };
        Self {
            app_name: self.app_name.or_else(|| client.app_name.clone()),
            binary: self.binary.or_else(|| client.binary.clone()),
            desktop_id: self.desktop_id.or_else(|| client.desktop_id.clone()),
            icon_name: self.icon_name.or_else(|| client.icon_name.clone()),
            app_pid: self.app_pid.or(client.app_pid),
            socket_pid: self.socket_pid.or(client.socket_pid),
        }
    }

    /// Played by Mumcord itself, or by a process nobody knows: never captured.
    fn is_own(&self, own: &[u32]) -> bool {
        let pids = [self.app_pid, self.socket_pid];
        pids.iter().all(Option::is_none) || pids.iter().flatten().any(|pid| own.contains(pid))
    }

    /// The stream as the matcher and the "Which app's sound?" list see it.
    fn stream(&self) -> SoundStream {
        SoundStream {
            app_name: self.app_name.clone(),
            binary: self.binary.clone(),
            desktop_id: self.desktop_id.clone(),
            icon_name: self.icon_name.clone(),
            pid: self.pid(),
        }
    }
}

/// Mumcord's own process ids: this process and all its descendants, the web view's processes
/// among them. Read anew whenever a stream appears, as the web view starts processes late.
pub fn own_pids() -> Vec<u32> {
    process_tree(std::process::id(), &process_table())
}

/// Every process with its parent, from `/proc`.
fn process_table() -> Vec<(u32, u32)> {
    let dir = match std::fs::read_dir("/proc") {
        Ok(dir) => dir,
        Err(e) => {
            tracing::warn!(error = %e, reason = "no_proc", "own processes unknown");
            return Vec::new();
        }
    };
    dir.flatten()
        .filter_map(|entry| {
            let pid = entry.file_name().to_str()?.parse().ok()?;
            // Processes may end while the table is read; they are simply left out.
            let stat = std::fs::read_to_string(entry.path().join("stat")).ok()?;
            Some((pid, parent_of(&stat)?))
        })
        .collect()
}

/// The parent process id from `/proc/<pid>/stat`. The process name in parentheses may contain
/// spaces and parentheses itself, so fields are counted from its last `)`.
fn parent_of(stat: &str) -> Option<u32> {
    let rest = stat.get(stat.rfind(')')? + 1..)?;
    rest.split_whitespace().nth(1)?.parse().ok()
}

/// `root` and all its descendants in `table` (pid, parent pid).
fn process_tree(root: u32, table: &[(u32, u32)]) -> Vec<u32> {
    let mut tree = vec![root];
    let mut next = 0;
    while let Some(&parent) = tree.get(next) {
        let children: Vec<u32> = table
            .iter()
            .filter(|(pid, ppid)| *ppid == parent && !tree.contains(pid))
            .map(|(pid, _)| *pid)
            .collect();
        tree.extend(children);
        next += 1;
    }
    tree
}

/// Which input of Mumcord's stereo capture stream a playback channel goes to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Side {
    Left,
    Right,
    /// Mono, centre, LFE and unnamed channels go to both.
    Both,
}

/// The side of a port's `audio.channel` (`FL`, `RR`, `MONO`, `AUX0`, …).
fn side(channel: Option<&str>) -> Side {
    match channel {
        Some(c) if c.ends_with('L') => Side::Left,
        Some(c) if c.ends_with('R') => Side::Right,
        _ => Side::Both,
    }
}

/// The links to have: each output port of a wanted stream into the capture input of its side
/// (`inputs` = left, right), as (output port, input port).
fn wanted_links(outputs: impl Iterator<Item = (u32, Side)>, inputs: [u32; 2]) -> Vec<(u32, u32)> {
    let [left, right] = inputs;
    let mut links = Vec::new();
    for (port, side) in outputs {
        match side {
            Side::Left => links.push((port, left)),
            Side::Right => links.push((port, right)),
            Side::Both => links.extend([(port, left), (port, right)]),
        }
    }
    links
}

/// Turns interleaved stereo f32 samples into 10 ms mono frames. Runs in the process callback:
/// no allocation.
#[derive(Debug)]
struct Frames {
    frame: Frame,
    fill: usize,
}

impl Default for Frames {
    fn default() -> Self {
        Self {
            frame: [0.0; FRAME_SAMPLES],
            fill: 0,
        }
    }
}

impl Frames {
    /// Adds little-endian stereo f32 `bytes`; `done` gets every frame that fills up.
    fn push_stereo_le(&mut self, bytes: &[u8], mut done: impl FnMut(&Frame)) {
        for &[l0, l1, l2, l3, r0, r1, r2, r3] in bytes.as_chunks::<8>().0 {
            let left = f32::from_le_bytes([l0, l1, l2, l3]);
            let right = f32::from_le_bytes([r0, r1, r2, r3]);
            if let Some(sample) = self.frame.get_mut(self.fill) {
                // Several apps add up on the same port, so the sum may leave the range.
                *sample = ((left + right) * 0.5).clamp(-1.0, 1.0);
            }
            self.fill += 1;
            if self.fill == FRAME_SAMPLES {
                done(&self.frame);
                self.fill = 0;
            }
        }
    }
}

#[cfg(target_os = "linux")]
mod pipewire_side {
    use std::cell::{Cell, RefCell};
    use std::collections::HashMap;
    use std::rc::Rc;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc;
    use std::thread::JoinHandle;
    use std::time::{Duration, Instant};

    use crossbeam_queue::ArrayQueue;
    use mumcord_audio::{Frame, SAMPLE_RATE};
    use pipewire as pw;
    use pw::client::{Client, ClientChangeMask, ClientListener};
    use pw::context::ContextRc;
    use pw::core::CoreRc;
    use pw::link::Link;
    use pw::loop_::Timeout;
    use pw::main_loop::MainLoopRc;
    use pw::node::{Node, NodeChangeMask, NodeListener};
    use pw::properties::PropertiesBox;
    use pw::registry::{GlobalObject, RegistryRc};
    use pw::spa;
    use pw::spa::utils::dict::DictRef;
    use pw::stream::{StreamFlags, StreamRc, StreamState};
    use pw::types::ObjectType;

    use super::{Frames, Playback, Side, SoundTarget, own_pids, side, wanted_links};
    use crate::screen_share::capture::SoundSource;
    use crate::screen_share::sound::SoundUnavailable;
    use crate::screen_share::sound::matcher::SoundStream;

    /// Ready frames between the PipeWire loop and the sound thread: half a second. When the
    /// sound thread stalls longer, the oldest frames are dropped.
    const QUEUE_FRAMES: usize = 50;
    /// How long connecting and opening the capture stream may take.
    const START_TIMEOUT: Duration = Duration::from_secs(3);
    /// How long one round trip to the daemon may take while listing.
    const LIST_TIMEOUT: Duration = Duration::from_secs(2);
    /// How often the sound thread looks for a new frame while it waits.
    const POLL: Duration = Duration::from_millis(2);

    const PLAYBACK_CLASS: &str = "Stream/Output/Audio";

    /// Whether a PipeWire daemon answers (FR-7.84). libpipewire itself is linked into Mumcord
    /// (cpal and the Wayland capture use it); what can be missing is the running daemon, e.g. on
    /// a PulseAudio-only system.
    pub fn running() -> bool {
        let connected = connect().is_ok();
        if !connected {
            tracing::info!(reason = "no_pipewire", "screen share sound unavailable");
        }
        connected
    }

    fn connect() -> Result<(MainLoopRc, CoreRc), pw::Error> {
        pw::init();
        let mainloop = MainLoopRc::new(None)?;
        let context = ContextRc::new(&mainloop, None)?;
        let core = context.connect_rc(None)?;
        Ok((mainloop, core))
    }

    /// The playback streams running now, without Mumcord's own: what the window matcher and
    /// the "Which app's sound?" list work with (FR-7.82, FR-7.83). Call it again to refresh.
    pub fn playback_streams() -> Result<Vec<SoundStream>, SoundUnavailable> {
        let found = list().map_err(|e| {
            tracing::info!(error = %e, reason = "no_pipewire", "playback streams not listed");
            SoundUnavailable::NoPipeWire
        })?;
        let own = own_pids();
        Ok(found
            .iter()
            .filter(|p| !p.is_own(&own))
            .map(Playback::stream)
            .collect())
    }

    fn list() -> Result<Vec<Playback>, pw::Error> {
        let (mainloop, core) = connect()?;
        let registry = core.get_registry_rc()?;
        let graph = Rc::new(RefCell::new(Graph::new(None, core.clone(), None)));
        let _registry_listener = Graph::watch(&graph, &registry);
        // Round trips: the globals, then the bound objects' info.
        roundtrip(&mainloop, &core)?;
        roundtrip(&mainloop, &core)?;
        let graph = graph.borrow();
        Ok(graph
            .nodes
            .keys()
            .filter_map(|id| graph.playback(*id))
            .collect())
    }

    /// Runs the loop until the daemon answered everything sent so far.
    fn roundtrip(mainloop: &MainLoopRc, core: &CoreRc) -> Result<(), pw::Error> {
        let done = Rc::new(Cell::new(false));
        let pending = core.sync(0)?;
        let _listener = core
            .add_listener_local()
            .done({
                let done = done.clone();
                move |id, seq| {
                    if id == pw::core::PW_ID_CORE && seq == pending {
                        done.set(true);
                    }
                }
            })
            .register();
        let deadline = Instant::now() + LIST_TIMEOUT;
        while !done.get() {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(pw::Error::CreationFailed);
            }
            mainloop.loop_().iterate(Timeout::Finite(left));
        }
        Ok(())
    }

    fn props_of<P: AsRef<DictRef>>(global: &GlobalObject<P>, key: &str) -> Option<String> {
        global
            .props
            .as_ref()
            .and_then(|p| p.as_ref().get(key))
            .map(String::from)
    }

    fn playback(props: &DictRef) -> Playback {
        Playback::from_props(|key| props.get(key))
    }

    /// The share's sound from PipeWire (FR-7.81, FR-7.82): the capture graph runs on its own
    /// thread and leaves ready frames on a queue. Dropping it ends the capture and its links.
    pub struct PipeWireSound {
        frames: Arc<ArrayQueue<Frame>>,
        failed: Arc<AtomicBool>,
        quit: pw::channel::Sender<()>,
        thread: Option<JoinHandle<()>>,
    }

    impl std::fmt::Debug for PipeWireSound {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("PipeWireSound")
                .field("failed", &self.failed.load(Ordering::Relaxed))
                .finish_non_exhaustive()
        }
    }

    impl PipeWireSound {
        /// Starts capturing `target`. `NoPipeWire` without a daemon, `CaptureFailed` when the
        /// capture stream could not be opened.
        pub fn start(target: SoundTarget) -> Result<Self, SoundUnavailable> {
            let frames = Arc::new(ArrayQueue::new(QUEUE_FRAMES));
            let failed = Arc::new(AtomicBool::new(false));
            let (quit, quit_rx) = pw::channel::channel();
            let (ready_tx, ready_rx) = mpsc::sync_channel(1);
            let thread = std::thread::Builder::new()
                .name("screen-sound-pw".into())
                .spawn({
                    let (frames, failed) = (frames.clone(), failed.clone());
                    move || {
                        if let Err(reason) = run(target, frames, &failed, quit_rx, &ready_tx) {
                            // Nobody may wait any more when the start timed out.
                            let _ = ready_tx.send(Err(reason));
                        }
                    }
                })
                .map_err(|e| {
                    tracing::warn!(error = %e, reason = "no_thread", "screen share sound not started");
                    SoundUnavailable::CaptureFailed
                })?;
            let sound = Self {
                frames,
                failed,
                quit,
                thread: Some(thread),
            };
            match ready_rx.recv_timeout(START_TIMEOUT) {
                Ok(Ok(())) => Ok(sound),
                Ok(Err(reason)) => Err(reason),
                Err(_) => {
                    tracing::warn!(reason = "start_timeout", "screen share sound not started");
                    Err(SoundUnavailable::CaptureFailed)
                }
            }
        }
    }

    impl SoundSource for PipeWireSound {
        fn next_frame(&mut self, frame: &mut Frame, timeout: Duration) -> bool {
            let deadline = Instant::now() + timeout;
            loop {
                if let Some(ready) = self.frames.pop() {
                    *frame = ready;
                    return true;
                }
                let now = Instant::now();
                if now >= deadline || self.failed() {
                    return false;
                }
                std::thread::sleep(POLL.min(deadline - now));
            }
        }

        fn failed(&self) -> bool {
            self.failed.load(Ordering::Relaxed)
        }
    }

    impl Drop for PipeWireSound {
        fn drop(&mut self) {
            // The loop is gone already when the capture failed.
            let _ = self.quit.send(());
            if let Some(thread) = self.thread.take()
                && thread.join().is_err()
            {
                tracing::warn!(
                    reason = "thread_panic",
                    "screen share sound thread panicked"
                );
            }
        }
    }

    /// The playback streams with their clients, and while capturing, the links that carry the
    /// wanted ones into the capture stream. A stream's app details come from its own
    /// properties, else from its client's: native PipeWire apps set them only on the client.
    struct Graph {
        /// `None` while only listing.
        target: Option<SoundTarget>,
        core: CoreRc,
        stream: Option<StreamRc>,
        clients: HashMap<u32, Watched<Client, ClientListener>>,
        nodes: HashMap<u32, Watched<Node, NodeListener>>,
        ports: HashMap<u32, Port>,
        links: HashMap<(u32, u32), Link>,
    }

    /// A bound client or playback stream with what its info said.
    struct Watched<P, L> {
        _proxy: P,
        _listener: L,
        props: Option<Playback>,
        /// A stream's client (from `client.id`).
        client: Option<u32>,
        /// A stream belongs in the share's sound.
        wanted: bool,
    }

    struct Port {
        node: u32,
        output: bool,
        side: Side,
    }

    impl Graph {
        fn new(target: Option<SoundTarget>, core: CoreRc, stream: Option<StreamRc>) -> Self {
            Self {
                target,
                core,
                stream,
                clients: HashMap::new(),
                nodes: HashMap::new(),
                ports: HashMap::new(),
                links: HashMap::new(),
            }
        }

        /// Follows the registry for as long as the returned listener lives.
        fn watch(graph: &Rc<RefCell<Self>>, registry: &RegistryRc) -> pw::registry::Listener {
            registry
                .add_listener_local()
                .global({
                    let (graph, registry) = (Rc::downgrade(graph), registry.downgrade());
                    move |global| {
                        if let (Some(graph), Some(registry)) = (graph.upgrade(), registry.upgrade())
                        {
                            Self::add(&graph, &registry, global);
                        }
                    }
                })
                .global_remove({
                    let graph = Rc::downgrade(graph);
                    move |id| {
                        if let Some(graph) = graph.upgrade() {
                            graph.borrow_mut().remove(id);
                        }
                    }
                })
                .register()
        }

        fn add(graph: &Rc<RefCell<Self>>, registry: &RegistryRc, global: &GlobalObject<&DictRef>) {
            let id = global.id;
            match global.type_ {
                ObjectType::Client => {
                    let Ok(client) = registry.bind::<Client, _>(global) else {
                        tracing::debug!(reason = "bind_failed", "sound client not watched");
                        return;
                    };
                    let weak = Rc::downgrade(graph);
                    let listener = client
                        .add_listener_local()
                        .info(move |info| {
                            if !info.change_mask().contains(ClientChangeMask::PROPS) {
                                return;
                            }
                            let (Some(graph), Some(props)) = (weak.upgrade(), info.props()) else {
                                return;
                            };
                            let mut graph = graph.borrow_mut();
                            if let Some(client) = graph.clients.get_mut(&id) {
                                client.props = Some(playback(props));
                            }
                            let streams: Vec<u32> = graph
                                .nodes
                                .iter()
                                .filter(|(_, n)| n.client == Some(id))
                                .map(|(node, _)| *node)
                                .collect();
                            if !streams.is_empty() {
                                let own = own_pids();
                                for node in streams {
                                    graph.decide(node, &own);
                                }
                                graph.reconcile();
                            }
                        })
                        .register();
                    graph.borrow_mut().clients.insert(
                        id,
                        Watched {
                            _proxy: client,
                            _listener: listener,
                            props: None,
                            client: None,
                            wanted: false,
                        },
                    );
                }
                ObjectType::Node
                    if props_of(global, "media.class").as_deref() == Some(PLAYBACK_CLASS) =>
                {
                    let Ok(node) = registry.bind::<Node, _>(global) else {
                        tracing::debug!(reason = "bind_failed", "playback stream not watched");
                        return;
                    };
                    let weak = Rc::downgrade(graph);
                    let listener = node
                        .add_listener_local()
                        .info(move |info| {
                            if !info.change_mask().contains(NodeChangeMask::PROPS) {
                                return;
                            }
                            let (Some(graph), Some(props)) = (weak.upgrade(), info.props()) else {
                                return;
                            };
                            let mut graph = graph.borrow_mut();
                            if let Some(node) = graph.nodes.get_mut(&id) {
                                node.props = Some(playback(props));
                            }
                            graph.decide(id, &own_pids());
                            graph.reconcile();
                        })
                        .register();
                    graph.borrow_mut().nodes.insert(
                        id,
                        Watched {
                            _proxy: node,
                            _listener: listener,
                            props: None,
                            client: props_of(global, "client.id").and_then(|c| c.parse().ok()),
                            wanted: false,
                        },
                    );
                }
                ObjectType::Port => {
                    let Some(node) = props_of(global, "node.id").and_then(|n| n.parse().ok())
                    else {
                        return;
                    };
                    let port = Port {
                        node,
                        output: props_of(global, "port.direction").as_deref() == Some("out"),
                        side: side(props_of(global, "audio.channel").as_deref()),
                    };
                    let mut graph = graph.borrow_mut();
                    graph.ports.insert(id, port);
                    graph.reconcile();
                }
                _ => {}
            }
        }

        fn remove(&mut self, id: u32) {
            self.clients.remove(&id);
            self.nodes.remove(&id);
            self.ports.remove(&id);
            self.links
                .retain(|(output, input), _| *output != id && *input != id);
        }

        /// A stream's details, once its own info and that of its client (if any) came.
        fn playback(&self, node: u32) -> Option<Playback> {
            let node = self.nodes.get(&node)?;
            let own = node.props.clone()?;
            let client = match node.client.and_then(|c| self.clients.get(&c)) {
                Some(client) => Some(client.props.as_ref()?),
                None => None,
            };
            Some(own.or_client(client))
        }

        fn decide(&mut self, node: u32, own: &[u32]) {
            let Some(target) = &self.target else { return };
            let Some(playback) = self.playback(node) else {
                return;
            };
            let wanted = target.wants(&playback, own);
            if let Some(node) = self.nodes.get_mut(&node) {
                node.wanted = wanted;
            }
        }

        /// Creates the links that are missing and drops those no longer wanted.
        fn reconcile(&mut self) {
            let Some(stream) = &self.stream else { return };
            let own = stream.node_id();
            let input = |side| {
                self.ports
                    .iter()
                    .find(|(_, p)| p.node == own && !p.output && p.side == side)
                    .map(|(id, _)| *id)
            };
            // Until the stream is connected and its ports are known there is nothing to link.
            let (Some(left), Some(right)) = (input(Side::Left), input(Side::Right)) else {
                return;
            };
            let outputs = self
                .ports
                .iter()
                .filter(|(_, p)| p.output && self.nodes.get(&p.node).is_some_and(|n| n.wanted));
            let wanted = wanted_links(outputs.map(|(id, p)| (*id, p.side)), [left, right]);
            self.links.retain(|key, _| wanted.contains(key));
            for (output, input) in wanted {
                if self.links.contains_key(&(output, input)) {
                    continue;
                }
                let Some(node) = self.ports.get(&output).map(|p| p.node) else {
                    continue;
                };
                let mut props = PropertiesBox::new();
                props.insert("link.output.node", node.to_string());
                props.insert("link.output.port", output.to_string());
                props.insert("link.input.node", own.to_string());
                props.insert("link.input.port", input.to_string());
                // The link belongs to this connection and ends with it.
                props.insert("object.linger", "false");
                match self.core.create_object::<Link>("link-factory", &props) {
                    Ok(link) => {
                        self.links.insert((output, input), link);
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, reason = "link_failed", "screen share sound link");
                    }
                }
            }
        }
    }

    /// The capture thread: connects, opens the capture stream, reports `ready` and runs until
    /// told to quit or PipeWire goes away.
    fn run(
        target: SoundTarget,
        frames: Arc<ArrayQueue<Frame>>,
        failed: &Arc<AtomicBool>,
        quit: pw::channel::Receiver<()>,
        ready: &mpsc::SyncSender<Result<(), SoundUnavailable>>,
    ) -> Result<(), SoundUnavailable> {
        let (mainloop, core) = connect().map_err(|e| {
            tracing::info!(error = %e, reason = "no_pipewire", "screen share sound not started");
            SoundUnavailable::NoPipeWire
        })?;
        let broken = |e: pw::Error| {
            tracing::warn!(error = %e, reason = "capture_failed", "screen share sound not started");
            SoundUnavailable::CaptureFailed
        };
        let registry = core.get_registry_rc().map_err(broken)?;
        let stream =
            StreamRc::new(core.clone(), "mumcord-share-sound", capture_props()).map_err(broken)?;
        let graph = Rc::new(RefCell::new(Graph::new(
            Some(target),
            core.clone(),
            Some(stream.clone()),
        )));

        let _quit = quit.attach(mainloop.loop_(), {
            let mainloop = mainloop.downgrade();
            move |()| {
                if let Some(mainloop) = mainloop.upgrade() {
                    mainloop.quit();
                }
            }
        });
        let stop = Rc::new({
            let (mainloop, failed) = (mainloop.downgrade(), failed.clone());
            move |reason: &'static str| {
                tracing::warn!(reason, "screen share sound stopped");
                failed.store(true, Ordering::Relaxed);
                if let Some(mainloop) = mainloop.upgrade() {
                    mainloop.quit();
                }
            }
        });
        let _core_listener = core
            .add_listener_local()
            .error({
                let stop = stop.clone();
                move |id, _, _, _| {
                    if id == pw::core::PW_ID_CORE {
                        stop("pipewire_lost");
                    } else {
                        tracing::debug!(reason = "pipewire_error", "screen share sound");
                    }
                }
            })
            .register();
        let _registry_listener = Graph::watch(&graph, &registry);
        let _stream_listener = stream
            .add_local_listener_with_user_data(Frames::default())
            .state_changed({
                let graph = Rc::downgrade(&graph);
                move |_, _, _, new| match new {
                    StreamState::Error(_) => stop("stream_error"),
                    // The stream's node id is known from here on; its ports may be too.
                    StreamState::Paused | StreamState::Streaming => {
                        if let Some(graph) = graph.upgrade() {
                            graph.borrow_mut().reconcile();
                        }
                    }
                    _ => {}
                }
            })
            .process(move |stream, builder| {
                // Real-time path: no allocation, locks, I/O or logging.
                let Some(mut buffer) = stream.dequeue_buffer() else {
                    return;
                };
                let Some(data) = buffer.datas_mut().first_mut() else {
                    return;
                };
                let chunk = data.chunk();
                let (offset, size) = (chunk.offset() as usize, chunk.size() as usize);
                let Some(bytes) = data.data().and_then(|d| d.get(offset..offset + size)) else {
                    return;
                };
                builder.push_stereo_le(bytes, |frame| {
                    frames.force_push(*frame);
                });
            })
            .register()
            .map_err(broken)?;
        let format = format_pod().ok_or_else(|| broken(pw::Error::CreationFailed))?;
        let pod =
            spa::pod::Pod::from_bytes(&format).ok_or_else(|| broken(pw::Error::CreationFailed))?;
        // Not AUTOCONNECT: the session manager must never link a microphone or a monitor in.
        stream
            .connect(
                spa::utils::Direction::Input,
                None,
                StreamFlags::MAP_BUFFERS,
                &mut [pod],
            )
            .map_err(broken)?;
        // The receiver is gone when the start timed out; the share then has no sound anyway.
        let _ = ready.send(Ok(()));
        mainloop.run();
        Ok(())
    }

    fn capture_props() -> PropertiesBox {
        let mut props = PropertiesBox::new();
        props.insert("media.type", "Audio");
        props.insert("media.category", "Capture");
        props.insert("node.name", "mumcord-share-sound");
        props.insert("node.autoconnect", "false");
        props.insert("node.dont-reconnect", "true");
        props
    }

    /// 48 kHz interleaved stereo f32: PipeWire converts whatever the apps play to that.
    fn format_pod() -> Option<Vec<u8>> {
        let mut info = spa::param::audio::AudioInfoRaw::new();
        info.set_format(spa::param::audio::AudioFormat::F32LE);
        info.set_rate(SAMPLE_RATE);
        info.set_channels(2);
        let mut position = [0; spa::sys::SPA_AUDIO_MAX_CHANNELS as usize];
        position[0] = spa::sys::SPA_AUDIO_CHANNEL_FL;
        position[1] = spa::sys::SPA_AUDIO_CHANNEL_FR;
        info.set_position(position);
        let object = spa::pod::Object {
            type_: spa::utils::SpaTypes::ObjectParamFormat.as_raw(),
            id: spa::param::ParamType::EnumFormat.as_raw(),
            properties: info.into(),
        };
        spa::pod::serialize::PodSerializer::serialize(
            std::io::Cursor::new(Vec::new()),
            &spa::pod::Value::Object(object),
        )
        .ok()
        .map(|(cursor, _)| cursor.into_inner())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn stream(pid: Option<u32>, binary: &str) -> Playback {
        Playback {
            app_name: Some(binary.to_uppercase()),
            binary: Some(binary.into()),
            app_pid: pid,
            ..Playback::default()
        }
    }

    #[test]
    fn properties_are_read_and_blank_ones_ignored() {
        let props = [
            ("application.name", "Firefox"),
            ("application.process.binary", "firefox"),
            ("application.process.id", "4242"),
            ("pipewire.sec.pid", "17"),
            ("application.id", " "),
            ("pipewire.access.portal.app_id", "org.mozilla.firefox"),
            ("application.icon-name", "firefox"),
        ];
        let get = |key: &str| props.iter().find(|(k, _)| *k == key).map(|(_, v)| *v);
        let playback = Playback::from_props(get);
        assert_eq!(playback.app_pid, Some(4242));
        assert_eq!(playback.socket_pid, Some(17));
        assert_eq!(playback.desktop_id.as_deref(), Some("org.mozilla.firefox"));
        let stream = playback.stream();
        assert_eq!(stream.pid, Some(4242), "the app's own pid wins");
        assert_eq!(stream.binary.as_deref(), Some("firefox"));
        assert_eq!(stream.icon_name.as_deref(), Some("firefox"));
    }

    #[test]
    fn a_native_stream_gets_its_details_from_its_client() {
        let node = Playback {
            app_name: Some("Stream name".into()),
            ..Playback::default()
        };
        let client = Playback {
            app_name: Some("mpv".into()),
            binary: Some("mpv".into()),
            app_pid: Some(77),
            socket_pid: Some(77),
            ..Playback::default()
        };
        let merged = node.clone().or_client(Some(&client));
        assert_eq!(
            merged.app_name.as_deref(),
            Some("Stream name"),
            "the stream's own wins"
        );
        assert_eq!(merged.binary.as_deref(), Some("mpv"));
        assert_eq!(merged.app_pid, Some(77));
        assert_eq!(node.clone().or_client(None), node);
    }

    #[test]
    fn the_desktop_takes_every_stream_but_mumcords_and_unknown_ones() {
        let own = [100, 101, 102];
        assert!(SoundTarget::Desktop.wants(&stream(Some(5), "mpv"), &own));
        assert!(
            !SoundTarget::Desktop.wants(&stream(Some(101), "WebKitWebProcess"), &own),
            "the web view is Mumcord"
        );
        assert!(
            !SoundTarget::Desktop.wants(&stream(None, "mpv"), &own),
            "no pid: could be Mumcord"
        );
        let socket_only = Playback {
            socket_pid: Some(100),
            ..stream(None, "mumcord")
        };
        assert!(!SoundTarget::Desktop.wants(&socket_only, &own));
        // Mumcord pretending to be another app by name is still Mumcord.
        assert!(!SoundTarget::Desktop.wants(&stream(Some(100), "firefox"), &own));
    }

    #[test]
    fn one_app_is_followed_by_pid_and_binary_only() {
        let app = SoundTarget::App(MatchedApp {
            app_name: Some("Firefox".into()),
            binary: Some("firefox".into()),
            icon_name: None,
            pids: vec![10, 11],
        });
        let own = [100];
        assert!(app.wants(&stream(Some(11), "firefox"), &own));
        assert!(
            app.wants(&stream(Some(12), "Firefox"), &own),
            "a new process of the same binary"
        );
        assert!(
            app.wants(&stream(Some(10), "plugin-container"), &own),
            "a known pid"
        );
        assert!(!app.wants(&stream(Some(13), "mpv"), &own));
        assert!(
            !app.wants(&stream(Some(100), "firefox"), &own),
            "never Mumcord"
        );

        let pulse = Playback {
            app_pid: None,
            socket_pid: Some(13),
            ..stream(None, "mpv")
        };
        assert!(!app.wants(&pulse, &own));
    }

    #[test]
    fn the_own_tree_holds_all_descendants() {
        let table = [
            (1, 0),
            (100, 1),
            (101, 100),
            (102, 101),
            (103, 100),
            (200, 1),
            (201, 200),
        ];
        let mut tree = process_tree(100, &table);
        tree.sort_unstable();
        assert_eq!(tree, [100, 101, 102, 103]);
        assert_eq!(process_tree(300, &table), [300], "unknown root");
    }

    #[test]
    fn the_parent_is_read_after_the_name() {
        assert_eq!(parent_of("42 (bash) S 7 42 42 0"), Some(7));
        assert_eq!(parent_of("43 (Web Content (x)) S 9 1 1"), Some(9));
        assert_eq!(parent_of("garbage"), None);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn the_own_tree_of_this_process_contains_its_children() {
        let mut child = std::process::Command::new("sleep")
            .arg("5")
            .spawn()
            .unwrap();
        let own = own_pids();
        let _ = child.kill();
        let _ = child.wait();
        assert!(own.contains(&std::process::id()));
        assert!(own.contains(&child.id()));
    }

    #[test]
    fn channels_go_to_their_side() {
        assert_eq!(side(Some("FL")), Side::Left);
        assert_eq!(side(Some("RL")), Side::Left);
        assert_eq!(side(Some("FR")), Side::Right);
        assert_eq!(side(Some("SR")), Side::Right);
        assert_eq!(side(Some("MONO")), Side::Both);
        assert_eq!(side(Some("FC")), Side::Both);
        assert_eq!(side(Some("LFE")), Side::Both);
        assert_eq!(side(None), Side::Both);
    }

    #[test]
    fn mono_ports_feed_both_inputs() {
        let links = wanted_links(
            [(1, Side::Left), (2, Side::Right), (3, Side::Both)].into_iter(),
            [90, 91],
        );
        assert_eq!(links, [(1, 90), (2, 91), (3, 90), (3, 91)]);
    }

    #[test]
    fn stereo_bytes_become_mono_frames() {
        let mut frames = Frames::default();
        let mut bytes = Vec::new();
        for _ in 0..FRAME_SAMPLES + 10 {
            bytes.extend(0.5f32.to_le_bytes());
            bytes.extend((-0.1f32).to_le_bytes());
        }
        let mut done = Vec::new();
        frames.push_stereo_le(&bytes, |f| done.push(*f));
        assert_eq!(done.len(), 1);
        assert!(done[0].iter().all(|s| (s - 0.2).abs() < 1e-6));
        assert_eq!(frames.fill, 10, "the rest waits for the next buffer");

        let loud: Vec<u8> = [1.0f32, 1.0]
            .repeat(FRAME_SAMPLES * 2)
            .iter()
            .flat_map(|s| (s * 3.0).to_le_bytes())
            .collect();
        let mut done = Vec::new();
        frames.push_stereo_le(&loud, |f| done.push(*f));
        assert!(done.iter().flatten().all(|s| *s <= 1.0), "clamped");
    }

    /// Against a real PipeWire with WirePlumber, playing test tones with `pw-play`. Runs only
    /// with `MUMCORD_PIPEWIRE_LIVE=1` (set by the E2E image), so `cargo test` never makes noise
    /// on a desktop; skipped without a daemon or `pw-play`.
    #[cfg(target_os = "linux")]
    mod live {
        use std::path::Path;
        use std::process::{Child, Command};
        use std::time::{Duration, Instant};

        use super::*;
        use crate::screen_share::capture::SoundSource;
        use mumcord_audio::SAMPLE_RATE;

        const LOW: f32 = 440.0;
        const MID: f32 = 1000.0;
        const HIGH: f32 = 2500.0;
        /// Starts only after the capture did.
        const LATE: f32 = 1600.0;

        fn wanted() -> bool {
            if std::env::var("MUMCORD_PIPEWIRE_LIVE").as_deref() != Ok("1") {
                eprintln!("skipped: MUMCORD_PIPEWIRE_LIVE is not 1");
                return false;
            }
            if !running() || Command::new("pw-play").arg("--version").output().is_err() {
                eprintln!("skipped: no PipeWire or no pw-play");
                return false;
            }
            true
        }

        /// A 16-bit mono WAV of `hz` lasting `seconds`.
        fn tone(path: &Path, hz: f32, seconds: u32) {
            let samples = SAMPLE_RATE * seconds;
            let mut wav = Vec::new();
            wav.extend(b"RIFF");
            wav.extend((36 + samples * 2).to_le_bytes());
            wav.extend(b"WAVEfmt ");
            wav.extend(16u32.to_le_bytes());
            wav.extend(1u16.to_le_bytes());
            wav.extend(1u16.to_le_bytes());
            wav.extend(SAMPLE_RATE.to_le_bytes());
            wav.extend((SAMPLE_RATE * 2).to_le_bytes());
            wav.extend(2u16.to_le_bytes());
            wav.extend(16u16.to_le_bytes());
            wav.extend(b"data");
            wav.extend((samples * 2).to_le_bytes());
            for n in 0..samples {
                let t = n as f32 / SAMPLE_RATE as f32;
                let v = (std::f32::consts::TAU * hz * t).sin() * 0.3;
                wav.extend(((v * f32::from(i16::MAX)) as i16).to_le_bytes());
            }
            std::fs::write(path, wav).unwrap();
        }

        /// `pw-play` outside this process tree (the shell exits, so it is re-parented): to
        /// Mumcord it is another app. Returns its pid.
        fn foreign_player(file: &Path, name: &str) -> u32 {
            foreign_player_as(Path::new("pw-play"), file, name)
        }

        /// [`foreign_player`] run as `program`, a link to `pw-play` under another binary name.
        fn foreign_player_as(program: &Path, file: &Path, name: &str) -> u32 {
            let out = Command::new("sh")
                .arg("-c")
                .arg(format!(
                    "'{}' -P '{{ application.name = {name} }}' '{}' >/dev/null 2>&1 & echo $!",
                    program.display(),
                    file.display()
                ))
                .output()
                .unwrap();
            String::from_utf8(out.stdout)
                .unwrap()
                .trim()
                .parse()
                .unwrap()
        }

        /// `pw-play` as a child of this process: to Mumcord it is its own sound.
        fn own_player(file: &Path) -> Child {
            Command::new("pw-play").arg(file).spawn().unwrap()
        }

        fn kill(pid: u32) {
            let _ = Command::new("kill").arg(pid.to_string()).status();
        }

        /// Relative power of `hz` in `samples` (Goertzel).
        fn power(samples: &[f32], hz: f32) -> f32 {
            let coeff = 2.0 * (std::f32::consts::TAU * hz / SAMPLE_RATE as f32).cos();
            let (mut s1, mut s2) = (0.0f32, 0.0f32);
            for x in samples {
                let s = x + coeff * s1 - s2;
                s2 = s1;
                s1 = s;
            }
            (s1 * s1 + s2 * s2 - coeff * s1 * s2) / samples.len() as f32
        }

        /// Half a second of the capture, after `started` ran and the links settled.
        fn record(target: SoundTarget, started: impl FnOnce()) -> Vec<f32> {
            let mut sound = PipeWireSound::start(target).unwrap();
            started();
            listen(&mut sound)
        }

        /// Half a second of `sound`, after the links settled.
        fn listen(sound: &mut dyn SoundSource) -> Vec<f32> {
            let mut frame = [0.0; FRAME_SAMPLES];
            let settle = Instant::now() + Duration::from_millis(300);
            while Instant::now() < settle {
                sound.next_frame(&mut frame, Duration::from_millis(20));
            }
            let mut samples = Vec::new();
            let deadline = Instant::now() + Duration::from_secs(3);
            while samples.len() < SAMPLE_RATE as usize / 2 && Instant::now() < deadline {
                if sound.next_frame(&mut frame, Duration::from_millis(50)) {
                    samples.extend(frame);
                }
            }
            assert!(!sound.failed());
            samples
        }

        fn heard(samples: &[f32], hz: f32) -> bool {
            power(samples, hz) > 1.0
        }

        #[test]
        fn desktop_and_app_sound_leave_mumcord_out() {
            if !wanted() {
                return;
            }
            let dir = std::env::temp_dir().join(format!("mumcord-pw-live-{}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            let files = [LOW, MID, HIGH, LATE].map(|hz| {
                let file = dir.join(format!("{hz}.wav"));
                tone(&file, hz, 8);
                file
            });
            let low = foreign_player(&files[0], "mumcord-test-low");
            let mid = foreign_player(&files[1], "mumcord-test-mid");
            let mut own = own_player(&files[2]);
            std::thread::sleep(Duration::from_millis(700));

            let streams = playback_streams().unwrap();
            let pids: Vec<_> = streams.iter().filter_map(|s| s.pid).collect();
            let mut late = None;
            let desktop = record(SoundTarget::Desktop, || {
                late = Some(foreign_player(&files[3], "mumcord-test-late"));
                std::thread::sleep(Duration::from_millis(500));
            });
            let app = record(
                SoundTarget::App(MatchedApp {
                    app_name: Some("mumcord-test-low".into()),
                    binary: None,
                    icon_name: None,
                    pids: vec![low],
                }),
                || {},
            );

            kill(low);
            kill(mid);
            late.into_iter().for_each(kill);
            let _ = own.kill();
            let _ = own.wait();
            let _ = std::fs::remove_dir_all(&dir);

            assert!(
                pids.contains(&low) && pids.contains(&mid),
                "foreign players listed"
            );
            assert!(
                !pids.contains(&own.id()),
                "Mumcord's own player never listed"
            );
            assert!(
                heard(&desktop, LOW) && heard(&desktop, MID),
                "desktop mixes both apps"
            );
            assert!(
                heard(&desktop, LATE),
                "a stream that starts during the share is added"
            );
            assert!(!heard(&desktop, HIGH), "desktop leaves Mumcord out");
            assert!(heard(&app, LOW), "the app is heard");
            assert!(!heard(&app, MID) && !heard(&app, HIGH), "only the app");
        }

        /// The window path of the capture files (FR-7.82, FR-7.83): a hint that matches one
        /// app shares it right away; without one the list offers both apps, and the chosen
        /// one is shared.
        #[test]
        fn a_window_share_takes_the_matched_or_the_chosen_app() {
            use crate::screen_share::sound::pick;
            use crate::screen_share::sound::{SoundOutcome, SoundRequest};

            if !wanted() {
                return;
            }
            let dir =
                std::env::temp_dir().join(format!("mumcord-pw-window-{}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            let out = Command::new("sh")
                .args(["-c", "command -v pw-play"])
                .output()
                .unwrap();
            let pw_play = String::from_utf8(out.stdout).unwrap();
            // Distinct binaries, so the players are two apps and not one app's two streams.
            let [p1, p2] =
                [(LOW, "mumcord-test-p1"), (MID, "mumcord-test-p2")].map(|(hz, name)| {
                    let file = dir.join(format!("{hz}.wav"));
                    tone(&file, hz, 8);
                    let program = dir.join(name);
                    std::os::unix::fs::symlink(pw_play.trim(), &program).unwrap();
                    foreign_player_as(&program, &file, name)
                });
            std::thread::sleep(Duration::from_millis(700));

            let clear = pick::take(SoundRequest::Window, &["mumcord-test-p1"]);
            let SoundOutcome::Choose(listed) = pick::take(SoundRequest::Window, &[]) else {
                panic!("no hint asks the user");
            };
            let names: Vec<_> = listed.iter().map(|a| a.name.as_str().to_owned()).collect();
            let p2_id = listed
                .iter()
                .find(|a| a.name.as_str() == "mumcord-test-p2")
                .map(|a| a.id.clone())
                .unwrap();
            let SoundOutcome::Captured {
                source: mut matched,
                ..
            } = clear
            else {
                panic!("a clear hint captures the app");
            };
            let matched = listen(matched.as_mut());
            let Some(SoundOutcome::Captured {
                source: mut chosen, ..
            }) = pick::start_chosen(&p2_id).unwrap()
            else {
                panic!("the chosen app is captured");
            };
            let chosen = listen(chosen.as_mut());

            kill(p1);
            kill(p2);
            let _ = std::fs::remove_dir_all(&dir);

            assert!(names.iter().any(|n| n == "mumcord-test-p1") && names.len() >= 2);
            assert!(
                heard(&matched, LOW) && !heard(&matched, MID),
                "only the matched app"
            );
            assert!(
                heard(&chosen, MID) && !heard(&chosen, LOW),
                "only the chosen app"
            );
        }
    }
}
