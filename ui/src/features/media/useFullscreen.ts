/** Full screen for one element (spec 5.4: F toggles, Escape leaves). */
import { useCallback, useEffect, useState, type RefObject } from "react";

export function useFullscreen(ref: RefObject<HTMLElement | null>): {
  fullscreen: boolean;
  toggle: () => void;
  exit: () => void;
} {
  const [fullscreen, setFullscreen] = useState(false);

  useEffect(() => {
    const element = ref.current;
    // The browser also leaves full screen by itself (Escape), so follow its state.
    const onChange = () => {
      setFullscreen(element !== null && document.fullscreenElement === element);
    };
    document.addEventListener("fullscreenchange", onChange);
    return () => {
      document.removeEventListener("fullscreenchange", onChange);
      if (element !== null && document.fullscreenElement === element) {
        document.exitFullscreen().catch((error: unknown) => {
          console.warn("leaving full screen failed", error);
        });
      }
    };
  }, [ref]);

  const exit = useCallback(() => {
    if (document.fullscreenElement === null) return;
    document.exitFullscreen().catch((error: unknown) => {
      console.warn("leaving full screen failed", error);
    });
  }, []);

  const toggle = useCallback(() => {
    const element = ref.current;
    if (element === null) return;
    if (document.fullscreenElement === element) {
      exit();
      return;
    }
    element.requestFullscreen().catch((error: unknown) => {
      console.warn("full screen failed", error);
    });
  }, [ref, exit]);

  return { fullscreen, toggle, exit };
}
