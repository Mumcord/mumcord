import { useId, useRef, useState, type KeyboardEvent, type MouseEvent } from "react";
import { useTranslation } from "react-i18next";
import {
  getApi,
  type PttAvailability,
  type PttMouseStatus,
  type Settings,
  type TalkMode,
} from "../../api";
import { errorCode } from "./errors";
import { keyCombo, mouseKey, mouseLabel } from "./keyCombo";
import ingameOverlayExample from "../../assets/ingame-overlay-example.png";
import { StepFrame } from "./StepFrame";

export interface TalkChoice {
  talkMode: TalkMode;
  pttKey: string | null;
  /** Register Mumcord as the `mumble://` handler (FR-1.6). */
  registerLinkHandler: boolean;
  /** Start Mumcord when the user logs in (FR-1.48d). */
  autostart: boolean;
  /** Show link previews and embeds in chat (FR-1.49). */
  linkPreviews: boolean;
  /** Share the own channel history with other Mumcord users (FR-1.60). */
  historySharing: boolean;
  /** Turn on the in-game overlay globally (FR-1.46b); `null` where it is unsupported. */
  ingameOverlay: boolean | null;
}

type Capture = "off" | "listening" | "pending";

interface TalkModeStepProps {
  settings: Settings;
  /** `null` when availability could not be determined; capture then reports any problem. */
  availability: PttAvailability | null;
  /** Mouse-button support (FR-1.21); `null` when unknown, then only keys can be captured. */
  mouseStatus?: PttMouseStatus | null;
  /** Whether Mumcord already handles `mumble://` links; `null` when unknown. */
  linkHandlerActive: boolean | null;
  /** Whether the in-game overlay works on this platform (FR-1.46b, QA.58); hides the choice. */
  ingameOverlaySupported: boolean;
  focusOnMount: boolean;
  onBack: () => void;
  /** Saves the choice and leaves the wizard; rejects when saving failed. */
  onFinish: (choice: TalkChoice) => Promise<void>;
}

/**
 * Step 4: voice activation or push-to-talk with key capture, then the link handler,
 * start-at-login and link-preview choices.
 */
export function TalkModeStep({
  settings,
  availability,
  mouseStatus = null,
  linkHandlerActive,
  ingameOverlaySupported,
  focusOnMount,
  onBack,
  onFinish,
}: TalkModeStepProps) {
  const { t } = useTranslation("wizard");
  const unavailable = availability?.status === "unavailable" ? availability.reason : null;
  // Wayland reads mice only after the user opts in under Settings → Voice (QA.4).
  const mouseAllowed =
    mouseStatus?.support === "native" ||
    (mouseStatus?.support === "evdev" && settings.allowMouseButtons);
  const mouseNeedsOptIn = mouseStatus?.support === "evdev" && !settings.allowMouseButtons;
  const [mode, setMode] = useState<TalkMode>(unavailable ? "voiceActivation" : settings.talkMode);
  const [pttKey, setPttKey] = useState<string | null>(settings.pttKey);
  const [capture, setCapture] = useState<Capture>("off");
  const [keyError, setKeyError] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [registerLink, setRegisterLink] = useState(true);
  const [autostart, setAutostart] = useState(settings.autostart);
  const [linkPreviews, setLinkPreviews] = useState(settings.linkPreviews);
  const [historySharing, setHistorySharing] = useState(settings.historySharing);
  // In-game overlay: default off (FR-1.46b, QA.55).
  const [ingameOverlay, setIngameOverlay] = useState(false);
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
    autostartHint: useId(),
    linkPreviewsHint: useId(),
    historySharingHint: useId(),
    ingameHint: useId(),
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
        const code = errorCode(cause);
        if (code !== "invalidInput") console.error("capturing the push-to-talk key failed", cause);
        setKeyError(
          code === "invalidInput"
            ? t("talk.keyInvalid")
            : code === "shortcutNotConfirmed"
              ? t("talk.keyNotConfirmed")
              : t("errors.generic"),
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

  /** Middle and side buttons while listening; left/right clicks keep working as clicks. */
  function onCaptureMouseDown(event: MouseEvent<HTMLButtonElement>) {
    if (capture !== "listening" || !mouseAllowed) return;
    const key = mouseKey(event.button);
    if (key === null) return;
    event.preventDefault();
    void captureKey(key);
  }

  /** Keeps side buttons from navigating and the middle button from pasting while capturing. */
  function suppressMouse(event: MouseEvent<HTMLButtonElement>) {
    if (capture !== "off" && mouseKey(event.button) !== null) event.preventDefault();
  }

  function keyName(key: string): string {
    const mouse = mouseLabel(key);
    return mouse === null ? key : t(`talk.mouse.${mouse}`);
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
        autostart,
        linkPreviews,
        historySharing,
        ingameOverlay: ingameOverlaySupported ? ingameOverlay : null,
      });
    } catch {
      setBusy(false);
      setError(t("errors.saveFailed"));
    }
  }

  const captureLabel =
    capture === "listening"
      ? mouseAllowed
        ? t("talk.pressKeyOrMouse")
        : t("talk.pressKey")
      : capture === "pending"
        ? t("talk.confirmDialog")
        : pttKey === null
          ? t("talk.chooseKey")
          : t("talk.changeKey");

  return (
    <StepFrame
      step={5}
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
            disabled={unavailable !== null && !mouseAllowed}
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
            {mouseAllowed && ` ${t("talk.mouseStillWorks")}`}
          </p>
        )}
        {mode === "pushToTalk" && mouseNeedsOptIn && (
          <p className="wizard-muted wizard-small wizard-indent">{t("talk.mouseOptIn")}</p>
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
              onMouseDown={onCaptureMouseDown}
              onMouseUp={suppressMouse}
              onAuxClick={suppressMouse}
              onBlur={() => {
                if (capture === "listening") setCapture("off");
              }}
            >
              {captureLabel}
            </button>
            <span id={ids.key}>
              {pttKey === null ? t("talk.noKey") : t("talk.currentKey", { key: keyName(pttKey) })}
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
      <div>
        <label className="wizard-radio">
          <input
            type="checkbox"
            checked={autostart}
            aria-describedby={ids.autostartHint}
            onChange={(event) => {
              setAutostart(event.target.checked);
            }}
          />
          {t("talk.autostart")}
        </label>
        <p id={ids.autostartHint} className="wizard-muted wizard-small wizard-indent">
          {t("talk.autostartHint")}
        </p>
      </div>
      <div>
        <label className="wizard-radio">
          <input
            type="checkbox"
            checked={linkPreviews}
            aria-describedby={ids.linkPreviewsHint}
            onChange={(event) => {
              setLinkPreviews(event.target.checked);
            }}
          />
          {t("talk.linkPreviews")}
        </label>
        <p id={ids.linkPreviewsHint} className="wizard-muted wizard-small wizard-indent">
          {t("talk.linkPreviewsHint")}
        </p>
      </div>
      <div>
        <label className="wizard-radio">
          <input
            type="checkbox"
            checked={historySharing}
            aria-describedby={ids.historySharingHint}
            onChange={(event) => {
              setHistorySharing(event.target.checked);
            }}
          />
          {t("talk.historySharing")}
        </label>
        <p id={ids.historySharingHint} className="wizard-muted wizard-small wizard-indent">
          {t("talk.historySharingHint")}
        </p>
      </div>
      {ingameOverlaySupported && (
        <div>
          <label className="wizard-radio">
            <input
              type="checkbox"
              checked={ingameOverlay}
              aria-describedby={ids.ingameHint}
              onChange={(event) => {
                setIngameOverlay(event.target.checked);
              }}
            />
            {t("talk.ingameOverlay")}
          </label>
          <p id={ids.ingameHint} className="wizard-muted wizard-small wizard-indent">
            {t("talk.ingameOverlayHint")}
          </p>
          <img
            className="wizard-overlay-example wizard-indent"
            src={ingameOverlayExample}
            alt={t("talk.ingameOverlayAlt")}
            width={360}
            height={210}
          />
          <p className="wizard-note wizard-indent">{t("talk.ingameOverlayWarning")}</p>
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
