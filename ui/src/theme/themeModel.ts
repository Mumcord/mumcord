/** Colour themes (FR-2.14): a theme is a name plus a few key colours; every other shade is derived. */

export const THEME_COLOR_KEYS = [
  "background",
  "surface",
  "text",
  "accent",
  "talking",
  "warning",
  "error",
] as const;

export type ThemeColorKey = (typeof THEME_COLOR_KEYS)[number];

/** Lowercase `#rrggbb` values only, so a theme can never carry anything but colours. */
export type ThemeColors = Record<ThemeColorKey, string>;

export type ThemeVariant = "dark" | "light";

export const THEME_VARIANTS = ["dark", "light"] as const satisfies readonly ThemeVariant[];

export function otherVariant(variant: ThemeVariant): ThemeVariant {
  return variant === "dark" ? "light" : "dark";
}

/**
 * A theme with its dark and its light version (FR-2.14); the Dark / Light / System setting picks
 * which one is shown. Presets use the same shape but are built in and read-only.
 */
export interface ThemeRecord {
  id: string;
  name: string;
  dark: ThemeColors;
  light: ThemeColors;
  /** The version generated from the other one and not edited by hand since; it follows edits. */
  generated: ThemeVariant | null;
}

const HEX_COLOR = /^#[0-9a-f]{6}$/;

export function isHexColor(value: string): boolean {
  return HEX_COLOR.test(value);
}

export const PRESET_ID_PREFIX = "preset:";
export const MUMCORD_PRESET_ID = "preset:mumcord";
/** The former separate Mumcord Dark and Light presets, still named by older settings. */
const LEGACY_MUMCORD_IDS: readonly string[] = ["preset:dark", "preset:light"];

/*
 * Mumcord repeats the key colours of the hand-tuned token sets in tokens.css; applying it uses
 * those token sets directly instead of derived shades.
 */
export const MUMCORD_PRESET: ThemeRecord = {
  id: MUMCORD_PRESET_ID,
  name: "Mumcord",
  dark: {
    background: "#1a1342",
    surface: "#231b55",
    text: "#f6f0e8",
    accent: "#a78bff",
    talking: "#3ee39e",
    warning: "#fbbf24",
    error: "#ff8f8f",
  },
  light: {
    background: "#ffffff",
    surface: "#eee9f8",
    text: "#17113a",
    accent: "#6a40e0",
    talking: "#15803d",
    warning: "#7f4d00",
    error: "#b01c1c",
  },
  generated: null,
};

/* The other version of each curated preset started as the generated one and is shipped fixed. */
export const PRESET_THEMES: readonly ThemeRecord[] = [
  MUMCORD_PRESET,
  {
    id: "preset:ocean",
    name: "Deep Ocean",
    dark: {
      background: "#0b1d2e",
      surface: "#12293f",
      text: "#e6f1fa",
      accent: "#4cc3e6",
      talking: "#4ade80",
      warning: "#facc15",
      error: "#fb8c8c",
    },
    light: {
      background: "#f8fbfd",
      surface: "#e6eff8",
      text: "#12202b",
      accent: "#1990b3",
      talking: "#1c9b4b",
      warning: "#7e6503",
      error: "#d20707",
    },
    generated: null,
  },
  {
    id: "preset:forest",
    name: "Forest",
    dark: {
      background: "#132018",
      surface: "#1c2d22",
      text: "#eef3e6",
      accent: "#9bd46a",
      talking: "#5eead4",
      warning: "#fcd34d",
      error: "#fca5a5",
    },
    light: {
      background: "#fafcfb",
      surface: "#edf4ef",
      text: "#202518",
      accent: "#5e982c",
      talking: "#149b86",
      warning: "#856602",
      error: "#d80707",
    },
    generated: null,
  },
  {
    id: "preset:paper",
    name: "Paper",
    dark: {
      background: "#221a09",
      surface: "#352a14",
      text: "#f0edea",
      accent: "#c55822",
      talking: "#218a42",
      warning: "#d68300",
      error: "#e7726c",
    },
    light: {
      background: "#fbf7ef",
      surface: "#f1e9da",
      text: "#2b2118",
      accent: "#b4501f",
      talking: "#1d7a3a",
      warning: "#7a4b00",
      error: "#a8231c",
    },
    generated: null,
  },
];

export function isPresetId(id: string): boolean {
  return id.startsWith(PRESET_ID_PREFIX);
}

export function findPreset(id: string): ThemeRecord | undefined {
  if (LEGACY_MUMCORD_IDS.includes(id)) return MUMCORD_PRESET;
  return PRESET_THEMES.find((preset) => preset.id === id);
}
