import { describe, expect, it } from "vitest";
import {
  findImported,
  fromStoredTheme,
  resolveShownTheme,
  selectedAppThemeId,
  toStoredTheme,
  type ThemeChoice,
} from "./appTheme";
import { themeBase, validateTheme } from "./derive";
import { findPreset, MUMCORD_PRESET, MUMCORD_PRESET_ID, type ThemeRecord } from "./themeModel";

const ocean = findPreset("preset:ocean") ?? MUMCORD_PRESET;
const custom: ThemeRecord = {
  id: "custom:1",
  name: "Mine",
  dark: { ...MUMCORD_PRESET.dark, accent: "#ff00aa" },
  light: MUMCORD_PRESET.light,
  generated: "light",
};

function choice(patch: Partial<ThemeChoice>): ThemeChoice {
  return {
    appThemeId: MUMCORD_PRESET_ID,
    customThemes: [custom],
    serverThemeId: null,
    preview: null,
    ...patch,
  };
}

const noSets = { colorsDark: {}, colorsLight: {}, lightGenerated: false, darkGenerated: false };

describe("fromStoredTheme", () => {
  it("round-trips both versions and which one is generated", () => {
    expect(fromStoredTheme(toStoredTheme(custom))).toEqual(custom);
    const darkGenerated = { ...custom, generated: "dark" as const };
    expect(fromStoredTheme(toStoredTheme(darkGenerated))).toEqual(darkGenerated);
  });

  it("generates the other version of a theme saved with one set", () => {
    const theme = fromStoredTheme({
      id: "custom:2",
      name: "Old",
      colors: { background: "#FFFFFF", text: "#000000", accent: "red" },
      ...noSets,
    });
    expect(theme.light).toEqual({
      ...MUMCORD_PRESET.light,
      background: "#ffffff",
      text: "#000000",
    });
    expect(theme.generated).toBe("dark");
    expect(themeBase(theme.dark)).toBe("dark");
    expect(validateTheme(theme.dark).ok).toBe(true);
  });

  it("fills missing or broken colours of a set from Mumcord's matching version", () => {
    const theme = fromStoredTheme({
      id: "custom:3",
      name: "Gaps",
      colors: {},
      colorsDark: { accent: "#FF00AA", text: "url(x)" },
      colorsLight: { accent: "#6a40e0" },
      lightGenerated: false,
      darkGenerated: false,
    });
    expect(theme.dark).toEqual({ ...MUMCORD_PRESET.dark, accent: "#ff00aa" });
    expect(theme.light).toEqual(MUMCORD_PRESET.light);
    expect(theme.generated).toBeNull();
  });
});

describe("selectedAppThemeId", () => {
  it("keeps a known theme and maps unknown and former ids to Mumcord", () => {
    expect(selectedAppThemeId("custom:1", [custom])).toBe("custom:1");
    expect(selectedAppThemeId("preset:forest", [])).toBe("preset:forest");
    expect(selectedAppThemeId("preset:light", [])).toBe(MUMCORD_PRESET_ID);
    expect(selectedAppThemeId("preset:dark", [])).toBe(MUMCORD_PRESET_ID);
    expect(selectedAppThemeId("custom:gone", [])).toBe(MUMCORD_PRESET_ID);
  });
});

describe("resolveShownTheme", () => {
  it("shows the server's theme over the app theme in the chosen version", () => {
    expect(resolveShownTheme(choice({ serverThemeId: "preset:ocean" }), "light")).toEqual({
      theme: ocean,
      variant: "light",
    });
    expect(
      resolveShownTheme(choice({ appThemeId: "custom:1", serverThemeId: "gone" }), "dark"),
    ).toEqual({ theme: custom, variant: "dark" });
    expect(resolveShownTheme(choice({ appThemeId: "gone" }), "light")).toEqual({
      theme: MUMCORD_PRESET,
      variant: "light",
    });
  });

  it("lets the editor preview the app theme or a draft in its version over the server's theme", () => {
    const server = { appThemeId: "custom:1", serverThemeId: "preset:ocean" };
    expect(
      resolveShownTheme(choice({ ...server, preview: { kind: "app", variant: "light" } }), "dark"),
    ).toEqual({ theme: custom, variant: "light" });
    const draft = { ...custom, id: "draft" };
    expect(
      resolveShownTheme(
        choice({ ...server, preview: { kind: "draft", theme: draft, variant: "dark" } }),
        "light",
      ),
    ).toEqual({ theme: draft, variant: "dark" });
  });
});

describe("findImported", () => {
  it("matches name and every colour of both versions", () => {
    expect(findImported({ ...custom, generated: null }, [custom])).toBe(custom);
    expect(findImported({ ...custom, name: "Other" }, [custom])).toBeNull();
    expect(
      findImported({ ...custom, light: { ...custom.light, text: "#000000" } }, [custom]),
    ).toBeNull();
  });
});
