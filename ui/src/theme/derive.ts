/**
 * Turns a theme's key colours into the full `--color-*` token set (FR-2.14) and keeps it at WCAG AA
 * (FR-2.15): derived shades are nudged until they pass, key colours that fail are reported.
 */
import { contrastRatio, relativeLuminance } from "./contrast";
import type { ThemeColorKey, ThemeColors, ThemeVariant } from "./themeModel";

export type ThemeBase = ThemeVariant;

/** The `--color-*` tokens a theme sets; names match tokens.css without the `--color-` prefix. */
export const THEME_TOKENS = [
  "bg-servers",
  "bg-sidebar",
  "bg-chat",
  "bg-elevated",
  "bg-hover",
  "bg-selected",
  "text",
  "text-secondary",
  "text-muted",
  "accent",
  "on-accent",
  "accent-text",
  "success",
  "warning",
  "danger",
  "local-muted",
  "talking",
  "accent-stop-1",
  "accent-stop-2",
  "accent-stop-3",
  "primary-bg",
  "primary-text",
  "border",
  "border-strong",
  "focus",
] as const;

export type ThemeToken = (typeof THEME_TOKENS)[number];
export type ThemeTokens = Record<ThemeToken, string>;

// The same pairs as contrast.test.ts checks for the built-in token sets.
const BACKGROUNDS = [
  "bg-servers",
  "bg-sidebar",
  "bg-chat",
  "bg-elevated",
  "bg-hover",
  "bg-selected",
] as const satisfies readonly ThemeToken[];
const TEXT = [
  "text",
  "text-secondary",
  "text-muted",
  "accent-text",
  "success",
  "warning",
  "danger",
  "local-muted",
] as const satisfies readonly ThemeToken[];
const NON_TEXT = [
  "focus",
  "border-strong",
  "accent",
  "primary-bg",
  "talking",
] as const satisfies readonly ThemeToken[];
const GRADIENT_STOPS = ["accent-stop-1", "accent-stop-2", "accent-stop-3"] as const;

export const TEXT_MIN_RATIO = 4.5;
export const UI_MIN_RATIO = 3;

function toRgb(hex: string): [number, number, number] {
  const channel = (start: number) => parseInt(hex.slice(start, start + 2), 16);
  return [channel(1), channel(3), channel(5)];
}

/** Linear sRGB mix: `amount` 0 keeps `from`, 1 gives `to`. */
export function mix(from: string, to: string, amount: number): string {
  const a = toRgb(from);
  const b = toRgb(to);
  return `#${a
    .map((value, index) =>
      Math.round(value + ((b[index] ?? value) - value) * amount)
        .toString(16)
        .padStart(2, "0"),
    )
    .join("")}`;
}

function minContrast(color: string, against: readonly string[]): number {
  return Math.min(...against.map((other) => contrastRatio(color, other)));
}

/**
 * Moves `color` towards `target` in small steps until it reaches `min` against every colour in
 * `against`. `target` is always a colour already known to pass, so the walk ends in a passing value.
 */
function ensureContrast(
  color: string,
  target: string,
  against: readonly string[],
  min: number,
): string {
  for (let step = 0; step <= 20; step++) {
    const candidate = mix(color, target, step / 20);
    if (minContrast(candidate, against) >= min) return candidate;
  }
  return target;
}

/** Text lighter than the background means a dark theme; it picks the matching base token set. */
export function themeBase(colors: ThemeColors): ThemeBase {
  return relativeLuminance(colors.text) > relativeLuminance(colors.background) ? "dark" : "light";
}

export function deriveTokens(colors: ThemeColors): ThemeTokens {
  const dark = themeBase(colors) === "dark";
  const { background, surface, text } = colors;

  // Backgrounds only move back towards the main background, where the text is known to pass.
  const backdropShade = (shade: string) =>
    ensureContrast(shade, background, [text], TEXT_MIN_RATIO);
  const bg = {
    "bg-servers": backdropShade(mix(background, dark ? "#000000" : text, dark ? 0.45 : 0.1)),
    "bg-sidebar": backdropShade(mix(background, dark ? "#000000" : surface, dark ? 0.2 : 0.5)),
    "bg-chat": background,
    "bg-elevated": surface,
    "bg-hover": backdropShade(mix(surface, text, 0.08)),
    "bg-selected": backdropShade(mix(surface, colors.accent, 0.15)),
  };
  const backgrounds = Object.values(bg);
  const readable = (color: string) => ensureContrast(color, text, backgrounds, TEXT_MIN_RATIO);
  const visible = (color: string) => ensureContrast(color, text, backgrounds, UI_MIN_RATIO);

  const accent = visible(colors.accent);
  const onAccent =
    contrastRatio("#ffffff", accent) >= contrastRatio("#000000", accent) ? "#ffffff" : "#000000";
  // Gradient stops move away from the on-accent colour, ending at the opposite extreme (21:1).
  const opposite = onAccent === "#ffffff" ? "#000000" : "#ffffff";
  const stop = (color: string) => ensureContrast(color, opposite, [onAccent], TEXT_MIN_RATIO);
  const danger = readable(colors.error);

  return {
    ...bg,
    text,
    "text-secondary": readable(mix(text, background, 0.2)),
    "text-muted": readable(mix(text, background, 0.33)),
    accent,
    "on-accent": onAccent,
    "accent-text": readable(colors.accent),
    success: readable(colors.talking),
    warning: readable(colors.warning),
    danger,
    "local-muted": danger,
    talking: visible(colors.talking),
    "accent-stop-1": stop(mix(accent, colors.warning, 0.5)),
    "accent-stop-2": stop(mix(accent, colors.error, 0.5)),
    "accent-stop-3": stop(accent),
    // The primary pill is the accent itself, so it inherits the accent/on-accent guarantees.
    "primary-bg": accent,
    "primary-text": onAccent,
    border: mix(surface, text, 0.12),
    "border-strong": visible(mix(surface, text, 0.45)),
    focus: visible(mix(colors.accent, text, 0.25)),
  };
}

export interface ContrastFailure {
  foreground: ThemeToken;
  background: ThemeToken;
  ratio: number;
  min: number;
}

/** Checks a token set against the same pairs as the built-in themes' contrast test. */
export function checkTokens(tokens: ThemeTokens): ContrastFailure[] {
  const pairs: [ThemeToken, ThemeToken, number][] = [
    ...BACKGROUNDS.flatMap((bg) =>
      TEXT.map((fg): [ThemeToken, ThemeToken, number] => [fg, bg, TEXT_MIN_RATIO]),
    ),
    ...BACKGROUNDS.flatMap((bg) =>
      NON_TEXT.map((fg): [ThemeToken, ThemeToken, number] => [fg, bg, UI_MIN_RATIO]),
    ),
    ["on-accent", "accent", TEXT_MIN_RATIO],
    ["primary-text", "primary-bg", TEXT_MIN_RATIO],
    ...GRADIENT_STOPS.map((stop): [ThemeToken, ThemeToken, number] => [
      "on-accent",
      stop,
      TEXT_MIN_RATIO,
    ]),
  ];
  return pairs.flatMap(([foreground, background, min]) => {
    const ratio = contrastRatio(tokens[foreground], tokens[background]);
    return ratio < min ? [{ foreground, background, ratio, min }] : [];
  });
}

/** A key colour that misses AA on the background or surface; shown as a warning at that colour. */
export interface ThemeIssue {
  key: ThemeColorKey;
  ratio: number;
  min: number;
}

export interface ThemeValidation {
  /** False when the text itself isn't readable: such a theme can't be saved or imported. */
  ok: boolean;
  issues: ThemeIssue[];
}

// Accent and talking are drawn as UI colours (their text shades are derived); warning and error
// are also used as text.
const KEY_MIN_RATIOS: readonly [ThemeColorKey, number][] = [
  ["text", TEXT_MIN_RATIO],
  ["accent", UI_MIN_RATIO],
  ["talking", UI_MIN_RATIO],
  ["warning", TEXT_MIN_RATIO],
  ["error", TEXT_MIN_RATIO],
];

export function validateTheme(colors: ThemeColors): ThemeValidation {
  const issues = KEY_MIN_RATIOS.flatMap(([key, min]) => {
    const ratio = minContrast(colors[key], [colors.background, colors.surface]);
    return ratio < min ? [{ key, ratio, min }] : [];
  });
  const textIssue = issues.some((issue) => issue.key === "text");
  return { ok: !textIssue && checkTokens(deriveTokens(colors)).length === 0, issues };
}

type Hsl = [hue: number, saturation: number, lightness: number];

function toHsl(hex: string): Hsl {
  const [r, g, b] = toRgb(hex).map((value) => value / 255) as [number, number, number];
  const max = Math.max(r, g, b);
  const min = Math.min(r, g, b);
  const lightness = (max + min) / 2;
  const delta = max - min;
  if (delta === 0) return [0, 0, lightness];
  const saturation = delta / (1 - Math.abs(2 * lightness - 1));
  const sector =
    max === r ? ((g - b) / delta + 6) % 6 : max === g ? (b - r) / delta + 2 : (r - g) / delta + 4;
  return [sector * 60, saturation, lightness];
}

function fromHsl([hue, saturation, lightness]: Hsl): string {
  const chroma = (1 - Math.abs(2 * lightness - 1)) * saturation;
  const x = chroma * (1 - Math.abs(((hue / 60) % 2) - 1));
  const m = lightness - chroma / 2;
  const sector = Math.floor(hue / 60) % 6;
  const [r, g, b] = [
    [chroma, x, 0],
    [x, chroma, 0],
    [0, chroma, x],
    [0, x, chroma],
    [x, 0, chroma],
    [chroma, 0, x],
  ][sector] ?? [0, 0, 0];
  return `#${[r, g, b]
    .map((value) =>
      Math.round(((value ?? 0) + m) * 255)
        .toString(16)
        .padStart(2, "0"),
    )
    .join("")}`;
}

const clamp = (value: number) => Math.min(1, Math.max(0, value));

/**
 * Walks the lightness of `color` towards black (light themes) or white (dark themes) until it
 * reaches `min` against every colour in `against`. Black and white are the strongest contrast a
 * background allows, so a reachable minimum is always found.
 */
function walkLightness(
  color: string,
  target: ThemeVariant,
  against: readonly string[],
  min: number,
) {
  const [hue, saturation, lightness] = toHsl(color);
  const direction = target === "light" ? -1 : 1;
  for (let step = 0; step <= 50; step++) {
    const candidate = fromHsl([hue, saturation, clamp(lightness + (direction * step) / 50)]);
    if (minContrast(candidate, against) >= min) return candidate;
  }
  return target === "light" ? "#000000" : "#ffffff";
}

/**
 * The other version of a theme (FR-2.14): backgrounds and text swap lightness while hue and
 * saturation stay, so the theme keeps its character; key colours keep their hue and move just far
 * enough in lightness to meet the same contrast rules as a hand-made theme (FR-2.15).
 */
export function generateVariant(colors: ThemeColors, target: ThemeVariant): ThemeColors {
  const [bgHue, bgSaturation, bgLightness] = toHsl(colors.background);
  const [surfaceHue, surfaceSaturation, surfaceLightness] = toHsl(colors.surface);
  // Light backgrounds sit near white, dark ones well below mid grey, whatever the source was; the
  // surface keeps at least a visible step away from the background.
  const backgroundLightness =
    target === "light"
      ? 1 - Math.min(bgLightness, 0.5) * 0.15
      : 0.06 + (1 - Math.max(bgLightness, 0.5)) * 0.6;
  const step = Math.max(0.04, Math.abs(surfaceLightness - bgLightness));
  const background = fromHsl([bgHue, bgSaturation, backgroundLightness]);
  const surface = fromHsl([
    surfaceHue,
    surfaceSaturation,
    clamp(backgroundLightness + (target === "light" ? -step : step)),
  ]);
  const backdrops = [background, surface];
  const [textHue, textSaturation] = toHsl(colors.text);
  // Extra headroom over AA so the derived sidebar and hover shades keep the text readable too.
  const text = walkLightness(
    fromHsl([textHue, textSaturation * 0.6, target === "light" ? 0.12 : 0.93]),
    target,
    backdrops,
    TEXT_MIN_RATIO + 2,
  );
  // Key colours start from themselves and only move as far as the new background needs.
  const keyColor = (color: string, min: number) => walkLightness(color, target, backdrops, min);
  return {
    background,
    surface,
    text,
    accent: keyColor(colors.accent, UI_MIN_RATIO),
    talking: keyColor(colors.talking, UI_MIN_RATIO),
    warning: keyColor(colors.warning, TEXT_MIN_RATIO),
    error: keyColor(colors.error, TEXT_MIN_RATIO),
  };
}
