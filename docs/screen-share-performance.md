# Screen share performance

Measured cost of screen sharing (FR-2.42 – FR-2.46, AC-2.29) for the person sharing and for the people watching: CPU, memory, bitrate and frame rate at the default 720p30 with the maximum of eight viewers, and what happens when the upload is too slow.

These numbers were measured when 720p30 (≈ 2000 kbps) was the best quality step. The default is now 1080p30 at ≈ 4000 kbps, with 720p30 as the next step down, so a sharer's upload and encoding cost at the top step are roughly twice the 720p30 figures below.

## Method

- **Test:** `crates/engine/tests/live_screen_share_e2e.rs`, test `screen_share_ac_2_29`, built in release mode, against a local Mumble 1.5.735 test server (`testenv/up.sh v15`).

      MUMCORD_LIVE=1 cargo test --release -p mumcord-engine --features test-backend \
        --test live_screen_share_e2e -- --ignored --test-threads=1 --nocapture

- **Processes:** the sharer runs as its own process (the test binary started again as a "sharer bot", driven over stdin/stdout), so its CPU and memory are measured on their own. All ten viewer engines (eight viewers, the refused ninth and one person in another channel) run together in the test process.
- **Picture:** the built-in test pattern (a full-screen gradient with a bar sweeping across, so every frame changes). Real screen capture is not used in this test; see [Limits](#limits).
- **Network:** all engines on one machine, direct connections over loopback (QUIC). A slow upload is simulated in-process with a token bucket on the sharer's whole upload (all viewers together), because network-level throttling needs root.
- **CPU:** user + system time from `/proc/<pid>/stat` over a 10 s window after the stream has settled, given as a percentage of one core (100% = one core fully busy). Per-thread numbers come from `/proc/<pid>/task/*/stat`.
- **Memory:** resident memory (`VmRSS`) and its peak (`VmHWM`) from `/proc/<pid>/status`.
- **Bitrate and frame rate:** the encoder's own numbers for the last second, and per viewer the frames and bytes actually received over the 10 s window.
- **Encoder:** libvpx VP9, software, real-time mode, speed 7, up to 4 threads, error resilient, three temporal layers.
- **Machine:** AMD Ryzen 7 5800X3D (8 cores, 16 threads), 32 GB RAM, Linux 7.3 (CachyOS), rustc 1.98.1. Measured 2026-10-25 on branch `epic-2`; the numbers below are from two consecutive runs.

## Results

### Sharer at 720p30 with eight viewers

|                                   | Run 1                       | Run 2                       |
| --------------------------------- | --------------------------- | --------------------------- |
| CPU (whole sharer process)        | 29% of one core             | 28% of one core             |
| – libvpx worker threads           | 15%                         | 14%                         |
| – encode thread (`screen-encode`) | 11%                         | 10%                         |
| – capture (test pattern)          | 2%                          | 2%                          |
| – network (tokio workers)         | 1%                          | 1%                          |
| Memory (RSS, peak)                | 144 MiB                     | 143 MiB                     |
| Encoder output                    | 1280×720, 31 fps, 1963 kbps | 1280×720, 30 fps, 2011 kbps |
| Encode time per frame             | 1.9 ms                      | 3.7 ms                      |
| Upload for eight viewers          | ≈ 16 Mbit/s                 | ≈ 16 Mbit/s                 |

The video is encoded once; sending it to eight viewers instead of one adds only the network threads (about 1% of a core). Almost all of the cost is the VP9 encoder.

### Viewers

| Situation                                                    | Each viewer receives                                    | Viewer process (10 engines)   |
| ------------------------------------------------------------ | ------------------------------------------------------- | ----------------------------- |
| 720p30, decoding in the webview (WebCodecs, the normal case) | 30.0 fps, ≈ 1990 kbps, 1280×720, first frame a keyframe | 22–23% of one core, 57–60 MiB |
| 720p30, all eight decoding in Rust (libvpx fallback)         | 30.0 fps decoded                                        | 101–105% of one core, 139 MiB |

In the normal case the engine only forwards the encoded frames to the webview, so a viewer's engine costs about 3% of a core. Decoding itself happens in the webview and is not part of this number. With the Rust decoder fallback (for webviews without WebCodecs) a viewer costs about 13% of a core per 720p30 stream on this machine.

### Slow upload (AC-2.29, FR-2.44)

| Whole upload limit                                                              | Reaction                                                                                                  | Encoder                   | Each viewer receives                                                     | Sharer CPU |
| ------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------- | ------------------------- | ------------------------------------------------------------------------ | ---------- |
| none                                                                            | –                                                                                                         | 720p30, ≈ 2000 kbps       | 30 fps                                                                   | 28–29%     |
| 7000 kbit/s (room for 8 × 480p15; run 4, 2026-10-26, after the step-up backoff) | 480p15 after 4.3 s; one try at 720p15 in the next 22 s (the second wait is 10 s), which does not fit      | 854×480, 15 fps           | 12.4–12.6 fps, ≈ 605–615 kbps (measured from 22 s after reaching 480p15) | 12%        |
| 3000 kbit/s (slow home upload; run 3, after 7000)                               | stays at 480p15, viewers lose temporal layers                                                             | 854×480, 15 fps, 676 kbps | 6.2–6.7 fps, ≈ 360 kbps                                                  | 12%        |
| lifted again                                                                    | back to 720p30 after 28.1 s (run 4; 21.6 s and 29.4 s in the two runs before; 16–17 s before the backoff) | 720p30                    | ≥ 20 fps after 4 s                                                       | –          |

The drop applies to all viewers at once (one encoder). Below what eight 480p15 streams need, the picture stays at 480p and viewers get fewer frames per second instead of a broken picture.

### Behaviour checked in the same run

- Eight viewers watch over direct connections; the ninth gets "This share is full" and receives no frames.
- Someone in another channel sees no share and cannot watch ("that person is not in your channel").
- A second share in the same channel is watched at the same time as the first: one viewer received both at 30 fps and 1280×720.
- "Stop watching" ends that viewer's stream within a second and frees the place; the ninth then starts at a keyframe and gets 30 fps.
- Stopping the share removes it for every viewer and no more frames arrive.

## Limits

- **Test pattern, not a real screen.** Real desktops are mostly static (much cheaper to encode); games and video are busier and can cost more. Real capture (Wayland portal, X11, Windows, macOS) adds its own cost, mainly the conversion to I420, which the test pattern replaces with about 2% of a core.
- **Quality probing at 480p15.** After 5 s without lagging viewers the sharer tries the next step up. With 7000 kbit/s for eight viewers, 720p15 (1200 kbps each) does not fit, so it steps down again. A step up that goes down again within 10 s is a failed try, and the wait for that step doubles: 5 s, 10, 20, 40 s at most. It starts from 5 s again once a step up has held for 60 s. A unit test simulates a constant limit: 8 tries in 60 s before, at most 2 now. Worst-case recovery after the limit is lifted is about 45 s (5 s for 480p15 → 720p15, plus up to 40 s for 720p15 → 720p30), measured 21–29 s; before the backoff it was 16–17 s. The measured 12.4–12.6 fps at 7000 kbit/s (earlier 12.7) shows the probing was not the main cause: with the tries over, viewers still get about 12.5 of 15 fps, so the limit itself (the sender's rate limiter) holds back some frames at that bitrate. The encoder itself stays near the 480p15 target of 700 kbps: in `the_floor_stays_near_its_bitrate` (the test pattern after 10 s at 720p30 and 2 s at 720p15) every second at 480p15 is 620–705 kbps, the second right after the step down included; with the earlier 1000 ms rate-control buffer that second was 915 kbps. Until 2026-10-26 the encoder's per-second numbers were also about 7% high (16 frames counted per 1.07 s window, shown as "16 fps").
- **Loopback network.** No packet loss, jitter or real latency; upload limits are simulated in-process. Real home connections will reach the lower steps earlier and recover more slowly.
- **One machine.** All viewers and the sharer share one CPU; the numbers are from a fast 8-core desktop CPU. Older or mobile CPUs will need more of a core for the same work; the encoder uses at most 4 threads.
- **Webview decoding not measured.** The normal viewer path decodes in the webview (WebCodecs, often hardware-accelerated); its cost depends on the platform and is not part of this test.
- **Sound not measured here.** The share was started without sound (Linux shares no sound, FR-2.42); the sound path is covered by `live_screen_share_sound.rs`. Per-app sound on Windows and macOS, the X11 "whole screen only" picker and the "no sound" hint need the manual checklist on those systems.
- **Memory** includes the engine's normal baseline (database, connections, libvpx buffers); it is not the screen share alone.
