//! Measures realtime encode speed:
//! `cargo run --release -p mumcord-screenshare --example encode_speed -- [vp9|av1|av1-text] [WIDTHxHEIGHT] [FPS]`
//! (default: vp9 1280x720 30). `av1-text` turns on AV1's screen-content tools (Sharp text).

use std::time::{Duration, Instant};

use mumcord_screenshare::{
    Av1Encoder, Av1Options, Encoder, EncoderConfig, I420Frame, VideoEncoder,
};

const SECONDS: u32 = 10;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let codec = args.first().map_or("vp9", String::as_str);
    let size = args.get(1).map_or("1280x720", String::as_str);
    let (width, height) = size.split_once('x').ok_or("size must look like 1280x720")?;
    let (width, height): (u32, u32) = (width.parse()?, height.parse()?);
    let fps: u32 = args.get(2).map_or(Ok(30), |fps| fps.parse())?;
    // About 0.07 bits per pixel, the scale of the quality steps (720p30 ≈ 2 Mbit/s, FR-7.37).
    let bitrate_kbps = (u64::from(width * height) * u64::from(fps) * 7 / 100_000) as u32;
    let config = EncoderConfig {
        width,
        height,
        fps,
        bitrate_kbps,
    };
    let mut encoder: Box<dyn VideoEncoder> = match codec {
        "vp9" => Box::new(Encoder::new(config)?),
        "av1" => Box::new(Av1Encoder::new(config, Av1Options::default())?),
        "av1-text" => Box::new(Av1Encoder::new(
            config,
            Av1Options {
                screen_content: true,
            },
        )?),
        other => return Err(format!("unknown codec {other}").into()),
    };
    let frames: Vec<I420Frame> = (0..fps)
        .map(|index| screen_like(width, height, index))
        .collect::<Result<_, _>>()?;

    let total = fps * SECONDS;
    let mut bytes = 0;
    let mut busy = Duration::ZERO;
    let start = Instant::now();
    for index in 0..total {
        let frame = &frames[(index % fps) as usize];
        let begin = Instant::now();
        for packet in encoder.encode(frame, i64::from(index * 1000 / fps), false)? {
            bytes += packet.data.len();
        }
        busy += begin.elapsed();
    }
    let wall = start.elapsed().as_secs_f64();
    let per_frame_ms = busy.as_secs_f64() * 1000.0 / f64::from(total);
    println!(
        "{codec} {total} frames {width}x{height}: {:.0} fps max, {per_frame_ms:.1} ms/frame \
         ({:.0}% of the frame budget), {:.0} kbit/s for a target of {bitrate_kbps} at {fps} fps",
        f64::from(total) / wall,
        per_frame_ms * f64::from(fps) / 10.0,
        bytes as f64 * 8.0 / 1000.0 / f64::from(SECONDS),
    );
    Ok(())
}

/// Text-like rows over a gradient, scrolling a little each frame.
fn screen_like(
    width: u32,
    height: u32,
    index: u32,
) -> Result<I420Frame, mumcord_screenshare::Error> {
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
    I420Frame::new(width, height, data)
}
