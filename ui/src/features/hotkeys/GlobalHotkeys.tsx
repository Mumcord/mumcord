import { useEffect, useRef, useState, type RefObject } from "react";
import { getApi, type HotkeyAction, type HotkeysMode } from "../../api";
import { selfUser, useServerStateOf, useVoiceServerId } from "../channels/serverStore";
import { keyCombo } from "../settings/pttKey";
import { canonicalCombo, typingTarget } from "./hotkeyKeys";
import { useHotkeySettings } from "./hotkeyStore";
import { runHotkey, type VoicePlace } from "./runHotkey";

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

/**
 * Runs the global hotkeys (FR-3.72): the engine reports presses while Mumcord is in the
 * background. With "Work while Mumcord is in the background" off, or on a desktop without global
 * shortcuts, the keys work while the window has the focus instead, except while typing and
 * while an overlay such as the settings is open. Renders nothing.
 */
export function GlobalHotkeys({ overlayOpen }: { overlayOpen: boolean }) {
  const { globalHotkeys, hotkeys } = useHotkeySettings();
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

  const inApp = !globalHotkeys || mode === "unavailable";
  useEffect(() => {
    if (!inApp || overlayOpen || hotkeys.length === 0) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.defaultPrevented || event.repeat || typingTarget(event.target)) return;
      const combo = keyCombo(event);
      if (combo === null) return;
      const key = canonicalCombo(combo);
      const hotkey = hotkeys.find((h) => h.key === key);
      if (hotkey === undefined) return;
      event.preventDefault();
      runHotkey(hotkey.action, previous.current).catch((error: unknown) => {
        console.error(`Running the hotkey action ${hotkey.action.kind} failed`, error);
      });
    };
    window.addEventListener("keydown", onKeyDown);
    return () => {
      window.removeEventListener("keydown", onKeyDown);
    };
  }, [inApp, overlayOpen, hotkeys, previous]);

  return null;
}
