import { useId } from "react";
import { useTranslation } from "react-i18next";
import type { PluginConfirmKind, PluginGateReason } from "../../../api";
import { highRiskSentences, type HighRiskFacts } from "./highRisk";

export interface ConfirmBoxProps {
  confirm: PluginConfirmKind;
  reasons: readonly PluginGateReason[];
  pluginName: string;
  /** The third-party repository the plugin comes from, if any. */
  repository: string | null;
  highRisk: HighRiskFacts;
  checked: boolean;
  disabled: boolean;
  onChange: (checked: boolean) => void;
}

/**
 * The one confirmation box of the install prompt (FR-9.80): High risk, own code and websites and
 * the first-install repository warning merge into one box with one tick.
 */
export function ConfirmBox({
  confirm,
  reasons,
  pluginName,
  repository,
  highRisk,
  checked,
  disabled,
  onChange,
}: ConfirmBoxProps) {
  const { t } = useTranslation("pluginGate");
  const { t: tBrowse } = useTranslation("pluginBrowse");
  const titleId = useId();
  const ownCode = reasons.includes("ownCodeAndWebsites");
  // The engine decides whether the first-install warning (FR-6.61) still applies.
  const repositoryText =
    repository === null || !reasons.includes("repositoryWarning")
      ? null
      : t("trustRepository", { repository });
  const high = confirm === "highRisk";

  let title: string;
  let text: string[];
  if (high) {
    title = t("highRiskTitle");
    // The high-risk wording covers the merged reasons too (score mockup 03): one text, one tick.
    text = highRiskSentences(t, highRisk);
  } else {
    title = ownCode ? t("ownCodeAndWebsites") : tBrowse("prompt.warningTitle");
    text = repositoryText === null ? [] : [repositoryText];
  }

  return (
    <div
      className={`plugin-prompt-confirm${high ? " plugin-prompt-confirm--danger" : ""}`}
      role="group"
      aria-labelledby={titleId}
    >
      {high ? (
        <>
          <strong id={titleId}>{title}</strong>
          {text.length > 0 && <p>{text.join(" ")}</p>}
        </>
      ) : (
        <p>
          <strong id={titleId}>{title}</strong>
          {text.length > 0 && ` ${text.join(" ")}`}
        </p>
      )}
      <label className="plugin-prompt-check">
        <input
          type="checkbox"
          checked={checked}
          disabled={disabled}
          onChange={(event) => {
            onChange(event.target.checked);
          }}
        />
        {high ? t("trustAuthor", { plugin: pluginName }) : t("understand")}
      </label>
    </div>
  );
}
