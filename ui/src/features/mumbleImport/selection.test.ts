import { describe, expect, it } from "vitest";
import type { MumbleImportFound, Settings } from "../../api";
import { DEMO_MUMBLE } from "../../api/mock";
import {
  available,
  importRequest,
  initialSelection,
  passwordsChosen,
  previousValues,
  selectedCount,
  settingsPatch,
  shown,
  spacedKey,
} from "./selection";

const found: MumbleImportFound = {
  ...DEMO_MUMBLE,
  favourites: DEMO_MUMBLE.favourites.map((f) => ({ ...f, duplicate: f.label === "Book Club" })),
};

describe("Mumble import selection", () => {
  it("preselects everything usable except volume and voice settings", () => {
    const selection = initialSelection(found);
    expect([...selection.items].sort()).toEqual(
      ["devices", "favourites", "identity", "passwords", "ptt"].sort(),
    );
    expect(selection.favourites.size).toBe(5);
    expect(selectedCount(found, selection)).toBe(5);
    expect(passwordsChosen(found, selection)).toBe(3);
  });

  it("builds the engine request without duplicates and with the certificate password", () => {
    const selection = initialSelection(found);
    const request = importRequest(found, selection, "mumble");
    expect(request.favourites).toHaveLength(5);
    expect(request.favourites).not.toContainEqual({ host: "books.example.com", port: 64738 });
    expect(request).toMatchObject({
      passwords: true,
      certificate: true,
      certificatePassword: "mumble",
    });
    const unlocked: MumbleImportFound = {
      ...found,
      certificate: { name: "Zoe", validUntil: null, needsPassword: false, alreadyImported: false },
    };
    expect(importRequest(unlocked, selection, "typed").certificatePassword).toBeNull();
  });

  it("passwords only count with their favourites", () => {
    const selection = initialSelection(found);
    const noFavourites = {
      ...selection,
      items: new Set([...selection.items].filter((i) => i !== "favourites")),
    };
    expect(passwordsChosen(found, noFavourites)).toBe(0);
    expect(selectedCount(found, noFavourites)).toBe(3);
    expect(importRequest(found, noFavourites, "").favourites).toEqual([]);
  });

  it("turns the chosen rows into a settings patch and remembers the old values", () => {
    const selection = initialSelection(found);
    const patch = settingsPatch(found, selection);
    expect(patch).toEqual({
      inputDevice: "pipewire:usb-headset",
      outputDevice: "pipewire:speakers",
      pttKey: "Ctrl+Space",
      talkMode: "pushToTalk",
    });
    const withVolume = settingsPatch(found, {
      ...selection,
      items: new Set([...selection.items, "volume"]),
    });
    expect(withVolume.outputVolumePercent).toBe(80);
    const settings = {
      inputDevice: null,
      outputDevice: "old",
      pttKey: null,
      talkMode: "voiceActivation",
    } as unknown as Settings;
    expect(previousValues(settings, patch)).toEqual({
      inputDevice: null,
      outputDevice: "old",
      pttKey: null,
      talkMode: "voiceActivation",
    });
  });

  it("hides rows without data and disables rows that cannot be used", () => {
    const sparse: MumbleImportFound = {
      ...found,
      certificate: null,
      inputDevice: null,
      outputDevice: null,
      devicesMissing: true,
      pttKey: null,
      pttIssue: "modifierOnly",
      talkMode: null,
      outputVolumePercent: null,
    };
    expect(shown(sparse, "identity")).toBe(false);
    expect(shown(sparse, "volume")).toBe(false);
    expect(shown(sparse, "devices")).toBe(true);
    expect(available(sparse, "devices")).toBe(false);
    expect(shown(sparse, "ptt")).toBe(true);
    expect(available(sparse, "ptt")).toBe(false);
    expect(settingsPatch(sparse, initialSelection(sparse))).toEqual({});
  });

  it("spaces key combinations", () => {
    expect(spacedKey("Ctrl+Shift+A")).toBe("Ctrl + Shift + A");
  });
});
