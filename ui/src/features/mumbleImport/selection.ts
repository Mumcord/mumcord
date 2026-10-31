import type { MumbleImportFound, MumbleImportRequest, Settings } from "../../api";

/** The rows of the import preview, in display order (issue #66). */
export type ImportItem = "favourites" | "passwords" | "identity" | "devices" | "ptt" | "volume";

export const ITEMS: readonly ImportItem[] = [
  "favourites",
  "passwords",
  "identity",
  "devices",
  "ptt",
  "volume",
];

/** What the user picked: rows and the single favourites (`host:port`). */
export interface ImportSelection {
  items: ReadonlySet<ImportItem>;
  favourites: ReadonlySet<string>;
}

export function favouriteKey(f: { host: string; port: number }): string {
  return `${f.host}:${String(f.port)}`;
}

/** Favourites that can be imported (not already in Mumcord). */
export function newFavourites(found: MumbleImportFound) {
  return found.favourites.filter((f) => !f.duplicate);
}

/** Whether a row has anything to import; rows without are hidden or disabled. */
export function available(found: MumbleImportFound, item: ImportItem): boolean {
  switch (item) {
    case "favourites":
      return newFavourites(found).length > 0;
    case "passwords":
      return newFavourites(found).some((f) => f.hasPassword);
    case "identity":
      return found.certificate !== null && !found.certificate.alreadyImported;
    case "devices":
      return found.inputDevice !== null || found.outputDevice !== null;
    case "ptt":
      return found.pttKey !== null;
    case "volume":
      return found.outputVolumePercent !== null || found.talkMode !== null;
  }
}

/** Whether a row is shown at all: when Mumble has the data, even if it cannot be used. */
export function shown(found: MumbleImportFound, item: ImportItem): boolean {
  switch (item) {
    case "favourites":
      return found.favourites.length > 0 || found.favouritesFailed;
    case "passwords":
      return available(found, "passwords");
    case "identity":
      return found.certificate !== null;
    case "devices":
      return available(found, "devices") || found.devicesMissing;
    case "ptt":
      return found.pttKey !== null || found.pttIssue !== null;
    case "volume":
      return available(found, "volume");
  }
}

/**
 * Everything usable is preselected, except volume and voice settings: Mumble's scales differ
 * from Mumcord's, so they are only copied on request.
 */
export function initialSelection(found: MumbleImportFound): ImportSelection {
  return {
    items: new Set(ITEMS.filter((item) => item !== "volume" && available(found, item))),
    favourites: new Set(newFavourites(found).map(favouriteKey)),
  };
}

/** Rows that will do something; "passwords" counts only with favourites that have one. */
export function selectedCount(found: MumbleImportFound, selection: ImportSelection): number {
  return ITEMS.filter((item) => {
    if (!selection.items.has(item) || !available(found, item)) return false;
    if (item === "favourites") return selection.favourites.size > 0;
    if (item === "passwords") return passwordsChosen(found, selection) > 0;
    return true;
  }).length;
}

/** Chosen favourites with a stored password. */
export function passwordsChosen(found: MumbleImportFound, selection: ImportSelection): number {
  if (!selection.items.has("favourites")) return 0;
  return newFavourites(found).filter(
    (f) => f.hasPassword && selection.favourites.has(favouriteKey(f)),
  ).length;
}

/** The engine request: favourites, passwords and the certificate. */
export function importRequest(
  found: MumbleImportFound,
  selection: ImportSelection,
  certificatePassword: string,
): MumbleImportRequest {
  const favourites = selection.items.has("favourites")
    ? newFavourites(found)
        .filter((f) => selection.favourites.has(favouriteKey(f)))
        .map((f) => ({ host: f.host, port: f.port }))
    : [];
  const certificate = selection.items.has("identity") && available(found, "identity");
  return {
    favourites,
    passwords: selection.items.has("passwords"),
    certificate,
    certificatePassword:
      certificate && found.certificate?.needsPassword === true ? certificatePassword : null,
  };
}

/** The settings the chosen rows change; saved by the UI with the normal settings update. */
export function settingsPatch(
  found: MumbleImportFound,
  selection: ImportSelection,
): Partial<Settings> {
  const patch: Partial<Settings> = {};
  const has = (item: ImportItem) => selection.items.has(item) && available(found, item);
  if (has("devices")) {
    if (found.inputDevice) patch.inputDevice = found.inputDevice.id;
    if (found.outputDevice) patch.outputDevice = found.outputDevice.id;
  }
  if (has("ptt") && found.pttKey !== null) {
    patch.pttKey = found.pttKey;
    // Someone who talked with push-to-talk in Mumble expects the key to work right away.
    if (found.talkMode === "pushToTalk") patch.talkMode = "pushToTalk";
  }
  if (has("volume")) {
    if (found.outputVolumePercent !== null) patch.outputVolumePercent = found.outputVolumePercent;
    if (found.talkMode !== null) patch.talkMode = found.talkMode;
  }
  return patch;
}

/** The current values of the fields a patch changes, to restore them on undo. */
export function previousValues(settings: Settings, patch: Partial<Settings>): Partial<Settings> {
  const before: Partial<Settings> = {};
  for (const key of Object.keys(patch) as (keyof Settings)[]) {
    Object.assign(before, { [key]: settings[key] });
  }
  return before;
}

/** "Ctrl+Space" → "Ctrl + Space", as the preview shows it. */
export function spacedKey(key: string): string {
  return key.split("+").join(" + ");
}
