//! Mumble Link: the shared memory interface games and mods write their positions into.
//!
//! The layout is the official `LinkedMem` struct (see Mumble's `plugins/link`), with `wchar_t`
//! strings, so it differs per platform: 4-byte characters (UTF-32) on Linux and macOS, 2-byte
//! characters (UTF-16) on Windows.
//!
//! ```c
//! struct LinkedMem {
//!     uint32_t uiVersion;            // 1 or 2
//!     uint32_t uiTick;               // incremented by the game on every update
//!     float    fAvatarPosition[3], fAvatarFront[3], fAvatarTop[3];
//!     wchar_t  name[256];            // game name
//!     float    fCameraPosition[3], fCameraFront[3], fCameraTop[3];   // version 2
//!     wchar_t  identity[256];                                         // version 2
//!     uint32_t context_len;                                           // version 2
//!     unsigned char context[256];                                     // version 2
//!     wchar_t  description[2048];
//! };
//! ```
//!
//! Like the official client, a game counts as active while `uiTick` keeps changing and as gone
//! after [`INACTIVE_AFTER`] without a change.

use std::time::{Duration, Instant};

use crate::shm::SharedMemory;
use crate::{Fetch, Pose, PositionSource, Positional};

/// A game that stops updating `uiTick` for this long is treated as closed (official client: 5 s).
pub const INACTIVE_AFTER: Duration = Duration::from_secs(5);
/// Display name of this source (FR-1.43).
pub const SOURCE_NAME: &str = "Mumble Link";
/// Plugin name the official client puts in front of the context (`"Link\0" + context`).
const PLUGIN_NAME: &str = "Link";
const NAME_CHARS: usize = 256;
const DESCRIPTION_CHARS: usize = 2048;
const CONTEXT_BYTES: usize = 256;
/// The official link plugin uses at most 255 context bytes.
const CONTEXT_MAX: usize = 255;
const TICK_OFFSET: usize = 4;

/// Size of a `wchar_t`, which decides the struct layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WcharWidth {
    /// UTF-16 (Windows).
    Two,
    /// UTF-32 (Linux, macOS).
    Four,
}

impl WcharWidth {
    pub const NATIVE: Self = if cfg!(windows) { Self::Two } else { Self::Four };

    const fn bytes(self) -> usize {
        match self {
            Self::Two => 2,
            Self::Four => 4,
        }
    }
}

/// Byte offsets of the `LinkedMem` fields.
#[derive(Debug, Clone, Copy)]
struct Layout {
    w: WcharWidth,
    name: usize,
    camera: usize,
    identity: usize,
    context_len: usize,
    context: usize,
    size: usize,
}

impl Layout {
    const fn new(w: WcharWidth) -> Self {
        let wb = w.bytes();
        let name = 8 + 36;
        let camera = name + NAME_CHARS * wb;
        let identity = camera + 36;
        let context_len = identity + NAME_CHARS * wb;
        let context = context_len + 4;
        let size = context + CONTEXT_BYTES + DESCRIPTION_CHARS * wb;
        Self {
            w,
            name,
            camera,
            identity,
            context_len,
            context,
            size,
        }
    }
}

/// Size of `LinkedMem` with the given `wchar_t` width.
pub const fn linked_mem_size(w: WcharWidth) -> usize {
    Layout::new(w).size
}

/// The fields of one `LinkedMem` snapshot.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LinkedMem {
    pub version: u32,
    pub tick: u32,
    pub avatar: Pose,
    pub name: String,
    pub camera: Pose,
    pub identity: String,
    /// Raw context bytes (at most 255).
    pub context: Vec<u8>,
}

impl LinkedMem {
    /// Parses a snapshot; `None` if `data` is shorter than the struct.
    pub fn parse(data: &[u8], w: WcharWidth) -> Option<Self> {
        let l = Layout::new(w);
        if data.len() < l.size {
            return None;
        }
        let u32_at =
            |o: usize| u32::from_ne_bytes([data[o], data[o + 1], data[o + 2], data[o + 3]]);
        let f32_at = |o: usize| f32::from_bits(u32_at(o));
        let vec_at = |o: usize| [f32_at(o), f32_at(o + 4), f32_at(o + 8)];
        let pose_at = |o: usize| Pose {
            position: vec_at(o),
            front: vec_at(o + 12),
            top: vec_at(o + 24),
        };
        let context_len = (u32_at(l.context_len) as usize).min(CONTEXT_MAX);
        Some(Self {
            version: u32_at(0),
            tick: u32_at(TICK_OFFSET),
            avatar: pose_at(8),
            name: wide_string(&data[l.name..l.camera], l.w),
            camera: pose_at(l.camera),
            identity: wide_string(&data[l.identity..l.context_len], l.w),
            context: data[l.context..l.context + context_len].to_vec(),
        })
    }

    /// Positions as the official link plugin reports them: version 1 has no camera (the avatar
    /// is used), context or identity. `None` for unknown versions.
    pub fn positional(&self) -> Option<Positional> {
        match self.version {
            1 => Some(Positional {
                avatar: self.avatar,
                camera: self.avatar,
                context: Vec::new(),
                identity: String::new(),
            }),
            2 => Some(Positional {
                avatar: self.avatar,
                camera: self.camera,
                context: wire_context(&self.context),
                identity: self.identity.clone(),
            }),
            _ => None,
        }
    }
}

/// The official client sends `plugin name + NUL + context` (through a `QString`, so invalid
/// UTF-8 is replaced) and nothing for an empty context.
fn wire_context(raw: &[u8]) -> Vec<u8> {
    if raw.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(PLUGIN_NAME.len() + 1 + raw.len());
    out.extend_from_slice(PLUGIN_NAME.as_bytes());
    out.push(0);
    out.extend_from_slice(String::from_utf8_lossy(raw).as_bytes());
    out
}

/// A NUL-terminated `wchar_t` string; invalid characters are replaced.
fn wide_string(data: &[u8], w: WcharWidth) -> String {
    match w {
        WcharWidth::Two => {
            let units = data
                .as_chunks::<2>()
                .0
                .iter()
                .map(|&c| u16::from_ne_bytes(c))
                .take_while(|&u| u != 0);
            char::decode_utf16(units)
                .map(|c| c.unwrap_or(char::REPLACEMENT_CHARACTER))
                .collect()
        }
        WcharWidth::Four => data
            .as_chunks::<4>()
            .0
            .iter()
            .map(|&c| u32::from_ne_bytes(c))
            .take_while(|&u| u != 0)
            .map(|u| char::from_u32(u).unwrap_or(char::REPLACEMENT_CHARACTER))
            .collect(),
    }
}

/// Tracks `uiTick` to tell whether a game is running.
#[derive(Debug, Default)]
struct TickWatch {
    last_tick: Option<u32>,
    changed_at: Option<Instant>,
}

impl TickWatch {
    /// Records `tick`; returns whether it changed since the previous call. The first value
    /// seen is not a change: a segment left behind by a closed game keeps its last tick.
    fn observe(&mut self, tick: u32, now: Instant) -> bool {
        let changed = self.last_tick.is_some_and(|t| t != tick);
        if changed {
            self.changed_at = Some(now);
        }
        self.last_tick = Some(tick);
        changed
    }

    fn active(&self, now: Instant) -> bool {
        self.changed_at
            .is_some_and(|at| now.saturating_duration_since(at) < INACTIVE_AFTER)
    }
}

/// The Mumble Link shared memory as a [`PositionSource`].
pub struct LinkSource {
    shm: Option<SharedMemory>,
    name: String,
    /// When opening failed last; retried after [`INACTIVE_AFTER`].
    failed_at: Option<Instant>,
    watch: TickWatch,
    buffer: Vec<u8>,
    current: Option<Fetch>,
}

impl LinkSource {
    const LAYOUT: Layout = Layout::new(WcharWidth::NATIVE);

    /// The segment of the current user, opened (or created) on the first fetch.
    pub fn new() -> Self {
        Self::with_name(SharedMemory::default_name())
    }

    /// A segment with another name, for tests and fake games.
    pub fn with_name(name: String) -> Self {
        Self {
            shm: None,
            name,
            failed_at: None,
            watch: TickWatch::default(),
            buffer: Vec::new(),
            current: None,
        }
    }

    /// Opens the segment unless it is open or opening failed recently.
    fn ensure_open(&mut self, now: Instant) {
        if self.shm.is_some()
            || self
                .failed_at
                .is_some_and(|at| now.saturating_duration_since(at) < INACTIVE_AFTER)
        {
            return;
        }
        match SharedMemory::open(&self.name, Self::LAYOUT.size) {
            Ok(shm) => self.shm = Some(shm),
            Err(e) => {
                tracing::warn!(error = %e, "opening the Mumble Link shared memory failed");
                self.failed_at = Some(now);
            }
        }
    }
}

impl Default for LinkSource {
    fn default() -> Self {
        Self::new()
    }
}

impl PositionSource for LinkSource {
    fn fetch(&mut self, now: Instant) -> Option<Fetch> {
        self.ensure_open(now);
        let shm = self.shm.as_ref()?;
        let tick = shm.read_u32(TICK_OFFSET);
        if self.watch.observe(tick, now) {
            shm.read_into(&mut self.buffer);
            self.current = LinkedMem::parse(&self.buffer, WcharWidth::NATIVE).and_then(|mem| {
                let data = mem.positional()?;
                Some(Fetch {
                    source: SOURCE_NAME.to_owned(),
                    game: (!mem.name.trim().is_empty()).then(|| mem.name.trim().to_owned()),
                    data,
                })
            });
        }
        if !self.watch.active(now) {
            self.current = None;
        }
        self.current.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Writes a `LinkedMem` the way a game does.
    struct Writer {
        data: Vec<u8>,
        l: Layout,
    }

    impl Writer {
        fn new(w: WcharWidth) -> Self {
            let l = Layout::new(w);
            Self {
                data: vec![0; l.size],
                l,
            }
        }

        fn u32(&mut self, o: usize, v: u32) {
            self.data[o..o + 4].copy_from_slice(&v.to_ne_bytes());
        }

        fn pose(&mut self, o: usize, p: &Pose) {
            for (i, f) in p.position.iter().chain(&p.front).chain(&p.top).enumerate() {
                self.u32(o + 4 * i, f.to_bits());
            }
        }

        fn wide(&mut self, o: usize, s: &str) {
            let wb = self.l.w.bytes();
            match self.l.w {
                WcharWidth::Two => {
                    for (i, u) in s.encode_utf16().enumerate() {
                        self.data[o + i * wb..o + i * wb + 2].copy_from_slice(&u.to_ne_bytes());
                    }
                }
                WcharWidth::Four => {
                    for (i, c) in s.chars().enumerate() {
                        self.u32(o + i * wb, c as u32);
                    }
                }
            }
        }

        fn game(&mut self, version: u32, tick: u32, avatar: &Pose, camera: &Pose, ctx: &[u8]) {
            self.u32(0, version);
            self.u32(TICK_OFFSET, tick);
            self.pose(8, avatar);
            self.wide(self.l.name, "Test Game");
            self.pose(self.l.camera, camera);
            self.wide(self.l.identity, "Player Ünïcode");
            self.u32(self.l.context_len, ctx.len() as u32);
            self.data[self.l.context..self.l.context + ctx.len()].copy_from_slice(ctx);
        }
    }

    fn pose(x: f32) -> Pose {
        Pose {
            position: [x, 1.5, -3.0],
            front: [0.0, 0.0, 1.0],
            top: [0.0, 1.0, 0.0],
        }
    }

    #[test]
    fn layout_matches_the_official_struct() {
        assert_eq!(linked_mem_size(WcharWidth::Four), 10580);
        assert_eq!(linked_mem_size(WcharWidth::Two), 5460);
    }

    #[test]
    fn parses_version_2_with_both_widths() {
        for w in [WcharWidth::Two, WcharWidth::Four] {
            let mut writer = Writer::new(w);
            writer.game(2, 7, &pose(1.0), &pose(2.0), b"server:1");
            let mem = LinkedMem::parse(&writer.data, w).expect("parsed");
            assert_eq!(mem.version, 2);
            assert_eq!(mem.tick, 7);
            assert_eq!(mem.name, "Test Game");
            assert_eq!(mem.identity, "Player Ünïcode");
            let p = mem.positional().expect("valid version");
            assert_eq!(p.avatar, pose(1.0));
            assert_eq!(p.camera, pose(2.0));
            assert_eq!(p.context, b"Link\0server:1");
            assert_eq!(p.identity, "Player Ünïcode");
        }
    }

    #[test]
    fn version_1_uses_the_avatar_as_camera_and_has_no_context() {
        let mut writer = Writer::new(WcharWidth::Four);
        writer.game(1, 1, &pose(1.0), &pose(2.0), b"ignored");
        let p = LinkedMem::parse(&writer.data, WcharWidth::Four)
            .and_then(|m| m.positional())
            .expect("valid");
        assert_eq!(p.camera, pose(1.0));
        assert!(p.context.is_empty() && p.identity.is_empty());
    }

    #[test]
    fn rejects_short_data_unknown_versions_and_caps_the_context() {
        assert!(LinkedMem::parse(&[0; 100], WcharWidth::Four).is_none());
        let mut writer = Writer::new(WcharWidth::Four);
        writer.game(3, 1, &pose(1.0), &pose(2.0), b"");
        assert!(
            LinkedMem::parse(&writer.data, WcharWidth::Four)
                .expect("parsed")
                .positional()
                .is_none()
        );
        let l = writer.l;
        writer.u32(l.context_len, 100_000);
        let mem = LinkedMem::parse(&writer.data, WcharWidth::Four).expect("parsed");
        assert_eq!(mem.context.len(), CONTEXT_MAX);
    }

    #[test]
    fn tick_watch_needs_a_change_and_expires() {
        let t0 = Instant::now();
        let mut watch = TickWatch::default();
        assert!(!watch.observe(5, t0), "a stale segment is no game");
        assert!(!watch.active(t0));
        assert!(watch.observe(6, t0 + Duration::from_millis(20)));
        assert!(watch.active(t0 + Duration::from_secs(1)));
        assert!(!watch.observe(6, t0 + Duration::from_secs(2)));
        assert!(!watch.active(t0 + Duration::from_secs(6)));
    }

    #[cfg(unix)]
    #[test]
    fn reads_a_fake_game_from_shared_memory() {
        let name = format!("/MumcordLinkTest.{}", std::process::id());
        SharedMemory::unlink(&name);
        let mut source = LinkSource::with_name(name.clone());
        let t0 = Instant::now();
        assert!(source.fetch(t0).is_none(), "created empty");

        // The game attaches to the segment the client created.
        let mut game = SharedMemory::open(&name, LinkSource::LAYOUT.size).expect("attach");
        let mut writer = Writer::new(WcharWidth::NATIVE);
        writer.game(2, 1, &pose(1.0), &pose(2.0), b"ctx");
        game.write(&writer.data);
        let first = source
            .fetch(t0 + Duration::from_millis(20))
            .expect("tick changed");
        assert_eq!(first.data.avatar.position[0], 1.0);
        writer.game(2, 2, &pose(4.0), &pose(2.0), b"ctx");
        game.write(&writer.data);
        let fetch = source
            .fetch(t0 + Duration::from_millis(40))
            .expect("game active");
        assert_eq!(fetch.source, SOURCE_NAME);
        assert_eq!(fetch.game.as_deref(), Some("Test Game"));
        assert_eq!(fetch.data.avatar.position[0], 4.0);
        assert_eq!(fetch.data.context, b"Link\0ctx");

        // The game stops updating.
        assert!(source.fetch(t0 + Duration::from_secs(3)).is_some());
        assert!(source.fetch(t0 + Duration::from_secs(6)).is_none());
        drop(game);
        SharedMemory::unlink(&name);
    }
}
