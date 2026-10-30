import { useId, type Ref } from "react";
import { InfoTip } from "../../../components/InfoTip";

interface SwitchProps {
  label: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
  description?: string;
  disabled?: boolean;
  /** Short tag next to the label, e.g. "Beta". */
  badge?: string;
  /** Explains the switch behind a "?" button instead of a line of text. */
  tip?: { label: string; text: string };
  /** Lets the caller focus the switch, e.g. when another section links to it. */
  inputRef?: Ref<HTMLInputElement>;
}

/** On/off toggle: a checkbox exposed as a switch so screen readers say "on"/"off". */
export function Switch({
  label,
  checked,
  onChange,
  description,
  disabled = false,
  badge,
  tip,
  inputRef,
}: SwitchProps) {
  const id = useId();
  const descriptionId = `${id}-description`;
  return (
    <div className="settings-switch">
      <div className="settings-switch__text">
        <label htmlFor={id}>{label}</label>
        {badge !== undefined && <span className="settings-switch__badge">{badge}</span>}
        {tip !== undefined && <InfoTip label={tip.label} text={tip.text} />}
        {description !== undefined && (
          <p id={descriptionId} className="settings-hint">
            {description}
          </p>
        )}
      </div>
      <input
        ref={inputRef}
        id={id}
        type="checkbox"
        role="switch"
        className="settings-switch__input"
        checked={checked}
        disabled={disabled}
        aria-describedby={description !== undefined ? descriptionId : undefined}
        onChange={(event) => {
          onChange(event.target.checked);
        }}
      />
    </div>
  );
}
