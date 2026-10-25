import { useId } from "react";
import { useTranslation } from "react-i18next";
import type { OverlayCorner } from "../../../api";

const OVERLAY_CORNERS: OverlayCorner[] = ["topLeft", "topRight", "bottomLeft", "bottomRight"];

/** Slider with a visible value, shared by the window and per-game overlay options. */
export function SliderField({
  label,
  value,
  min,
  max,
  step,
  unit,
  onChange,
}: {
  label: string;
  value: number;
  min: number;
  max: number;
  step: number;
  unit: string;
  onChange: (value: number) => void;
}) {
  const id = useId();
  return (
    <div className="settings-field overlay-settings__slider">
      <label htmlFor={id}>{label}</label>
      <input
        id={id}
        type="range"
        min={min}
        max={max}
        step={step}
        value={value}
        aria-valuetext={value.toString() + unit}
        onChange={(event) => {
          onChange(Number(event.target.value));
        }}
      />
      <output htmlFor={id}>
        {value}
        {unit}
      </output>
    </div>
  );
}

/** Corner picker: the four corners plus "custom" (a dragged place), which is display-only. */
export function CornerSelect({
  label,
  value,
  custom,
  disabled = false,
  onChange,
}: {
  label: string;
  value: OverlayCorner;
  custom: boolean;
  disabled?: boolean;
  onChange: (corner: OverlayCorner) => void;
}) {
  const { t } = useTranslation("settings");
  const id = useId();
  return (
    <div className="settings-field">
      <label htmlFor={id}>{label}</label>
      <select
        className="settings-input"
        id={id}
        value={custom ? "custom" : value}
        disabled={disabled}
        onChange={(event) => {
          const next = event.target.value;
          if (next !== "custom") onChange(next as OverlayCorner);
        }}
      >
        {custom && <option value="custom">{t("overlay.corner.custom")}</option>}
        {OVERLAY_CORNERS.map((corner) => (
          <option key={corner} value={corner}>
            {t(`overlay.corner.${corner}`)}
          </option>
        ))}
      </select>
    </div>
  );
}
