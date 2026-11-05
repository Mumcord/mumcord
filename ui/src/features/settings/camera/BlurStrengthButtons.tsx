import { useId } from "react";
import { useTranslation } from "react-i18next";
import { BLUR_LIGHT, BLUR_STRONG, isStrongBlur } from "./blurStrength";

/** Blur strength as two buttons, Light and Strong (mockup 11), each a fixed engine strength. */
export function BlurStrengthButtons({
  value,
  onChange,
}: {
  value: number;
  onChange: (value: number) => void;
}) {
  const { t } = useTranslation("settings");
  const id = useId();
  const strong = isStrongBlur(value);
  const choices = [
    { strength: BLUR_LIGHT, pressed: !strong, key: "camera.blurLight" },
    { strength: BLUR_STRONG, pressed: strong, key: "camera.blurStrong" },
  ] as const;
  return (
    <div className="settings-control-row">
      <span id={id}>{t("camera.blurStrength")}</span>
      <span className="settings-camera-seg" role="group" aria-labelledby={id}>
        {choices.map((choice) => (
          <button
            key={choice.key}
            type="button"
            className="settings-camera-seg__button"
            aria-pressed={choice.pressed}
            onClick={() => {
              if (!choice.pressed) onChange(choice.strength);
            }}
          >
            {t(choice.key)}
          </button>
        ))}
      </span>
    </div>
  );
}
