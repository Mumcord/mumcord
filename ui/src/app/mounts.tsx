/**
 * Mount points for feature UIs. Each feature task replaces the placeholder in its own mount
 * with its root component; the shell layout itself stays untouched.
 */
import { useTranslation } from "react-i18next";
import { SettingsScreen } from "../features/settings/SettingsScreen";
import { Wizard } from "../features/wizard";
import { UserPanel } from "../features/userpanel";
import { ChannelsPanel } from "../features/channels";
import { ChatView } from "../features/chat";
import { MembersPanel } from "../features/members";
import { ConnectionStatus, ServersRoot } from "../features/servers";

/** Server bar (B19). */
export function ServersMount() {
  return <ServersRoot />;
}

/** Channel tree (B20). */
export function ChannelsMount() {
  return <ChannelsPanel />;
}

/** User panel at the bottom of the channel column (B23). */
export function UserPanelMount() {
  return <UserPanel />;
}

/** Thin status bar above the chat, e.g. "Reconnecting…" (B19). Renders nothing when idle. */
export function ChatStatusMount() {
  return <ConnectionStatus />;
}

/** Chat header, message list and input (B21). */
export function ChatMount() {
  return <ChatView />;
}

/** Member list (B20). */
export function MembersMount() {
  return <MembersPanel />;
}

/** First-start wizard content (B18); calls `useView().finishWizard()` when done. */
export function WizardMount() {
  const { t } = useTranslation("app");
  return (
    <>
      <h1>{t("wizard.title")}</h1>
      <Wizard />
    </>
  );
}

/** Settings content inside the settings overlay (B24). */
export function SettingsMount() {
  return <SettingsScreen />;
}
