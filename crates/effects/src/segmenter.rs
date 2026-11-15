//! Segmenters for the test hooks (FR-8.41): the test camera's known outline in place of the
//! model, and a wrapper that makes any segmenter slow like a weak PC.

use std::time::Duration;

use bgmatte::{ModelShape, Segmenter};
use mumcord_camera::TestCamera;

/// The test camera's person outline at the frame shown at `timestamp` (hook
/// `MUMCORD_TEST_SEGMENT_MASK=fixture`). The test camera does not stamp frame numbers, so the
/// number is recovered from the timestamp at the capture rate; one frame off moves the outline
/// by under one pixel, far below one model pixel (five camera pixels).
pub struct FixtureSegmenter {
    shape: ModelShape,
    fps: u32,
}

impl FixtureSegmenter {
    pub fn new(shape: ModelShape, fps: u32) -> Self {
        Self {
            shape,
            fps: fps.max(1),
        }
    }
}

impl Segmenter for FixtureSegmenter {
    fn shape(&self) -> ModelShape {
        self.shape
    }

    fn segment(
        &mut self,
        _rgb: &[f32],
        timestamp: Duration,
        mask: &mut [f32],
    ) -> Result<(), bgmatte::Error> {
        let frame = (timestamp.as_secs_f64() * f64::from(self.fps)).floor() as u64;
        fixture_mask(self.shape.size(), frame, mask);
        Ok(())
    }
}

/// Rasterises the test camera's person (1280×720 coordinates) at `w × h`, sampling each mask
/// pixel at its centre.
fn fixture_mask((w, h): (usize, usize), frame: u64, mask: &mut [f32]) {
    let (cw, ch) = (
        mumcord_camera::MAX_WIDTH as f32,
        mumcord_camera::MAX_HEIGHT as f32,
    );
    for y in 0..h {
        let cy = ((y as f32 + 0.5) * ch / h as f32) as u32;
        let span = TestCamera::person_span(cy, frame);
        for x in 0..w {
            let cx = ((x as f32 + 0.5) * cw / w as f32) as u32;
            let inside = span.is_some_and(|(start, end)| (start..end).contains(&cx));
            mask[y * w + x] = if inside { 1.0 } else { 0.0 };
        }
    }
}

/// Waits `delay` before each mask (hook `MUMCORD_TEST_EFFECT_SLOW`). bgmatte measures the
/// effect time itself, so the delay has to be real for Auto to see a slow PC and step down.
pub(crate) struct SlowSegmenter {
    pub(crate) inner: Box<dyn Segmenter>,
    pub(crate) delay: Duration,
}

impl Segmenter for SlowSegmenter {
    fn shape(&self) -> ModelShape {
        self.inner.shape()
    }

    fn segment(
        &mut self,
        rgb: &[f32],
        timestamp: Duration,
        mask: &mut [f32],
    ) -> Result<(), bgmatte::Error> {
        std::thread::sleep(self.delay);
        self.inner.segment(rgb, timestamp, mask)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::*;

    #[test]
    fn the_fixture_mask_has_the_person_in_the_middle_and_background_at_the_sides() {
        let mut segmenter = FixtureSegmenter::new(ModelShape::Landscape, 30);
        let (w, h) = ModelShape::Landscape.size();
        let mut mask = vec![-1.0f32; w * h];
        segmenter
            .segment(&[], Duration::ZERO, &mut mask)
            .expect("fixture mask");
        assert!(mask.iter().all(|&v| v == 0.0 || v == 1.0));
        assert_eq!(
            mask[(h - 1) * w + w / 2],
            1.0,
            "person at the bottom centre"
        );
        assert_eq!(mask[(h - 1) * w], 0.0, "background at the left edge");
    }

    #[test]
    fn the_slow_segmenter_waits_and_keeps_the_mask() {
        let mut slow = SlowSegmenter {
            inner: Box::new(FixtureSegmenter::new(ModelShape::Square, 30)),
            delay: Duration::from_millis(20),
        };
        assert_eq!(slow.shape(), ModelShape::Square);
        let (w, h) = ModelShape::Square.size();
        let (mut direct, mut delayed) = (vec![0.0f32; w * h], vec![0.0f32; w * h]);
        FixtureSegmenter::new(ModelShape::Square, 30)
            .segment(&[], Duration::ZERO, &mut direct)
            .expect("fixture mask");
        let started = Instant::now();
        slow.segment(&[], Duration::ZERO, &mut delayed)
            .expect("slow mask");
        assert!(started.elapsed() >= Duration::from_millis(20));
        assert_eq!(direct, delayed);
    }
}
