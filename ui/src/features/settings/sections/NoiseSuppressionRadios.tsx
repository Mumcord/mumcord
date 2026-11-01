import { useTranslation } from "react-i18next";
import type { NoiseBackend } from "../../../api";
import { RadioGroup } from "../components/RadioGroup";
import type { SettingsSectionProps } from "./types";

/** The noise suppression choices of FR-3.65 (Q1.13), weakest first: one of the backends, or off. */
type NoiseChoice = NoiseBackend | "off";
const NOISE_CHOICES: NoiseChoice[] = ["light", "standard", "advanced", "off"];

/** Noise suppression level, shared by Settings → Voice and the setup wizard's voice step. */
export function NoiseSuppressionRadios({ settings, update }: SettingsSectionProps) {
  const { t } = useTranslation("settings");
  const value: NoiseChoice = settings.noiseSuppression ? settings.noiseSuppressionBackend : "off";
  return (
    <RadioGroup
      legend={t("voice.noiseSuppression")}
      info={{
        label: t("moreInfo", { topic: t("voice.noiseSuppression") }),
        text: t("voice.noiseSuppressionHint"),
      }}
      value={value}
      options={NOISE_CHOICES.map((choice) => ({
        value: choice,
        label: t(`voice.noiseChoice.${choice}`),
        badge: choice === "advanced" ? t("voice.noiseChoiceBeta") : undefined,
        recommended: choice === "standard",
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
  );
}
