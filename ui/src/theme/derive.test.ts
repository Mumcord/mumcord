import { describe, expect, it } from "vitest";
import { contrastRatio } from "./contrast";
import { checkTokens, deriveTokens, mix, themeBase, validateTheme } from "./derive";
import { MUMCORD_PRESET, PRESET_THEMES } from "./themeModel";

const DARK = MUMCORD_PRESET.dark;
const LIGHT = MUMCORD_PRESET.light;

describe("presets", () => {
  it.each(
    PRESET_THEMES.flatMap((preset) => [
      [`${preset.name} dark`, preset.dark, "dark"],
      [`${preset.name} light`, preset.light, "light"],
    ]),
  )("%s passes AA without warnings", (_name, colors, base) => {
    expect(validateTheme(colors)).toEqual({ ok: true, issues: [] });
    expect(checkTokens(deriveTokens(colors))).toEqual([]);
    expect(themeBase(colors)).toBe(base);
  });

  it("have unique preset ids", () => {
    const ids = PRESET_THEMES.map((preset) => preset.id);
    expect(new Set(ids).size).toBe(ids.length);
    expect(ids.every((id) => id.startsWith("preset:"))).toBe(true);
  });
});

describe("themeBase", () => {
  it("is dark when the text is lighter than the background", () => {
    expect(themeBase(DARK)).toBe("dark");
    expect(themeBase(LIGHT)).toBe("light");
  });
});

describe("mix", () => {
  it("interpolates between two colours", () => {
    expect(mix("#000000", "#ffffff", 0)).toBe("#000000");
    expect(mix("#000000", "#ffffff", 1)).toBe("#ffffff");
    expect(mix("#000000", "#ffffff", 0.5)).toBe("#808080");
  });
});

describe("contrast fixes", () => {
  it("adjusts derived shades of a low-contrast accent and warns at the accent", () => {
    const colors = { ...DARK, accent: "#2a2060" };
    const result = validateTheme(colors);
    expect(result.ok).toBe(true);
    expect(result.issues.map((issue) => issue.key)).toEqual(["accent"]);
    const tokens = deriveTokens(colors);
    expect(tokens.accent).not.toBe("#2a2060");
    expect(contrastRatio(tokens["accent-text"], tokens["bg-chat"])).toBeGreaterThanOrEqual(4.5);
    expect(checkTokens(tokens)).toEqual([]);
  });

  it("derives the primary pill from the accent with AA-safe text", () => {
    const tokens = deriveTokens({ ...DARK, accent: "#2a2060" });
    expect(tokens["primary-bg"]).toBe(tokens.accent);
    expect(contrastRatio(tokens["primary-text"], tokens["primary-bg"])).toBeGreaterThanOrEqual(4.5);
    expect(contrastRatio(tokens["primary-bg"], tokens["bg-chat"])).toBeGreaterThanOrEqual(3);
  });

  it("fixes unreadable warning, error and talking colours in light themes", () => {
    const colors = {
      ...LIGHT,
      warning: "#fde68a",
      error: "#fecaca",
      talking: "#bbf7d0",
    };
    const result = validateTheme(colors);
    expect(result.ok).toBe(true);
    expect(result.issues.map((issue) => issue.key).sort()).toEqual(["error", "talking", "warning"]);
    expect(checkTokens(deriveTokens(colors))).toEqual([]);
  });

  it("refuses a theme whose text can't reach AA", () => {
    const result = validateTheme({ ...DARK, text: "#3a3070" });
    expect(result.ok).toBe(false);
    expect(result.issues.map((issue) => issue.key)).toContain("text");
  });

  it("refuses text that is only unreadable on the surface", () => {
    const result = validateTheme({ ...LIGHT, surface: "#3d3566" });
    expect(result.ok).toBe(false);
  });

  it("keeps derived backgrounds readable for a mid-tone background", () => {
    const colors = {
      ...LIGHT,
      background: "#b8b0d0",
      surface: "#c4bdd9",
      text: "#000000",
    };
    expect(validateTheme(colors).ok).toBe(true);
    expect(checkTokens(deriveTokens(colors))).toEqual([]);
  });
});
