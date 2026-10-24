import { useEffect, useState } from "react";
import { getApi } from "../../api";
import { useView } from "../../app/view";

export interface PushToTalk {
  /** The talk mode is push-to-talk (otherwise voice activation). */
  active: boolean;
  /** Readable shortcut, e.g. "Ctrl+A"; null while none is bound. */
  key: string | null;
}

/** "Ctrl+KeyA" → "Ctrl+A", "Digit1" → "1"; other codes (F13, Mouse4) stay as they are. */
export function keyLabel(combo: string): string {
  return combo
    .split("+")
    .map((part) => part.replace(/^(?:Key|Digit)(?=.$)/, ""))
    .join("+");
}

/**
 * Reads the talk mode and shortcut from the settings. There is no settings-changed event, so it
 * reloads whenever the settings overlay opens or closes.
 */
export function usePushToTalk(): PushToTalk {
  const { overlay } = useView();
  const [state, setState] = useState<PushToTalk>({ active: false, key: null });

  useEffect(() => {
    let active = true;
    getApi()
      .settingsGet()
      .then((settings) => {
        if (active)
          setState({
            active: settings.talkMode === "pushToTalk",
            key: settings.pttKey === null ? null : keyLabel(settings.pttKey),
          });
      })
      .catch((error: unknown) => {
        console.error("User panel could not load the talk mode", error);
      });
    return () => {
      active = false;
    };
  }, [overlay]);

  return state;
}
