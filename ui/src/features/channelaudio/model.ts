/**
 * Channel audio profiles (issue #171, FR-3.62–FR-3.64): presets, the badge a profile earns and how
 * a profile layers over the global settings. A `null` field keeps the global setting.
 */
import type { ChannelAudioProfile, Settings } from "../../api";
import { BITRATE_OPUS_MAX_KBPS } from "../settings/sections/BitrateSlider";

export type PresetId = "voice" | "music" | "jam";

/** Bitrate cap of the "Music" preset. */
export const MUSIC_BITRATE_KBPS = 256;

/** Quick presets of the dialog: Voice keeps the defaults, Music and Jam set what they need. */
export const PRESETS: Readonly<Record<PresetId, ChannelAudioProfile>> = {
  voice: { musicQuality: null, lowLatency: null, bitrateKbps: null },
  music: { musicQuality: true, lowLatency: null, bitrateKbps: MUSIC_BITRATE_KBPS },
  jam: { musicQuality: null, lowLatency: true, bitrateKbps: null },
};

export const PRESET_IDS: readonly PresetId[] = ["voice", "music", "jam"];

export function sameProfile(a: ChannelAudioProfile, b: ChannelAudioProfile): boolean {
  return (
    a.musicQuality === b.musicQuality &&
    a.lowLatency === b.lowLatency &&
    a.bitrateKbps === b.bitrateKbps
  );
}

/** The preset `profile` equals, if any. */
export function presetOf(profile: ChannelAudioProfile): PresetId | null {
  return PRESET_IDS.find((id) => sameProfile(PRESETS[id], profile)) ?? null;
}

/** Badge of a channel with a profile: ⚡ for low latency, ♪ for music quality, none otherwise. */
export type ProfileKind = "music" | "jam";

export function profileKind(profile: ChannelAudioProfile | null | undefined): ProfileKind | null {
  if (profile?.lowLatency === true) return "jam";
  if (profile?.musicQuality === true) return "music";
  return null;
}

/** The settings the audio runs with: every value the profile sets wins (channel > global). */
export function applyProfile(settings: Settings, profile: ChannelAudioProfile | null): Settings {
  if (!profile) return settings;
  return {
    ...settings,
    musicQuality: profile.musicQuality ?? settings.musicQuality,
    lowLatency: profile.lowLatency ?? settings.lowLatency,
    bitrateKbps: profile.bitrateKbps ?? settings.bitrateKbps,
  };
}

/** The values of the active profile, in the order the UI lists them. */
export type ProfileField = "musicQuality" | "lowLatency" | "bitrateKbps";

export function overrides(profile: ChannelAudioProfile | null, field: ProfileField): boolean {
  return profile !== null && profile[field] !== null;
}

/**
 * Splits a change made in the voice quality panel: a value the channel profile sets is changed in
 * the profile (that is what is in effect), everything else in the global settings. `profile` in the
 * result is `null` when the profile does not change.
 */
export function routePatch(
  patch: Partial<Settings>,
  profile: ChannelAudioProfile | null,
): { global: Partial<Settings>; profile: ChannelAudioProfile | null } {
  const global: Partial<Settings> = { ...patch };
  if (!profile) return { global, profile: null };
  const changes: Partial<ChannelAudioProfile> = {};
  if (patch.musicQuality !== undefined && profile.musicQuality !== null) {
    changes.musicQuality = patch.musicQuality;
    delete global.musicQuality;
  }
  if (patch.lowLatency !== undefined && profile.lowLatency !== null) {
    changes.lowLatency = patch.lowLatency;
    delete global.lowLatency;
  }
  if (patch.bitrateKbps !== undefined && profile.bitrateKbps !== null) {
    // The slider's "Max" is the highest cap a profile can hold; the server limit still wins.
    changes.bitrateKbps = patch.bitrateKbps ?? BITRATE_OPUS_MAX_KBPS;
    delete global.bitrateKbps;
  }
  return { global, profile: Object.keys(changes).length > 0 ? { ...profile, ...changes } : null };
}

/** How channel info names a channel's profile. */
export type ProfileSummary = "none" | PresetId | "custom";

export function profileSummary(profile: ChannelAudioProfile | null): ProfileSummary {
  if (!profile) return "none";
  return presetOf(profile) ?? "custom";
}
