import { beforeEach, describe, expect, it } from "vitest";
import type { Settings } from "../../api";
import { createMockApi } from "../../api/mock";
import { applyPreset } from "./presets";
import {
  applyStyle,
  changedSettings,
  presetValuesChanged,
  recordedPreset,
  type WizardDraft,
} from "./wizardDraft";

let settings: Settings;

beforeEach(async () => {
  settings = await createMockApi().settingsGet();
});

function draft(overrides: Partial<WizardDraft> = {}): WizardDraft {
  return {
    settings,
    ingameOverlay: false,
    dndFollowSystem: false,
    registerLinkHandler: false,
    ...overrides,
  };
}

describe("changedSettings", () => {
  it("lists only the values that differ, nested records compared by content", () => {
    const after: Settings = {
      ...settings,
      inputDevice: "mic-usb",
      privateMessages: { ...settings.privateMessages },
      sounds: { ...settings.sounds },
    };
    expect(changedSettings(settings, after)).toEqual({ inputDevice: "mic-usb" });
    const pm = {
      ...settings.privateMessages,
      keepRequests: !settings.privateMessages.keepRequests,
    };
    expect(changedSettings(settings, { ...settings, privateMessages: pm })).toEqual({
      privateMessages: pm,
    });
  });

  it("is empty when nothing changed", () => {
    expect(changedSettings(settings, { ...settings })).toEqual({});
  });
});

describe("applyStyle", () => {
  it("sets notify-only updates for Maximum privacy", () => {
    const applied = applyStyle(draft(), "maximumPrivacy", "off");
    expect(applied.settings).toMatchObject({
      setupPreset: "maximumPrivacy",
      p2pMode: "relayOnly",
      updateCheck: "yes",
      updateAutoDownload: false,
    });
  });

  it("goes back to the starting update mode for the other presets", () => {
    const strict = applyStyle(draft(), "maximumPrivacy", "off");
    const balanced = applyStyle(strict, "balanced", "off");
    expect(balanced.settings.updateCheck).not.toBe("yes");
    expect(balanced.settings.setupPreset).toBe("balanced");
  });

  it("keeps the link handler choice", () => {
    expect(applyStyle(draft({ registerLinkHandler: true }), "balanced", "off")).toMatchObject({
      registerLinkHandler: true,
    });
  });
});

describe("recordedPreset", () => {
  it("records a chosen preset as it is", () => {
    expect(recordedPreset("everythingOn", draft())).toBe("everythingOn");
  });

  it("records Custom as the preset its values match, else as custom", () => {
    const balanced = applyPreset(draft({ ingameOverlay: null }), "balanced");
    expect(recordedPreset("custom", balanced)).toBe("balanced");
    const changed = {
      ...balanced,
      settings: { ...balanced.settings, nowPlaying: true },
    };
    expect(recordedPreset("custom", changed)).toBe("custom");
  });
});

describe("presetValuesChanged", () => {
  it("sees a change of a preset value, not of other settings", () => {
    const before = draft();
    expect(
      presetValuesChanged(before, draft({ settings: { ...settings, inputDevice: "x" } })),
    ).toBe(false);
    expect(presetValuesChanged(before, draft({ dndFollowSystem: true }))).toBe(true);
  });
});
