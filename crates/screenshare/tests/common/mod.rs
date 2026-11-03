//! Synthetic screen-like test frames shared by the codec tests.

use mumcord_screenshare::I420Frame;

/// Gradient background with a moving bar of sharp-edged blocks, like scrolling text.
pub fn pattern(width: u32, height: u32, index: u32) -> I420Frame {
    let (w, h) = (width as usize, height as usize);
    let mut data = vec![0u8; I420Frame::buffer_len(width, height)];
    let shift = index as usize * 4;
    for y in 0..h {
        for x in 0..w {
            let text = (y / 16).is_multiple_of(3) && ((x + shift) / 8).is_multiple_of(2);
            data[y * w + x] = if text {
                20
            } else {
                (64 + (x * 128) / w + (y * 32) / h) as u8
            };
        }
    }
    let (cw, ch) = (w.div_ceil(2), h.div_ceil(2));
    for i in 0..cw * ch {
        data[w * h + i] = 110 + (i % cw * 20 / cw) as u8;
        data[w * h + cw * ch + i] = 140;
    }
    I420Frame::new(width, height, data).expect("pattern has the right size")
}

pub fn psnr(a: &[u8], b: &[u8]) -> f64 {
    let mse = a
        .iter()
        .zip(b)
        .map(|(x, y)| (f64::from(*x) - f64::from(*y)).powi(2))
        .sum::<f64>()
        / a.len() as f64;
    if mse == 0.0 {
        return f64::INFINITY;
    }
    10.0 * (255.0f64.powi(2) / mse).log10()
}

/// Rows of glyph-like dots scrolling up every frame: everything changes, the hardest case for
/// rate control.
pub fn scrolling_text(width: u32, height: u32, index: u32) -> I420Frame {
    let (w, h) = (width as usize, height as usize);
    let mut data = vec![128u8; I420Frame::buffer_len(width, height)];
    let scroll = index as usize * 3;
    for y in 0..h {
        for x in 0..w {
            let glyph =
                ((y + scroll) / 18).is_multiple_of(2) && ((x * 7 + y * 3 + scroll) % 11) < 4;
            data[y * w + x] = if glyph { 30 } else { (180 + x * 60 / w) as u8 };
        }
    }
    I420Frame::new(width, height, data).expect("pattern has the right size")
}
