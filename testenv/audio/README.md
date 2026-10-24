# Test audio clips

Self-generated, no third-party material: both files are CC0 (public domain dedication by the Mumcord authors).

| File | Content |
|---|---|
| `speech.wav` | 6 s speech-like clip: 16 vowel-like syllables (harmonic source through three formant filters, 105-150 Hz pitch, syllabic envelope, short pauses). Not intelligible; meant for level, timing and codec checks. About -23 dBFS RMS. |
| `speech_noisy.wav` | The same clip mixed with a fan (low-pass noise, 118 Hz hum) and about 22 keyboard clicks; for noise suppression and gate checks. |

Format: 48 kHz, mono, 16-bit PCM.

Regenerate (deterministic, seeded): `python3 testenv/audio/gen_clips.py` (needs numpy).
