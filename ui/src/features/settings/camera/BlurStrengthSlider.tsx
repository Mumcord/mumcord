import { useId, useState } from "react";
import { useTranslation } from "react-i18next";

/** Blur strength 0 (Light) … 100 (Strong) (FR-8.11); saved when the user lets go. */
export function BlurStrengthSlider({
  value,
  onChange,
}: {
  value: number;
  onChange: (value: number) => void;
}) {
  const { t } = useTranslation("settings");
  const id = useId();
  const [dragging, setDragging] = useState<number | null>(null);
  const shown = dragging ?? value;

  const commit = () => {
    if (dragging !== null && dragging !== value) onChange(dragging);
    setDragging(null);
  };

  return (
    <div className="settings-control-row">
      <label htmlFor={id}>{t("camera.blurStrength")}</label>
      <span className="settings-camera-blur">
        <span aria-hidden="true">{t("camera.blurLight")}</span>
        <input
          id={id}
          type="range"
          min={0}
          max={100}
          step={5}
          value={shown}
          aria-valuetext={t("camera.blurValue", { percent: shown })}
          onChange={(event) => {
            setDragging(Number(event.target.value));
          }}
          onPointerUp={commit}
          onKeyUp={commit}
          onBlur={commit}
        />
        <span aria-hidden="true">{t("camera.blurStrong")}</span>
      </span>
    </div>
  );
}
