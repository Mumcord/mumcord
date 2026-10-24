import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";
import { getApi, toEngineError, type PttAvailability } from "../../../api";
import { RadioGroup } from "../components/RadioGroup";
import { Group, Section } from "../components/Section";
import { Switch } from "../components/Switch";
import { errorText } from "../errors";
import { keyCombo } from "../pttKey";
import type { SettingsSectionProps } from "./types";

/** Voice: talk mode with push-to-talk key capture, and the voice processing switches. */
export function VoiceSection({ settings, update }: SettingsSectionProps) {
  const { t } = useTranslation("settings");
  const [availability, setAvailability] = useState<PttAvailability | null>(null);

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

  const unavailable = availability?.status === "unavailable" ? availability.reason : null;
  const ptt = settings.talkMode === "pushToTalk";

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
              disabled: unavailable !== null && !ptt,
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
        {ptt && (
          <KeyCapture
            current={settings.pttKey}
            onCaptured={(pttKey) => {
              update({ pttKey });
            }}
          />
        )}
      </Group>
      <Group title={t("voice.processing")}>
        <Switch
          label={t("voice.noiseSuppression")}
          description={t("voice.noiseSuppressionHint")}
          checked={settings.noiseSuppression}
          onChange={(noiseSuppression) => {
            update({ noiseSuppression });
          }}
        />
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
  onCaptured: (key: string) => void;
}

/**
 * Push-to-talk key capture: the button listens for the next key (with modifiers) and hands it
 * to the engine, which validates it or, on Wayland, opens the desktop's shortcut dialog.
 */
function KeyCapture({ current, onCaptured }: KeyCaptureProps) {
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

  const label =
    state === "listening"
      ? t("voice.keyListening")
      : state === "waiting"
        ? t("voice.keyWaiting")
        : current === null
          ? t("voice.keySet")
          : t("voice.keyChange");

  return (
    <div className="settings-field-row">
      <p className="settings-value">
        {t("voice.keyCurrent")} <kbd className="settings-kbd">{current ?? t("voice.keyNone")}</kbd>
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
