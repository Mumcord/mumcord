/**
 * The keys both media views share (issue #163, FR-3.128): Space play/pause, ← and → seek 5 s,
 * M mute, F full screen, Z focus, 1 for 1:1, Escape leaves full screen or focus. They work while
 * the view is open, but not while the user types (the chat input above the player keeps its keys).
 */
import { useCallback, useEffect } from "react";

export interface MediaActions {
  playPause?: () => void;
  /** Whole seconds, negative seeks back. */
  seek?: (seconds: number) => void;
  mute?: () => void;
  fullscreen?: () => void;
  focus?: () => void;
  fitOneToOne?: () => void;
  /** Escape: leave full screen, then focus. */
  escape?: () => void;
}

function typingIn(target: EventTarget): boolean {
  return (
    target instanceof HTMLElement &&
    (target.isContentEditable || ["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName))
  );
}

/** The step of ← and →, in seconds. */
export const SEEK_STEP = 5;

export function useMediaKeys(actions: MediaActions): void {
  const onKeyDown = useCallback(
    (event: KeyboardEvent) => {
      if (event.defaultPrevented || event.altKey || event.ctrlKey || event.metaKey) return;
      if (event.target !== null && typingIn(event.target)) return;
      const run = (action?: () => void) => {
        if (action === undefined) return;
        event.preventDefault();
        action();
      };
      switch (event.key) {
        case " ":
          run(actions.playPause);
          break;
        case "ArrowLeft":
          run(() => actions.seek?.(-SEEK_STEP));
          break;
        case "ArrowRight":
          run(() => actions.seek?.(SEEK_STEP));
          break;
        case "m":
        case "M":
          run(actions.mute);
          break;
        case "f":
        case "F":
          run(actions.fullscreen);
          break;
        case "z":
        case "Z":
          run(actions.focus);
          break;
        case "1":
          run(actions.fitOneToOne);
          break;
        case "Escape":
          run(actions.escape);
          break;
        default:
      }
    },
    [actions],
  );

  useEffect(() => {
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("keydown", onKeyDown);
    };
  }, [onKeyDown]);
}
