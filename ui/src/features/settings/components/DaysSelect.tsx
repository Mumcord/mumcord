import { useId } from "react";
import { useTranslation } from "react-i18next";

interface DaysSelectProps {
  label: string;
  /** Number of days, or `null` for "no limit". */
  value: number | null;
  /** Day counts offered besides "no limit". */
  options: readonly number[];
  /** Text of the `null` choice. */
  noneLabel: string;
  onChange: (days: number | null) => void;
}

const NONE = "none";

/** "Delete … after X days" choice; a stored value outside the presets stays selectable. */
export function DaysSelect({ label, value, options, noneLabel, onChange }: DaysSelectProps) {
  const { t } = useTranslation("settings");
  const id = useId();
  const days =
    value !== null && !options.includes(value)
      ? [...options, value].sort((a, b) => a - b)
      : options;
  return (
    <div className="settings-field-row">
      <label className="settings-field" htmlFor={id}>
        {label}
      </label>
      <select
        id={id}
        className="settings-input"
        value={value === null ? NONE : String(value)}
        onChange={(event) => {
          onChange(event.target.value === NONE ? null : Number(event.target.value));
        }}
      >
        {days.map((count) => (
          <option key={count} value={count}>
            {t("retention.days", { count })}
          </option>
        ))}
        <option value={NONE}>{noneLabel}</option>
      </select>
    </div>
  );
}
