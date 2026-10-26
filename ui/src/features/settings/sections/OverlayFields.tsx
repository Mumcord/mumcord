import { useId } from "react";
import { useTranslation } from "react-i18next";
import type { Corner } from "../../../api";
import { Select } from "../../../components/Select";

const OVERLAY_CORNERS: Corner[] = ["topLeft", "topRight", "bottomLeft", "bottomRight"];

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
  value: Corner;
  custom: boolean;
  disabled?: boolean;
  onChange: (corner: Corner) => void;
}) {
  const { t } = useTranslation("settings");
  const id = useId();
  return (
    <div className="settings-control-row">
      <label htmlFor={id}>{label}</label>
      <Select
        id={id}
        value={custom ? "custom" : value}
        disabled={disabled}
        options={[
          ...(custom ? [{ value: "custom", label: t("overlay.corner.custom") }] : []),
          ...OVERLAY_CORNERS.map((corner) => ({
            value: corner,
            label: t(`overlay.corner.${corner}`),
          })),
        ]}
        onChange={(next) => {
          if (next !== "custom") onChange(next as Corner);
        }}
      />
    </div>
  );
}
