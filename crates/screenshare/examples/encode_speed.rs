//! Measures realtime encode speed at 720p: `cargo run --release -p mumcord-screenshare --example encode_speed`.

use std::time::{Duration, Instant};

use mumcord_screenshare::{Encoder, EncoderConfig, I420Frame};

const WIDTH: u32 = 1280;
const HEIGHT: u32 = 720;
const FPS: u32 = 30;
const FRAMES: u32 = 300;

fn main() -> Result<(), mumcord_screenshare::Error> {
    let mut encoder = Encoder::new(EncoderConfig {
        width: WIDTH,
        height: HEIGHT,
        fps: FPS,
        bitrate_kbps: 1500,
    })?;
    let frames: Vec<I420Frame> = (0..FPS).map(screen_like).collect::<Result<_, _>>()?;

    let mut bytes = 0;
    let mut busy = Duration::ZERO;
    let start = Instant::now();
    for index in 0..FRAMES {
        let frame = &frames[(index % FPS) as usize];
        let begin = Instant::now();
        for packet in encoder.encode(frame, i64::from(index * 1000 / FPS), false)? {
            bytes += packet.data.len();
        }
        busy += begin.elapsed();
    }
    let wall = start.elapsed().as_secs_f64();
    println!(
        "{FRAMES} frames {WIDTH}x{HEIGHT}: {:.0} fps max, {:.1} ms/frame, {:.0} kbit/s at {FPS} fps",
        f64::from(FRAMES) / wall,
        busy.as_secs_f64() * 1000.0 / f64::from(FRAMES),
        bytes as f64 * 8.0 / 1000.0 / (f64::from(FRAMES) / f64::from(FPS)),
    );
    Ok(())
}

/// Text-like rows over a gradient, scrolling a little each frame.
fn screen_like(index: u32) -> Result<I420Frame, mumcord_screenshare::Error> {
    let (w, h) = (WIDTH as usize, HEIGHT as usize);
    let mut data = vec![128u8; I420Frame::buffer_len(WIDTH, HEIGHT)];
    let scroll = index as usize * 3;
    for y in 0..h {
        for x in 0..w {
            let glyph =
                ((y + scroll) / 18).is_multiple_of(2) && ((x * 7 + y * 3 + scroll) % 11) < 4;
            data[y * w + x] = if glyph { 30 } else { (180 + x * 60 / w) as u8 };
        }
    }
    I420Frame::new(WIDTH, HEIGHT, data)
}
