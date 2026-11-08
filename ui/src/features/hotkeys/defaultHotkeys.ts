import { useMemo } from "react";
import type { Hotkey, HotkeyAction, InAppShortcut } from "../../api";
import { useIsMacos } from "../../api/useIsMacos";
import { useHotkeySettings } from "./hotkeyStore";

/**
 * Default shortcuts of the hotkey rows (issue #584). They are never stored and never registered
 * with the engine: they work while the Mumcord window has the focus (see `GlobalHotkeys`), so no
 * system or game shortcut is taken over unnoticed. A row shows its default below the key field,
 * and "Reset all hotkeys" brings them back by forgetting the user's own keys.
 */

/** The camera hotkey is its own setting (FR-8.20), not an entry of the hotkeys list. */
export const CAMERA_ACTION = "camera";

/**
 * A row of Settings → Hotkeys: a fixed hotkey action, the camera (its own setting, FR-8.20), or
 * push-to-talk (its own setting too, FR-3.72).
 */
export type DefaultableAction =
  Exclude<HotkeyAction["kind"], "switchChannel"> | "pushToTalk" | "camera";

/**
 * One default per action, in the engine's canonical key form (see `normalize_key`). Ctrl reads as
 * Cmd on macOS. Push-to-talk, join / leave voice, switch voice server, back to the previous
 * channel, open Mumcord and their own "switch to channel" rows stay without a default.
 *
 * Toggle Do Not Disturb keeps Ctrl+Shift+N from the issue although the notification centre (#143)
 * has that key: the row then says why it is not active instead of binding it twice.
 */
const DEFAULT_KEYS: Partial<Record<DefaultableAction, string>> = {
  toggleMute: "Ctrl+Shift+M",
  toggleDeafen: "Ctrl+Shift+D",
  toggleDnd: "Ctrl+Shift+N",
  camera: "Ctrl+Shift+C",
  toggleMemberColumn: "Ctrl+Shift+U",
  openQuickSettings: "Ctrl+Comma",
};

/**
 * Mumcord's own window shortcuts, as the engine refuses them for global hotkeys; a default on one
 * of them would swallow it. Both spellings, since the default itself differs by platform.
 */
export const IN_APP_SHORTCUTS: readonly { key: string; shortcut: InAppShortcut }[] = [
  { key: "Ctrl+Shift+N", shortcut: "notificationCentre" },
  { key: "Super+Shift+N", shortcut: "notificationCentre" },
  { key: "Ctrl+K", shortcut: "search" },
  { key: "Super+K", shortcut: "search" },
  { key: "Ctrl+F", shortcut: "search" },
  { key: "Super+F", shortcut: "search" },
  { key: "Ctrl+W", shortcut: "closeWindow" },
  { key: "Super+W", shortcut: "closeWindow" },
];

/** Why a default stays off: what already uses its key. */
export type DefaultBlock =
  | { kind: "inApp"; shortcut: InAppShortcut }
  | { kind: "pushToTalk" }
  | { kind: "camera" }
  | { kind: "other"; action: HotkeyAction };

/** A default that is not active, with its key and what takes it. */
export interface BlockedDefault {
  key: string;
  reason: DefaultBlock;
}

/** Which default keys are on, and which are taken by something else. */
export interface Defaults {
  /** Active defaults by action. */
  active: ReadonlyMap<DefaultableAction, string>;
  /** Defaults whose key another thing already uses, by action. */
  blocked: ReadonlyMap<DefaultableAction, BlockedDefault>;
}

/** The action's default key, or `null` when it has none. */
export function defaultKey(action: DefaultableAction, macos: boolean): string | null {
  const key = DEFAULT_KEYS[action];
  if (key === undefined) return null;
  return macos ? key.replace(/^Ctrl\+/, "Super+") : key;
}

/**
 * The key the user set for `action`, or `null`. The camera and push-to-talk keep their key in
 * their own settings, so only the camera is answered here; the caller passes push-to-talk's own.
 */
export function ownKeyFor(
  action: DefaultableAction,
  hotkeys: readonly Hotkey[],
  cameraHotkey: string | null,
): string | null {
  if (action === CAMERA_ACTION) return cameraHotkey;
  return hotkeys.find((h) => h.action.kind === action)?.key ?? null;
}

/**
 * Resolves the defaults of the current settings: an action whose key the user set keeps that key,
 * and a default whose key is already used (Mumcord's own shortcuts, push-to-talk, the camera, or
 * another key the user set) stays off, so nothing is bound twice.
 */
export function resolveDefaults({
  macos,
  hotkeys,
  pttKey,
  cameraHotkey,
}: {
  macos: boolean;
  hotkeys: readonly Hotkey[];
  pttKey: string | null;
  cameraHotkey: string | null;
}): Defaults {
  const taken = new Map<string, DefaultBlock>();
  for (const { key, shortcut } of IN_APP_SHORTCUTS) {
    taken.set(key, { kind: "inApp", shortcut });
  }
  if (pttKey !== null) taken.set(pttKey, { kind: "pushToTalk" });
  if (cameraHotkey !== null) taken.set(cameraHotkey, { kind: "camera" });
  for (const hotkey of hotkeys) {
    if (!taken.has(hotkey.key)) taken.set(hotkey.key, { kind: "other", action: hotkey.action });
  }

  const active = new Map<DefaultableAction, string>();
  const blocked = new Map<DefaultableAction, BlockedDefault>();
  for (const action of Object.keys(DEFAULT_KEYS) as DefaultableAction[]) {
    if (ownKeyFor(action, hotkeys, cameraHotkey) !== null) continue;
    const key = defaultKey(action, macos);
    if (key === null) continue;
    const reason = taken.get(key);
    if (reason === undefined) active.set(action, key);
    else blocked.set(action, { key, reason });
  }
  return { active, blocked };
}

/** The key an action has right now: the user's own, or its active default (issue #584). */
export function useActionKey(action: DefaultableAction): string | null {
  const { hotkeys, pttKey, cameraHotkey } = useHotkeySettings();
  const macos = useIsMacos();
  return useMemo(() => {
    const own = ownKeyFor(action, hotkeys, cameraHotkey);
    if (own !== null) return own;
    return resolveDefaults({ macos, hotkeys, pttKey, cameraHotkey }).active.get(action) ?? null;
  }, [action, macos, hotkeys, pttKey, cameraHotkey]);
}
