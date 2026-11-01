import { useEffect, useRef, useState, type KeyboardEvent, type MouseEvent } from "react";
import { useTranslation } from "react-i18next";
import { getApi, toEngineError } from "../../../api";
import { errorText } from "../errors";
import { keyCombo, mouseKey, mouseLabel } from "../pttKey";

type CaptureState = "idle" | "listening" | "waiting";

interface PttKeyCaptureProps {
  current: string | null;
  /** Also accept the middle and side mouse buttons while listening. */
  mouseAllowed: boolean;
  onCaptured: (key: string) => void;
}

/**
 * Push-to-talk key capture: the button listens for the next key (with modifiers) and hands it
 * to the engine, which validates it or, on Wayland, opens the desktop's shortcut dialog.
 */
export function PttKeyCapture({ current, mouseAllowed, onCaptured }: PttKeyCaptureProps) {
  const { t } = useTranslation("settings");
  const [state, setState] = useState<CaptureState>("idle");
  const [problem, setProblem] = useState<string | null>(null);
  const waiting = useRef(false);
  // Bumped when the user cancels a pending capture, whose failure then shows no error.
  const attempts = useRef(0);

  // A desktop dialog still open when the section goes away is cancelled.
  useEffect(
    () => () => {
      if (waiting.current) void getApi().pttCaptureCancel().catch(console.error);
    },
    [],
  );

  const capture = async (combo: string) => {
    setState("waiting");
    waiting.current = true;
    const attempt = ++attempts.current;
    try {
      onCaptured(await getApi().pttCaptureKey(combo));
      setProblem(null);
    } catch (error: unknown) {
      const engineError = toEngineError(error);
      if (attempt !== attempts.current) setProblem(null);
      else if (engineError.code === "invalidInput") setProblem(t("voice.keyRejected"));
      else if (engineError.code === "shortcutNotConfirmed") {
        console.error("The desktop did not confirm the push-to-talk shortcut", error);
        setProblem(t("voice.keyNotConfirmed"));
      } else {
        console.error("Capturing the push-to-talk key failed", error);
        setProblem(errorText(t, error));
      }
    } finally {
      waiting.current = false;
      setState("idle");
    }
  };

  const cancelWaiting = () => {
    attempts.current++;
    getApi()
      .pttCaptureCancel()
      .catch((error: unknown) => {
        console.error("Cancelling the push-to-talk capture failed", error);
      });
  };

  const onKeyDown = (event: KeyboardEvent<HTMLButtonElement>) => {
    if (state === "waiting" && event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      cancelWaiting();
      return;
    }
    if (state !== "listening") return;
    // While listening every key belongs to the capture, Escape included (it must not close
    // the settings overlay).
    event.preventDefault();
    event.stopPropagation();
    if (event.key === "Escape") {
      setState("idle");
      return;
    }
    const combo = keyCombo(event);
    if (combo !== null) void capture(combo);
  };

  /** Middle and side buttons while listening; left/right clicks keep working as clicks. */
  const onMouseDown = (event: MouseEvent<HTMLButtonElement>) => {
    if (state !== "listening" || !mouseAllowed) return;
    const key = mouseKey(event.button);
    if (key === null) return;
    event.preventDefault();
    void capture(key);
  };

  /** Keeps side buttons from navigating and the middle button from pasting while capturing. */
  const suppressMouse = (event: MouseEvent<HTMLButtonElement>) => {
    if (state !== "idle" && mouseKey(event.button) !== null) event.preventDefault();
  };

  const mouseName = current === null ? null : mouseLabel(current);
  const shown = mouseName === null ? current : t(`voice.mouse.${mouseName}`);

  const label =
    state === "listening"
      ? mouseAllowed
        ? t("voice.keyListeningMouse")
        : t("voice.keyListening")
      : state === "waiting"
        ? t("voice.keyWaiting")
        : current === null
          ? t("voice.keySet")
          : t("voice.keyChange");

  return (
    <div className="settings-field-row">
      <p className="settings-value">
        {t("voice.keyCurrent")} <kbd className="settings-kbd">{shown ?? t("voice.keyNone")}</kbd>
      </p>
      <div className="settings-actions">
        <button
          type="button"
          className="settings-button"
          aria-pressed={state === "listening"}
          onClick={() => {
            // Not disabled while waiting: that would drop keyboard focus.
            if (state === "waiting") return;
            setProblem(null);
            setState(state === "listening" ? "idle" : "listening");
          }}
          onKeyDown={onKeyDown}
          onMouseDown={onMouseDown}
          onMouseUp={suppressMouse}
          onAuxClick={suppressMouse}
          onBlur={() => {
            if (state === "listening") setState("idle");
          }}
        >
          {label}
        </button>
        {state === "waiting" && (
          <button type="button" className="settings-button" onClick={cancelWaiting}>
            {t("voice.keyCancel")}
          </button>
        )}
      </div>
      {current === null && state === "idle" && (
        <p className="settings-warning">{t("voice.keyMissing")}</p>
      )}
      <p role="alert" className="settings-error">
        {problem}
      </p>
    </div>
  );
}
