// @vitest-environment node
import { describe, expect, it } from "vitest";
import { dominantFrequencies } from "./pwapps.ts";
import { RATE } from "./voice-audio.ts";

const TONES = [440, 550, 660, 880, 1000, 1320];

function sine(hz: number, amplitude = 0.5, phaseJump = false, length = RATE): Float32Array {
  return Float32Array.from({ length }, (_, i) => {
    const phase = phaseJump && i >= length / 2 ? Math.PI : 0;
    return amplitude * Math.sin((2 * Math.PI * hz * i) / RATE + phase);
  });
}

describe("share-sound tone detection", () => {
  it("still hears a tone when its phase changes halfway through the recording", () => {
    expect(dominantFrequencies(sine(440, 0.5, true), TONES)).toEqual([440]);
  });

  it("still hears a tone with a small playback-clock frequency offset", () => {
    expect(dominantFrequencies(sine(441), TONES)).toEqual([440]);
  });

  it("distinguishes simultaneous tones from the other apps and Mumcord's own sounds", () => {
    const mixed = sine(440);
    const second = sine(660);
    for (let i = 0; i < mixed.length; i++) mixed[i] += second[i];
    expect(dominantFrequencies(mixed, TONES)).toEqual([440, 660]);
  });

  it("does not mistake silence, an empty recording or a quiet tone for audible sound", () => {
    expect(dominantFrequencies(new Float32Array(RATE), TONES)).toEqual([]);
    expect(dominantFrequencies(new Float32Array(), TONES)).toEqual([]);
    expect(dominantFrequencies(sine(440, 0.01), TONES)).toEqual([]);
  });

  it("uses the requested power threshold", () => {
    expect(dominantFrequencies(sine(440, 0.1), TONES, 0.2 ** 2)).toEqual([]);
  });

  it("handles a partial measurement window", () => {
    expect(dominantFrequencies(sine(550, 0.5, false, RATE / 4), TONES)).toEqual([550]);
  });
});
