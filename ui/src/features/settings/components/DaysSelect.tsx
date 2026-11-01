import { useId } from "react";
import { useTranslation } from "react-i18next";
import { InfoTip } from "../../../components/InfoTip";
import { Select } from "../../../components/Select";

interface DaysSelectProps {
  label: string;
  /** Explains the choice behind a "?" next to the label. */
  description?: string;
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
export function DaysSelect({
  label,
  description,
  value,
  options,
  noneLabel,
  onChange,
}: DaysSelectProps) {
  const { t } = useTranslation("settings");
  const id = useId();
  const days =
    value !== null && !options.includes(value)
      ? [...options, value].sort((a, b) => a - b)
      : options;
  return (
    <div className="settings-control-row">
      <span className="settings-control-row__label">
        <label htmlFor={id}>{label}</label>
        {description !== undefined && (
          <InfoTip label={t("moreInfo", { topic: label })} text={description} />
        )}
      </span>
      <Select
        id={id}
        value={value === null ? NONE : String(value)}
        options={[
          ...days.map((count) => ({
            value: String(count),
            label: t("retention.days", { count }),
          })),
          { value: NONE, label: noneLabel },
        ]}
        onChange={(next) => {
          onChange(next === NONE ? null : Number(next));
        }}
      />
    </div>
  );
}
