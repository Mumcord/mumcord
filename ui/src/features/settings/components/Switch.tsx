import { useId } from "react";

interface SwitchProps {
  label: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
  description?: string;
  disabled?: boolean;
}

/** On/off toggle: a checkbox exposed as a switch so screen readers say "on"/"off". */
export function Switch({ label, checked, onChange, description, disabled = false }: SwitchProps) {
  const id = useId();
  const descriptionId = `${id}-description`;
  return (
    <div className="settings-switch">
      <div className="settings-switch__text">
        <label htmlFor={id}>{label}</label>
        {description !== undefined && (
          <p id={descriptionId} className="settings-hint">
            {description}
          </p>
        )}
      </div>
      <input
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
