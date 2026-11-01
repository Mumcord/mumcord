import { useTranslation } from "react-i18next";
import type { PrivateMessageSettings } from "../../api";
import { Switch } from "../settings/components/Switch";
import { PmRuleCards } from "../settings/sections/PrivateMessagesGroup";
import type { SettingsSectionProps } from "../settings/sections/types";
import { StepFrame } from "./StepFrame";

/**
 * "Who can message you?" (Custom only): the rule cards and the exceptions from Settings →
 * Private messages (#141), with the same keys. Per-server rules stay in Settings.
 */
export function PrivateMessagesStep({
  settings,
  update,
  onNext,
}: SettingsSectionProps & { onNext: () => void }) {
  const { t } = useTranslation("wizard");
  const { t: ts } = useTranslation("settings");
  const pm = settings.privateMessages;
  const change = (patch: Partial<PrivateMessageSettings>) => {
    update({ privateMessages: { ...pm, ...patch } });
  };
  const toggle = (key: "allowCurrentChannel" | "allowAccepted" | "keepRequests", label: string) => (
    <Switch
      label={ts(`privateMessages.${label}`)}
      tip={{
        label: t("tipLabel", { name: ts(`privateMessages.${label}`) }),
        text: ts(`privateMessages.${label}Hint`),
      }}
      checked={pm[key]}
      onChange={(on) => {
        change({ [key]: on });
      }}
    />
  );
  return (
    <StepFrame title={t("pm.title")} subtitle={t("pm.subtitle")} onSubmit={onNext}>
      <PmRuleCards
        value={pm.rule}
        onChange={(rule) => {
          change({ rule });
        }}
      />
      <section className="wizard-card wizard-rows" aria-label={ts("privateMessages.alwaysTitle")}>
        {toggle("allowCurrentChannel", "currentChannel")}
        {toggle("allowAccepted", "accepted")}
        {toggle("keepRequests", "keepRequests")}
      </section>
      <p className="wizard-note">{t("pm.local")}</p>
    </StepFrame>
  );
}
