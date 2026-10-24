import { useId, useRef, useState, type KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";
import { getApi, type PttAvailability, type Settings, type TalkMode } from "../../api";
import { errorCode } from "./errors";
import { keyCombo } from "./keyCombo";
import { StepFrame } from "./StepFrame";

export interface TalkChoice {
  talkMode: TalkMode;
  pttKey: string | null;
  /** Register Mumcord as the `mumble://` handler (FR-1.6). */
  registerLinkHandler: boolean;
}

type Capture = "off" | "listening" | "pending";

interface TalkModeStepProps {
  settings: Settings;
  /** `null` when availability could not be determined; capture then reports any problem. */
  availability: PttAvailability | null;
  /** Whether Mumcord already handles `mumble://` links; `null` when unknown. */
  linkHandlerActive: boolean | null;
  focusOnMount: boolean;
  onBack: () => void;
  /** Saves the choice and leaves the wizard; rejects when saving failed. */
  onFinish: (choice: TalkChoice) => Promise<void>;
}

/** Step 3: voice activation or push-to-talk with key capture, then the link handler offer. */
export function TalkModeStep({
  settings,
  availability,
  linkHandlerActive,
  focusOnMount,
  onBack,
  onFinish,
}: TalkModeStepProps) {
  const { t } = useTranslation("wizard");
  const unavailable = availability?.status === "unavailable" ? availability.reason : null;
  const [mode, setMode] = useState<TalkMode>(unavailable ? "voiceActivation" : settings.talkMode);
  const [pttKey, setPttKey] = useState<string | null>(settings.pttKey);
  const [capture, setCapture] = useState<Capture>("off");
  const [keyError, setKeyError] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [registerLink, setRegisterLink] = useState(true);
  const [busy, setBusy] = useState(false);
  // Bumped on cancel so a late failure of the cancelled capture shows no error.
  const captureAttempt = useRef(0);
  const captureButton = useRef<HTMLButtonElement>(null);
  const ids = {
    voiceHint: useId(),
    pttHint: useId(),
    unavailable: useId(),
    key: useId(),
    keyError: useId(),
    linkHint: useId(),
  };

  async function captureKey(combo: string) {
    setCapture("pending");
    const attempt = ++captureAttempt.current;
    try {
      const stored = await getApi().pttCaptureKey(combo);
      setPttKey(stored);
      setKeyError(null);
    } catch (cause: unknown) {
      if (attempt === captureAttempt.current) {
        setKeyError(
          errorCode(cause) === "invalidInput" ? t("talk.keyInvalid") : t("errors.generic"),
        );
      }
    } finally {
      if (attempt === captureAttempt.current) setCapture("off");
    }
  }

  function onCaptureKeyDown(event: KeyboardEvent<HTMLButtonElement>) {
    if (capture === "off") return;
    const plain = !event.ctrlKey && !event.altKey && !event.shiftKey && !event.metaKey;
    if (event.key === "Escape" && plain) {
      event.preventDefault();
      if (capture === "pending") {
        captureAttempt.current++;
        getApi()
          .pttCaptureCancel()
          .catch((cause: unknown) => {
            console.error("cancelling key capture failed", cause);
          });
      }
      setCapture("off");
      return;
    }
    // Tab keeps working so keyboard users are never trapped in the capture button.
    if (event.key === "Tab" || capture === "pending") return;
    event.preventDefault();
    const combo = keyCombo(event.nativeEvent);
    if (combo !== null) void captureKey(combo);
  }

  async function submit() {
    if (mode === "pushToTalk" && pttKey === null) {
      setKeyError(t("talk.keyRequired"));
      captureButton.current?.focus();
      return;
    }
    setBusy(true);
    setError(null);
    try {
      await onFinish({
        talkMode: mode,
        pttKey,
        registerLinkHandler: linkHandlerActive !== true && registerLink,
      });
    } catch {
      setBusy(false);
      setError(t("errors.saveFailed"));
    }
  }

  const captureLabel =
    capture === "listening"
      ? t("talk.pressKey")
      : capture === "pending"
        ? t("talk.confirmDialog")
        : pttKey === null
          ? t("talk.chooseKey")
          : t("talk.changeKey");

  return (
    <StepFrame
      step={3}
      title={t("talk.title")}
      focusOnMount={focusOnMount}
      busy={busy}
      submitLabel={t("finish")}
      onSubmit={() => void submit()}
      onBack={onBack}
    >
      <fieldset className="wizard-choice">
        <legend className="visually-hidden">{t("talk.mode")}</legend>
        <label className="wizard-radio">
          <input
            type="radio"
            name="talk-mode"
            value="voiceActivation"
            checked={mode === "voiceActivation"}
            aria-describedby={ids.voiceHint}
            onChange={() => {
              setMode("voiceActivation");
              setKeyError(null);
            }}
          />
          {t("talk.voiceActivation")}
        </label>
        <p id={ids.voiceHint} className="wizard-muted wizard-small wizard-indent">
          {t("talk.voiceActivationHint")}
        </p>
        <label className="wizard-radio">
          <input
            type="radio"
            name="talk-mode"
            value="pushToTalk"
            checked={mode === "pushToTalk"}
            disabled={unavailable !== null}
            aria-describedby={unavailable ? `${ids.pttHint} ${ids.unavailable}` : ids.pttHint}
            onChange={() => {
              setMode("pushToTalk");
            }}
          />
          {t("talk.pushToTalk")}
        </label>
        <p id={ids.pttHint} className="wizard-muted wizard-small wizard-indent">
          {t("talk.pushToTalkHint")}
        </p>
        {unavailable && (
          <p id={ids.unavailable} className="wizard-note wizard-indent">
            {t(`talk.${unavailable}`)}
          </p>
        )}
        {mode === "pushToTalk" && (
          <div className="wizard-row wizard-indent">
            <button
              type="button"
              ref={captureButton}
              className="button"
              aria-describedby={keyError ? `${ids.key} ${ids.keyError}` : ids.key}
              aria-pressed={capture !== "off"}
              onClick={() => {
                if (capture === "off") {
                  setKeyError(null);
                  setCapture("listening");
                }
              }}
              onKeyDown={onCaptureKeyDown}
              onBlur={() => {
                if (capture === "listening") setCapture("off");
              }}
            >
              {captureLabel}
            </button>
            <span id={ids.key}>
              {pttKey === null ? t("talk.noKey") : t("talk.currentKey", { key: pttKey })}
            </span>
          </div>
        )}
        {keyError && (
          <p id={ids.keyError} role="alert" className="wizard-error wizard-indent">
            {keyError}
          </p>
        )}
      </fieldset>
      {linkHandlerActive === true ? (
        <p className="wizard-muted">{t("talk.linkHandlerActive")}</p>
      ) : (
        <div>
          <label className="wizard-radio">
            <input
              type="checkbox"
              checked={registerLink}
              aria-describedby={ids.linkHint}
              onChange={(event) => {
                setRegisterLink(event.target.checked);
              }}
            />
            {t("talk.linkHandler")}
          </label>
          <p id={ids.linkHint} className="wizard-muted wizard-small wizard-indent">
            {t("talk.linkHandlerHint")}
          </p>
        </div>
      )}
      {error && (
        <p role="alert" className="wizard-error">
          {error}
        </p>
      )}
    </StepFrame>
  );
}
