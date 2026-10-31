import { useEffect, useRef, useState, type KeyboardEvent, type MouseEvent } from "react";
import { useTranslation } from "react-i18next";
import {
  getApi,
  toEngineError,
  type NoiseBackend,
  type PttAvailability,
  type PttMouseStatus,
  type PttSession,
  type Settings,
} from "../../../api";
import { RadioGroup } from "../components/RadioGroup";
import { Group, Section } from "../components/Section";
import { CompatibilityModeSwitch } from "./CompatibilityModeSwitch";
import { Switch } from "../components/Switch";
import { ThresholdMeter } from "../components/ThresholdMeter";
import { errorText } from "../errors";
import { keyCombo, mouseKey, mouseLabel } from "../pttKey";
import type { SettingsSectionProps } from "./types";

/** The command that adds the user to the `input` group (FR-1.21, AC-1.37). */
const INPUT_GROUP_COMMAND = "sudo usermod -aG input $USER";

/** The noise suppression choices of FR-3.65 (Q1.13), weakest first: one of the backends, or off. */
type NoiseChoice = NoiseBackend | "off";
const NOISE_CHOICES: NoiseChoice[] = ["light", "standard", "advanced", "off"];

/** Whether mouse buttons may be captured: natively, or on Wayland after opting in (QA.4). */
function mouseAllowed(status: PttMouseStatus | null, settings: Settings): boolean {
  return (
    status?.support === "native" || (status?.support === "evdev" && settings.allowMouseButtons)
  );
}

/** Voice: talk mode with push-to-talk key capture, and the voice processing switches. */
export function VoiceSection({ settings, update }: SettingsSectionProps) {
  const { t } = useTranslation("settings");
  const noiseChoice: NoiseChoice = settings.noiseSuppression
    ? settings.noiseSuppressionBackend
    : "off";
  const [availability, setAvailability] = useState<PttAvailability | null>(null);
  const [mouseStatus, setMouseStatus] = useState<PttMouseStatus | null>(null);
  const [session, setSession] = useState<PttSession | null>(null);

  useEffect(() => {
    let active = true;
    getApi()
      .pttSession()
      .then(
        (result) => {
          if (active) setSession(result);
        },
        (error: unknown) => {
          console.error("Checking the desktop session failed", error);
        },
      );
    return () => {
      active = false;
    };
  }, []);

  useEffect(() => {
    let active = true;
    getApi()
      .pttAvailability()
      .then(
        (result) => {
          if (active) setAvailability(result);
        },
        (error: unknown) => {
          console.error("Checking push-to-talk availability failed", error);
        },
      );
    return () => {
      active = false;
    };
  }, []);

  // Re-checked when the opt-in or the key changes (the macOS permission shows up on use).
  useEffect(() => {
    let active = true;
    getApi()
      .pttMouseStatus()
      .then(
        (result) => {
          if (active) setMouseStatus(result);
        },
        (error: unknown) => {
          console.error("Checking mouse push-to-talk support failed", error);
        },
      );
    return () => {
      active = false;
    };
  }, [settings.allowMouseButtons, settings.pttKey]);

  const unavailable = availability?.status === "unavailable" ? availability.reason : null;
  const ptt = settings.talkMode === "pushToTalk";
  const mouse = mouseAllowed(mouseStatus, settings);
  // In compatibility mode the X11 path handles mouse buttons; the evdev opt-in does not apply.
  const compatibilityActive = session?.compatibilityActive === true;

  return (
    <Section title={t("sections.voice")}>
      <Group title={t("voice.talkMode")}>
        <RadioGroup
          legend={t("voice.talkModeLegend")}
          value={settings.talkMode}
          options={[
            { value: "voiceActivation", label: t("voice.voiceActivation") },
            // Keep the option usable if it is already selected, so the user can still see it.
            {
              value: "pushToTalk",
              label: t("voice.pushToTalk"),
              disabled: unavailable !== null && !ptt && !mouse,
            },
          ]}
          onChange={(talkMode) => {
            update({ talkMode });
          }}
        />
        {unavailable !== null && (
          <p className="settings-warning">
            {t(`voice.unavailable.${unavailable}`)} {t("voice.suggestVoiceActivation")}
          </p>
        )}
        {!ptt && (
          <>
            <RadioGroup
              legend={t("voice.sensitivityLegend")}
              value={settings.voiceActivation}
              options={[
                { value: "automatic", label: t("voice.automatic") },
                { value: "manual", label: t("voice.manual") },
              ]}
              onChange={(voiceActivation) => {
                update({ voiceActivation });
              }}
            />
            {settings.voiceActivation === "manual" && (
              <ThresholdMeter
                value={settings.voiceThresholdDbfs}
                onChange={(voiceThresholdDbfs) => {
                  update({ voiceThresholdDbfs });
                }}
              />
            )}
          </>
        )}
        {ptt && (
          <KeyCapture
            current={settings.pttKey}
            mouseAllowed={mouse}
            onCaptured={(pttKey) => {
              update({ pttKey });
            }}
          />
        )}
        {session?.waylandDesktop === true && (
          <CompatibilityModeSwitch session={session} settings={settings} update={update} />
        )}
        {mouseStatus?.support === "evdev" && !compatibilityActive && (
          <>
            <Switch
              label={t("voice.mouseToggle")}
              description={t("voice.mouseToggleWarning")}
              checked={settings.allowMouseButtons}
              onChange={(allowMouseButtons) => {
                update({ allowMouseButtons });
              }}
            />
            {settings.allowMouseButtons && mouseStatus.state === "inputGroupMissing" && (
              <div className="settings-warning" role="status">
                <p>{t("voice.mouseInputGroup")}</p>
                <p>
                  <code className="settings-kbd">{INPUT_GROUP_COMMAND}</code>
                </p>
                <p>{t("voice.mouseLogOut")}</p>
              </div>
            )}
            {settings.allowMouseButtons && mouseStatus.state === "noDevice" && (
              <p className="settings-hint">{t("voice.mouseNoDevice")}</p>
            )}
          </>
        )}
        {mouseStatus?.state === "accessibilityMissing" && (
          <p className="settings-warning">{t("voice.mouseAccessibility")}</p>
        )}
      </Group>
      <Group title={t("voice.connecting")}>
        <Switch
          label={t("voice.connectMuted")}
          description={t("voice.connectMutedHint")}
          checked={settings.connectMuted}
          onChange={(connectMuted) => {
            update({ connectMuted });
          }}
        />
      </Group>
      <Group title={t("voice.processing")}>
        <RadioGroup
          legend={t("voice.noiseSuppression")}
          value={noiseChoice}
          options={NOISE_CHOICES.map((choice) => ({
            value: choice,
            label: t(`voice.noiseChoice.${choice}`),
            badge: choice === "advanced" ? t("voice.noiseChoiceBeta") : undefined,
            tip:
              choice === "off"
                ? undefined
                : {
                    label: t("voice.noiseChoiceTipLabel", {
                      choice: t(`voice.noiseChoice.${choice}`),
                    }),
                    text: t(`voice.noiseChoiceHint.${choice}`),
                  },
          }))}
          onChange={(choice) => {
            if (choice === "off") update({ noiseSuppression: false });
            else update({ noiseSuppression: true, noiseSuppressionBackend: choice });
          }}
        />
        <p className="settings-hint">{t("voice.noiseSuppressionHint")}</p>
        {settings.talkMode === "voiceActivation" &&
          (settings.noiseSuppressionBackend === "light" || !settings.noiseSuppression) && (
            <p className="settings-hint">{t("voice.detectorHint")}</p>
          )}
        <Switch
          label={t("voice.echoCancellation")}
          description={t("voice.echoCancellationHint")}
          checked={settings.echoCancellation}
          onChange={(echoCancellation) => {
            update({ echoCancellation });
          }}
        />
        <Switch
          label={t("voice.autoGain")}
          description={t("voice.autoGainHint")}
          checked={settings.autoGain}
          onChange={(autoGain) => {
            update({ autoGain });
          }}
        />
      </Group>
    </Section>
  );
}

type CaptureState = "idle" | "listening" | "waiting";

interface KeyCaptureProps {
  current: string | null;
  /** Also accept the middle and side mouse buttons while listening. */
  mouseAllowed: boolean;
  onCaptured: (key: string) => void;
}

/**
 * Push-to-talk key capture: the button listens for the next key (with modifiers) and hands it
 * to the engine, which validates it or, on Wayland, opens the desktop's shortcut dialog.
 */
function KeyCapture({ current, mouseAllowed, onCaptured }: KeyCaptureProps) {
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
