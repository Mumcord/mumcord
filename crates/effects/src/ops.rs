//! Plane operations on reduced-resolution `f32` planes (values 0..=1) and NV12 buffers.
//!
//! All functions write into caller-provided buffers and never allocate, so the per-frame path
//! stays allocation-free once the pipeline has sized its buffers.

/// Averages 4×4 blocks of the luma plane into a `w/4 × h/4` plane.
pub fn downsample_luma4(y: &[u8], width: usize, height: usize, out: &mut [f32]) {
    let (ow, oh) = (width / 4, height / 4);
    debug_assert!(y.len() >= width * height && out.len() >= ow * oh);
    for oy in 0..oh {
        let rows = [
            &y[(oy * 4) * width..][..width],
            &y[(oy * 4 + 1) * width..][..width],
            &y[(oy * 4 + 2) * width..][..width],
            &y[(oy * 4 + 3) * width..][..width],
        ];
        let out_row = &mut out[oy * ow..][..ow];
        for (ox, value) in out_row.iter_mut().enumerate() {
            let x = ox * 4;
            let mut sum = 0u32;
            for row in rows {
                sum += u32::from(row[x])
                    + u32::from(row[x + 1])
                    + u32::from(row[x + 2])
                    + u32::from(row[x + 3]);
            }
            *value = sum as f32 * (1.0 / (16.0 * 255.0));
        }
    }
}

/// Averages 2×2 blocks of the interleaved NV12 chroma plane (`width/2 × height/2` pairs) into
/// two `w/4 × h/4` planes.
pub fn downsample_chroma4(uv: &[u8], width: usize, height: usize, u: &mut [f32], v: &mut [f32]) {
    let (ow, oh) = (width / 4, height / 4);
    // One chroma row holds width/2 pairs, so `width` bytes.
    debug_assert!(uv.len() >= width * height / 2 && u.len() >= ow * oh && v.len() >= ow * oh);
    for oy in 0..oh {
        let r0 = &uv[(oy * 2) * width..][..width];
        let r1 = &uv[(oy * 2 + 1) * width..][..width];
        for ox in 0..ow {
            let x = ox * 4;
            let su =
                u32::from(r0[x]) + u32::from(r0[x + 2]) + u32::from(r1[x]) + u32::from(r1[x + 2]);
            let sv = u32::from(r0[x + 1])
                + u32::from(r0[x + 3])
                + u32::from(r1[x + 1])
                + u32::from(r1[x + 3]);
            u[oy * ow + ox] = su as f32 * (1.0 / (4.0 * 255.0));
            v[oy * ow + ox] = sv as f32 * (1.0 / (4.0 * 255.0));
        }
    }
}

/// Averages 2×2 blocks of a plane into a `w/2 × h/2` plane.
pub fn downsample2(src: &[f32], width: usize, height: usize, out: &mut [f32]) {
    let (ow, oh) = (width / 2, height / 2);
    for oy in 0..oh {
        let r0 = &src[(oy * 2) * width..][..width];
        let r1 = &src[(oy * 2 + 1) * width..][..width];
        for ox in 0..ow {
            let x = ox * 2;
            out[oy * ow + ox] = (r0[x] + r0[x + 1] + r1[x] + r1[x + 1]) * 0.25;
        }
    }
}

/// Bilinear resize with pixel-centre alignment.
pub fn resize_bilinear(src: &[f32], sw: usize, sh: usize, out: &mut [f32], dw: usize, dh: usize) {
    let sx = sw as f32 / dw as f32;
    let sy = sh as f32 / dh as f32;
    for oy in 0..dh {
        let (y0, y1, fy) = sample_pos(oy, sy, sh);
        let r0 = &src[y0 * sw..][..sw];
        let r1 = &src[y1 * sw..][..sw];
        for ox in 0..dw {
            let (x0, x1, fx) = sample_pos(ox, sx, sw);
            let top = r0[x0] + (r0[x1] - r0[x0]) * fx;
            let bottom = r1[x0] + (r1[x1] - r1[x0]) * fx;
            out[oy * dw + ox] = top + (bottom - top) * fy;
        }
    }
}

/// Source indices and weight for output index `o` at scale `step` (source pixels per output
/// pixel), centre-aligned and clamped to the edges.
fn sample_pos(o: usize, step: f32, len: usize) -> (usize, usize, f32) {
    let pos = ((o as f32 + 0.5) * step - 0.5).max(0.0);
    let i0 = (pos as usize).min(len - 1);
    let i1 = (i0 + 1).min(len - 1);
    (i0, i1, pos - i0 as f32)
}

/// Mean over a `(2r+1)²` window with edge-normalised counts (running sums, O(1) per pixel).
/// `tmp` must hold `w * h` values. Both passes walk rows, so they stay cache-friendly and the
/// vertical pass vectorises.
pub fn box_filter(src: &[f32], w: usize, h: usize, r: usize, tmp: &mut [f32], out: &mut [f32]) {
    // Horizontal running sums, divided by the column count, into tmp.
    for y in 0..h {
        let row = &src[y * w..][..w];
        let dst = &mut tmp[y * w..][..w];
        let mut sum: f32 = row[..r.min(w - 1) + 1].iter().sum();
        for x in 0..w {
            let count = (x + r + 1).min(w) - x.saturating_sub(r);
            dst[x] = sum / count as f32;
            if x + r + 1 < w {
                sum += row[x + r + 1];
            }
            if x >= r {
                sum -= row[x - r];
            }
        }
    }
    // Vertical running sums of whole rows: out row y = mean of tmp rows y-r ..= y+r.
    let first = &mut out[..w];
    first.fill(0.0);
    for y in 0..=r.min(h - 1) {
        for (s, t) in first.iter_mut().zip(&tmp[y * w..][..w]) {
            *s += t;
        }
    }
    for y in 1..h {
        let (done, todo) = out.split_at_mut(y * w);
        let prev = &done[(y - 1) * w..];
        let cur = &mut todo[..w];
        cur.copy_from_slice(prev);
        if y + r < h {
            for (c, t) in cur.iter_mut().zip(&tmp[(y + r) * w..][..w]) {
                *c += t;
            }
        }
        if y > r {
            for (c, t) in cur.iter_mut().zip(&tmp[(y - r - 1) * w..][..w]) {
                *c -= t;
            }
        }
    }
    for y in 0..h {
        let inv = 1.0 / ((y + r + 1).min(h) - y.saturating_sub(r)) as f32;
        for v in &mut out[y * w..][..w] {
            *v *= inv;
        }
    }
}

/// Minimum and maximum over each 3×3 neighbourhood (clamped at the edges); `tmp_lo` and
/// `tmp_hi` must hold `w * h` values.
pub fn min_max_3x3(
    lo_in: &[f32],
    hi_in: &[f32],
    (w, h): (usize, usize),
    (tmp_lo, tmp_hi): (&mut [f32], &mut [f32]),
    (lo_out, hi_out): (&mut [f32], &mut [f32]),
) {
    for y in 0..h {
        for x in 0..w {
            let (a, b) = (x.saturating_sub(1), (x + 1).min(w - 1));
            let i = y * w;
            tmp_lo[i + x] = lo_in[i + a].min(lo_in[i + x]).min(lo_in[i + b]);
            tmp_hi[i + x] = hi_in[i + a].max(hi_in[i + x]).max(hi_in[i + b]);
        }
    }
    for y in 0..h {
        let (a, b) = (y.saturating_sub(1) * w, (y + 1).min(h - 1) * w);
        let c = y * w;
        for x in 0..w {
            lo_out[c + x] = tmp_lo[a + x].min(tmp_lo[c + x]).min(tmp_lo[b + x]);
            hi_out[c + x] = tmp_hi[a + x].max(tmp_hi[c + x]).max(tmp_hi[b + x]);
        }
    }
}

/// Normalised Gaussian kernel of radius `ceil(3σ)`, written into `kernel` (which must hold at
/// least `2 * radius + 1` values). Returns the radius.
pub fn gaussian_kernel(sigma: f32, kernel: &mut [f32]) -> usize {
    let radius = ((sigma * 3.0).ceil() as usize).min((kernel.len() - 1) / 2);
    let mut sum = 0.0;
    for (i, k) in kernel[..2 * radius + 1].iter_mut().enumerate() {
        let d = i as f32 - radius as f32;
        *k = (-(d * d) / (2.0 * sigma * sigma)).exp();
        sum += *k;
    }
    for k in &mut kernel[..2 * radius + 1] {
        *k /= sum;
    }
    radius
}

/// Separable Gaussian blur with clamped edges. `kernel` holds `2 * radius + 1` weights.
///
/// Both passes are written as weighted sums of shifted rows so the compiler can vectorise them.
pub fn gaussian_blur(
    src: &[f32],
    w: usize,
    h: usize,
    kernel: &[f32],
    radius: usize,
    tmp: &mut [f32],
    out: &mut [f32],
) {
    let kernel = &kernel[..2 * radius + 1];
    for y in 0..h {
        let row = &src[y * w..][..w];
        let dst = &mut tmp[y * w..][..w];
        if w > 2 * radius {
            let inner = &mut dst[radius..w - radius];
            inner.fill(0.0);
            for (k, &weight) in kernel.iter().enumerate() {
                for (d, s) in inner.iter_mut().zip(&row[k..k + w - 2 * radius]) {
                    *d += s * weight;
                }
            }
        }
        // Edge pixels (and planes narrower than the kernel) with clamped indices.
        for x in (0..radius.min(w)).chain(w.saturating_sub(radius).max(radius.min(w))..w) {
            let mut acc = 0.0;
            for (k, weight) in kernel.iter().enumerate() {
                let sx = (x + k).saturating_sub(radius).min(w - 1);
                acc += row[sx] * weight;
            }
            dst[x] = acc;
        }
    }
    for y in 0..h {
        let dst = &mut out[y * w..][..w];
        dst.fill(0.0);
        for (k, &weight) in kernel.iter().enumerate() {
            let sy = (y + k).saturating_sub(radius).min(h - 1);
            for (d, s) in dst.iter_mut().zip(&tmp[sy * w..][..w]) {
                *d += s * weight;
            }
        }
    }
}

/// A reduced-resolution plane to be upscaled by `factor` (2, 4 or 8).
#[derive(Debug, Clone, Copy)]
pub struct Grid {
    pub w: usize,
    pub h: usize,
    pub factor: usize,
}

/// Writes `out[x0..x1]` of output row `y` of the bilinearly upscaled `plane` (centre-aligned,
/// clamped edges, the same sampling as [`resize_bilinear`]). `line` is scratch of at least
/// `grid.w + 2` values. Only the columns needed for the range are touched.
pub fn upsample_row(
    plane: &[f32],
    grid: Grid,
    y: usize,
    (x0, x1): (usize, usize),
    line: &mut [f32],
    out: &mut [f32],
) {
    match grid.factor {
        2 => upsample_row_by::<2>(plane, grid, y, (x0, x1), line, out),
        4 => upsample_row_by::<4>(plane, grid, y, (x0, x1), line, out),
        _ => upsample_row_by::<8>(plane, grid, y, (x0, x1), line, out),
    }
}

fn upsample_row_by<const F: usize>(
    plane: &[f32],
    grid: Grid,
    y: usize,
    (x0, x1): (usize, usize),
    line: &mut [f32],
    out: &mut [f32],
) {
    if x0 >= x1 {
        return;
    }
    let lw = grid.w;
    let (r0, r1, fy) = sample_pos(y, 1.0 / F as f32, grid.h);
    // `line[j]` holds low-resolution sample j - 1, clamped, so the edges need no branches.
    let i0 = x0 / F;
    let i1 = (x1 - 1) / F;
    let top = &plane[r0 * lw..][..lw];
    let bottom = &plane[r1 * lw..][..lw];
    for (j, l) in line.iter_mut().enumerate().take(i1 + 3).skip(i0) {
        let i = j.saturating_sub(1).min(lw - 1);
        *l = top[i] + (bottom[i] - top[i]) * fy;
    }
    // Output pixel k of a block sits (k + 0.5) / F - 0.5 samples from the block's own sample:
    // left of it, it blends with the previous sample; right of it, with the next one.
    let mut first = [0usize; F];
    let mut weight = [0.0f32; F];
    for k in 0..F {
        let d = (k as f32 + 0.5) / F as f32 - 0.5;
        (first[k], weight[k]) = if d < 0.0 { (0, 1.0 + d) } else { (1, d) };
    }
    let sample = |x: usize| {
        let j = x / F + first[x % F];
        let a = line[j];
        a + (line[j + 1] - a) * weight[x % F]
    };
    // Unaligned head and tail pixel by pixel; whole blocks with fixed indices, which the
    // compiler unrolls.
    let head_end = x0.next_multiple_of(F).min(x1);
    let tail_start = (x1 / F * F).max(head_end);
    for (o, x) in out[x0..head_end].iter_mut().zip(x0..) {
        *o = sample(x);
    }
    let (blocks, _) = out[head_end..tail_start].as_chunks_mut::<F>();
    for (block, i) in blocks.iter_mut().zip(head_end / F..) {
        let (prev, own, next) = (line[i], line[i + 1], line[i + 2]);
        for (k, o) in block.iter_mut().enumerate() {
            *o = if first[k] == 0 {
                prev + (own - prev) * weight[k]
            } else {
                own + (next - own) * weight[k]
            };
        }
    }
    for (o, x) in out[tail_start..x1].iter_mut().zip(tail_start..) {
        *o = sample(x);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn box_filter_of_a_constant_is_the_constant() {
        let (w, h) = (9, 7);
        let src = vec![0.25f32; w * h];
        let mut tmp = vec![0.0; w * h];
        let mut out = vec![0.0; w * h];
        box_filter(&src, w, h, 3, &mut tmp, &mut out);
        assert!(out.iter().all(|v| (v - 0.25).abs() < 1e-6));
    }

    #[test]
    fn box_filter_averages_the_window() {
        let (w, h) = (5, 1);
        let src = [0.0, 0.0, 1.0, 0.0, 0.0];
        let mut tmp = [0.0; 5];
        let mut out = [0.0; 5];
        box_filter(&src, w, h, 1, &mut tmp, &mut out);
        let expected = [0.0, 1.0 / 3.0, 1.0 / 3.0, 1.0 / 3.0, 0.0];
        for (o, e) in out.iter().zip(expected) {
            assert!((o - e).abs() < 1e-6, "{out:?}");
        }
    }

    #[test]
    fn gaussian_blur_keeps_the_mean_and_spreads_a_spike() {
        let (w, h) = (21, 21);
        let mut src = vec![0.0f32; w * h];
        src[10 * w + 10] = 1.0;
        let mut kernel = [0.0; 33];
        let radius = gaussian_kernel(2.0, &mut kernel);
        let mut tmp = vec![0.0; w * h];
        let mut out = vec![0.0; w * h];
        gaussian_blur(&src, w, h, &kernel, radius, &mut tmp, &mut out);
        let sum: f32 = out.iter().sum();
        assert!((sum - 1.0).abs() < 1e-4);
        assert!(out[10 * w + 10] < 0.1 && out[10 * w + 12] > 0.0);
    }

    #[test]
    fn downsampling_averages_blocks() {
        let (w, h) = (8, 4);
        let y: Vec<u8> = (0..w * h)
            .map(|i| if i % w < 4 { 0 } else { 255 })
            .collect();
        let mut out = [0.0f32; 2];
        downsample_luma4(&y, w, h, &mut out);
        assert_eq!(out, [0.0, 1.0]);
    }

    #[test]
    fn upsampling_matches_bilinear_resize() {
        let (lw, lh) = (5, 3);
        let plane: Vec<f32> = (0..lw * lh).map(|i| (i * 7 % 11) as f32 / 10.0).collect();
        for factor in [2, 4, 8] {
            let (w, h) = (lw * factor, lh * factor);
            let mut expected = vec![0.0; w * h];
            resize_bilinear(&plane, lw, lh, &mut expected, w, h);
            let grid = Grid {
                w: lw,
                h: lh,
                factor,
            };
            let mut line = vec![0.0; lw + 2];
            for y in 0..h {
                let mut row = vec![0.0; w];
                // Two ranges, to check that partial rows line up.
                upsample_row(&plane, grid, y, (0, w / 2 + 1), &mut line, &mut row);
                upsample_row(&plane, grid, y, (w / 2 + 1, w), &mut line, &mut row);
                for x in 0..w {
                    assert!(
                        (row[x] - expected[y * w + x]).abs() < 1e-5,
                        "{factor} {x},{y}"
                    );
                }
            }
        }
    }

    #[test]
    fn bilinear_resize_of_a_constant_is_the_constant() {
        let src = vec![0.5f32; 32 * 18];
        let mut out = vec![0.0f32; 256 * 144];
        resize_bilinear(&src, 32, 18, &mut out, 256, 144);
        assert!(out.iter().all(|v| (v - 0.5).abs() < 1e-6));
    }
}
