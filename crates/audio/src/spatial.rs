//! 3D placement of voices for positional audio (FR-1.41), tuned for headphones.
//!
//! Each voice is placed relative to the listener with three cues:
//! - level difference: equal-power panning, narrowed a little so a voice at the side still
//!   reaches the far ear (hard panning sounds unnatural on headphones);
//! - time difference: the far ear hears the voice up to [`MAX_ITD_SAMPLES`] later (≈ 0.65 ms,
//!   the delay around a human head);
//! - head shadow: a gentle low-pass on the far ear, and voices behind the listener are slightly
//!   quieter, which helps telling front from back.
//!
//! Loudness falls with distance: full volume up to the minimum distance, silent at the maximum
//! distance (see [`distance_gain`]).
//!
//! Everything here is allocation-free; [`Listener`] is shared lock-free between the engine
//! (writer) and the playback thread (reader). Coordinates follow Mumble's convention:
//! left-handed, X right, Y up, Z forward, in metres.

use std::f32::consts::{FRAC_PI_4, SQRT_2};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use crate::{FRAME_SAMPLES, Frame};

/// Longest interaural delay in 48 kHz samples (≈ 0.65 ms).
pub const MAX_ITD_SAMPLES: f32 = 31.0;
/// Fraction of the full equal-power pan range used.
const PAN_WIDTH: f32 = 0.85;
/// Low-pass amount on the far ear for a voice fully at the side (filter coefficient 0.4 there,
/// a cutoff of about 4 kHz).
const HEAD_SHADOW: f32 = 0.6;
/// Level reduction for a voice straight behind the listener.
const REAR_ATTENUATION: f32 = 0.25;
/// Delay line length (power of two above the largest delay plus interpolation tap).
const HISTORY: usize = 64;
/// Below this distance a voice is "on top of" the listener and is not panned.
const SAME_SPOT: f32 = 1e-3;

/// Default distances of the official client, in metres.
pub const DEFAULT_MIN_DISTANCE: f32 = 1.0;
pub const DEFAULT_MAX_DISTANCE: f32 = 15.0;

type Vec3 = [f32; 3];

fn sub(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: Vec3, b: Vec3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: Vec3, b: Vec3) -> Vec3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn normalized(v: Vec3) -> Option<Vec3> {
    let len = dot(v, v).sqrt();
    (len.is_finite() && len > 1e-6).then(|| [v[0] / len, v[1] / len, v[2] / len])
}

/// Where the listener is and how loud distant voices are; what the playback thread needs to
/// place voices.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ListenerState {
    pub position: Vec3,
    /// Unit vector the listener looks at.
    pub front: Vec3,
    /// Unit vector pointing to the right ear.
    pub right: Vec3,
    pub min_distance: f32,
    pub max_distance: f32,
}

impl ListenerState {
    /// `None` if `front` and `top` do not describe an orientation.
    pub fn new(position: Vec3, front: Vec3, top: Vec3, min: f32, max: f32) -> Option<Self> {
        let front = normalized(front)?;
        let top = normalized(top)?;
        // Left-handed: top × front points right (Y × Z = X).
        let right = normalized(cross(top, front))?;
        position.iter().all(|c| c.is_finite()).then_some(Self {
            position,
            front,
            right,
            min_distance: min,
            max_distance: max,
        })
    }
}

/// The listener, shared lock-free between the engine and the playback thread. Fields are
/// separate atomics: a pose read while it is being updated may mix two consecutive poses,
/// which is inaudible at 10 ms frames.
#[derive(Debug)]
pub struct Listener {
    active: AtomicBool,
    min_distance: AtomicU32,
    max_distance: AtomicU32,
    pose: [AtomicU32; 9],
}

impl Default for Listener {
    fn default() -> Self {
        Self {
            active: AtomicBool::new(false),
            min_distance: AtomicU32::new(DEFAULT_MIN_DISTANCE.to_bits()),
            max_distance: AtomicU32::new(DEFAULT_MAX_DISTANCE.to_bits()),
            pose: std::array::from_fn(|_| AtomicU32::new(0)),
        }
    }
}

impl Listener {
    /// Distances in metres; invalid values are corrected (min ≥ 0, max > min).
    pub fn set_distances(&self, min: f32, max: f32) {
        let min = if min.is_finite() { min.max(0.0) } else { 0.0 };
        let max = if max.is_finite() && max > min {
            max
        } else {
            min + 1.0
        };
        self.min_distance.store(min.to_bits(), Ordering::Relaxed);
        self.max_distance.store(max.to_bits(), Ordering::Relaxed);
    }

    /// The listener's camera pose; voices are placed around it from now on.
    pub fn set_pose(&self, position: Vec3, front: Vec3, top: Vec3) {
        for (slot, v) in self
            .pose
            .iter()
            .zip(position.iter().chain(&front).chain(&top))
        {
            slot.store(v.to_bits(), Ordering::Relaxed);
        }
        self.active.store(true, Ordering::Release);
    }

    /// No pose (positional audio off or no game): voices play unplaced.
    pub fn clear(&self) {
        self.active.store(false, Ordering::Release);
    }

    pub fn state(&self) -> Option<ListenerState> {
        if !self.active.load(Ordering::Acquire) {
            return None;
        }
        let v = |i: usize| f32::from_bits(self.pose[i].load(Ordering::Relaxed));
        ListenerState::new(
            [v(0), v(1), v(2)],
            [v(3), v(4), v(5)],
            [v(6), v(7), v(8)],
            f32::from_bits(self.min_distance.load(Ordering::Relaxed)),
            f32::from_bits(self.max_distance.load(Ordering::Relaxed)),
        )
    }
}

/// Loudness at `distance`: 1 up to `min`, 0 from `max` on, falling like `min / distance`
/// (natural loudness drop) faded linearly to silence in between.
pub fn distance_gain(distance: f32, min: f32, max: f32) -> f32 {
    if distance <= min {
        return 1.0;
    }
    if distance >= max {
        return 0.0;
    }
    let natural = if min > 0.0 { min / distance } else { 1.0 };
    natural * (max - distance) / (max - min)
}

/// Per-ear rendering parameters of a voice: index 0 is the left ear, 1 the right.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    pub gain: [f32; 2],
    /// Delay in samples.
    pub delay: [f32; 2],
    /// One-pole low-pass coefficient (1.0 = unfiltered).
    pub filter: [f32; 2],
}

impl Placement {
    /// A voice without position: both ears at full level, as plain playback.
    pub const CENTER: Self = Self {
        gain: [1.0, 1.0],
        delay: [0.0, 0.0],
        filter: [1.0, 1.0],
    };

    /// Places a voice at `source` around `listener`.
    pub fn at(listener: &ListenerState, source: Vec3) -> Self {
        if !source.iter().all(|c| c.is_finite()) {
            return Self::CENTER;
        }
        let offset = sub(source, listener.position);
        let distance = dot(offset, offset).sqrt();
        let (side, ahead) = if distance < SAME_SPOT {
            (0.0, 1.0)
        } else {
            (
                (dot(offset, listener.right) / distance).clamp(-1.0, 1.0),
                (dot(offset, listener.front) / distance).clamp(-1.0, 1.0),
            )
        };
        let angle = (side * PAN_WIDTH + 1.0) * FRAC_PI_4;
        let level = distance_gain(distance, listener.min_distance, listener.max_distance)
            * (1.0 - REAR_ATTENUATION * (-ahead).max(0.0));
        // The ear facing away from the voice hears it later and duller.
        let far = [side.max(0.0), (-side).max(0.0)];
        Self {
            gain: [angle.cos() * SQRT_2 * level, angle.sin() * SQRT_2 * level],
            delay: far.map(|f| f * MAX_ITD_SAMPLES),
            filter: far.map(|f| 1.0 - HEAD_SHADOW * f),
        }
    }
}

/// Renders one voice into a stereo pair, gliding from the previous placement to the new one
/// within each frame so moving voices do not click.
#[derive(Debug, Clone)]
pub struct Spatializer {
    history: [f32; HISTORY],
    /// Index of the most recent input sample in `history`.
    head: usize,
    current: Placement,
    lowpass: [f32; 2],
}

impl Default for Spatializer {
    fn default() -> Self {
        Self {
            history: [0.0; HISTORY],
            head: 0,
            current: Placement::CENTER,
            lowpass: [0.0; 2],
        }
    }
}

impl Spatializer {
    /// Forgets the voice (new speaker in this slot): starts unplaced and silent.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Clears the delay line and filters after a silent frame, keeping the placement.
    pub fn silence(&mut self) {
        self.history = [0.0; HISTORY];
        self.lowpass = [0.0; 2];
    }

    /// Adds `input` to `left`/`right`, placed at `target`; `volume` ramps from `from` to `to`
    /// over the frame (local volume, mute).
    pub fn render(
        &mut self,
        input: &Frame,
        target: &Placement,
        from: f32,
        to: f32,
        left: &mut Frame,
        right: &mut Frame,
    ) {
        let start = self.current;
        let step = 1.0 / FRAME_SAMPLES as f32;
        for (i, &sample) in input.iter().enumerate() {
            let t = (i + 1) as f32 * step;
            let lerp = |a: f32, b: f32| a + (b - a) * t;
            self.head = (self.head + 1) % HISTORY;
            self.history[self.head] = sample;
            let volume = lerp(from, to);
            for ear in 0..2 {
                let delayed = self.delayed(lerp(start.delay[ear], target.delay[ear]));
                let a = lerp(start.filter[ear], target.filter[ear]);
                let state = &mut self.lowpass[ear];
                if a >= 1.0 {
                    *state = delayed;
                } else {
                    *state += a * (delayed - *state);
                }
                // Keeps the filter out of denormal numbers when the input falls silent.
                if state.abs() < 1e-20 {
                    *state = 0.0;
                }
                let out = *state * lerp(start.gain[ear], target.gain[ear]) * volume;
                if ear == 0 {
                    left[i] += out;
                } else {
                    right[i] += out;
                }
            }
        }
        self.current = *target;
    }

    /// The input `delay` samples ago, linearly interpolated.
    fn delayed(&self, delay: f32) -> f32 {
        let delay = delay.clamp(0.0, MAX_ITD_SAMPLES);
        let whole = delay as usize;
        let frac = delay - whole as f32;
        let at = |back: usize| self.history[(self.head + HISTORY - back) % HISTORY];
        at(whole) * (1.0 - frac) + at(whole + 1) * frac
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listener() -> ListenerState {
        ListenerState::new([0.0; 3], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0], 1.0, 15.0)
            .expect("valid orientation")
    }

    fn tone() -> Frame {
        std::array::from_fn(|i| (i as f32 * 0.05).sin() * 0.5)
    }

    fn energy(frame: &Frame) -> f32 {
        frame.iter().map(|s| s * s).sum()
    }

    /// Renders a few frames of a tone at `source` and returns the energy per ear of the last.
    fn render_at(source: Vec3) -> (f32, f32) {
        let placement = Placement::at(&listener(), source);
        let mut spatializer = Spatializer::default();
        let (mut left, mut right) = ([0.0; FRAME_SAMPLES], [0.0; FRAME_SAMPLES]);
        for _ in 0..3 {
            left.fill(0.0);
            right.fill(0.0);
            spatializer.render(&tone(), &placement, 1.0, 1.0, &mut left, &mut right);
        }
        (energy(&left), energy(&right))
    }

    #[test]
    fn a_voice_on_the_left_is_louder_in_the_left_ear() {
        let (l, r) = render_at([-3.0, 0.0, 0.0]);
        assert!(l > 4.0 * r, "left {l} right {r}");
        let (l, r) = render_at([3.0, 0.0, 0.0]);
        assert!(r > 4.0 * l, "left {l} right {r}");
    }

    #[test]
    fn a_voice_in_front_is_centered() {
        let (l, r) = render_at([0.0, 0.0, 3.0]);
        assert!((l - r).abs() < 1e-3 * l, "left {l} right {r}");
    }

    #[test]
    fn the_far_ear_is_delayed() {
        let p = Placement::at(&listener(), [5.0, 0.0, 0.0]);
        assert_eq!(p.delay[1], 0.0);
        assert!(p.delay[0] > 25.0);
        assert!(p.filter[0] < p.filter[1]);
    }

    #[test]
    fn follows_the_listener_orientation() {
        // Looking along +X, a voice at +Z is on the left.
        let turned = ListenerState::new([0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], 1.0, 15.0)
            .expect("valid");
        let p = Placement::at(&turned, [0.0, 0.0, 4.0]);
        assert!(p.gain[0] > p.gain[1]);
    }

    #[test]
    fn gets_quieter_with_distance_and_is_silent_beyond_max() {
        let near = render_at([0.0, 0.0, 0.5]);
        let mid = render_at([0.0, 0.0, 5.0]);
        let far = render_at([0.0, 0.0, 12.0]);
        let gone = render_at([0.0, 0.0, 20.0]);
        assert!(
            near.0 > mid.0 && mid.0 > far.0 && far.0 > 0.0,
            "{near:?} {mid:?} {far:?}"
        );
        assert_eq!(gone, (0.0, 0.0));
        assert_eq!(distance_gain(0.5, 1.0, 15.0), 1.0);
        assert_eq!(distance_gain(15.0, 1.0, 15.0), 0.0);
    }

    #[test]
    fn behind_is_a_little_quieter_than_in_front() {
        let front = render_at([0.0, 0.0, 3.0]);
        let back = render_at([0.0, 0.0, -3.0]);
        assert!(back.0 < front.0);
    }

    #[test]
    fn center_placement_passes_audio_through() {
        let mut spatializer = Spatializer::default();
        let (mut left, mut right) = ([0.0; FRAME_SAMPLES], [0.0; FRAME_SAMPLES]);
        spatializer.render(&tone(), &Placement::CENTER, 1.0, 1.0, &mut left, &mut right);
        assert_eq!(left, tone());
        assert_eq!(right, tone());
    }

    #[test]
    fn listener_is_shared_and_validated() {
        let shared = Listener::default();
        assert!(shared.state().is_none());
        shared.set_distances(2.0, 1.0);
        shared.set_pose([1.0, 2.0, 3.0], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]);
        let state = shared.state().expect("active");
        assert_eq!(state.position, [1.0, 2.0, 3.0]);
        assert_eq!(state.right, [1.0, 0.0, 0.0]);
        assert_eq!((state.min_distance, state.max_distance), (2.0, 3.0));
        shared.set_pose([0.0; 3], [0.0; 3], [0.0, 1.0, 0.0]);
        assert!(shared.state().is_none(), "no orientation");
        shared.set_pose([0.0; 3], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]);
        shared.clear();
        assert!(shared.state().is_none());
    }
}
