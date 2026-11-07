//! The per-frame effect pipeline (FR-8.34) on NV12 frames, in place.
//!
//! Work happens at quarter resolution (320×180 for 720p) wherever the eye can't tell, and only
//! the final blend touches every pixel:
//!
//! 1. luma and chroma are averaged down to quarter resolution; the model input is resampled
//!    from there (256×256 square at the full level, 256×144 landscape at the light level);
//! 2. full level: a fast guided filter turns the model mask into per-pixel linear coefficients
//!    `alpha = a · luma + b`, evaluated later against the full-resolution luma so edges follow
//!    hair and shoulders; light level: the mask gets a small blur and is upsampled bilinearly;
//! 3. temporal smoothing, weighted by motion: where the picture changed, the new result wins;
//!    still areas keep most of the history (no flicker, no ghosting);
//! 4. composite: the soft mask is remapped slightly towards the inside (no halo) and stays
//!    feathered over a few pixels by the quarter-resolution upsampling; images get a light wrap
//!    (the background's colour seeps onto the person's edge);
//! 5. blur: separable Gaussian at quarter resolution (eighth at the light level), two radii
//!    blended by the strength, upscaled during the blend.
//!
//! All buffers are sized when the frame size or the background changes; processing a frame
//! does not allocate.

use std::time::Duration;

use crate::EffectsError;
use crate::background::{Background, BackgroundImage, rgb_to_nv12};
use crate::guided::GuidedFilter;
use crate::level::Level;
use crate::ops::{
    Grid, box_filter, downsample_chroma4, downsample_luma4, downsample2, gaussian_blur,
    gaussian_kernel, min_max_3x3, resize_bilinear, upsample_row,
};
use crate::segmenter::{ModelShape, Segmenter};

/// A mask with less than this share of person (or of background) looks wrong.
const IMPLAUSIBLE_SHARE: f32 = 0.01;
/// Model runs in a row with such a mask before it is logged (about two seconds).
const IMPLAUSIBLE_RUNS: u32 = 30;

/// Light level: the model runs at 15 fps; frames in between reuse the smoothed mask.
const LIGHT_MODEL_INTERVAL: Duration = Duration::from_millis(62);
/// Mask remap: values up to `LO` are background, from `HI` person. The model leaves a soft
/// fringe of about 0.3–0.5 around hair and ears that would show as a halo; the midpoint 0.7
/// also pulls the edge slightly inwards.
const REMAP_LO: f32 = 0.5;
const REMAP_HI: f32 = 0.9;
/// Temporal smoothing: weight of the new result in still areas, and how fast motion raises it.
const STILL_WEIGHT: f32 = 0.3;
const MOTION_GAIN: f32 = 10.0;
/// Blur sigmas in quarter-resolution pixels for strength 0 and 100 (about 6 and 20 px at 720p).
const BLUR_SIGMA_LIGHT: f32 = 1.5;
const BLUR_SIGMA_STRONG: f32 = 5.0;
/// Light wrap: strongest share of background colour on the person's edge, and the reach of
/// the wrap in quarter-resolution pixels.
const WRAP_STRENGTH: f32 = 0.35;
const WRAP_RADIUS: usize = 3;
const KERNEL_LEN: usize = 2 * 16 + 1;

/// Cell classes: every pixel of the 4×4 block is person (left as is), background (no blend
/// needed), or needs the per-pixel blend. Most of a frame is one of the first two.
const OPAQUE: u8 = 0;
const TRANSPARENT: u8 = 1;
const MIXED: u8 = 2;

/// The background as prepared for the current frame size.
enum Prepared {
    None,
    Blur { mix: f32 },
    Image(PreparedImage),
}

struct PreparedImage {
    nv12: Vec<u8>,
    /// Quarter-resolution blurred copy (Y, U, V, 0..=1) for the light wrap.
    wrap_y: Vec<f32>,
    wrap_u: Vec<f32>,
    wrap_v: Vec<f32>,
}

/// Scratch rows of frame width.
#[derive(Default)]
struct Rows {
    line: Vec<f32>,
    a: Vec<f32>,
    b: Vec<f32>,
    bg: Vec<f32>,
    wrap: Vec<f32>,
    wrap_colour: Vec<f32>,
    alpha: [Vec<f32>; 2],
    // Chroma rows (frame width / 2).
    c_alpha: Vec<f32>,
    c_u: Vec<f32>,
    c_v: Vec<f32>,
    c_wrap: Vec<f32>,
}

pub struct Pipeline {
    width: usize,
    height: usize,
    w4: usize,
    h4: usize,
    background: Background,
    prepared: Prepared,
    // Quarter-resolution planes (0..=1).
    y4: Vec<f32>,
    u4: Vec<f32>,
    v4: Vec<f32>,
    prev_y4: Vec<f32>,
    mask4: Vec<f32>,
    a4: Vec<f32>,
    b4: Vec<f32>,
    new_a4: Vec<f32>,
    new_b4: Vec<f32>,
    wrap4: Vec<f32>,
    tmp4: Vec<f32>,
    tmp4b: Vec<f32>,
    scratch_lo: Vec<f32>,
    scratch_hi: Vec<f32>,
    bg_y: Vec<f32>,
    bg_u: Vec<f32>,
    bg_v: Vec<f32>,
    // Eighth resolution for the light level's coarser blur.
    y8: Vec<f32>,
    u8_: Vec<f32>,
    v8: Vec<f32>,
    model_in: Vec<f32>,
    model_mask: Vec<f32>,
    implausible_run: u32,
    mask_warned: bool,
    model_shape: Option<ModelShape>,
    gf: GuidedFilter,
    kernel_light: [f32; KERNEL_LEN],
    kernel_strong: [f32; KERNEL_LEN],
    /// Per quarter-resolution cell: [`OPAQUE`], [`TRANSPARENT`] or [`MIXED`].
    cells: Vec<u8>,
    rows: Rows,
    has_history: bool,
    last_model_at: Option<Duration>,
    level: Level,
}

impl Pipeline {
    /// Whether frames of this size can be processed (NV12 with sides divisible by 8).
    pub fn supports(width: u32, height: u32) -> bool {
        width >= 64 && height >= 64 && width.is_multiple_of(8) && height.is_multiple_of(8)
    }

    /// Allocates every buffer for `width × height` frames.
    pub fn new(width: u32, height: u32, background: Background) -> Self {
        let (w, h) = (width as usize, height as usize);
        let (w4, h4) = (w / 4, h / 4);
        let (w8, h8) = (w4 / 2, h4 / 2);
        let n4 = w4 * h4;
        let n8 = w8 * h8;
        let mut kernel_light = [0.0; KERNEL_LEN];
        let mut kernel_strong = [0.0; KERNEL_LEN];
        gaussian_kernel(BLUR_SIGMA_LIGHT, &mut kernel_light);
        gaussian_kernel(BLUR_SIGMA_STRONG, &mut kernel_strong);
        let row = || vec![0.0f32; w];
        let chroma_row = || vec![0.0f32; w / 2];
        let mut pipeline = Self {
            width: w,
            height: h,
            w4,
            h4,
            background: Background::None,
            prepared: Prepared::None,
            y4: vec![0.0; n4],
            u4: vec![0.0; n4],
            v4: vec![0.0; n4],
            prev_y4: vec![0.0; n4],
            mask4: vec![0.0; n4],
            a4: vec![0.0; n4],
            b4: vec![0.0; n4],
            new_a4: vec![0.0; n4],
            new_b4: vec![0.0; n4],
            wrap4: vec![0.0; n4],
            tmp4: vec![0.0; n4],
            tmp4b: vec![0.0; n4],
            scratch_lo: vec![0.0; n4],
            scratch_hi: vec![0.0; n4],
            bg_y: vec![0.0; n4],
            bg_u: vec![0.0; n4],
            bg_v: vec![0.0; n4],
            y8: vec![0.0; n8],
            u8_: vec![0.0; n8],
            v8: vec![0.0; n8],
            model_in: Vec::new(),
            model_mask: Vec::new(),
            implausible_run: 0,
            mask_warned: false,
            model_shape: None,
            gf: GuidedFilter::new(w4, h4),
            kernel_light,
            kernel_strong,
            cells: vec![MIXED; n4],
            rows: Rows {
                line: row(),
                a: row(),
                b: row(),
                bg: row(),
                wrap: row(),
                wrap_colour: row(),
                alpha: [row(), row()],
                c_alpha: chroma_row(),
                c_u: chroma_row(),
                c_v: chroma_row(),
                c_wrap: chroma_row(),
            },
            has_history: false,
            last_model_at: None,
            level: Level::Full,
        };
        pipeline.set_background(background);
        pipeline
    }

    pub fn size(&self) -> (u32, u32) {
        (self.width as u32, self.height as u32)
    }

    pub fn background(&self) -> &Background {
        &self.background
    }

    /// Changes the background; an image is scaled to the frame here, not per frame.
    pub fn set_background(&mut self, background: Background) {
        self.prepared = match &background {
            Background::None => Prepared::None,
            Background::Blur { strength } => Prepared::Blur {
                mix: f32::from((*strength).min(100)) / 100.0,
            },
            Background::Image(image) => Prepared::Image(self.prepare_image(image)),
        };
        self.background = background;
    }

    /// Switching level restarts the temporal history: the two models disagree slightly.
    pub fn set_level(&mut self, level: Level) {
        if self.level != level {
            self.level = level;
            self.has_history = false;
            self.last_model_at = None;
        }
    }

    fn prepare_image(&self, image: &BackgroundImage) -> PreparedImage {
        let (w, h) = (self.width, self.height);
        let mut nv12 = vec![0u8; w * h * 3 / 2];
        rgb_to_nv12(&image.cover(w as u32, h as u32), &mut nv12);
        let n4 = self.w4 * self.h4;
        let (mut y, mut u, mut v) = (vec![0.0; n4], vec![0.0; n4], vec![0.0; n4]);
        downsample_luma4(&nv12[..w * h], w, h, &mut y);
        downsample_chroma4(&nv12[w * h..], w, h, &mut u, &mut v);
        let mut kernel = [0.0; KERNEL_LEN];
        let radius = gaussian_kernel(3.0, &mut kernel);
        let mut tmp = vec![0.0; n4];
        let mut blurred = |plane: &[f32]| {
            let mut out = vec![0.0; n4];
            gaussian_blur(plane, self.w4, self.h4, &kernel, radius, &mut tmp, &mut out);
            out
        };
        PreparedImage {
            wrap_y: blurred(&y),
            wrap_u: blurred(&u),
            wrap_v: blurred(&v),
            nv12,
        }
    }

    /// Model input buffers for `shape`; sized once per model.
    fn ensure_model(&mut self, shape: ModelShape) {
        if self.model_shape != Some(shape) {
            let (mw, mh) = shape.size();
            self.model_in = vec![0.0; 3 * mw * mh];
            self.model_mask = vec![0.0; mw * mh];
            self.model_shape = Some(shape);
        }
    }

    /// Applies the background effect to `frame` (NV12 of the pipeline's size) in place.
    pub fn process(
        &mut self,
        frame: &mut [u8],
        timestamp: Duration,
        segmenter: &mut dyn Segmenter,
    ) -> Result<(), EffectsError> {
        if matches!(self.prepared, Prepared::None) {
            return Ok(());
        }
        let (w, h) = (self.width, self.height);
        if frame.len() < w * h * 3 / 2 {
            return Err(EffectsError::FrameSize);
        }
        // Allocates only when the model changes, not per frame.
        self.ensure_model(segmenter.shape());
        let (luma, chroma) = frame.split_at_mut(w * h);
        downsample_luma4(luma, w, h, &mut self.y4);
        downsample_chroma4(chroma, w, h, &mut self.u4, &mut self.v4);

        let model_due = match (self.level, self.last_model_at) {
            (Level::Full, _) | (_, None) => true,
            (Level::Light, Some(last)) => {
                timestamp.saturating_sub(last) >= LIGHT_MODEL_INTERVAL || timestamp < last
            }
        };
        if model_due {
            self.run_model(timestamp, segmenter)?;
            self.last_model_at = Some(timestamp);
            self.refine();
        }
        self.prepare_background();
        self.classify();
        self.composite(frame);
        Ok(())
    }

    fn run_model(
        &mut self,
        timestamp: Duration,
        segmenter: &mut dyn Segmenter,
    ) -> Result<(), EffectsError> {
        let (mw, mh) = segmenter.shape().size();
        let n = mw * mh;
        let (r, rest) = self.model_in.split_at_mut(n);
        let (g, b) = rest.split_at_mut(n);
        resize_bilinear(&self.y4, self.w4, self.h4, r, mw, mh);
        resize_bilinear(&self.u4, self.w4, self.h4, g, mw, mh);
        resize_bilinear(&self.v4, self.w4, self.h4, b, mw, mh);
        // In place: the three planes hold Y, U, V and become R, G, B (BT.601 limited range).
        for i in 0..n {
            let y = (r[i] * 255.0 - 16.0) * (1.0 / 219.0);
            let u = (g[i] * 255.0 - 128.0) * (1.0 / 224.0);
            let v = (b[i] * 255.0 - 128.0) * (1.0 / 224.0);
            r[i] = (y + 1.402 * v).clamp(0.0, 1.0);
            g[i] = (y - 0.344_136 * u - 0.714_136 * v).clamp(0.0, 1.0);
            b[i] = (y + 1.772 * u).clamp(0.0, 1.0);
        }
        segmenter.segment(&self.model_in, timestamp, &mut self.model_mask)?;
        self.check_mask();
        resize_bilinear(&self.model_mask, mw, mh, &mut self.mask4, self.w4, self.h4);
        Ok(())
    }

    /// Logs once per session when the model sees no person (or only person) for a while: then
    /// blur covers the person too, and a picture of that is the only hint (#568).
    fn check_mask(&mut self) {
        if self.mask_warned || self.model_mask.is_empty() {
            return;
        }
        let person = self.model_mask.iter().filter(|&&m| m > 0.5).count();
        let share = person as f32 / self.model_mask.len() as f32;
        if (IMPLAUSIBLE_SHARE..=1.0 - IMPLAUSIBLE_SHARE).contains(&share) {
            self.implausible_run = 0;
            return;
        }
        self.implausible_run += 1;
        if self.implausible_run >= IMPLAUSIBLE_RUNS {
            self.mask_warned = true;
            tracing::warn!(
                person_share = share,
                runs = self.implausible_run,
                width = self.width,
                height = self.height,
                "background effects find no person in the camera picture"
            );
        }
    }

    /// Steps 2 and 3: edge refinement and motion-weighted temporal smoothing.
    fn refine(&mut self) {
        let (w4, h4) = (self.w4, self.h4);
        match self.level {
            Level::Full => {
                self.gf
                    .run(&self.y4, &self.mask4, &mut self.new_a4, &mut self.new_b4);
            }
            Level::Light => {
                self.new_a4.fill(0.0);
                box_filter(&self.mask4, w4, h4, 1, &mut self.tmp4, &mut self.new_b4);
            }
        }
        if self.has_history {
            for i in 0..w4 * h4 {
                let motion = (self.y4[i] - self.prev_y4[i]).abs();
                let weight = (STILL_WEIGHT + motion * MOTION_GAIN).min(1.0);
                self.a4[i] += (self.new_a4[i] - self.a4[i]) * weight;
                self.b4[i] += (self.new_b4[i] - self.b4[i]) * weight;
            }
        } else {
            self.a4.copy_from_slice(&self.new_a4);
            self.b4.copy_from_slice(&self.new_b4);
            self.has_history = true;
        }
        self.prev_y4.copy_from_slice(&self.y4);
    }

    /// Step 5 for blur, and the light-wrap weights for images.
    fn prepare_background(&mut self) {
        let (w4, h4) = (self.w4, self.h4);
        match &self.prepared {
            Prepared::None => {}
            Prepared::Blur { mix } => {
                let mix = *mix;
                let (w8, h8) = (w4 / 2, h4 / 2);
                let kernels = (&self.kernel_light, &self.kernel_strong);
                let scratch = (self.tmp4.as_mut_slice(), self.tmp4b.as_mut_slice());
                // Chroma is blurred at eighth resolution at both levels: colour detail is
                // invisible in a blurred background, and it saves two thirds of the work. At
                // eighth resolution the same kernels reach twice as far.
                downsample2(&self.u4, w4, h4, &mut self.u8_);
                downsample2(&self.v4, w4, h4, &mut self.v8);
                blur_mix(
                    &self.u8_,
                    (w8, h8),
                    kernels,
                    mix,
                    (&mut *scratch.0, &mut *scratch.1),
                    &mut self.bg_u,
                );
                blur_mix(
                    &self.v8,
                    (w8, h8),
                    kernels,
                    mix,
                    (&mut *scratch.0, &mut *scratch.1),
                    &mut self.bg_v,
                );
                match self.level {
                    Level::Full => {
                        blur_mix(&self.y4, (w4, h4), kernels, mix, scratch, &mut self.bg_y);
                    }
                    Level::Light => {
                        // The light level's coarser blur: luma at eighth resolution too.
                        downsample2(&self.y4, w4, h4, &mut self.y8);
                        blur_mix(&self.y8, (w8, h8), kernels, mix, scratch, &mut self.bg_y);
                    }
                }
            }
            Prepared::Image(_) => {
                if self.level == Level::Full {
                    // Smoothed quarter-resolution mask, widened: 1 deep inside the person,
                    // falling off towards the edge; the wrap weight is its complement.
                    for i in 0..w4 * h4 {
                        self.tmp4b[i] = remap(self.a4[i] * self.y4[i] + self.b4[i]);
                    }
                    box_filter(
                        &self.tmp4b,
                        w4,
                        h4,
                        WRAP_RADIUS,
                        &mut self.tmp4,
                        &mut self.wrap4,
                    );
                    for v in &mut self.wrap4 {
                        *v = (1.0 - *v) * WRAP_STRENGTH;
                    }
                }
            }
        }
    }

    /// Sorts the quarter-resolution cells into person, background and edge. The pixel value
    /// `a · luma + b` interpolates the coefficients of the cell and its neighbours, so it lies
    /// within their range over all luma values 0..=1; a cell is decided when that whole range
    /// is clearly person or clearly background (and, for images, no light wrap reaches it).
    fn classify(&mut self) {
        let (w4, h4, n4) = (self.w4, self.h4, self.w4 * self.h4);
        for i in 0..n4 {
            let (a, b) = (self.a4[i], self.b4[i]);
            self.new_a4[i] = b.min(a + b);
            self.new_b4[i] = b.max(a + b);
        }
        // new_a4/new_b4 are free until the next model run; tmp4/tmp4b hold the results.
        min_max_3x3(
            &self.new_a4,
            &self.new_b4,
            (w4, h4),
            (&mut self.scratch_lo, &mut self.scratch_hi),
            (&mut self.tmp4, &mut self.tmp4b),
        );
        let wrap = self.level == Level::Full && matches!(self.prepared, Prepared::Image(_));
        if wrap {
            // Widen the wrap reach by one cell, like the mask range.
            min_max_3x3(
                &self.wrap4,
                &self.wrap4,
                (w4, h4),
                (&mut self.scratch_lo, &mut self.scratch_hi),
                (&mut self.new_a4, &mut self.new_b4),
            );
        }
        for i in 0..n4 {
            let wrap_max = if wrap { self.new_b4[i] } else { 0.0 };
            self.cells[i] = if self.tmp4b[i] <= REMAP_LO {
                TRANSPARENT
            } else if self.tmp4[i] >= REMAP_HI && wrap_max <= 0.0 {
                OPAQUE
            } else {
                MIXED
            };
        }
    }

    /// Step 4: blends person and background, two luma rows and one chroma row at a time, run
    /// by run of equally classified cells.
    fn composite(&mut self, frame: &mut [u8]) {
        let (w, h, w4, h4) = (self.width, self.height, self.w4, self.h4);
        let (luma, chroma) = frame.split_at_mut(w * h);
        let rows = &mut self.rows;
        let quarter = Grid {
            w: w4,
            h: h4,
            factor: 4,
        };
        let quarter_c = Grid {
            factor: 2,
            ..quarter
        };
        // Blur planes: quarter resolution at the full level, eighth at the light level.
        let eighth = Grid {
            w: w4 / 2,
            h: h4 / 2,
            factor: 8,
        };
        // Blur planes: luma at quarter resolution (full level) or eighth (light level); chroma
        // at eighth resolution, 4× below the chroma plane.
        let bg = match self.level {
            Level::Full => quarter,
            Level::Light => eighth,
        };
        let bg_c = Grid {
            factor: 4,
            ..eighth
        };
        let wrap = self.level == Level::Full;
        for cy in 0..h / 2 {
            let cells = &self.cells[(cy / 2) * w4..][..w4];
            for half in 0..2 {
                let y = cy * 2 + half;
                let out = &mut luma[y * w..][..w];
                for (start, end, class) in runs(cells) {
                    let (x0, x1) = (start * 4, end * 4);
                    if class == OPAQUE {
                        continue;
                    }
                    if class == TRANSPARENT {
                        match &self.prepared {
                            Prepared::None => {}
                            Prepared::Blur { .. } => {
                                upsample_row(
                                    &self.bg_y,
                                    bg,
                                    y,
                                    (x0, x1),
                                    &mut rows.line,
                                    &mut rows.bg,
                                );
                                for (o, b) in out[x0..x1].iter_mut().zip(&rows.bg[x0..x1]) {
                                    *o = to_u8(b * 255.0);
                                }
                            }
                            Prepared::Image(image) => {
                                out[x0..x1].copy_from_slice(&image.nv12[y * w + x0..y * w + x1]);
                            }
                        }
                        continue;
                    }
                    upsample_row(&self.a4, quarter, y, (x0, x1), &mut rows.line, &mut rows.a);
                    upsample_row(&self.b4, quarter, y, (x0, x1), &mut rows.line, &mut rows.b);
                    let alpha_row = &mut rows.alpha[half];
                    for x in x0..x1 {
                        let q = rows.a[x] * f32::from(out[x]) * (1.0 / 255.0) + rows.b[x];
                        alpha_row[x] = remap(q);
                    }
                    match &self.prepared {
                        Prepared::None => {}
                        Prepared::Blur { .. } => {
                            upsample_row(&self.bg_y, bg, y, (x0, x1), &mut rows.line, &mut rows.bg);
                            for x in x0..x1 {
                                out[x] = blend(f32::from(out[x]), rows.bg[x] * 255.0, alpha_row[x]);
                            }
                        }
                        Prepared::Image(image) => {
                            let bg_row = &image.nv12[y * w..][..w];
                            if wrap {
                                upsample_row(
                                    &self.wrap4,
                                    quarter,
                                    y,
                                    (x0, x1),
                                    &mut rows.line,
                                    &mut rows.wrap,
                                );
                                upsample_row(
                                    &image.wrap_y,
                                    quarter,
                                    y,
                                    (x0, x1),
                                    &mut rows.line,
                                    &mut rows.wrap_colour,
                                );
                                for x in x0..x1 {
                                    let fg = f32::from(out[x]);
                                    let fg = fg + (rows.wrap_colour[x] * 255.0 - fg) * rows.wrap[x];
                                    out[x] = blend(fg, f32::from(bg_row[x]), alpha_row[x]);
                                }
                            } else {
                                for x in x0..x1 {
                                    out[x] = blend(
                                        f32::from(out[x]),
                                        f32::from(bg_row[x]),
                                        alpha_row[x],
                                    );
                                }
                            }
                        }
                    }
                }
            }
            let out = &mut chroma[cy * w..][..w];
            for (start, end, class) in runs(cells) {
                // Chroma pixels: two per cell; bytes: four per cell.
                let (c0, c1) = (start * 2, end * 2);
                if class == OPAQUE {
                    continue;
                }
                if class == TRANSPARENT {
                    match &self.prepared {
                        Prepared::None => {}
                        Prepared::Blur { .. } => {
                            upsample_row(
                                &self.bg_u,
                                bg_c,
                                cy,
                                (c0, c1),
                                &mut rows.line,
                                &mut rows.c_u,
                            );
                            upsample_row(
                                &self.bg_v,
                                bg_c,
                                cy,
                                (c0, c1),
                                &mut rows.line,
                                &mut rows.c_v,
                            );
                            for cx in c0..c1 {
                                out[2 * cx] = to_u8(rows.c_u[cx] * 255.0);
                                out[2 * cx + 1] = to_u8(rows.c_v[cx] * 255.0);
                            }
                        }
                        Prepared::Image(image) => {
                            let src = &image.nv12[w * h + cy * w..][..w];
                            out[2 * c0..2 * c1].copy_from_slice(&src[2 * c0..2 * c1]);
                        }
                    }
                    continue;
                }
                // Alpha is the mean of the 2×2 luma alphas under each chroma sample.
                for cx in c0..c1 {
                    rows.c_alpha[cx] = (rows.alpha[0][2 * cx]
                        + rows.alpha[0][2 * cx + 1]
                        + rows.alpha[1][2 * cx]
                        + rows.alpha[1][2 * cx + 1])
                        * 0.25;
                }
                match &self.prepared {
                    Prepared::None => {}
                    Prepared::Blur { .. } => {
                        upsample_row(
                            &self.bg_u,
                            bg_c,
                            cy,
                            (c0, c1),
                            &mut rows.line,
                            &mut rows.c_u,
                        );
                        upsample_row(
                            &self.bg_v,
                            bg_c,
                            cy,
                            (c0, c1),
                            &mut rows.line,
                            &mut rows.c_v,
                        );
                        for cx in c0..c1 {
                            let a = rows.c_alpha[cx];
                            out[2 * cx] = blend(f32::from(out[2 * cx]), rows.c_u[cx] * 255.0, a);
                            out[2 * cx + 1] =
                                blend(f32::from(out[2 * cx + 1]), rows.c_v[cx] * 255.0, a);
                        }
                    }
                    Prepared::Image(image) => {
                        let src = &image.nv12[w * h + cy * w..][..w];
                        if wrap {
                            upsample_row(
                                &self.wrap4,
                                quarter_c,
                                cy,
                                (c0, c1),
                                &mut rows.line,
                                &mut rows.c_wrap,
                            );
                            upsample_row(
                                &image.wrap_u,
                                quarter_c,
                                cy,
                                (c0, c1),
                                &mut rows.line,
                                &mut rows.c_u,
                            );
                            upsample_row(
                                &image.wrap_v,
                                quarter_c,
                                cy,
                                (c0, c1),
                                &mut rows.line,
                                &mut rows.c_v,
                            );
                            for cx in c0..c1 {
                                let (a, k) = (rows.c_alpha[cx], rows.c_wrap[cx]);
                                let u = f32::from(out[2 * cx]);
                                let v = f32::from(out[2 * cx + 1]);
                                let u = u + (rows.c_u[cx] * 255.0 - u) * k;
                                let v = v + (rows.c_v[cx] * 255.0 - v) * k;
                                out[2 * cx] = blend(u, f32::from(src[2 * cx]), a);
                                out[2 * cx + 1] = blend(v, f32::from(src[2 * cx + 1]), a);
                            }
                        } else {
                            for cx in c0..c1 {
                                let a = rows.c_alpha[cx];
                                out[2 * cx] =
                                    blend(f32::from(out[2 * cx]), f32::from(src[2 * cx]), a);
                                out[2 * cx + 1] = blend(
                                    f32::from(out[2 * cx + 1]),
                                    f32::from(src[2 * cx + 1]),
                                    a,
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Runs of equal values in `cells`, as `(start, end, value)`.
fn runs(cells: &[u8]) -> impl Iterator<Item = (usize, usize, u8)> + '_ {
    let mut start = 0;
    std::iter::from_fn(move || {
        let value = *cells.get(start)?;
        let len = cells[start..].iter().take_while(|&&c| c == value).count();
        let run = (start, start + len, value);
        start += len;
        Some(run)
    })
}

#[inline]
fn to_u8(value: f32) -> u8 {
    (value + 0.5).clamp(0.0, 255.0) as u8
}

/// Blurs `src` with both kernels and mixes them: `mix` 0 = light blur, 1 = strong blur.
fn blur_mix(
    src: &[f32],
    (w, h): (usize, usize),
    (light, strong): (&[f32; KERNEL_LEN], &[f32; KERNEL_LEN]),
    mix: f32,
    (tmp, scratch): (&mut [f32], &mut [f32]),
    dst: &mut [f32],
) {
    let n = w * h;
    let radius_light = (BLUR_SIGMA_LIGHT * 3.0).ceil() as usize;
    let radius_strong = (BLUR_SIGMA_STRONG * 3.0).ceil() as usize;
    gaussian_blur(
        src,
        w,
        h,
        light,
        radius_light,
        &mut tmp[..n],
        &mut scratch[..n],
    );
    gaussian_blur(
        src,
        w,
        h,
        strong,
        radius_strong,
        &mut tmp[..n],
        &mut dst[..n],
    );
    for (d, s) in dst[..n].iter_mut().zip(&scratch[..n]) {
        *d = s + (*d - s) * mix;
    }
}

#[inline]
fn remap(q: f32) -> f32 {
    ((q - REMAP_LO) * (1.0 / (REMAP_HI - REMAP_LO))).clamp(0.0, 1.0)
}

/// `alpha` 1 keeps the person pixel exactly, 0 the background pixel exactly.
#[inline]
fn blend(person: f32, background: f32, alpha: f32) -> u8 {
    (background + (person - background) * alpha + 0.5).clamp(0.0, 255.0) as u8
}
