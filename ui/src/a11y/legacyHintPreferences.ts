import { getApi, type Settings } from "../api";
import { HINT_KEYS, type HintPreferences } from "./hintPreferences";

/** Where the switches lived before they moved into the settings record. */
const LEGACY_KEY = "mumcord.accessibilityHints";

function readLegacy(storage: Storage): Partial<HintPreferences> | null {
  const raw = storage.getItem(LEGACY_KEY);
  if (raw === null) return null;
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    return {};
  }
  if (typeof parsed !== "object" || parsed === null) return {};
  const record = parsed as Partial<Record<keyof HintPreferences, unknown>>;
  const found: Partial<HintPreferences> = {};
  for (const key of HINT_KEYS) {
    const value = record[key];
    if (typeof value === "boolean") found[key] = value;
  }
  return found;
}

/**
 * One-time migration: copies switches an older version kept in the webview's storage into the
 * settings record, then removes the storage entry. Returns the record to use; if saving fails the
 * entry stays and the next start tries again.
 */
export async function migrateLegacyHintPreferences(
  settings: Settings,
  storage?: Storage,
): Promise<Settings> {
  try {
    const store = storage ?? localStorage;
    const legacy = readLegacy(store);
    if (legacy === null) return settings;
    const changed = HINT_KEYS.some(
      (key) => legacy[key] !== undefined && legacy[key] !== settings[key],
    );
    const saved = changed ? await getApi().settingsUpdate({ ...settings, ...legacy }) : settings;
    store.removeItem(LEGACY_KEY);
    return saved;
  } catch (error) {
    console.warn("Moving the accessibility switches into the settings failed", error);
    return settings;
  }
}
