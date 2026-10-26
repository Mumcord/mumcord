import { describe, expect, it } from "vitest";
import { applyThemeRecord, clearThemeColors, resolveTheme } from "./theme";
import { findPreset, MUMCORD_PRESET } from "./themeModel";

describe("resolveTheme", () => {
  it("follows the OS for the system setting", () => {
    expect(resolveTheme("system", true)).toBe("dark");
    expect(resolveTheme("system", false)).toBe("light");
  });

  it("falls back to dark when the OS preference is unknown", () => {
    expect(resolveTheme("system", null)).toBe("dark");
  });

  it("lets a manual override win over the OS", () => {
    expect(resolveTheme("light", true)).toBe("light");
    expect(resolveTheme("dark", false)).toBe("dark");
  });
});

describe("applyThemeRecord", () => {
  const root = document.documentElement;

  it("sets the derived colours of the chosen version on its base token set", () => {
    const forest = findPreset("preset:forest") ?? MUMCORD_PRESET;
    applyThemeRecord(forest, "dark");
    expect(root.dataset.theme).toBe("dark");
    expect(root.style.getPropertyValue("--color-bg-chat")).toBe(forest.dark.background);
    expect(root.style.getPropertyValue("--color-text")).toBe(forest.dark.text);

    applyThemeRecord(forest, "light");
    expect(root.dataset.theme).toBe("light");
    expect(root.style.getPropertyValue("--color-bg-chat")).toBe(forest.light.background);
    expect(root.style.getPropertyValue("--color-text")).toBe(forest.light.text);
  });

  it("uses the hand-tuned token sets for both versions of Mumcord", () => {
    applyThemeRecord(MUMCORD_PRESET, "light");
    expect(root.dataset.theme).toBe("light");
    expect(root.style.getPropertyValue("--color-bg-chat")).toBe("");
    applyThemeRecord(MUMCORD_PRESET, "dark");
    expect(root.dataset.theme).toBe("dark");
    expect(root.style.getPropertyValue("--color-bg-chat")).toBe("");
  });

  it("clears custom colours", () => {
    applyThemeRecord({ ...MUMCORD_PRESET, id: "custom-1" }, "dark");
    expect(root.style.getPropertyValue("--color-text")).not.toBe("");
    clearThemeColors();
    expect(root.style.getPropertyValue("--color-text")).toBe("");
  });
});
