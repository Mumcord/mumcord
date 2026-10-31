import { useId, type ReactNode, type Ref } from "react";
import { useTranslation } from "react-i18next";
import { InfoTip } from "../../../components/InfoTip";

interface SwitchProps {
  label: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
  /** Explains the switch behind a "?" next to the label, so the rows stay one line high. */
  description?: string;
  /** Accessible name of the "?" button; defaults to "More information: <label>". */
  descriptionLabel?: string;
  /** Current state worth seeing at a glance (e.g. "Not compatible"), shown under the label. */
  status?: string;
  disabled?: boolean;
  /** Tag next to the label: a short text, or a ready badge such as `<BetaBadge />`. */
  badge?: ReactNode;
  /** Lets the caller focus the switch, e.g. when another section links to it. */
  inputRef?: Ref<HTMLInputElement>;
}

/** On/off toggle: a checkbox exposed as a switch so screen readers say "on"/"off". */
export function Switch({
  label,
  checked,
  onChange,
  description,
  descriptionLabel,
  status,
  disabled = false,
  badge,
  inputRef,
}: SwitchProps) {
  const { t } = useTranslation("settings");
  const id = useId();
  const statusId = `${id}-status`;
  return (
    <div className="settings-switch">
      <div className="settings-switch__text">
        <label htmlFor={id}>{label}</label>
        {typeof badge === "string" ? (
          <span className="settings-switch__badge">{badge}</span>
        ) : (
          badge
        )}
        {description !== undefined && (
          <InfoTip label={descriptionLabel ?? t("moreInfo", { topic: label })} text={description} />
        )}
        {status !== undefined && (
          <p id={statusId} className="settings-switch__status settings-hint">
            {status}
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
        aria-describedby={status !== undefined ? statusId : undefined}
        onChange={(event) => {
          onChange(event.target.checked);
        }}
      />
    </div>
  );
}
