import { describe, expect, it } from "vitest";
import { createMockApi, type Settings } from "../../api";
import { applyPreset, type PresetDraft } from "../wizard/presets";
import {
  baselineDraft,
  changedSections,
  changedSettings,
  defaultDraft,
  presetSwitchPlan,
  resetSection,
  TRACKED_SETTINGS,
  uniqueLabels,
} from "./changes";
import { SECTION_IDS } from "./sectionIds";

async function defaults(): Promise<Settings> {
  return createMockApi().settingsDefault();
}

function ids(settings: readonly { id: string }[]): string[] {
  return settings.map((setting) => setting.id);
}

describe("tracked settings (#161)", () => {
  it("lists every setting once and only under existing groups", () => {
    const all = ids(TRACKED_SETTINGS);
    expect(new Set(all).size).toBe(all.length);
    for (const setting of TRACKED_SETTINGS) expect(SECTION_IDS).toContain(setting.section);
  });
});

describe("changedSettings", () => {
  it("finds nothing on a fresh install without a preset", async () => {
    const start = defaultDraft(await defaults(), true);
    expect(changedSettings(start, baselineDraft(start, null))).toEqual([]);
  });

  it("compares settings no preset covers with their default", async () => {
    const base = defaultDraft(await defaults(), true);
    const current: PresetDraft = {
      ...base,
      settings: {
        ...base.settings,
        autoGain: !base.settings.autoGain,
        sounds: { ...base.settings.sounds, kicked: !base.settings.sounds.kicked },
      },
    };
    const changed = changedSettings(current, baselineDraft(base, null));
    expect(ids(changed)).toEqual(["autoGain", "sounds.kicked"]);
    expect([...changedSections(changed)]).toEqual(["voiceAudio"]);
  });

  it("compares preset-covered settings with the active preset, not the default", async () => {
    const base = defaultDraft(await defaults(), true);
    const balanced = applyPreset(base, "balanced");
    expect(changedSettings(balanced, baselineDraft(base, "balanced"))).toEqual([]);
    const shared = { ...balanced, settings: { ...balanced.settings, nowPlaying: true } };
    expect(ids(changedSettings(shared, baselineDraft(base, "balanced")))).toEqual(["nowPlaying"]);
  });

  it("compares with the defaults for Custom", async () => {
    const base = defaultDraft(await defaults(), true);
    const custom: PresetDraft = {
      ...base,
      settings: { ...base.settings, setupPreset: "custom", linkPreviews: false },
    };
    expect(ids(changedSettings(custom, baselineDraft(base, "custom")))).toEqual(["linkPreviews"]);
  });

  it("leaves the in-game overlay out where the platform has none", async () => {
    const base = defaultDraft(await defaults(), false);
    expect(base.ingameOverlay).toBeNull();
    const privacy = applyPreset(base, "everythingOn");
    expect(ids(changedSettings(privacy, baselineDraft(base, "everythingOn")))).toEqual([]);
  });
});

describe("resetSection", () => {
  it("resets only the group's changed settings, preset values to the preset", async () => {
    const base = defaultDraft(await defaults(), true);
    const balanced = applyPreset(base, "balanced");
    const current: PresetDraft = {
      ...balanced,
      settings: {
        ...balanced.settings,
        linkPreviews: false,
        previewCacheDays: 1,
        autoGain: !balanced.settings.autoGain,
      },
    };
    const { draft, changes } = resetSection(current, baselineDraft(base, "balanced"), "privacy");
    expect(ids(changes)).toEqual(["linkPreviews", "previewCacheDays"]);
    expect(draft.settings.linkPreviews).toBe(true);
    expect(draft.settings.previewCacheDays).toBe(base.settings.previewCacheDays);
    // Another group's change stays.
    expect(draft.settings.autoGain).toBe(current.settings.autoGain);
    expect(draft.settings.setupPreset).toBe("balanced");
  });

  it("puts overlay and Do Not Disturb back too", async () => {
    const base = defaultDraft(await defaults(), true);
    const current: PresetDraft = { ...base, ingameOverlay: true, dndFollowSystem: false };
    const baseline = baselineDraft(base, null);
    expect(resetSection(current, baseline, "gaming").draft.ingameOverlay).toBe(false);
    expect(resetSection(current, baseline, "chatNotifications").draft.dndFollowSystem).toBe(true);
  });

  it("changes nothing when the group has no changes", async () => {
    const base = defaultDraft(await defaults(), true);
    const { draft, changes } = resetSection(base, baselineDraft(base, null), "appearance");
    expect(changes).toEqual([]);
    expect(draft).toBe(base);
  });
});

describe("presetSwitchPlan", () => {
  it("applies the preset and lists the labels of the settings that change", async () => {
    const base = defaultDraft(await defaults(), true);
    const balanced = applyPreset(base, "balanced");
    const plan = presetSwitchPlan(balanced, "maximumPrivacy");
    expect(plan.draft.settings.setupPreset).toBe("maximumPrivacy");
    expect(plan.draft.settings.p2pMode).toBe("relayOnly");
    expect(plan.draft.ingameOverlay).toBe(false);
    expect(plan.labels).toEqual([
      "privateMessages.currentChannel",
      "privateMessages.accepted",
      "privateMessages.keepRequests",
      "p2p.title",
      "linkPreviews.switch",
      "fullImages.offer",
      "fullImages.load",
      "historySharing.switch",
      "overlay.inGame.mode",
    ]);
  });

  it("keeps every value for Custom and only records it", async () => {
    const base = defaultDraft(await defaults(), true);
    const plan = presetSwitchPlan(base, "custom");
    expect(plan.labels).toEqual([]);
    expect(plan.draft).toEqual({ ...base, settings: { ...base.settings, setupPreset: "custom" } });
  });

  it("lists nothing when the values already match", async () => {
    const base = defaultDraft(await defaults(), true);
    expect(presetSwitchPlan(applyPreset(base, "balanced"), "balanced").labels).toEqual([]);
  });
});

describe("uniqueLabels", () => {
  it("drops repeated labels", () => {
    const sounds = TRACKED_SETTINGS.filter((setting) => setting.id.startsWith("sounds."));
    expect(uniqueLabels([...sounds, ...sounds])).toHaveLength(sounds.length);
  });
});
