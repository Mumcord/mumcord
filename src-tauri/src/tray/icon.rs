//! Tray icon pictures (FR-3.132): the app icon greyed out while disconnected, dimmed with a red
//! slash while muted or deafened, with a green ring while someone in the user's channel talks,
//! plus a pink unread badge (with the mention count) and a moon for Do Not Disturb.

use tauri::image::Image;

use super::state::{IconState, Look};

const GREEN: [u8; 3] = [35, 209, 139];
const AMBER: [u8; 3] = [240, 178, 50];
const RED: [u8; 3] = [239, 68, 68];
const PINK: [u8; 3] = [236, 72, 153];
const MOON: [u8; 3] = [221, 214, 254];
const OUTLINE: [u8; 3] = [20, 20, 24];

/// Box-filters the app icon down to `size` × `size` RGBA.
pub fn scale(icon: &Image<'_>, size: u32) -> Image<'static> {
    let (w, h) = (icon.width().max(1), icon.height().max(1));
    let src = icon.rgba();
    let mut out = vec![0u8; (size * size * 4) as usize];
    for y in 0..size {
        for x in 0..size {
            let (x0, x1) = (x * w / size, ((x + 1) * w / size).max(x * w / size + 1));
            let (y0, y1) = (y * h / size, ((y + 1) * h / size).max(y * h / size + 1));
            let mut sum = [0u32; 4];
            for sy in y0..y1.min(h) {
                for sx in x0..x1.min(w) {
                    let i = ((sy * w + sx) * 4) as usize;
                    for (c, s) in sum.iter_mut().enumerate() {
                        *s += u32::from(src.get(i + c).copied().unwrap_or(0));
                    }
                }
            }
            let n = ((x1.min(w) - x0) * (y1.min(h) - y0)).max(1);
            let o = ((y * size + x) * 4) as usize;
            for c in 0..4 {
                out[o + c] = u8::try_from(sum[c] / n).unwrap_or(u8::MAX);
            }
        }
    }
    Image::new_owned(out, size, size)
}

/// RGBA pixels with drawing helpers; coordinates are pixel centres.
struct Canvas {
    rgba: Vec<u8>,
    w: u32,
    h: u32,
}

impl Canvas {
    fn size(&self) -> f32 {
        self.w.min(self.h) as f32
    }

    /// Calls `paint` for every pixel with its centre; a returned colour replaces the pixel.
    fn paint(&mut self, paint: impl Fn(f32, f32) -> Option<[u8; 4]>) {
        for y in 0..self.h {
            for x in 0..self.w {
                if let Some(px) = paint(x as f32 + 0.5, y as f32 + 0.5) {
                    let i = ((y * self.w + x) * 4) as usize;
                    self.rgba[i..i + 4].copy_from_slice(&px);
                }
            }
        }
    }

    fn map(&mut self, f: impl Fn([u8; 4]) -> [u8; 4]) {
        for px in self.rgba.as_chunks_mut::<4>().0 {
            *px = f(*px);
        }
    }

    /// A filled disc with an outline ring, as badges sit on the icon.
    fn badge(&mut self, cx: f32, cy: f32, radius: f32, colour: [u8; 3]) {
        let ring = self.size() * 0.05;
        self.paint(|x, y| {
            let d = (x - cx).hypot(y - cy);
            if d <= radius {
                Some(opaque(colour))
            } else if d <= radius + ring {
                Some(opaque(OUTLINE))
            } else {
                None
            }
        });
    }
}

fn opaque([r, g, b]: [u8; 3]) -> [u8; 4] {
    [r, g, b, 255]
}

fn luma([r, g, b, _]: [u8; 4]) -> f32 {
    0.299 * f32::from(r) + 0.587 * f32::from(g) + 0.114 * f32::from(b)
}

fn channel(v: f32) -> u8 {
    v.round().clamp(0.0, 255.0) as u8
}

/// Distance from `(x, y)` to the segment `a`–`b`.
fn segment_distance(x: f32, y: f32, a: (f32, f32), b: (f32, f32)) -> f32 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let t = (((x - a.0) * dx + (y - a.1) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
    (x - (a.0 + t * dx)).hypot(y - (a.1 + t * dy))
}

/// Signed distance to a centred rounded square with half side `half` and corner `radius`
/// (negative inside).
fn rounded_square_distance(x: f32, y: f32, centre: f32, half: f32, radius: f32) -> f32 {
    let qx = (x - centre).abs() - half + radius;
    let qy = (y - centre).abs() - half + radius;
    qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - radius
}

/// 3 × 5 pixel glyphs for the mention count, one row per entry, bit 2 on the left.
fn glyph(c: char) -> [u8; 5] {
    match c {
        '0' => [7, 5, 5, 5, 7],
        '1' => [2, 6, 2, 2, 7],
        '2' => [7, 1, 7, 4, 7],
        '3' => [7, 1, 7, 1, 7],
        '4' => [5, 5, 7, 1, 1],
        '5' => [7, 4, 7, 1, 7],
        '6' => [7, 4, 7, 5, 7],
        '7' => [7, 1, 2, 2, 2],
        '8' => [7, 5, 7, 5, 7],
        '9' => [7, 5, 7, 1, 7],
        _ => [0, 2, 7, 2, 0],
    }
}

/// The mention count as drawn in the badge: 1–9, then "9+".
fn count_text(mentions: u32) -> String {
    if mentions > 9 {
        "9+".into()
    } else {
        mentions.to_string()
    }
}

/// Draws `text` centred on `(cx, cy)` with square pixels of `px` side.
fn draw_text(canvas: &mut Canvas, text: &str, cx: f32, cy: f32, px: f32) {
    let glyphs: Vec<[u8; 5]> = text.chars().map(glyph).collect();
    let width = glyphs.len() as f32 * 4.0 - 1.0;
    let (left, top) = (cx - width * px / 2.0, cy - 2.5 * px);
    canvas.paint(|x, y| {
        let col = ((x - left) / px).floor();
        let row = ((y - top) / px).floor();
        if col < 0.0 || row < 0.0 || row >= 5.0 || col >= width {
            return None;
        }
        let (index, bit) = ((col as usize) / 4, (col as usize) % 4);
        let lit = bit < 3 && glyphs[index][row as usize] & (4 >> bit) != 0;
        lit.then_some([255, 255, 255, 255])
    });
}

/// The tray icon for `state`, drawn over the scaled app icon `base`.
pub fn compose(base: &Image<'_>, state: IconState) -> Image<'static> {
    let mut canvas = Canvas {
        rgba: base.rgba().to_vec(),
        w: base.width(),
        h: base.height(),
    };
    let size = canvas.size();
    match state.look {
        Look::Disconnected | Look::Connecting => canvas.map(|px| {
            let grey = channel(luma(px) * 0.8);
            [grey, grey, grey, channel(f32::from(px[3]) * 0.6)]
        }),
        Look::Muted => {
            canvas.map(|[r, g, b, a]| {
                let dim = |v: u8| channel(f32::from(v) * 0.45);
                [dim(r), dim(g), dim(b), a]
            });
            let (a, b) = ((size * 0.2, size * 0.8), (size * 0.8, size * 0.2));
            let (stroke, outline) = (size * 0.06, size * 0.1);
            canvas.paint(|x, y| {
                let d = segment_distance(x, y, a, b);
                if d <= stroke {
                    Some(opaque(RED))
                } else if d <= outline {
                    Some(opaque(OUTLINE))
                } else {
                    None
                }
            });
        }
        Look::Talking => {
            let (centre, half, ring) = (size / 2.0, size / 2.0, size * 0.09);
            canvas.paint(|x, y| {
                let d = rounded_square_distance(x, y, centre, half, size * 0.22);
                (d <= 0.0 && d >= -ring).then_some(opaque(GREEN))
            });
        }
        Look::Idle => {}
    }
    if state.look == Look::Connecting {
        let r = size * 0.16;
        canvas.badge(size - r - size * 0.06, size - r - size * 0.06, r, AMBER);
    }
    if state.unread || state.mentions > 0 {
        let r = if state.mentions > 0 {
            size * 0.27
        } else {
            size * 0.17
        };
        let (cx, cy) = (size - r - size * 0.03, r + size * 0.03);
        canvas.badge(cx, cy, r, PINK);
        if state.mentions > 0 {
            let px = (r * 2.0 * 0.55 / 5.0).floor().max(1.0);
            draw_text(&mut canvas, &count_text(state.mentions), cx, cy, px);
        }
    }
    if state.dnd {
        let r = size * 0.2;
        let (cx, cy) = (r + size * 0.05, size - r - size * 0.05);
        canvas.badge(cx, cy, r, OUTLINE);
        let (moon, offset) = (r * 0.7, r * 0.45);
        canvas.paint(|x, y| {
            let inside = (x - cx).hypot(y - cy) <= moon;
            let bitten = (x - cx - offset).hypot(y - cy + offset) <= moon;
            (inside && !bitten).then_some(opaque(MOON))
        });
    }
    Image::new_owned(canvas.rgba, canvas.w, canvas.h)
}

/// A small coloured dot for menu rows (server state, unread chats).
pub fn dot(colour: [u8; 3], size: u32) -> Image<'static> {
    let mut canvas = Canvas {
        rgba: vec![0; (size * size * 4) as usize],
        w: size,
        h: size,
    };
    let (c, r) = (size as f32 / 2.0, size as f32 * 0.3);
    canvas.paint(|x, y| ((x - c).hypot(y - c) <= r).then_some(opaque(colour)));
    Image::new_owned(canvas.rgba, size, size)
}

#[cfg(test)]
mod tests {
    use super::*;

    const WHITE: [u8; 4] = [255, 255, 255, 255];

    fn state(look: Look) -> IconState {
        IconState {
            look,
            unread: false,
            mentions: 0,
            dnd: false,
        }
    }

    fn at(img: &Image<'_>, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * img.width() + x) * 4) as usize;
        img.rgba()[i..i + 4].try_into().expect("four bytes")
    }

    fn white() -> Image<'static> {
        Image::new_owned(WHITE.repeat(64 * 64), 64, 64)
    }

    #[test]
    fn idle_is_the_plain_icon() {
        assert_eq!(compose(&white(), state(Look::Idle)).rgba(), white().rgba());
    }

    #[test]
    fn disconnected_is_grey_and_faded() {
        let img = compose(&white(), state(Look::Disconnected));
        assert_eq!(at(&img, 32, 32), [204, 204, 204, 153]);
    }

    #[test]
    fn muted_is_dimmed_with_a_red_slash() {
        let img = compose(&white(), state(Look::Muted));
        assert_eq!(at(&img, 32, 32), opaque(RED));
        assert_eq!(at(&img, 10, 10), [115, 115, 115, 255]);
    }

    #[test]
    fn talking_draws_a_green_ring_along_the_edge() {
        let img = compose(&white(), state(Look::Talking));
        assert_eq!(at(&img, 32, 1), opaque(GREEN));
        assert_eq!(at(&img, 32, 32), WHITE);
        // The rounded corner stays outside the ring.
        assert_eq!(at(&img, 0, 0), WHITE);
    }

    #[test]
    fn unread_adds_a_pink_dot_and_mentions_a_number() {
        let mut s = state(Look::Idle);
        s.unread = true;
        let dot = compose(&white(), s);
        assert_eq!(at(&dot, 52, 12), opaque(PINK));
        assert_eq!(at(&dot, 10, 54), WHITE);
        s.mentions = 3;
        let counted = compose(&white(), s);
        // The middle bar of the "3" in the badge centre.
        assert_eq!(at(&counted, 44, 19), WHITE);
        assert_eq!(at(&counted, 41, 16), opaque(PINK));
    }

    #[test]
    fn dnd_adds_a_moon_bottom_left() {
        let mut s = state(Look::Idle);
        s.dnd = true;
        let img = compose(&white(), s);
        assert_eq!(at(&img, 12, 51), opaque(MOON));
        assert_eq!(at(&img, 52, 12), WHITE);
    }

    #[test]
    fn mention_count_caps_at_nine_plus() {
        assert_eq!(count_text(4), "4");
        assert_eq!(count_text(12), "9+");
    }

    #[test]
    fn menu_dot_is_round() {
        let img = dot([1, 2, 3], 16);
        assert_eq!(at(&img, 8, 8), [1, 2, 3, 255]);
        assert_eq!(at(&img, 0, 0), [0, 0, 0, 0]);
    }

    #[test]
    fn scaling_averages_pixels() {
        let base = Image::new_owned(vec![200; 128 * 128 * 4], 128, 128);
        let img = scale(&base, 64);
        assert_eq!((img.width(), img.height()), (64, 64));
        assert!(img.rgba().iter().all(|&b| b == 200));
    }
}
