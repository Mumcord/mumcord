import type { ThemeRecord as StoredTheme } from "../api";
import type { SharedTheme } from "./shareCodec";
import { applyThemeRecord } from "./theme";
import {
  findPreset,
  isHexColor,
  MUMCORD_PRESET,
  MUMCORD_PRESET_ID,
  THEME_COLOR_KEYS,
  type ThemeColors,
  type ThemeRecord,
  type ThemeVariant,
} from "./themeModel";
import { completeVersions, fromSingleSet, type PartialColors } from "./versions";

const CUSTOM_ID_PREFIX = "custom:";

export function newCustomThemeId(): string {
  return CUSTOM_ID_PREFIX + crypto.randomUUID();
}

function storedColors(raw: Partial<Record<string, string>>): PartialColors {
  const colors: PartialColors = {};
  for (const key of THEME_COLOR_KEYS) {
    const value = raw[key]?.toLowerCase();
    if (value !== undefined && isHexColor(value)) colors[key] = value;
  }
  return colors;
}

/**
 * A stored custom theme as a full theme. The store keeps only `#rrggbb` values and can't make
 * colours, so a theme saved with one set (before themes had two versions) gets its other version
 * generated here, and missing colours are filled from Mumcord's matching version.
 */
export function fromStoredTheme(stored: StoredTheme): ThemeRecord {
  const dark = storedColors(stored.colorsDark);
  const light = storedColors(stored.colorsLight);
  const generated = stored.lightGenerated ? "light" : stored.darkGenerated ? "dark" : null;
  const versions =
    Object.keys(dark).length === 0 && Object.keys(light).length === 0
      ? fromSingleSet(storedColors(stored.colors))
      : completeVersions(dark, light, generated);
  return { id: stored.id, name: stored.name, ...versions };
}

export function toStoredTheme(theme: ThemeRecord): StoredTheme {
  return {
    id: theme.id,
    name: theme.name,
    colors: {},
    colorsDark: theme.dark,
    colorsLight: theme.light,
    lightGenerated: theme.generated === "light",
    darkGenerated: theme.generated === "dark",
  };
}

export function findTheme(id: string, customThemes: readonly ThemeRecord[]): ThemeRecord | null {
  return findPreset(id) ?? customThemes.find((theme) => theme.id === id) ?? null;
}

/** The entry the theme list shows as selected; ids naming no theme mean Mumcord. */
export function selectedAppThemeId(
  appThemeId: string,
  customThemes: readonly ThemeRecord[],
): string {
  return findTheme(appThemeId, customThemes)?.id ?? MUMCORD_PRESET_ID;
}

/** What the window shows: a theme in the version the Dark / Light / System setting picks. */
export interface ShownTheme {
  theme: ThemeRecord;
  variant: ThemeVariant;
}

/**
 * Overrides the shown theme while Settings → Appearance is open (live preview, FR-2.14): the app
 * theme or the unsaved edits, in the version the editor shows.
 */
export type ThemePreview =
  | { kind: "app"; variant: ThemeVariant }
  | { kind: "draft"; theme: ThemeRecord; variant: ThemeVariant };

export interface ThemeChoice {
  appThemeId: string;
  customThemes: readonly ThemeRecord[];
  /** Theme of the shown server (FR-2.17); `null` or unknown shows the app theme. */
  serverThemeId: string | null;
  preview: ThemePreview | null;
}

export function resolveShownTheme(choice: ThemeChoice, variant: ThemeVariant): ShownTheme {
  const { preview } = choice;
  if (preview?.kind === "draft") return { theme: preview.theme, variant: preview.variant };
  const serverTheme =
    preview === null && choice.serverThemeId !== null
      ? findTheme(choice.serverThemeId, choice.customThemes)
      : null;
  const theme = serverTheme ?? findTheme(choice.appThemeId, choice.customThemes) ?? MUMCORD_PRESET;
  return { theme, variant: preview?.variant ?? variant };
}

let lastApplied: string | null = null;

/**
 * Applies the shown theme. Re-applying costs a style pass over the whole window, so an unchanged
 * theme and version is skipped.
 */
export function applyShownTheme(shown: ShownTheme): void {
  const key = JSON.stringify([shown.theme.id, shown.variant, shown.theme[shown.variant]]);
  if (key === lastApplied) return;
  lastApplied = key;
  applyThemeRecord(shown.theme, shown.variant);
}

/** Forgets what was applied, so the next call applies again; for tests. */
export function resetAppliedTheme(): void {
  lastApplied = null;
}

function sameColors(a: ThemeColors, b: ThemeColors): boolean {
  return THEME_COLOR_KEYS.every((key) => a[key] === b[key]);
}

/** A custom theme with the same name and colours, so importing twice adds it only once (FR-2.16). */
export function findImported(
  shared: SharedTheme,
  customThemes: readonly ThemeRecord[],
): ThemeRecord | null {
  return (
    customThemes.find(
      (theme) =>
        theme.name === shared.name &&
        sameColors(theme.dark, shared.dark) &&
        sameColors(theme.light, shared.light),
    ) ?? null
  );
}
