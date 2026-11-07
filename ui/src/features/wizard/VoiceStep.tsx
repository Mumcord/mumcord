import { useId } from "react";
import { useTranslation } from "react-i18next";
import type { PttAvailability, PttMouseStatus, Settings, TalkMode } from "../../api";
import { RadioGroup } from "../settings/components/RadioGroup";
import { RecommendedBadge } from "../settings/components/SettingBadges";
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
  const unavailable = availability?.status === "unavailable" ? availability.reason : null;
  // Wayland reads mice only after the user opts in under Settings → Voice (QA.4).
  const mouseAllowed =
    mouseStatus?.support === "native" ||
    (mouseStatus?.support === "evdev" && settings.allowMouseButtons);
  // Push-to-talk needs a key before the wizard can go on (#589): Continue stays unavailable while
  // the key capture in the card says why.
  const keyMissing = settings.talkMode === "pushToTalk" && settings.pttKey === null;

  return (
    <StepFrame
      title={t("voice.title")}
      subtitle={t("voice.subtitle")}
      blocked={keyMissing}
      onSubmit={onNext}
    >
      <div className="wizard-columns">
        <div className="wizard-column">
          <MicPanel settings={settings} onDevicesChange={update} />
          <section className="wizard-card" aria-label={ts("voice.noiseSuppression")}>
            <NoiseSuppressionRadios settings={settings} update={update} />
          </section>
        </div>
        <div className="wizard-column">
          {/* Option cards with the nested choice of the chosen mode (#589, #586 variant C). */}
          <fieldset className="wizard-options wizard-talk">
            <legend className="wizard-section-title">{t("voice.talkLegend")}</legend>
            {TALK_MODES.map((mode) => {
              const selected = settings.talkMode === mode;
              const disabled = mode === "pushToTalk" && unavailable !== null && !mouseAllowed;
              const inputId = `${name}-${mode}`;
              const descId = `${inputId}-desc`;
              return (
                <div key={mode} className="wizard-talk__card" data-selected={selected}>
                  <label className="wizard-talk__head" htmlFor={inputId}>
                    <input
                      id={inputId}
                      type="radio"
                      name={name}
                      value={mode}
                      checked={selected}
                      disabled={disabled}
                      aria-describedby={descId}
                      onChange={() => {
                        update({ talkMode: mode });
                      }}
                    />
                    <span className="wizard-talk__text">
                      <span className="wizard-option__title">{ts(`voice.${mode}`)}</span>
                      <span id={descId} className="wizard-talk__desc">
                        {t(`voice.${mode}Desc`)}
                      </span>
                    </span>
                    {mode === "voiceActivation" && <RecommendedBadge />}
                  </label>
                  {selected && mode === "voiceActivation" && (
                    <div className="wizard-talk__nested">
                      <RadioGroup
                        legend={t("voice.sensitivity")}
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
                    <div className="wizard-talk__nested wizard-talk__nested--plain">
                      {/* The capture names the key and warns while none is set. */}
                      <PttKeyCapture
                        current={settings.pttKey}
                        mouseAllowed={mouseAllowed}
                        onCaptured={(pttKey) => {
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
          <p className="wizard-note wizard-talk__later">{t("voice.changeLater")}</p>
        </div>
      </div>
    </StepFrame>
  );
}
