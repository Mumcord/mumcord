import { useId, type ReactNode, type Ref } from "react";
import { useTranslation } from "react-i18next";
import { InfoTip } from "../../../components/InfoTip";
import { ChangedBadge, RecommendedBadge } from "./SettingBadges";

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
  /** Shows the green "Recommended" pill: this switch's recommended choice is "on" (#159). */
  recommended?: boolean;
  /** Lets the caller focus the switch, e.g. when another section links to it. */
  inputRef?: Ref<HTMLInputElement>;
}

/**
 * On/off toggle: a checkbox exposed as a switch so screen readers say "on"/"off". The badges
 * ("Beta", "Recommended", "Changed") are part of its accessible name (#161).
 */
export function Switch({
  label,
  checked,
  onChange,
  description,
  descriptionLabel,
  status,
  disabled = false,
  badge,
  recommended = false,
  inputRef,
}: SwitchProps) {
  const { t } = useTranslation("settings");
  const id = useId();
  const statusId = `${id}-status`;
  const labelId = `${id}-label`;
  const badgesId = `${id}-badges`;
  return (
    <div className="settings-switch">
      <div className="settings-switch__text">
        <label id={labelId} htmlFor={id}>
          {label}
        </label>
        <span id={badgesId} className="setting-badges">
          {typeof badge === "string" ? (
            <span className="settings-switch__badge">{badge}</span>
          ) : (
            badge
          )}
          {/* Spaces keep the badges apart in the accessible name; the flex layout drops them. */}{" "}
          {recommended && <RecommendedBadge />} <ChangedBadge label={label} />
        </span>
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
        aria-labelledby={`${labelId} ${badgesId}`}
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
