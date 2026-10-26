/**
 * Theme share strings (FR-2.16): `🎨 Mumcord theme "‹name›" – mct1.‹code›`. The code is base64url
 * of UTF-8 JSON `{ v: creator version, n: name, dark: { key colour: "#rrggbb" }, light: { … } }`.
 * Codes from older builds carry one set (`c`, or `colors`) and are read as the version matching
 * its brightness, the other version generated. Codes arrive in chat from anyone, so decoding
 * accepts colour values only and treats the name as plain text.
 */
import { validateTheme } from "./derive";
import { isHexColor, THEME_COLOR_KEYS, type ThemeRecord } from "./themeModel";
import { completeVersions, type PartialColors } from "./versions";

export const THEME_CODE_PREFIX = "mct1.";
/** Two sets of seven colours, a full-length name and a version fit in well under half of this. */
export const MAX_THEME_CODE_LENGTH = 2048;
export const MAX_THEME_NAME_LENGTH = 64;
const MAX_VERSION_LENGTH = 40;

const VERSION = /^(\d+)\.(\d+)\.(\d+)(?:[-+][0-9A-Za-z.-]*)?$/;
const BASE64URL = /^[A-Za-z0-9_-]+$/;
const CODE_IN_TEXT = new RegExp(`mct1\\.[A-Za-z0-9_-]{1,${String(MAX_THEME_CODE_LENGTH)}}`);
// Control and format characters (including bidi overrides) could disguise the name in the UI.
const HIDDEN_CHARACTERS = /[\p{Cc}\p{Cf}]/gu;

/** A theme as a share code carries it; `generated` is the version this client made up. */
export type SharedTheme = Pick<ThemeRecord, "name" | "dark" | "light" | "generated">;

export type ThemeDecodeError = "invalid" | "unreadable";

export type ThemeDecodeResult =
  | {
      ok: true;
      theme: SharedTheme;
      /** Mumcord version that created the theme. */
      createdWith: string;
      /** Created by a newer Mumcord: colours this version doesn't know were ignored. */
      madeWithNewer: boolean;
    }
  | { ok: false; error: ThemeDecodeError };

const graphemes = new Intl.Segmenter(undefined, { granularity: "grapheme" });

/** Strips hidden characters and caps the length in visible characters, not UTF-16 units. */
export function cleanThemeName(name: string): string {
  const visible = name.replace(HIDDEN_CHARACTERS, "").trim();
  return Array.from(graphemes.segment(visible), ({ segment }) => segment)
    .slice(0, MAX_THEME_NAME_LENGTH)
    .join("");
}

function parseVersion(version: string): [number, number, number] | null {
  const match = VERSION.exec(version);
  if (!match) return null;
  return [Number(match[1]), Number(match[2]), Number(match[3])];
}

/** True when `version` is a higher release than `current`; pre-release suffixes are ignored. */
export function isNewerVersion(version: string, current: string): boolean {
  const a = parseVersion(version);
  const b = parseVersion(current);
  if (!a || !b) return false;
  for (let index = 0; index < 3; index++) {
    const difference = (a[index] ?? 0) - (b[index] ?? 0);
    if (difference !== 0) return difference > 0;
  }
  return false;
}

function toBase64Url(text: string): string {
  const binary = Array.from(new TextEncoder().encode(text), (byte) => String.fromCharCode(byte));
  return btoa(binary.join("")).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

function fromBase64Url(code: string): string {
  const binary = atob(code.replace(/-/g, "+").replace(/_/g, "/"));
  const bytes = Uint8Array.from(binary, (char) => char.charCodeAt(0));
  return new TextDecoder("utf-8", { fatal: true }).decode(bytes);
}

export function encodeThemeCode(theme: Omit<SharedTheme, "generated">, version: string): string {
  const payload = {
    v: version,
    n: cleanThemeName(theme.name),
    dark: theme.dark,
    light: theme.light,
  };
  return THEME_CODE_PREFIX + toBase64Url(JSON.stringify(payload));
}

export function formatShareMessage(theme: Omit<SharedTheme, "generated">, version: string): string {
  return `🎨 Mumcord theme "${cleanThemeName(theme.name)}" – ${encodeThemeCode(theme, version)}`;
}

/** The first theme code in a chat message, if any. */
export function findThemeCode(text: string): string | null {
  return CODE_IN_TEXT.exec(text)?.[0] ?? null;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function parsePayload(code: string): unknown {
  if (code.length > MAX_THEME_CODE_LENGTH || !code.startsWith(THEME_CODE_PREFIX)) return null;
  const body = code.slice(THEME_CODE_PREFIX.length);
  if (!BASE64URL.test(body)) return null;
  try {
    return JSON.parse(fromBase64Url(body));
  } catch {
    // Not base64, not UTF-8 or not JSON: all mean the same to the user.
    return null;
  }
}

/** The colours of one set, `null` when any value is not a colour; a missing set is empty. */
function readColors(raw: unknown): PartialColors | null {
  if (raw === undefined) return {};
  if (!isRecord(raw)) return null;
  const colors: PartialColors = {};
  for (const key of THEME_COLOR_KEYS) {
    if (!Object.hasOwn(raw, key)) continue;
    const value = raw[key];
    if (typeof value !== "string" || !isHexColor(value.toLowerCase())) return null;
    colors[key] = value.toLowerCase();
  }
  return colors;
}

export function decodeThemeCode(code: string, currentVersion: string): ThemeDecodeResult {
  const invalid: ThemeDecodeResult = { ok: false, error: "invalid" };
  const payload = parsePayload(code);
  if (!isRecord(payload)) return invalid;
  const { v: version, n: rawName } = payload;
  if (typeof version !== "string" || version.length > MAX_VERSION_LENGTH) return invalid;
  if (!parseVersion(version) || typeof rawName !== "string") return invalid;
  const name = cleanThemeName(rawName);
  if (name === "") return invalid;

  const dark = readColors(payload.dark);
  const light = readColors(payload.light);
  const single = readColors(payload.c ?? payload.colors);
  if (dark === null || light === null || single === null) return invalid;
  const twoSets = Object.keys(dark).length > 0 || Object.keys(light).length > 0;
  if (!twoSets && Object.keys(single).length === 0) return invalid;
  // A code with one set (older builds, or only `dark` / `light`) gets the other version generated.
  const versions = twoSets
    ? completeVersions(dark, light, null)
    : completeVersions(single, {}, null);

  if (!validateTheme(versions.dark).ok || !validateTheme(versions.light).ok) {
    return { ok: false, error: "unreadable" };
  }
  return {
    ok: true,
    theme: { name, ...versions },
    createdWith: version,
    madeWithNewer: isNewerVersion(version, currentVersion),
  };
}
