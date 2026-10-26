import { describe, expect, it } from "vitest";
import { themeBase, validateTheme } from "./derive";
import { MUMCORD_PRESET, PRESET_THEMES, type ThemeVariant } from "./themeModel";
import { editVersion, fromSingleSet, generateVersion, variantOf } from "./versions";

const VARIANTS: readonly [ThemeVariant, ThemeVariant][] = [
  ["dark", "light"],
  ["light", "dark"],
];

describe("generateVersion (FR-2.14, FR-2.15)", () => {
  it.each(
    PRESET_THEMES.flatMap((preset) =>
      VARIANTS.map(([from, to]) => [`${preset.name} ${from} → ${to}`, preset[from], to] as const),
    ),
  )("%s passes AA and has the right brightness", (_name, colors, target) => {
    const generated = generateVersion(colors, target);
    expect(themeBase(generated)).toBe(target);
    expect(validateTheme(generated)).toEqual({ ok: true, issues: [] });
  });

  it("keeps the hue of the accent", () => {
    const light = generateVersion({ ...MUMCORD_PRESET.dark, accent: "#ff3030" }, "light");
    const [r, g, b] = [1, 3, 5].map((index) => parseInt(light.accent.slice(index, index + 2), 16));
    expect(r).toBeGreaterThan(g ?? 0);
    expect(r).toBeGreaterThan(b ?? 0);
  });

  it("makes a passing version even from extreme colours", () => {
    const harsh = {
      background: "#000000",
      surface: "#000000",
      text: "#ffffff",
      accent: "#000080",
      talking: "#003300",
      warning: "#330000",
      error: "#110000",
    };
    expect(validateTheme(generateVersion(harsh, "light")).ok).toBe(true);
  });
});

describe("fromSingleSet", () => {
  it("keeps the given set as the version matching its brightness", () => {
    expect(variantOf(MUMCORD_PRESET.light)).toBe("light");
    const versions = fromSingleSet(MUMCORD_PRESET.light);
    expect(versions.light).toEqual(MUMCORD_PRESET.light);
    expect(versions.generated).toBe("dark");
    expect(themeBase(versions.dark)).toBe("dark");
  });
});

describe("editVersion", () => {
  const start = fromSingleSet(MUMCORD_PRESET.dark);

  it("re-generates the other version while it was never edited by hand", () => {
    const dark = { ...start.dark, accent: "#ff3030" };
    const edited = editVersion(start, "dark", dark);
    expect(edited.dark).toEqual(dark);
    expect(edited.light).toEqual(generateVersion(dark, "light"));
    expect(edited.light).not.toEqual(start.light);
    expect(edited.generated).toBe("light");
  });

  it("keeps a hand-edited version when the other one changes", () => {
    const light = { ...start.light, accent: "#123456" };
    const handMade = editVersion(start, "light", light);
    expect(handMade.generated).toBeNull();
    const edited = editVersion(handMade, "dark", { ...start.dark, accent: "#ff3030" });
    expect(edited.light).toEqual(light);
    expect(edited.generated).toBeNull();
  });
});
