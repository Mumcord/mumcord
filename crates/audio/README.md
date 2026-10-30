# mumcord-audio

The voice audio layer of [Mumcord](https://github.com/Mumcord/mumcord), a modern voice chat client for standard Mumble servers. It is meant for clients and bots that need to send and receive Mumble voice.

It adds microphone and speaker I/O (via cpal), resampling and the microphone test to `mumcord-codec`, which provides the Opus encoder and decoder, a jitter buffer, a mixer with positional placement and the voice processing chain (noise suppression, echo cancellation, gate, leveler); this crate re-exports all of it. Network transport is not part of this crate; see `mumcord-protocol` for that.

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

## Crate split

Everything that works without devices lives in [`mumcord-codec`](https://crates.io/crates/mumcord-codec) and is re-exported here, so `mumcord_audio::codec`, `::mixer` and so on keep working. Headless clients and bots that need no microphone or speakers should depend on `mumcord-codec` alone: it has no cpal or ALSA dependency and builds for static musl targets.

```toml
mumcord-codec = "0.4"
```

## Building on Linux

The crate links system libraries, so building needs their development packages and `pkg-config`: `libasound2-dev` (ALSA, for cpal), `libpipewire-0.3-dev` and `libclang-dev` (PipeWire, for cpal's PipeWire host; the bindings are generated with libclang) and `libdbus-1-dev` (D-Bus, for requesting real-time thread priority from rtkit). On Debian and Ubuntu:

```sh
sudo apt install libasound2-dev libpipewire-0.3-dev libclang-dev libdbus-1-dev pkg-config cmake
```

On a PipeWire desktop the device list shows the sound server's sinks and sources (each output and input of a multi-endpoint interface separately, named as the desktop names them, identified by their stable node names), and streams open on the chosen node. Without PipeWire and its PulseAudio protocol server (pipewire-pulse), the ALSA devices are listed as before.

`cmake` is needed to build the bundled Opus sources. Other platforms need no extra packages.

The `test-backend` feature adds a null/WAV-file replacement for the audio devices, for headless tests.

## Licence

AGPL-3.0-only.
