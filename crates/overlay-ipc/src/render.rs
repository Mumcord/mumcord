//! Draws a scene into an RGBA image on the CPU. Runs on the game-side IPC thread whenever the
//! scene or options change, never per frame; the render thread only uploads the finished image.

use ab_glyph::{Font, FontRef, PxScale, ScaleFont, point};

use crate::wire::{Config, Person, Scene};

/// Noto Sans Medium, subset to Latin, Greek and Cyrillic (SIL OFL 1.1, see `assets/OFL.txt`).
const FONT: &[u8] = include_bytes!("../assets/NotoSans-Medium-subset.ttf");

/// The bundled font. `None` only if the embedded file were broken (covered by a test).
pub fn font() -> Option<FontRef<'static>> {
    FontRef::try_from_slice(FONT).ok()
}

/// Premultiplied RGBA pixels, row by row, `width * height * 4` bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl Image {
    fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            pixels: vec![0; (width as usize) * (height as usize) * 4],
        }
    }

    /// Premultiplied pixel at (x, y).
    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let i = ((y as usize) * (self.width as usize) + x as usize) * 4;
        [
            self.pixels[i],
            self.pixels[i + 1],
            self.pixels[i + 2],
            self.pixels[i + 3],
        ]
    }

    /// Source-over blend of `rgb` with coverage `alpha` (0–1) at (x, y).
    fn blend(&mut self, x: i64, y: i64, rgb: [u8; 3], alpha: f32) {
        if x < 0 || y < 0 || x >= i64::from(self.width) || y >= i64::from(self.height) {
            return;
        }
        let alpha = alpha.clamp(0.0, 1.0);
        if alpha <= 0.0 {
            return;
        }
        // Both are non-negative and within the image, checked above.
        #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
        let i = ((y as usize) * (self.width as usize) + x as usize) * 4;
        let px = &mut self.pixels[i..i + 4];
        let keep = 1.0 - alpha;
        for c in 0..3 {
            px[c] = to_u8(f32::from(rgb[c]) * alpha + f32::from(px[c]) * keep);
        }
        px[3] = to_u8(255.0 * alpha + f32::from(px[3]) * keep);
    }
}

fn to_u8(v: f32) -> u8 {
    // Clamped to the u8 range first.
    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
    let v = v.round().clamp(0.0, 255.0) as u8;
    v
}

const BACKGROUND: [u8; 3] = [16, 18, 24];
const BACKGROUND_ALPHA: f32 = 0.78;
const TEXT: [u8; 3] = [242, 243, 247];
const DIM_TEXT: [u8; 3] = [170, 175, 186];
const TALKING: [u8; 3] = [67, 196, 129];
const SILENT: [u8; 3] = [112, 117, 130];
const MUTED: [u8; 3] = [237, 76, 79];

/// Layout in pixels at a scale of 1.
const PAD: f32 = 8.0;
const ROW: f32 = 26.0;
const NAME_PX: f32 = 15.0;
const HEADER_PX: f32 = 12.0;
const HEADER_ROW: f32 = 20.0;
const DOT_RADIUS: f32 = 5.0;
const GAP: f32 = 8.0;
const RADIUS: f32 = 8.0;
const MIN_WIDTH: f32 = 120.0;
const MAX_WIDTH: f32 = 320.0;

/// The overlay panel for `scene`, or `None` when there is nothing to show. `display_scale`
/// grows the panel on large screens (e.g. 2.0 at 4K); the per-game size and opacity come from
/// `config`.
pub fn render(
    config: &Config,
    scene: &Scene,
    font: &FontRef<'_>,
    display_scale: f32,
) -> Option<Image> {
    if !config.enabled || scene.people.is_empty() {
        return None;
    }
    let size = f32::from(config.size_percent.clamp(50, 200)) / 100.0;
    let u = (size * display_scale.clamp(0.5, 4.0)).max(0.25);
    let name_font = font.as_scaled(PxScale::from(NAME_PX * u));
    let header_font = font.as_scaled(PxScale::from(HEADER_PX * u));
    let header = !scene.channel.is_empty();

    let text_left = PAD * u + DOT_RADIUS * 2.0 * u + GAP * u;
    let max_text = MAX_WIDTH * u - text_left - PAD * u;
    let names: Vec<String> = scene
        .people
        .iter()
        .map(|p| fit(&name_font, &p.name, max_text))
        .collect();
    let channel = fit(&header_font, &scene.channel, MAX_WIDTH * u - 2.0 * PAD * u);
    let widest_name = names
        .iter()
        .map(|n| width(&name_font, n))
        .fold(0.0, f32::max);
    let header_width = if header {
        width(&header_font, &channel) + 2.0 * PAD * u
    } else {
        0.0
    };
    let panel_w = (text_left + widest_name + PAD * u)
        .max(header_width)
        .clamp(MIN_WIDTH * u, MAX_WIDTH * u)
        .ceil();
    let header_h = if header { HEADER_ROW * u } else { 0.0 };
    // `people` is at most a few dozen entries.
    #[allow(clippy::cast_precision_loss)]
    let rows = scene.people.len() as f32;
    let panel_h = (PAD * u * 2.0 + header_h + ROW * u * rows).ceil();
    // Both are positive and bounded by MAX_WIDTH × 4 and a few thousand pixels.
    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
    let mut image = Image::new(panel_w as u32, panel_h as u32);

    rounded_rect(&mut image, RADIUS * u, BACKGROUND, BACKGROUND_ALPHA);
    let mut y = PAD * u;
    if header {
        let baseline = y + (HEADER_ROW * u + header_font.ascent() + header_font.descent()) / 2.0;
        draw_text(
            &mut image,
            font,
            HEADER_PX * u,
            &channel,
            PAD * u,
            baseline,
            DIM_TEXT,
        );
        y += header_h;
    }
    for (person, name) in scene.people.iter().zip(&names) {
        let center_y = y + ROW * u / 2.0;
        let dot_x = PAD * u + DOT_RADIUS * u;
        circle(
            &mut image,
            dot_x,
            center_y,
            DOT_RADIUS * u,
            dot_color(person),
        );
        if person.talking {
            ring(
                &mut image,
                dot_x,
                center_y,
                DOT_RADIUS * u + 2.5 * u,
                1.2 * u,
                TALKING,
            );
        }
        let baseline = center_y + (name_font.ascent() + name_font.descent()) / 2.0;
        let color = if person.talking { TEXT } else { DIM_TEXT };
        draw_text(
            &mut image,
            font,
            NAME_PX * u,
            name,
            text_left,
            baseline,
            color,
        );
        y += ROW * u;
    }

    let opacity = f32::from(config.opacity_percent.clamp(20, 100)) / 100.0;
    if opacity < 1.0 {
        for v in &mut image.pixels {
            *v = to_u8(f32::from(*v) * opacity);
        }
    }
    Some(image)
}

fn dot_color(person: &Person) -> [u8; 3] {
    if person.muted || person.deafened {
        MUTED
    } else if person.talking {
        TALKING
    } else {
        SILENT
    }
}

fn width<F: Font, S: ScaleFont<F>>(font: &S, text: &str) -> f32 {
    let mut w = 0.0;
    let mut previous = None;
    for c in text.chars() {
        let id = font.glyph_id(c);
        if let Some(p) = previous {
            w += font.kern(p, id);
        }
        w += font.h_advance(id);
        previous = Some(id);
    }
    w
}

/// `text`, shortened with an ellipsis to fit `max` pixels.
fn fit<F: Font, S: ScaleFont<F>>(font: &S, text: &str, max: f32) -> String {
    // Control characters would draw as boxes; names are plain text anyway.
    let clean: String = text.chars().filter(|c| !c.is_control()).collect();
    if width(font, &clean) <= max {
        return clean;
    }
    let mut cut = clean;
    while !cut.is_empty() {
        cut.pop();
        let candidate = format!("{}…", cut.trim_end());
        if width(font, &candidate) <= max {
            return candidate;
        }
    }
    String::from("…")
}

fn draw_text(
    image: &mut Image,
    font: &FontRef<'_>,
    px: f32,
    text: &str,
    x: f32,
    baseline: f32,
    rgb: [u8; 3],
) {
    let scale = PxScale::from(px);
    let scaled = font.as_scaled(scale);
    let mut caret = x;
    let mut previous = None;
    for c in text.chars() {
        let id = scaled.glyph_id(c);
        if let Some(p) = previous {
            caret += scaled.kern(p, id);
        }
        let glyph = id.with_scale_and_position(scale, point(caret, baseline));
        caret += scaled.h_advance(id);
        previous = Some(id);
        if let Some(outline) = font.outline_glyph(glyph) {
            let bounds = outline.px_bounds();
            outline.draw(|gx, gy, coverage| {
                // Glyph bounds are small and within the image size range.
                #[allow(clippy::cast_possible_truncation)]
                let (px, py) = (
                    bounds.min.x as i64 + i64::from(gx),
                    bounds.min.y as i64 + i64::from(gy),
                );
                image.blend(px, py, rgb, coverage);
            });
        }
    }
}

/// Coverage of a pixel centre at distance `d` from an edge (positive = inside), anti-aliased
/// over one pixel.
fn edge(d: f32) -> f32 {
    (d + 0.5).clamp(0.0, 1.0)
}

fn rounded_rect(image: &mut Image, radius: f32, rgb: [u8; 3], alpha: f32) {
    // Sizes are far below f32 precision limits.
    #[allow(clippy::cast_precision_loss)]
    let (w, h) = (image.width as f32, image.height as f32);
    for y in 0..image.height {
        for x in 0..image.width {
            #[allow(clippy::cast_precision_loss)]
            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
            let cx = px.clamp(radius, w - radius);
            let cy = py.clamp(radius, h - radius);
            let d = radius - ((px - cx).powi(2) + (py - cy).powi(2)).sqrt();
            image.blend(i64::from(x), i64::from(y), rgb, alpha * edge(d));
        }
    }
}

fn for_box(
    image: &mut Image,
    cx: f32,
    cy: f32,
    r: f32,
    mut f: impl FnMut(&mut Image, i64, i64, f32),
) {
    // Small, positive panel coordinates.
    #[allow(clippy::cast_possible_truncation)]
    let (x0, x1, y0, y1) = (
        (cx - r - 1.0).floor() as i64,
        (cx + r + 1.0).ceil() as i64,
        (cy - r - 1.0).floor() as i64,
        (cy + r + 1.0).ceil() as i64,
    );
    for y in y0..=y1 {
        for x in x0..=x1 {
            #[allow(clippy::cast_precision_loss)]
            let d = ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt();
            f(image, x, y, d);
        }
    }
}

fn circle(image: &mut Image, cx: f32, cy: f32, r: f32, rgb: [u8; 3]) {
    for_box(image, cx, cy, r, |img, x, y, d| {
        img.blend(x, y, rgb, edge(r - d))
    });
}

fn ring(image: &mut Image, cx: f32, cy: f32, r: f32, thickness: f32, rgb: [u8; 3]) {
    for_box(image, cx, cy, r, |img, x, y, d| {
        img.blend(x, y, rgb, edge(thickness / 2.0 - (d - r).abs()));
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::Corner;

    fn config() -> Config {
        Config {
            enabled: true,
            corner: Corner::TopLeft,
            size_percent: 100,
            opacity_percent: 100,
            show_whole_channel: false,
            show_channel_name: true,
        }
    }

    fn person(name: &str, talking: bool) -> Person {
        Person {
            name: name.into(),
            talking,
            muted: false,
            deafened: false,
            is_self: false,
        }
    }

    #[test]
    fn the_bundled_font_loads_and_covers_accents() {
        let font = font().expect("font");
        for c in ['A', 'ä', 'ß', 'Ж', 'λ', 'ő', '…'] {
            assert_ne!(font.glyph_id(c).0, 0, "missing glyph for {c}");
        }
    }

    #[test]
    fn nothing_to_draw_without_people_or_when_disabled() {
        let font = font().expect("font");
        let scene = Scene::default();
        assert!(render(&config(), &scene, &font, 1.0).is_none());
        let scene = Scene {
            channel: String::new(),
            people: vec![person("A", true)],
        };
        let off = Config {
            enabled: false,
            ..config()
        };
        assert!(render(&off, &scene, &font, 1.0).is_none());
    }

    #[test]
    fn rows_grow_the_panel_and_draw_the_talking_dot_and_text() {
        let font = font().expect("font");
        let one = Scene {
            channel: String::new(),
            people: vec![person("Ana", true)],
        };
        let two = Scene {
            channel: "Lobby".into(),
            people: vec![person("Ana", true), person("Bob", false)],
        };
        let a = render(&config(), &one, &font, 1.0).expect("image");
        let b = render(&config(), &two, &font, 1.0).expect("image");
        assert_eq!(a.height, 16 + 26);
        assert_eq!(b.height, 16 + 20 + 52);
        assert_eq!(a.width, 120);
        assert_eq!(a.pixels.len(), (a.width * a.height * 4) as usize);
        // The talking dot's centre is green and opaque.
        let dot = a.pixel(13, 21);
        assert_eq!(dot[3], 255);
        assert!(dot[1] > dot[0] && dot[1] > dot[2], "dot is {dot:?}");
        // Some pixels in the text area are bright (the glyphs).
        let bright = (26..60)
            .flat_map(|x| (8..34).map(move |y| (x, y)))
            .any(|(x, y)| a.pixel(x, y)[0] > 200);
        assert!(bright);
        // Corners are transparent (rounded).
        assert_eq!(a.pixel(0, 0)[3], 0);
        // Inside the background is the translucent panel colour.
        assert!(a.pixel(a.width - 4, a.height / 2)[3] > 150);
    }

    #[test]
    fn size_and_opacity_apply() {
        let font = font().expect("font");
        let scene = Scene {
            channel: String::new(),
            people: vec![person("Ana", true)],
        };
        let big = Config {
            size_percent: 200,
            ..config()
        };
        let image = render(&big, &scene, &font, 1.0).expect("image");
        assert_eq!(image.height, 2 * (16 + 26));
        let faint = Config {
            opacity_percent: 50,
            ..config()
        };
        let image = render(&faint, &scene, &font, 1.0).expect("image");
        let dot = image.pixel(13, 21);
        assert!((126..=129).contains(&dot[3]), "alpha {}", dot[3]);
        // Premultiplied: no colour channel exceeds alpha.
        assert!(
            image
                .pixels
                .chunks(4)
                .all(|p| p[0] <= p[3] && p[1] <= p[3] && p[2] <= p[3])
        );
    }

    #[test]
    fn long_names_are_shortened() {
        let font = font().expect("font");
        let scaled = font.as_scaled(PxScale::from(15.0));
        let name = "A very long name that does not fit into the overlay at all";
        let short = fit(&scaled, name, 100.0);
        assert!(short.ends_with('…'));
        assert!(width(&scaled, &short) <= 100.0);
        assert_eq!(fit(&scaled, "Ana\u{7}", 100.0), "Ana");
        let scene = Scene {
            channel: String::new(),
            people: vec![person(name, true)],
        };
        let image = render(&config(), &scene, &font, 1.0).expect("image");
        assert!((300..=320).contains(&image.width), "width {}", image.width);
    }
}
