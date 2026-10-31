import { beforeEach, describe, expect, it } from "vitest";
import type { Settings } from "../../api";
import { createMockApi } from "../../api/mock";
import {
  PRESET_IDS,
  PRESET_KEYS,
  PRESETS,
  RECOMMENDED_PRESET,
  applyPreset,
  matchingPreset,
  presetDifferences,
  presetValueOf,
  type PresetDraft,
  type PresetValues,
} from "./presets";

let settings: Settings;

beforeEach(async () => {
  settings = await createMockApi().settingsGet();
});

function draft(overrides: Partial<PresetDraft> = {}): PresetDraft {
  return { settings, ingameOverlay: false, dndFollowSystem: false, ...overrides };
}

describe("PRESETS", () => {
  it("lists the presets in card order with Balanced recommended", () => {
    expect(PRESET_IDS).toEqual(["everythingOn", "balanced", "maximumPrivacy"]);
    expect(RECOMMENDED_PRESET).toBe("balanced");
  });

  it("defines every value of the issue table", () => {
    const expected: Record<string, PresetValues> = {
      everythingOn: {
        "privateMessages.rule": "everyone",
        "privateMessages.allowCurrentChannel": true,
        "privateMessages.allowAccepted": true,
        "privateMessages.keepRequests": true,
        p2pMode: "directRelay",
        linkPreviews: true,
        offerFullImages: true,
        loadFullImages: true,
        historySharing: true,
        nowPlaying: true,
        nowGaming: true,
        ingameOverlay: true,
        dndFollowSystem: true,
      },
      balanced: {
        "privateMessages.rule": "friendsOnly",
        "privateMessages.allowCurrentChannel": true,
        "privateMessages.allowAccepted": true,
        "privateMessages.keepRequests": true,
        p2pMode: "directRelay",
        linkPreviews: true,
        offerFullImages: true,
        loadFullImages: true,
        historySharing: true,
        nowPlaying: false,
        nowGaming: false,
        ingameOverlay: true,
        dndFollowSystem: true,
      },
      maximumPrivacy: {
        "privateMessages.rule": "friendsOnly",
        "privateMessages.allowCurrentChannel": false,
        "privateMessages.allowAccepted": false,
        "privateMessages.keepRequests": false,
        p2pMode: "relayOnly",
        linkPreviews: false,
        offerFullImages: false,
        loadFullImages: false,
        historySharing: false,
        nowPlaying: false,
        nowGaming: false,
        ingameOverlay: false,
        dndFollowSystem: true,
      },
    };
    expect(PRESETS).toEqual(expected);
    for (const id of PRESET_IDS)
      expect(Object.keys(PRESETS[id]).sort()).toEqual([...PRESET_KEYS].sort());
  });
});

describe("applyPreset", () => {
  it.each(PRESET_IDS)("sets every value of %s and records it", (id) => {
    const applied = applyPreset(draft(), id);
    for (const key of PRESET_KEYS) expect(presetValueOf(applied, key)).toBe(PRESETS[id][key]);
    expect(applied.settings.setupPreset).toBe(id);
    expect(presetDifferences(applied, id)).toEqual([]);
  });

  it("keeps every other setting, per-server PM rules and the input draft", () => {
    const base = draft({
      settings: {
        ...settings,
        autostart: true,
        updateCheck: "yes",
        privateMessages: {
          ...settings.privateMessages,
          serverRules: [{ serverId: 3, rule: "nobody" }],
        },
      },
    });
    const before = structuredClone(base);
    const applied = applyPreset(base, "maximumPrivacy");
    expect(base).toEqual(before);
    expect(applied.settings.autostart).toBe(true);
    expect(applied.settings.updateCheck).toBe("yes");
    expect(applied.settings.theme).toBe(settings.theme);
    expect(applied.settings.privateMessages.serverRules).toEqual([{ serverId: 3, rule: "nobody" }]);
  });

  it("leaves an unsupported in-game overlay alone", () => {
    expect(applyPreset(draft({ ingameOverlay: null }), "everythingOn").ingameOverlay).toBeNull();
  });
});

describe("presetDifferences", () => {
  it("lists the changed keys in display order", () => {
    const balanced = applyPreset(draft(), "balanced");
    const changed: PresetDraft = {
      ...balanced,
      settings: { ...balanced.settings, nowGaming: true, p2pMode: "relayFirst" },
      dndFollowSystem: false,
    };
    expect(presetDifferences(changed, "balanced")).toEqual([
      "p2pMode",
      "nowGaming",
      "dndFollowSystem",
    ]);
  });

  it("counts a nested PM value", () => {
    const balanced = applyPreset(draft(), "balanced");
    const changed: PresetDraft = {
      ...balanced,
      settings: {
        ...balanced.settings,
        privateMessages: { ...balanced.settings.privateMessages, keepRequests: false },
      },
    };
    expect(presetDifferences(changed, "balanced")).toEqual(["privateMessages.keepRequests"]);
  });

  it("ignores the in-game overlay where it is unsupported", () => {
    const privacy = applyPreset(draft({ ingameOverlay: null }), "maximumPrivacy");
    expect(presetDifferences(privacy, "balanced")).not.toContain("ingameOverlay");
    expect(presetDifferences(applyPreset(draft(), "maximumPrivacy"), "balanced")).toContain(
      "ingameOverlay",
    );
  });

  it("lists the differences between two presets", () => {
    expect(presetDifferences(applyPreset(draft(), "everythingOn"), "balanced")).toEqual([
      "privateMessages.rule",
      "nowPlaying",
      "nowGaming",
    ]);
  });
});

describe("matchingPreset", () => {
  it.each(PRESET_IDS)("detects %s", (id) => {
    expect(matchingPreset(applyPreset(draft(), id))).toBe(id);
    expect(matchingPreset(applyPreset(draft({ ingameOverlay: null }), id))).toBe(id);
  });

  it("returns null for customised values", () => {
    const balanced = applyPreset(draft(), "balanced");
    expect(
      matchingPreset({ ...balanced, settings: { ...balanced.settings, linkPreviews: false } }),
    ).toBeNull();
  });

  it("does not depend on the recorded preset", () => {
    const balanced = applyPreset(draft(), "balanced");
    expect(
      matchingPreset({ ...balanced, settings: { ...balanced.settings, setupPreset: "custom" } }),
    ).toBe("balanced");
  });
});
