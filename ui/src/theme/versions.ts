/**
 * The dark and light versions of a theme (FR-2.14): filling in a missing version, and keeping a
 * generated version in step with the hand-made one while it was never edited by hand.
 */
import { generateVariant, themeBase, validateTheme } from "./derive";
import {
  MUMCORD_PRESET,
  otherVariant,
  type ThemeColors,
  type ThemeRecord,
  type ThemeVariant,
} from "./themeModel";

export type PartialColors = Partial<ThemeColors>;

export type ThemeVersions = Pick<ThemeRecord, "dark" | "light" | "generated">;

/** Which version a lone colour set is, by its own brightness; unknown colours count as Mumcord's. */
export function variantOf(colors: PartialColors): ThemeVariant {
  return themeBase({ ...MUMCORD_PRESET.dark, ...colors });
}

/** `colors` turned into the `target` version; Mumcord's version in the unlikely case it can't pass. */
export function generateVersion(colors: ThemeColors, target: ThemeVariant): ThemeColors {
  const generated = generateVariant(colors, target);
  return validateTheme(generated).ok ? generated : MUMCORD_PRESET[target];
}

/** Both versions from one colour set (a theme from before themes had two versions). */
export function fromSingleSet(colors: PartialColors): ThemeVersions {
  const variant = variantOf(colors);
  const given: ThemeColors = { ...MUMCORD_PRESET[variant], ...colors };
  const other = otherVariant(variant);
  const generated = generateVersion(given, other);
  return variant === "dark"
    ? { dark: given, light: generated, generated: other }
    : { dark: generated, light: given, generated: other };
}

/**
 * Both versions from what a stored or shared theme carries. With only one set it is read like a
 * single-set theme; missing colours in a set are filled from Mumcord's matching version.
 */
export function completeVersions(
  dark: PartialColors,
  light: PartialColors,
  generated: ThemeVariant | null,
): ThemeVersions {
  const hasDark = Object.keys(dark).length > 0;
  const hasLight = Object.keys(light).length > 0;
  if (hasDark && hasLight) {
    return {
      dark: { ...MUMCORD_PRESET.dark, ...dark },
      light: { ...MUMCORD_PRESET.light, ...light },
      generated,
    };
  }
  return fromSingleSet(hasDark ? dark : light);
}

/**
 * `versions` with `variant` set to `colors`. An edited version counts as hand-made from now on;
 * the other one is re-generated while it is still the generated one.
 */
export function editVersion(
  versions: ThemeVersions,
  variant: ThemeVariant,
  colors: ThemeColors,
): ThemeVersions {
  const other = otherVariant(variant);
  const generated = versions.generated === variant ? null : versions.generated;
  const otherColors = generated === other ? generateVersion(colors, other) : versions[other];
  return variant === "dark"
    ? { dark: colors, light: otherColors, generated }
    : { dark: otherColors, light: colors, generated };
}
