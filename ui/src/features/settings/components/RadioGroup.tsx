import { useId } from "react";
import { InfoTip } from "../../../components/InfoTip";

interface RadioOption<T extends string> {
  value: T;
  label: string;
  disabled?: boolean;
  /** Language of the label when it differs from the UI, so screen readers pronounce it right. */
  lang?: string;
  /** Small marker after the label, such as "Beta". */
  badge?: string | undefined;
  /** A "?" next to the option that explains it. */
  tip?: { label: string; text: string } | undefined;
}

interface RadioGroupProps<T extends string> {
  legend: string;
  value: T;
  options: RadioOption<T>[];
  onChange: (value: T) => void;
}

/** Native radio buttons in a fieldset (arrow keys move between options). */
export function RadioGroup<T extends string>({
  legend,
  value,
  options,
  onChange,
}: RadioGroupProps<T>) {
  const name = useId();
  return (
    <fieldset className="settings-radios">
      <legend>{legend}</legend>
      {options.map((option) => (
        <div key={option.value} className="settings-radios__row">
          <label className="settings-radios__option" lang={option.lang}>
            <input
              type="radio"
              name={name}
              value={option.value}
              checked={option.value === value}
              disabled={option.disabled ?? false}
              onChange={() => {
                onChange(option.value);
              }}
            />
            {option.label}
            {option.badge !== undefined && (
              <span className="settings-switch__badge">{option.badge}</span>
            )}
          </label>
          {/* Outside the label, so the "?" is its own control and does not pick the option; to the
              right, so the tip does not cover the neighbouring options. */}
          {option.tip !== undefined && (
            <InfoTip label={option.tip.label} text={option.tip.text} placement="right" />
          )}
        </div>
      ))}
    </fieldset>
  );
}
