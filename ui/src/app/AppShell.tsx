import { useRef, useState, type CSSProperties, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { SkipLinks } from "../a11y";
import { ResizeHandle } from "./ResizeHandle";
import { useColumnWidths } from "./useColumnWidths";
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
 * The channel and member columns are resizable by their inner border (QA.29).
 */
export function AppShell({ slots, inert = false }: { slots: ShellSlots; inert?: boolean }) {
  const { t } = useTranslation("app");
  const [membersOpen, setMembersOpen] = useState(false);
  const shellRef = useRef<HTMLDivElement>(null);
  const serverBarRef = useRef<HTMLElement>(null);
  const widths = useColumnWidths(shellRef, serverBarRef);
  // Custom properties are not part of React's style type.
  const style = {
    "--channels-width": `${String(widths.shown.channels)}rem`,
    "--members-width": `${String(widths.shown.members)}rem`,
  } as CSSProperties;

  return (
    <div
      ref={shellRef}
      className="shell"
      style={style}
      data-members-open={membersOpen}
      inert={inert}
    >
      <SkipLinks
        targets={[
          { id: SHELL_IDS.chat, label: t("skipLinks.chat") },
          { id: SHELL_IDS.channels, label: t("skipLinks.channels") },
        ]}
      />
      <nav ref={serverBarRef} className="shell__servers" aria-label={t("regions.servers")}>
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
      <ResizeHandle
        column="channels"
        label={t("resize.channels")}
        controls={SHELL_IDS.channels}
        value={widths.shown.channels}
        max={widths.max.channels}
        onResize={(rem) => {
          widths.resize("channels", rem);
        }}
        onCommit={widths.commit}
        onReset={() => {
          widths.reset("channels");
        }}
      />
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
      <ResizeHandle
        column="members"
        label={t("resize.members")}
        controls={SHELL_IDS.members}
        value={widths.shown.members}
        max={widths.max.members}
        onResize={(rem) => {
          widths.resize("members", rem);
        }}
        onCommit={widths.commit}
        onReset={() => {
          widths.reset("members");
        }}
      />
      <aside id={SHELL_IDS.members} className="shell__members" aria-label={t("regions.members")}>
        {slots.members}
      </aside>
    </div>
  );
}
