//! Fast guided filter (He & Sun, 2015) for edge-aware mask refinement (FR-8.34 step 2).
//!
//! The filter runs at quarter resolution and yields the linear coefficients `a` and `b`. The
//! compositor upsamples them and evaluates `alpha = a · luma + b` against the full-resolution
//! luma, so the mask edge snaps to the real picture edges (hair, shoulders) even though the
//! model only saw a 256-pixel image.

use crate::ops::box_filter;

/// Window radius at quarter resolution (4 px there is about 16 px in the 720p frame).
const RADIUS: usize = 4;
/// Regularisation: larger values smooth more and follow weak edges less.
const EPS: f32 = 1e-3;

#[derive(Debug, Default)]
pub struct GuidedFilter {
    w: usize,
    h: usize,
    mean_i: Vec<f32>,
    mean_p: Vec<f32>,
    prod: Vec<f32>,
    corr_ip: Vec<f32>,
    corr_ii: Vec<f32>,
    tmp: Vec<f32>,
}

impl GuidedFilter {
    pub fn new(w: usize, h: usize) -> Self {
        let n = w * h;
        Self {
            w,
            h,
            mean_i: vec![0.0; n],
            mean_p: vec![0.0; n],
            prod: vec![0.0; n],
            corr_ip: vec![0.0; n],
            corr_ii: vec![0.0; n],
            tmp: vec![0.0; n],
        }
    }

    /// Computes the smoothed coefficients for guide `guide` (luma) and input `mask`.
    pub fn run(&mut self, guide: &[f32], mask: &[f32], a: &mut [f32], b: &mut [f32]) {
        let (w, h, n) = (self.w, self.h, self.w * self.h);
        box_filter(guide, w, h, RADIUS, &mut self.tmp, &mut self.mean_i);
        box_filter(mask, w, h, RADIUS, &mut self.tmp, &mut self.mean_p);
        for ((p, i), m) in self.prod.iter_mut().zip(guide).zip(mask) {
            *p = i * m;
        }
        box_filter(&self.prod, w, h, RADIUS, &mut self.tmp, &mut self.corr_ip);
        for (p, i) in self.prod.iter_mut().zip(guide) {
            *p = i * i;
        }
        box_filter(&self.prod, w, h, RADIUS, &mut self.tmp, &mut self.corr_ii);
        for k in 0..n {
            let mi = self.mean_i[k];
            let var = self.corr_ii[k] - mi * mi;
            let cov = self.corr_ip[k] - mi * self.mean_p[k];
            let ak = cov / (var + EPS);
            // Reuse the product buffers for the raw coefficients.
            self.prod[k] = ak;
            self.corr_ip[k] = self.mean_p[k] - ak * mi;
        }
        box_filter(&self.prod, w, h, RADIUS, &mut self.tmp, a);
        box_filter(&self.corr_ip, w, h, RADIUS, &mut self.tmp, b);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The refined edge follows the guide's edge, not the blocky mask edge.
    #[test]
    fn the_refined_edge_follows_the_guide() {
        let (w, h) = (40, 20);
        // Guide: bright left of column 18, dark from 18. Mask: person left of column 20.
        let guide: Vec<f32> = (0..w * h)
            .map(|i| if i % w < 18 { 0.8 } else { 0.2 })
            .collect();
        let mask: Vec<f32> = (0..w * h)
            .map(|i| if i % w < 20 { 1.0 } else { 0.0 })
            .collect();
        let mut gf = GuidedFilter::new(w, h);
        let (mut a, mut b) = (vec![0.0; w * h], vec![0.0; w * h]);
        gf.run(&guide, &mask, &mut a, &mut b);
        let alpha = |x: usize| a[10 * w + x] * guide[10 * w + x] + b[10 * w + x];
        assert!(alpha(5) > 0.95, "inside {}", alpha(5));
        assert!(alpha(35) < 0.05, "outside {}", alpha(35));
        // Columns 18 and 19 are dark like the background, so they are mostly background.
        assert!(alpha(19) < 0.5, "edge {}", alpha(19));
        assert!(alpha(17) > 0.5, "edge {}", alpha(17));
    }
}
