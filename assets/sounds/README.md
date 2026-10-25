# Notification sounds

Short, soft tones for FR-1.27 (join/leave, self mute/deafen, new message, own voice join,
hang up). No speech.

Synthesized from scratch by `generate.py` (plain sine tones with a small second harmonic,
fast attack, exponential decay; stdlib only). No third-party audio is used, so there is
nothing to attribute. Dedicated to the public domain under CC0 1.0.

Format: 48 kHz, mono, 16-bit PCM. Peak about -18 dBFS, all under 400 ms. Embedded into the
engine with `include_bytes!` (`crates/engine/src/sounds.rs`); regenerate with
`python3 generate.py`.

| File                            | Event                                                        |
| ------------------------------- | ------------------------------------------------------------ |
| user_joined.wav / user_left.wav | someone joins / leaves your channel (rising / falling)       |
| mute.wav / unmute.wav           | you mute / unmute                                            |
| deafen.wav / undeafen.wav       | you deafen / undeafen                                        |
| message.wav                     | new message in a followed channel or private chat            |
| mention.wav                     | someone mentions you (`@Name`); replaces message.wav (QA.41) |
| voice_join.wav                  | you join a channel for voice (QA.23)                         |
| hang_up.wav                     | you disconnect from a server (QA.19)                         |
