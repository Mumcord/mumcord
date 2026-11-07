import { useEffect, useMemo, useRef, useState, type RefObject } from "react";
import { getApi, type HotkeyAction, type HotkeysMode } from "../../api";
import { useIsMacos } from "../../api/useIsMacos";
import { selfUser, useServerStateOf, useVoiceServerId } from "../channels/serverStore";
import { keyCombo } from "../settings/pttKey";
import { CAMERA_ACTION, resolveDefaults, type DefaultableAction } from "./defaultHotkeys";
import { canonicalCombo, typingTarget } from "./hotkeyKeys";
import { useHotkeySettings } from "./hotkeyStore";
import { runCameraHotkey, runHotkey, type VoicePlace } from "./runHotkey";

/** Remembers the channel voice was in before the current one, for "Back to previous channel". */
function usePreviousVoicePlace(): RefObject<VoicePlace | null> {
  const voiceId = useVoiceServerId();
  const channelId = selfUser(useServerStateOf(voiceId))?.channelId ?? null;
  const last = useRef<VoicePlace | null>(null);
  const previous = useRef<VoicePlace | null>(null);
  useEffect(() => {
    if (voiceId === null || channelId === null) return;
    const before = last.current;
    if (before !== null && (before.serverId !== voiceId || before.channelId !== channelId)) {
      previous.current = before;
    }
    last.current = { serverId: voiceId, channelId };
  }, [voiceId, channelId]);
  return previous;
}

/** What a key press in the window runs: a hotkey action, or the camera's own hotkey. */
type InWindowKey = HotkeyAction | typeof CAMERA_ACTION;

/**
 * The action of a default. Push-to-talk has none, and a "switch to channel" row is the user's own,
 * so only the fixed actions and the camera are left.
 */
function inWindowAction(action: DefaultableAction): InWindowKey | null {
  switch (action) {
    case "pushToTalk":
      return null;
    case CAMERA_ACTION:
      return CAMERA_ACTION;
    default:
      return { kind: action };
  }
}

/**
 * Runs the global hotkeys (FR-3.72): the engine reports presses while Mumcord is in the
 * background. With "Work while Mumcord is in the background" off, or on a desktop without global
 * shortcuts, the keys work while the window has the focus instead, except while typing and
 * while an overlay such as the settings is open. The defaults (issue #584) are never registered
 * with the engine, so they work in the window either way. Renders nothing.
 */
export function GlobalHotkeys({ overlayOpen }: { overlayOpen: boolean }) {
  const { globalHotkeys, hotkeys, pttKey, cameraHotkey } = useHotkeySettings();
  const macos = useIsMacos();
  const previous = usePreviousVoicePlace();
  const [mode, setMode] = useState<HotkeysMode | null>(null);

  useEffect(() => {
    getApi()
      .hotkeysStatus()
      .then(
        (status) => {
          setMode(status.mode);
        },
        (error: unknown) => {
          console.error("Reading the hotkey status failed", error);
        },
      );
  }, []);

  useEffect(() => {
    const run = (action: HotkeyAction) => {
      runHotkey(action, previous.current).catch((error: unknown) => {
        console.error(`Running the hotkey action ${action.kind} failed`, error);
      });
    };
    return getApi().on("hotkeyPressed", ({ action }) => {
      run(action);
    });
  }, [previous]);

  /** The keys the window handles itself, by their stored form. */
  const inWindow = useMemo(() => {
    const engineHandles = globalHotkeys && mode !== "unavailable";
    const keys = new Map<string, InWindowKey>();
    // While the engine registers the user's keys they reach the UI as `hotkeyPressed`.
    if (!engineHandles) {
      for (const hotkey of hotkeys) keys.set(hotkey.key, hotkey.action);
    }
    const { active } = resolveDefaults({ macos, hotkeys, pttKey, cameraHotkey });
    for (const [action, key] of active) {
      const target = inWindowAction(action);
      if (target !== null && !keys.has(key)) keys.set(key, target);
    }
    return keys;
  }, [globalHotkeys, mode, hotkeys, macos, pttKey, cameraHotkey]);

  useEffect(() => {
    if (overlayOpen || inWindow.size === 0) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.defaultPrevented || event.repeat || typingTarget(event.target)) return;
      const combo = keyCombo(event);
      if (combo === null) return;
      const key = canonicalCombo(combo);
      const action = inWindow.get(key);
      if (action === undefined) return;
      event.preventDefault();
      const ran =
        action === CAMERA_ACTION ? runCameraHotkey() : runHotkey(action, previous.current);
      ran.catch((error: unknown) => {
        console.error(`Running the hotkey action ${key} failed`, error);
      });
    };
    window.addEventListener("keydown", onKeyDown);
    return () => {
      window.removeEventListener("keydown", onKeyDown);
    };
  }, [overlayOpen, inWindow, previous]);

  return null;
}
