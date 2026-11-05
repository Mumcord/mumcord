//! Synthetic frames and masks shared by the integration tests.

#![allow(dead_code)]

use std::time::Duration;

use mumcord_effects::{EffectsError, ModelShape, Segmenter};

pub const W: usize = 1280;
pub const H: usize = 720;
/// The synthetic person: a rectangle in camera pixels.
pub const PERSON_X: std::ops::Range<usize> = 480..800;
pub const PERSON_Y: std::ops::Range<usize> = 160..720;
pub const PERSON_YUV: (u8, u8, u8) = (200, 100, 150);

/// Vertical bars 16 px wide (dark/bright luma, two chroma tints) with the person in front.
pub fn frame() -> Vec<u8> {
    let mut nv12 = vec![0u8; W * H * 3 / 2];
    for y in 0..H {
        for x in 0..W {
            nv12[y * W + x] = if is_person(x, y) {
                PERSON_YUV.0
            } else if (x / 16) % 2 == 0 {
                40
            } else {
                220
            };
        }
    }
    for cy in 0..H / 2 {
        for cx in 0..W / 2 {
            let (u, v) = if is_person(cx * 2, cy * 2) {
                (PERSON_YUV.1, PERSON_YUV.2)
            } else if (cx / 8) % 2 == 0 {
                (90, 170)
            } else {
                (160, 90)
            };
            nv12[W * H + cy * W + cx * 2] = u;
            nv12[W * H + cy * W + cx * 2 + 1] = v;
        }
    }
    nv12
}

pub fn is_person(x: usize, y: usize) -> bool {
    PERSON_X.contains(&x) && PERSON_Y.contains(&y)
}

/// Distance in pixels from the person's outline (the bottom side is the frame edge, not an
/// outline).
pub fn edge_distance(x: usize, y: usize) -> usize {
    if is_person(x, y) {
        (x - PERSON_X.start)
            .min(PERSON_X.end - 1 - x)
            .min(y - PERSON_Y.start)
    } else {
        let dx = PERSON_X
            .start
            .saturating_sub(x)
            .max((x + 1).saturating_sub(PERSON_X.end));
        let dy = PERSON_Y.start.saturating_sub(y);
        dx.max(dy)
    }
}

/// Segmenter that returns the rectangle as its mask.
pub struct RectSegmenter(pub ModelShape);

impl Segmenter for RectSegmenter {
    fn shape(&self) -> ModelShape {
        self.0
    }

    fn segment(
        &mut self,
        _rgb: &[f32],
        _timestamp: Duration,
        mask: &mut [f32],
    ) -> Result<(), EffectsError> {
        let (w, h) = self.0.size();
        for y in 0..h {
            for x in 0..w {
                let cx = ((x as f32 + 0.5) * W as f32 / w as f32) as usize;
                let cy = ((y as f32 + 0.5) * H as f32 / h as f32) as usize;
                mask[y * w + x] = if is_person(cx, cy) { 1.0 } else { 0.0 };
            }
        }
        Ok(())
    }
}
