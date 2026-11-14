//! Measures the allocating I420-to-RGBA fallback conversion, not decoding, IPC or painting:
//! `cargo run --release -p mumcord-screenshare --example rgba_speed`.

use std::hint::black_box;
use std::time::Instant;

use mumcord_screenshare::I420Frame;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    for (width, height) in [(1280, 720), (1920, 1080), (3840, 2160)] {
        let data = (0..I420Frame::buffer_len(width, height))
            .map(|index| (index.wrapping_mul(17) % 256) as u8)
            .collect();
        let picture = I420Frame::new(width, height, data)?;
        let mut samples = Vec::new();
        for _ in 0..7 {
            let start = Instant::now();
            for _ in 0..20 {
                black_box(black_box(&picture).to_rgba());
            }
            samples.push(start.elapsed().as_secs_f64() * 1000.0 / 20.0);
        }
        samples.sort_by(f64::total_cmp);
        println!(
            "{width}x{height}: {:.3} ms/frame median; samples {samples:?}",
            samples[samples.len() / 2],
        );
    }
    Ok(())
}
