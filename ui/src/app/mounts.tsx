/**
 * Mount points for feature UIs. Each feature task replaces the placeholder in its own mount
 * with its root component; the shell layout itself stays untouched.
 */
import { useTranslation } from "react-i18next";

function Placeholder({ label }: { label: string }) {
  const { t } = useTranslation("app");
  return (
    <p className="placeholder">
      {label}: {t("placeholder")}
    </p>
  );
}

/** Server bar (B19). */
export function ServersMount() {
  const { t } = useTranslation("app");
  return <Placeholder label={t("regions.servers")} />;
}

/** Channel tree (B20). */
export function ChannelsMount() {
  const { t } = useTranslation("app");
  return <Placeholder label={t("regions.channels")} />;
}

/** User panel at the bottom of the channel column (B23). */
export function UserPanelMount() {
  const { t } = useTranslation("app");
  return <Placeholder label={t("regions.userPanel")} />;
}

/** Thin status bar above the chat, e.g. "Reconnecting…" (B19). Renders nothing when idle. */
export function ChatStatusMount() {
  return null;
}

/** Chat header, message list and input (B21). */
export function ChatMount() {
  const { t } = useTranslation("app");
  return <Placeholder label={t("regions.chat")} />;
}

/** Member list (B20). */
export function MembersMount() {
  const { t } = useTranslation("app");
  return <Placeholder label={t("regions.members")} />;
}

/** First-start wizard content (B18); calls `useView().finishWizard()` when done. */
export function WizardMount() {
  const { t } = useTranslation("app");
  return (
    <>
      <h1>{t("wizard.title")}</h1>
      <Placeholder label={t("wizard.title")} />
    </>
  );
}

/** Settings content inside the settings overlay (B24). */
export function SettingsMount() {
  const { t } = useTranslation("app");
  return <Placeholder label={t("settings.title")} />;
}
