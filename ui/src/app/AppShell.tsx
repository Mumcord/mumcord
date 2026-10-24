import { useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { SkipLinks } from "../a11y";
import "./shell.css";

export interface ShellSlots {
  servers: ReactNode;
  channels: ReactNode;
  userPanel: ReactNode;
  chatStatus: ReactNode;
  chat: ReactNode;
  members: ReactNode;
}

/** Element ids of the shell regions, used as skip-link targets. */
const SHELL_IDS = { channels: "channels", chat: "chat", members: "members" } as const;

/**
 * Four-column main window frame (server bar, channels + user panel, chat, members). Purely
 * presentational: features fill the slots. Narrow windows hide the member column behind a toggle.
 */
export function AppShell({ slots, inert = false }: { slots: ShellSlots; inert?: boolean }) {
  const { t } = useTranslation("app");
  const [membersOpen, setMembersOpen] = useState(false);

  return (
    <div className="shell" data-members-open={membersOpen} inert={inert}>
      <SkipLinks
        targets={[
          { id: SHELL_IDS.chat, label: t("skipLinks.chat") },
          { id: SHELL_IDS.channels, label: t("skipLinks.channels") },
        ]}
      />
      <nav className="shell__servers" aria-label={t("regions.servers")}>
        {slots.servers}
      </nav>
      <div className="shell__channel-column">
        <nav
          id={SHELL_IDS.channels}
          className="shell__channels"
          aria-label={t("regions.channels")}
          tabIndex={-1}
        >
          {slots.channels}
        </nav>
        <section className="shell__user-panel" aria-label={t("regions.userPanel")}>
          {slots.userPanel}
        </section>
      </div>
      <main
        id={SHELL_IDS.chat}
        className="shell__chat"
        aria-label={t("regions.chat")}
        tabIndex={-1}
      >
        <button
          type="button"
          className="shell__members-toggle"
          aria-expanded={membersOpen}
          aria-controls={SHELL_IDS.members}
          onClick={() => {
            setMembersOpen((open) => !open);
          }}
        >
          {membersOpen ? t("members.hide") : t("members.show")}
        </button>
        {slots.chatStatus}
        <div className="shell__chat-body">{slots.chat}</div>
      </main>
      <aside id={SHELL_IDS.members} className="shell__members" aria-label={t("regions.members")}>
        {slots.members}
      </aside>
    </div>
  );
}
