import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  getApi,
  type PttAvailability,
  type PttMouseStatus,
  type PttSession,
  type Settings,
} from "../../../api";
import { RadioGroup } from "../components/RadioGroup";
import { Group } from "../components/Section";
import { BitrateSlider } from "./BitrateSlider";
import { CompatibilityModeSwitch } from "./CompatibilityModeSwitch";
import { LowLatencyGroup } from "./LowLatencyGroup";
import { Switch } from "../components/Switch";
import { ThresholdMeter } from "../components/ThresholdMeter";
import { NoiseSuppressionRadios } from "./NoiseSuppressionRadios";
import { PttKeyCapture } from "./PttKeyCapture";
import type { SettingsSectionProps } from "./types";

/** The command that adds the user to the `input` group (FR-1.21, AC-1.37). */
const INPUT_GROUP_COMMAND = "sudo usermod -aG input $USER";

/** Whether mouse buttons may be captured: natively, or on Wayland after opting in (QA.4). */
function mouseAllowed(status: PttMouseStatus | null, settings: Settings): boolean {
  return (
    status?.support === "native" || (status?.support === "evdev" && settings.allowMouseButtons)
  );
}

/** Voice activation or push-to-talk with key capture, and the Linux input options (FR-1.21). */
export function TalkModeCard({ settings, update }: SettingsSectionProps) {
  const { t } = useTranslation("settings");
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
    <Group title={t("voice.talkMode")} icon="waveform">
      <RadioGroup
        legend={t("voice.talkModeLegend")}
        value={settings.talkMode}
        options={[
          { value: "voiceActivation", label: t("voice.voiceActivation"), recommended: true },
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
        <PttKeyCapture
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
  );
}

export function ConnectingCard({ settings, update }: SettingsSectionProps) {
  const { t } = useTranslation("settings");
  return (
    <Group title={t("voice.connecting")} icon="power">
      <Switch
        label={t("voice.connectMuted")}
        description={t("voice.connectMutedHint")}
        checked={settings.connectMuted}
        onChange={(connectMuted) => {
          update({ connectMuted });
        }}
      />
    </Group>
  );
}

/** Voice processing switches plus the quality options: music quality, low latency, bitrate. */
export function ProcessingCard({ settings, update }: SettingsSectionProps) {
  const { t } = useTranslation("settings");
  return (
    <Group title={t("voice.processing")} icon="sliders">
      <NoiseSuppressionRadios settings={settings} update={update} />
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
      <Switch
        label={t("audio.musicQuality")}
        description={t("audio.musicQualityHint")}
        checked={settings.musicQuality}
        onChange={(musicQuality) => {
          update({ musicQuality });
        }}
      />
      <LowLatencyGroup settings={settings} update={update} />
      <BitrateSlider settings={settings} update={update} />
    </Group>
  );
}
