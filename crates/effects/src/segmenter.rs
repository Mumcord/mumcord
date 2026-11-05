//! Person segmentation: the MediaPipe Selfie Segmenter through ONNX Runtime (FR-8.33), and the
//! test camera's known outline for the `MUMCORD_TEST_SEGMENT_MASK` hook (FR-8.41).

use std::time::Duration;

use mumcord_camera::TestCamera;
use ort::session::Session;
use ort::session::builder::GraphOptimizationLevel;
use ort::value::TensorRef;

use crate::EffectsError;

/// Square 256×256 model for the full level ("Best").
static SQUARE_MODEL: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/selfie_square.onnx"));
/// Landscape 256×144 model for the light level.
static LANDSCAPE_MODEL: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/selfie_landscape.onnx"));

/// One CPU thread for the model: with capture and encoding the camera must stay within about
/// 1.5 cores (spec 08 §9), and a second thread gained little at these input sizes.
const MODEL_THREADS: usize = 1;

/// Which of the two shipped models to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelShape {
    Square,
    Landscape,
}

impl ModelShape {
    /// Input width and height in pixels.
    pub fn size(self) -> (usize, usize) {
        match self {
            Self::Square => (256, 256),
            Self::Landscape => (256, 144),
        }
    }
}

/// Produces a person mask (0 = background, 1 = person) of the model's input size from a planar
/// RGB image (`3 × h × w`, values 0..=1).
pub trait Segmenter: Send {
    fn shape(&self) -> ModelShape;

    /// `timestamp` is the camera frame's timestamp; only the fixture mask uses it.
    fn segment(
        &mut self,
        rgb: &[f32],
        timestamp: Duration,
        mask: &mut [f32],
    ) -> Result<(), EffectsError>;
}

/// The Selfie Segmenter run by ONNX Runtime on the CPU.
pub struct OnnxSegmenter {
    shape: ModelShape,
    session: Session,
}

impl OnnxSegmenter {
    /// Creates the session. Fails with [`EffectsError::Unavailable`] when ONNX Runtime can't be
    /// loaded on this PC.
    pub fn new(shape: ModelShape) -> Result<Self, EffectsError> {
        crate::runtime::availability().map_err(EffectsError::Unavailable)?;
        let model = match shape {
            ModelShape::Square => SQUARE_MODEL,
            ModelShape::Landscape => LANDSCAPE_MODEL,
        };
        let session = build_session(model).map_err(EffectsError::Model)?;
        Ok(Self { shape, session })
    }
}

fn build_session(model: &[u8]) -> Result<Session, String> {
    Session::builder()
        .map_err(|e| e.to_string())?
        .with_optimization_level(GraphOptimizationLevel::Level3)
        .map_err(|e| e.to_string())?
        .with_intra_threads(MODEL_THREADS)
        .map_err(|e| e.to_string())?
        .with_inter_threads(1)
        .map_err(|e| e.to_string())?
        // Spinning worker threads would burn a core between frames.
        .with_intra_op_spinning(false)
        .map_err(|e| e.to_string())?
        .commit_from_memory(model)
        .map_err(|e| e.to_string())
}

impl Segmenter for OnnxSegmenter {
    fn shape(&self) -> ModelShape {
        self.shape
    }

    fn segment(
        &mut self,
        rgb: &[f32],
        _timestamp: Duration,
        mask: &mut [f32],
    ) -> Result<(), EffectsError> {
        let (w, h) = self.shape.size();
        let input = TensorRef::from_array_view(([1usize, 3, h, w], rgb))
            .map_err(|err| EffectsError::Model(err.to_string()))?;
        let outputs = self
            .session
            .run(ort::inputs!["pixel_values" => input])
            .map_err(|err| EffectsError::Model(err.to_string()))?;
        let (_, alphas) = outputs["alphas"]
            .try_extract_tensor::<f32>()
            .map_err(|err| EffectsError::Model(err.to_string()))?;
        if alphas.len() != mask.len() {
            return Err(EffectsError::Model(format!(
                "unexpected mask size {}",
                alphas.len()
            )));
        }
        mask.copy_from_slice(alphas);
        Ok(())
    }
}

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
    ) -> Result<(), EffectsError> {
        let frame = (timestamp.as_secs_f64() * f64::from(self.fps)).floor() as u64;
        fixture_mask(self.shape.size(), frame, mask);
        Ok(())
    }
}

/// Rasterises the test camera's person (1280×720 coordinates) at `w × h`, sampling each mask
/// pixel at its centre.
pub fn fixture_mask((w, h): (usize, usize), frame: u64, mask: &mut [f32]) {
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
