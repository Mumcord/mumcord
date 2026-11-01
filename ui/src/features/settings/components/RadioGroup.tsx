import { useId } from "react";
import { InfoTip } from "../../../components/InfoTip";
import { ChangedBadge, RecommendedBadge } from "./SettingBadges";

interface RadioOption<T extends string> {
  value: T;
  label: string;
  disabled?: boolean;
  /** Language of the label when it differs from the UI, so screen readers pronounce it right. */
  lang?: string;
  /** Small marker after the label, such as "Beta". */
  badge?: string | undefined;
  /** Shows the green "Recommended" pill after the label (#159). */
  recommended?: boolean | undefined;
  /** A "?" next to the option that explains it. */
  tip?: { label: string; text: string } | undefined;
}

interface RadioGroupProps<T extends string> {
  legend: string;
  /** Explains the whole choice behind a "?" next to the legend. */
  info?: { label: string; text: string } | undefined;
  value: T;
  options: RadioOption<T>[];
  onChange: (value: T) => void;
}

/** Native radio buttons in a fieldset (arrow keys move between options). */
export function RadioGroup<T extends string>({
  legend,
  info,
  value,
  options,
  onChange,
}: RadioGroupProps<T>) {
  const name = useId();
  return (
    // Named by the legend text and the "Changed" badge, so the "?" button does not end up in the
    // group's name. The badge sits outside the legend, which keeps the legend's text the label.
    <fieldset className="settings-radios" aria-labelledby={`${name}-legend ${name}-badges`}>
      <legend className="settings-radios__legend">
        <span id={`${name}-legend`}>{legend}</span>
        {info !== undefined && <InfoTip label={info.label} text={info.text} />}
      </legend>
      <span id={`${name}-badges`} className="setting-badges settings-radios__badges">
        <ChangedBadge label={legend} />
      </span>
      {options.map((option) => (
        <div key={option.value} className="settings-radios__row">
          <label
            id={`${name}-${option.value}`}
            className="settings-radios__option"
            lang={option.lang}
          >
            <input
              type="radio"
              name={name}
              value={option.value}
              aria-labelledby={
                option.recommended === true
                  ? `${name}-${option.value} ${name}-${option.value}-recommended`
                  : undefined
              }
              checked={option.value === value}
              disabled={option.disabled ?? false}
              onChange={() => {
                onChange(option.value);
              }}
            />
            {option.label}
            {/* The space keeps label and badge apart in the radio's accessible name. */}
            {option.badge !== undefined && (
              <>
                {" "}
                <span className="settings-switch__badge">{option.badge}</span>
              </>
            )}
          </label>
          {/* Outside the label, so the label text stays the option's name; the radio's name still
              includes it through aria-labelledby. */}
          {option.recommended === true && (
            <RecommendedBadge id={`${name}-${option.value}-recommended`} />
          )}
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
