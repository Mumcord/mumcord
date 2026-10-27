# mumcord-codec

The device-free voice layer of [Mumcord](https://github.com/Mumcord/mumcord), a modern voice chat client for standard Mumble servers. It is meant for clients and bots that need to send and receive Mumble voice without touching audio hardware.

It provides the Opus encoder and decoder (with a default and a "Music quality" mode), a jitter buffer, a mixer with positional placement, adaptive bitrate control and the voice processing chain (noise suppression, echo cancellation, gate, leveler). It has no cpal, ALSA or resampling dependency, so it builds for static musl targets. Microphone and speaker I/O is in `mumcord-audio`, which re-exports everything here. Network transport is in `mumcord-protocol`.

All audio is 48 kHz mono `f32`, split into 10 ms frames of 480 samples.

## Example

Encode one 20 ms packet and decode it again:

```rust
use mumcord_codec::codec::{MAX_DECODED_SAMPLES, MAX_PACKET_BYTES, OpusDecoder, OpusEncoder, PacketDuration};
use mumcord_codec::FRAME_SAMPLES;

let mut encoder = OpusEncoder::new(40_000, PacketDuration::Ms20).expect("encoder");
let mut decoder = OpusDecoder::new().expect("decoder");
let mut packet = [0u8; MAX_PACKET_BYTES];
let mut pcm = vec![0.0f32; MAX_DECODED_SAMPLES];

let frame = [0.0f32; FRAME_SAMPLES];
assert!(encoder.push_frame(&frame, &mut packet).expect("encode").is_none());
let len = encoder.push_frame(&frame, &mut packet).expect("encode").expect("packet");
let samples = decoder.decode(&packet[..len], &mut pcm).expect("decode");
assert_eq!(samples, 2 * FRAME_SAMPLES);
```

## Licence

AGPL-3.0-only.
