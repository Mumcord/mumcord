import { describe, expect, it } from "vitest";
import type { Settings } from "../../api";
import {
  applyProfile,
  overrides,
  PRESETS,
  presetOf,
  profileKind,
  profileSummary,
  routePatch,
} from "./model";

const global = { musicQuality: false, lowLatency: false, bitrateKbps: 96 } as Settings;
const music = PRESETS.music;

describe("channel audio model", () => {
  it("lets every value the profile sets win and keeps the rest global", () => {
    expect(applyProfile(global, null)).toBe(global);
    expect(applyProfile(global, music)).toMatchObject({
      musicQuality: true,
      lowLatency: false,
      bitrateKbps: 256,
    });
    expect(
      applyProfile({ ...global, musicQuality: true }, { ...PRESETS.voice, musicQuality: false }),
    ).toMatchObject({ musicQuality: false, bitrateKbps: 96 });
  });

  it("names presets, badges and summaries", () => {
    expect(presetOf(music)).toBe("music");
    expect(presetOf({ ...music, bitrateKbps: 320 })).toBeNull();
    expect(profileKind(PRESETS.jam)).toBe("jam");
    expect(profileKind({ ...music, lowLatency: true })).toBe("jam");
    expect(profileKind(music)).toBe("music");
    expect(profileKind(PRESETS.voice)).toBeNull();
    expect(profileKind(null)).toBeNull();
    expect(profileSummary(null)).toBe("none");
    expect(profileSummary(PRESETS.voice)).toBe("voice");
    expect(profileSummary({ ...music, bitrateKbps: 320 })).toBe("custom");
    expect(overrides(music, "lowLatency")).toBe(false);
    expect(overrides(music, "bitrateKbps")).toBe(true);
    expect(overrides(null, "musicQuality")).toBe(false);
  });

  it("routes panel changes of profile values into the profile", () => {
    expect(routePatch({ musicQuality: true }, null)).toEqual({
      global: { musicQuality: true },
      profile: null,
    });
    expect(routePatch({ musicQuality: false, lowLatency: true }, music)).toEqual({
      global: { lowLatency: true },
      profile: { ...music, musicQuality: false },
    });
    // "Max" on the slider is the highest cap a profile can hold.
    expect(routePatch({ bitrateKbps: null }, music)).toEqual({
      global: {},
      profile: { ...music, bitrateKbps: 510 },
    });
    expect(routePatch({ lowLatency: true }, music).profile).toBeNull();
  });
});
