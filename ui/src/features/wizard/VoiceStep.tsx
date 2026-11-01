import { useId, useState } from "react";
import { useTranslation } from "react-i18next";
import type { PttAvailability, PttMouseStatus, Settings, TalkMode } from "../../api";
import { InfoTip } from "../../components/InfoTip";
import { RadioGroup } from "../settings/components/RadioGroup";
import { ThresholdMeter } from "../settings/components/ThresholdMeter";
import { NoiseSuppressionRadios } from "../settings/sections/NoiseSuppressionRadios";
import { PttKeyCapture } from "../settings/sections/PttKeyCapture";
import type { UpdateSettings } from "../settings/useSettings";
import { MicPanel } from "./MicPanel";
import { StepFrame } from "./StepFrame";

interface VoiceStepProps {
  settings: Settings;
  update: UpdateSettings;
  /** `null` when unknown; the key capture then reports any problem itself. */
  availability: PttAvailability | null;
  /** Mouse-button support (FR-1.21); `null` when unknown, then only keys can be captured. */
  mouseStatus: PttMouseStatus | null;
  onNext: () => void;
}

const TALK_MODES: readonly TalkMode[] = ["voiceActivation", "pushToTalk"];

/**
 * "Sound good?": devices with the live test, the noise suppression level (#292) and the talk
 * mode with its threshold or push-to-talk key. Uses the same controls and keys as Settings → Voice.
 */
export function VoiceStep({ settings, update, availability, mouseStatus, onNext }: VoiceStepProps) {
  const { t } = useTranslation("wizard");
  const { t: ts } = useTranslation("settings");
  const name = useId();
  const [keyMissing, setKeyMissing] = useState(false);
  const unavailable = availability?.status === "unavailable" ? availability.reason : null;
  // Wayland reads mice only after the user opts in under Settings → Voice (QA.4).
  const mouseAllowed =
    mouseStatus?.support === "native" ||
    (mouseStatus?.support === "evdev" && settings.allowMouseButtons);
  const ptt = settings.talkMode === "pushToTalk";

  function submit() {
    if (ptt && settings.pttKey === null) {
      setKeyMissing(true);
      return;
    }
    onNext();
  }

  return (
    <StepFrame title={t("voice.title")} subtitle={t("voice.subtitle")} onSubmit={submit}>
      <div className="wizard-columns">
        <div className="wizard-column">
          <MicPanel settings={settings} onDevicesChange={update} />
          <section className="wizard-card" aria-label={ts("voice.noiseSuppression")}>
            <NoiseSuppressionRadios settings={settings} update={update} />
          </section>
        </div>
        <div className="wizard-column">
          <fieldset className="wizard-options">
            <legend className="wizard-section-title">{t("voice.talkLegend")}</legend>
            {TALK_MODES.map((mode) => {
              const selected = settings.talkMode === mode;
              const disabled = mode === "pushToTalk" && unavailable !== null && !mouseAllowed;
              return (
                <div key={mode} className="wizard-option" data-selected={selected}>
                  <label className="wizard-option__label">
                    <input
                      type="radio"
                      name={name}
                      value={mode}
                      checked={selected}
                      disabled={disabled}
                      onChange={() => {
                        setKeyMissing(false);
                        update({ talkMode: mode });
                      }}
                    />
                    <span className="wizard-option__title">{ts(`voice.${mode}`)}</span>
                    {mode === "voiceActivation" && (
                      <>
                        {" "}
                        <span className="wizard-badge wizard-badge--recommended">
                          {t("recommended")}
                        </span>
                      </>
                    )}
                  </label>
                  <InfoTip
                    label={t("voice.tipLabel", { mode: ts(`voice.${mode}`) })}
                    text={t(`voice.${mode}Hint`)}
                  />
                  {selected && mode === "voiceActivation" && (
                    <div className="wizard-option__extra">
                      <RadioGroup
                        legend={ts("voice.sensitivityLegend")}
                        value={settings.voiceActivation}
                        options={[
                          { value: "automatic", label: ts("voice.automatic") },
                          { value: "manual", label: ts("voice.manual") },
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
                    </div>
                  )}
                  {selected && mode === "pushToTalk" && (
                    <div className="wizard-option__extra">
                      <PttKeyCapture
                        current={settings.pttKey}
                        mouseAllowed={mouseAllowed}
                        onCaptured={(pttKey) => {
                          setKeyMissing(false);
                          update({ pttKey });
                        }}
                      />
                    </div>
                  )}
                </div>
              );
            })}
          </fieldset>
          {unavailable !== null && (
            <p className="wizard-note">{ts(`voice.unavailable.${unavailable}`)}</p>
          )}
          {keyMissing && (
            <p role="alert" className="wizard-error">
              {t("voice.keyRequired")}
            </p>
          )}
          <p className="wizard-note">{t("voice.notSure")}</p>
        </div>
      </div>
    </StepFrame>
  );
}
