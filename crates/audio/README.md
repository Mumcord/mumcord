# mumcord-audio

The voice audio layer of [Mumcord](https://github.com/Mumcord/mumcord), a modern voice chat client for standard Mumble servers. It is meant for clients and bots that need to send and receive Mumble voice.

It provides the Opus encoder and decoder, a jitter buffer, a mixer with positional placement, microphone and speaker I/O (via cpal), resampling, and the voice processing chain (noise suppression, echo cancellation, gate, leveler). Network transport is not part of this crate; see `mumcord-protocol` for that.

All audio inside the crate is 48 kHz mono `f32`, split into 10 ms frames of 480 samples.

## Example

Encode one 20 ms packet and decode it again:

```rust
use mumcord_audio::codec::{MAX_DECODED_SAMPLES, MAX_PACKET_BYTES, OpusDecoder, OpusEncoder, PacketDuration};
use mumcord_audio::FRAME_SAMPLES;

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

## Features

- `io` (default): microphone and speaker I/O via cpal (capture, playback, device listing, the stream controller, resampling and the microphone test). It pulls in cpal and, on Linux, ALSA.
- `test-backend`: a null/WAV-file replacement for the audio devices, for headless tests. Implies `io`.

With `default-features = false` the crate keeps the Opus codec, jitter buffer, mixer, spatial placement, bitrate control and the processing chain, and has no cpal or ALSA dependency, so headless clients and bots can build it for static musl targets:

```toml
mumcord-audio = { version = "0.4", default-features = false }
```

## Licence

AGPL-3.0-only.
