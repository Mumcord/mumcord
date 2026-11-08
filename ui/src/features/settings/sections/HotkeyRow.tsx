import { useEffect, useId, useRef, useState, type KeyboardEvent } from "react";
import { Trans, useTranslation } from "react-i18next";
import { useIsMacos } from "../../../api/useIsMacos";
import { keyParts } from "../../hotkeys/hotkeyKeys";
import { keyCombo } from "../pttKey";

/** Recording stops by itself after this long without a key (mockup `epic3-64`). */
export const RECORD_TIMEOUT_MS = 5000;

/** A key someone else already uses, waiting for "Assign here" or "Cancel". */
export interface PendingConflict {
  key: string;
  /** Name of the action that has the key; `null` for Mumcord's own shortcuts. */
  other: string | null;
  /** For Mumcord's own shortcuts: what the shortcut does. */
  inApp: string | null;
}

interface HotkeyRowProps {
  label: string;
  /** Second line under the label (e.g. the channel of "switch to channel"). */
  description?: string | undefined;
  /** The key the field shows: the user's own key, or the action's active default. */
  value: string | null;
  /** Whether `value` is the user's own key rather than a default (issue #584). */
  own?: boolean;
  /** The action's default: a key, `null` for "Default: none", or absent to hide the line. */
  defaultKey?: string | null;
  /** Why an off default is off, as a sentence under the row (e.g. it is taken already). */
  defaultNote?: string | null;
  /** Shown under the row instead of nothing, e.g. "could not be registered". */
  warning?: string | null;
  /** Starts recording at once (a new "switch to channel" row). */
  startRecording?: boolean;
  /** Checks a captured key; resolves with a conflict to show, or `null` once it is assigned. */
  onCapture: (combo: string) => Promise<PendingConflict | null>;
  /** "Assign here": takes the key from its other owner. */
  onAssign: (conflict: PendingConflict) => void;
  onClear: () => void;
  /** "Use default": forgets the user's own key, so the default applies again. */
  onUseDefault?: () => void;
  /** Recording ended without a key (Esc, timeout, focus lost). */
  onCancel?: () => void;
}

/**
 * One action of Settings → Hotkeys (FR-3.72): the key field records the next key combination;
 * Esc cancels, Backspace clears, and recording stops after five seconds. A key already in use
 * shows a conflict with "Assign here". Below the field, right-aligned, the action's default
 * (issue #584) with "Use default" while the user's own key differs from it.
 */
export function HotkeyRow({
  label,
  description,
  value,
  own = false,
  defaultKey,
  defaultNote,
  warning,
  startRecording = false,
  onCapture,
  onAssign,
  onClear,
  onUseDefault,
  onCancel,
}: HotkeyRowProps) {
  const { t } = useTranslation("settings");
  const macos = useIsMacos();
  const id = useId();
  const field = useRef<HTMLButtonElement>(null);
  const [recording, setRecording] = useState(startRecording);
  const [busy, setBusy] = useState(false);
  const [conflict, setConflict] = useState<PendingConflict | null>(null);
  const [problem, setProblem] = useState<string | null>(null);

  useEffect(() => {
    if (startRecording) field.current?.focus();
  }, [startRecording]);

  useEffect(() => {
    if (!recording) return;
    const timer = window.setTimeout(() => {
      setRecording(false);
      onCancel?.();
    }, RECORD_TIMEOUT_MS);
    return () => {
      window.clearTimeout(timer);
    };
  }, [recording, onCancel]);

  const stop = () => {
    setRecording(false);
    onCancel?.();
  };

  const capture = async (combo: string) => {
    setRecording(false);
    setBusy(true);
    try {
      setConflict(await onCapture(combo));
      setProblem(null);
    } catch (error: unknown) {
      setProblem(error instanceof Error ? error.message : String(error));
      onCancel?.();
    } finally {
      setBusy(false);
    }
  };

  const onKeyDown = (event: KeyboardEvent<HTMLButtonElement>) => {
    if (!recording) return;
    // While recording every key belongs to the capture, Esc included (it must not close the
    // settings overlay).
    event.preventDefault();
    event.stopPropagation();
    const plain = !event.ctrlKey && !event.altKey && !event.shiftKey && !event.metaKey;
    if (event.key === "Escape") {
      stop();
      return;
    }
    if (plain && event.key === "Backspace") {
      setRecording(false);
      setConflict(null);
      onClear();
      return;
    }
    const combo = keyCombo(event);
    if (combo !== null) void capture(combo);
  };

  const shown = conflict?.key ?? value;
  const keyText = shown === null ? t("hotkeys.notSet") : keyParts(shown, macos).join(" + ");
  const fieldClass = [
    "hotkey-row__field",
    recording ? "hotkey-row__field--recording" : "",
    conflict !== null ? "hotkey-row__field--conflict" : "",
  ]
    .filter(Boolean)
    .join(" ");
  // A default replaces the user's own key, so it is cleared through "Use default" instead.
  const canUseDefault = own && defaultKey != null && onUseDefault !== undefined;
  const clearable = own && shown !== null && !canUseDefault;

  return (
    <div className="hotkey-row">
      <div className="hotkey-row__main">
        <div className="hotkey-row__label">
          <span id={`${id}-label`}>{label}</span>
          {description !== undefined && (
            <span className="hotkey-row__description">{description}</span>
          )}
        </div>
        <div className="hotkey-row__key">
          <button
            ref={field}
            type="button"
            className={fieldClass}
            aria-labelledby={`${id}-label ${id}-value`}
            aria-describedby={recording ? `${id}-hint` : undefined}
            aria-pressed={recording}
            onClick={() => {
              if (busy) return;
              setProblem(null);
              setConflict(null);
              if (recording) stop();
              else setRecording(true);
            }}
            onKeyDown={onKeyDown}
            onBlur={() => {
              if (recording) stop();
            }}
          >
            <span id={`${id}-value`} className="hotkey-row__keys">
              {recording ? (
                <span className="hotkey-row__recording">
                  <span className="hotkey-row__dot" aria-hidden="true" />
                  {t("hotkeys.recording")}
                </span>
              ) : shown === null ? (
                <span className="hotkey-row__placeholder">{keyText}</span>
              ) : (
                <>
                  <span className="visually-hidden">{keyText}</span>
                  <span aria-hidden="true" className="hotkey-row__chips">
                    {keyParts(shown, macos).map((part, i) => (
                      <span key={i}>
                        {i > 0 && <span className="hotkey-row__plus">+</span>}
                        <kbd className="settings-kbd">{part}</kbd>
                      </span>
                    ))}
                  </span>
                </>
              )}
            </span>
          </button>
          {defaultKey !== undefined && conflict === null && !recording && (
            <p className="hotkey-row__default">
              {defaultKey === null
                ? t("hotkeys.defaultNone")
                : t("hotkeys.defaultKey", { key: keyParts(defaultKey, macos).join(" + ") })}{" "}
              {canUseDefault && (
                <button
                  type="button"
                  className="settings-link"
                  onClick={() => {
                    setProblem(null);
                    setConflict(null);
                    onUseDefault();
                  }}
                >
                  {t("hotkeys.useDefault")}
                </button>
              )}
            </p>
          )}
        </div>
        <span className="hotkey-row__side">
          {recording ? (
            <button type="button" className="settings-link" onMouseDown={stop}>
              {t("hotkeys.cancel")}
            </button>
          ) : (
            clearable && (
              <button
                type="button"
                className="settings-link"
                aria-label={t("hotkeys.clearLabel", { action: label })}
                onClick={() => {
                  setConflict(null);
                  onClear();
                }}
              >
                {t("hotkeys.clear")}
              </button>
            )
          )}
        </span>
      </div>
      {recording && (
        <p id={`${id}-hint`} className="settings-hint" aria-live="polite">
          <Trans
            t={t}
            i18nKey="hotkeys.recordingHint"
            components={{
              esc: <kbd className="settings-kbd" />,
              back: <kbd className="settings-kbd" />,
            }}
          />
        </p>
      )}
      {conflict !== null && (
        <div className="hotkey-row__conflict" role="alert">
          <p>
            <strong>{t("hotkeys.conflictTitle")}</strong>{" "}
            {conflict.other !== null
              ? t("hotkeys.conflict", { other: conflict.other })
              : t("hotkeys.inApp", { shortcut: conflict.inApp ?? "" })}
          </p>
          <div className="settings-actions">
            {conflict.other !== null && (
              <button
                type="button"
                className="settings-button settings-button--primary"
                onClick={() => {
                  setConflict(null);
                  onAssign(conflict);
                }}
              >
                {t("hotkeys.assignHere")}
              </button>
            )}
            <button
              type="button"
              className="settings-button"
              onClick={() => {
                setConflict(null);
                onCancel?.();
                field.current?.focus();
              }}
            >
              {t("hotkeys.cancel")}
            </button>
          </div>
        </div>
      )}
      {warning != null && conflict === null && <p className="settings-warning">{warning}</p>}
      {defaultNote != null && conflict === null && (
        <p className="settings-warning">{defaultNote}</p>
      )}
      <p role="alert" className="settings-error">
        {problem}
      </p>
    </div>
  );
}
