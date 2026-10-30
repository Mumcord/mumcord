import { useId, type Ref } from "react";

interface SwitchProps {
  label: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
  description?: string;
  disabled?: boolean;
  /** Short tag next to the label, e.g. "Beta". */
  badge?: string;
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
  inputRef,
}: SwitchProps) {
  const id = useId();
  const descriptionId = `${id}-description`;
  return (
    <div className="settings-switch">
      <div className="settings-switch__text">
        <label htmlFor={id}>{label}</label>
        {badge !== undefined && <span className="settings-switch__badge">{badge}</span>}
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
