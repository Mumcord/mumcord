import { describe, expect, it } from "vitest";
import {
  cleanThemeName,
  decodeThemeCode,
  encodeThemeCode,
  findThemeCode,
  formatShareMessage,
  isNewerVersion,
  MAX_THEME_CODE_LENGTH,
} from "./shareCodec";
import { themeBase, validateTheme } from "./derive";
import { findPreset, MUMCORD_PRESET } from "./themeModel";

const ocean = findPreset("preset:ocean") ?? MUMCORD_PRESET;
const DARK = MUMCORD_PRESET.dark;
const LIGHT = MUMCORD_PRESET.light;

function rawCode(payload: unknown): string {
  const json = new TextEncoder().encode(JSON.stringify(payload));
  const base64 = btoa(String.fromCharCode(...json));
  return `mct1.${base64.replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "")}`;
}

describe("theme share codec", () => {
  it("round-trips name, both versions and version", () => {
    const code = encodeThemeCode({ ...ocean, name: "Nächtliche Ozeane 🌊" }, "0.4.0");
    expect(code).toMatch(/^mct1\.[A-Za-z0-9_-]+$/);
    expect(decodeThemeCode(code, "0.4.0")).toEqual({
      ok: true,
      theme: {
        name: "Nächtliche Ozeane 🌊",
        dark: ocean.dark,
        light: ocean.light,
        generated: null,
      },
      createdWith: "0.4.0",
      madeWithNewer: false,
    });
  });

  it("carries both colour sets as the spec defines it", () => {
    const payload: unknown = JSON.parse(
      atob(encodeThemeCode(ocean, "0.4.0").slice(5).replace(/-/g, "+").replace(/_/g, "/")),
    );
    expect(payload).toEqual({ v: "0.4.0", n: "Deep Ocean", dark: ocean.dark, light: ocean.light });
  });

  it("formats the share message as the spec defines it", () => {
    const message = formatShareMessage(ocean, "0.4.0");
    expect(message).toBe(`🎨 Mumcord theme "Deep Ocean" – ${encodeThemeCode(ocean, "0.4.0")}`);
    expect(findThemeCode(`look: ${message} nice`)).toBe(encodeThemeCode(ocean, "0.4.0"));
    expect(findThemeCode("no theme here, mct1. nothing")).toBeNull();
  });

  it("flags themes from a newer Mumcord and ignores colours it doesn't know", () => {
    const code = rawCode({
      v: "0.9.0",
      n: "Future",
      dark: { ...ocean.dark, glow: "#ff00ff" },
      light: ocean.light,
      x: 1,
    });
    const result = decodeThemeCode(code, "0.4.0");
    expect(result).toMatchObject({ ok: true, createdWith: "0.9.0", madeWithNewer: true });
    expect(result.ok && result.theme.dark).toEqual(ocean.dark);
  });

  it("reads an old single-set code as the version matching its brightness", () => {
    const dark = decodeThemeCode(rawCode({ v: "0.3.0", n: "Old", colors: ocean.dark }), "0.4.0");
    if (!dark.ok) throw new Error("not decoded");
    expect(dark.theme.dark).toEqual(ocean.dark);
    expect(dark.theme.generated).toBe("light");
    expect(themeBase(dark.theme.light)).toBe("light");
    expect(validateTheme(dark.theme.light).ok).toBe(true);

    const light = decodeThemeCode(rawCode({ v: "0.3.0", n: "Old", c: ocean.light }), "0.4.0");
    if (!light.ok) throw new Error("not decoded");
    expect(light.theme.light).toEqual(ocean.light);
    expect(light.theme.generated).toBe("dark");
    expect(themeBase(light.theme.dark)).toBe("dark");
  });

  it("treats a code with only one of dark and light like a single-set code", () => {
    const result = decodeThemeCode(rawCode({ v: "0.4.0", n: "Half", light: ocean.light }), "0.4.0");
    if (!result.ok) throw new Error("not decoded");
    expect(result.theme.light).toEqual(ocean.light);
    expect(result.theme.generated).toBe("dark");
    expect(validateTheme(result.theme.dark).ok).toBe(true);
  });

  it("fills colours missing in themes from older versions with defaults", () => {
    const dark = decodeThemeCode(
      rawCode({ v: "0.3.0", n: "Old", c: { accent: "#9bd46a" } }),
      "0.4.0",
    );
    expect(dark.ok && dark.theme.dark).toEqual({ ...DARK, accent: "#9bd46a" });
    const light = decodeThemeCode(
      rawCode({ v: "0.3.0", n: "Old", c: { background: "#fafafa" } }),
      "0.4.0",
    );
    expect(light.ok && light.theme.light).toEqual({ ...LIGHT, background: "#fafafa" });
  });

  it.each([
    ["wrong prefix", "mct2.e30"],
    ["no body", "mct1."],
    ["non-base64 characters", "mct1.e30=+/"],
    ["not JSON", `mct1.${btoa("hello").replace(/=+$/, "")}`],
    ["invalid UTF-8", `mct1.${btoa("\xff\xfe").replace(/=+$/, "")}`],
    ["an array", rawCode([1, 2])],
    ["missing colours object", rawCode({ v: "0.4.0", n: "A" })],
    ["a CSS expression as colour", rawCode({ v: "0.4.0", n: "A", c: { accent: "url(x)" } })],
    ["a named colour", rawCode({ v: "0.4.0", n: "A", c: { accent: "red" } })],
    ["a numeric colour", rawCode({ v: "0.4.0", n: "A", c: { accent: 255 } })],
    ["a missing version", rawCode({ n: "A", c: {} })],
    [
      "a CSS expression in the light set",
      rawCode({ v: "0.4.0", n: "A", dark: {}, light: { text: "var(--x)" } }),
    ],
    ["a malformed version", rawCode({ v: "<b>1</b>", n: "A", c: {} })],
    ["an empty name", rawCode({ v: "0.4.0", n: " ‮ ", c: {} })],
    ["an oversized code", `mct1.${"A".repeat(MAX_THEME_CODE_LENGTH)}`],
  ])("rejects %s as invalid", (_label, code) => {
    expect(decodeThemeCode(code, "0.4.0")).toEqual({ ok: false, error: "invalid" });
  });

  it("rejects a theme whose text isn't readable", () => {
    const code = rawCode({ v: "0.4.0", n: "Murky", c: { ...DARK, text: "#2a2060" } });
    expect(decodeThemeCode(code, "0.4.0")).toEqual({ ok: false, error: "unreadable" });
  });

  it("rejects a theme whose light version isn't readable", () => {
    const light = { ...LIGHT, text: LIGHT.background };
    const code = rawCode({ v: "0.4.0", n: "Murky", dark: DARK, light });
    expect(decodeThemeCode(code, "0.4.0")).toEqual({ ok: false, error: "unreadable" });
  });

  it("accepts upper-case hex and stores it lower-case", () => {
    const code = rawCode({ v: "0.4.0", n: "Caps", c: { accent: "#A78BFF" } });
    const result = decodeThemeCode(code, "0.4.0");
    expect(result.ok && result.theme.dark.accent).toBe("#a78bff");
  });
});

describe("cleanThemeName", () => {
  it("strips hidden characters and caps the length", () => {
    expect(cleanThemeName("  Evil‮\u0000Name  ")).toBe("EvilName");
    expect(cleanThemeName("🎨".repeat(100))).toBe("🎨".repeat(64));
  });
});

describe("isNewerVersion", () => {
  it("compares release numbers", () => {
    expect(isNewerVersion("0.10.0", "0.9.9")).toBe(true);
    expect(isNewerVersion("1.0.0", "1.0.0")).toBe(false);
    expect(isNewerVersion("0.3.1", "0.4.0")).toBe(false);
    expect(isNewerVersion("0.5.0-beta.1", "0.4.0")).toBe(true);
    expect(isNewerVersion("garbage", "0.4.0")).toBe(false);
  });
});
