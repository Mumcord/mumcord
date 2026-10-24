#!/usr/bin/env python3
"""Synthesizes the Mumcord notification sounds (48 kHz mono 16-bit WAV). CC0.

Run: python3 generate.py   (writes the .wav files next to this script)
"""
import math, struct, wave, os

RATE = 48000
PEAK = 0.12  # about -18 dBFS: subtle by design


def tone(freq, dur, attack=0.006, decay=0.035):
    n = int(RATE * dur)
    out = []
    for i in range(n):
        t = i / RATE
        env = min(1.0, t / attack) * math.exp(-t / decay)
        # slight second harmonic for a soft, round timbre
        s = math.sin(2 * math.pi * freq * t) + 0.25 * math.sin(4 * math.pi * freq * t)
        out.append(s * env)
    # short fade-out so the end is click-free
    fade = int(RATE * 0.008)
    for i in range(fade):
        out[n - 1 - i] *= i / fade
    return out


def seq(parts):
    """parts: list of (freq, dur, start_offset_s)."""
    total = max(o + d for _, d, o in parts)
    buf = [0.0] * int(RATE * total)
    for f, d, o in parts:
        for i, v in enumerate(tone(f, d)):
            buf[int(RATE * o) + i] += v
    m = max(abs(v) for v in buf)
    return [v / m * PEAK for v in buf]


SOUNDS = {
    "user_joined": [(659.3, 0.11, 0.0), (880.0, 0.15, 0.08)],
    "user_left": [(880.0, 0.11, 0.0), (659.3, 0.15, 0.08)],
    "mute": [(523.3, 0.12, 0.0), (392.0, 0.16, 0.07)],
    "unmute": [(392.0, 0.12, 0.0), (523.3, 0.16, 0.07)],
    "deafen": [(440.0, 0.11, 0.0), (330.0, 0.11, 0.07), (247.0, 0.15, 0.14)],
    "undeafen": [(247.0, 0.11, 0.0), (330.0, 0.11, 0.07), (440.0, 0.15, 0.14)],
    "message": [(1046.5, 0.18, 0.0)],
    # you join a channel for voice: rising major arpeggio, brighter than user_joined
    "voice_join": [(523.3, 0.10, 0.0), (659.3, 0.10, 0.07), (784.0, 0.17, 0.14)],
    # you disconnect: a low falling fifth, like putting down a phone
    "hang_up": [(523.3, 0.12, 0.0), (349.2, 0.20, 0.10)],
}

here = os.path.dirname(os.path.abspath(__file__))
for name, parts in SOUNDS.items():
    with wave.open(os.path.join(here, name + ".wav"), "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(RATE)
        w.writeframes(b"".join(struct.pack("<h", round(v * 32767)) for v in seq(parts)))
