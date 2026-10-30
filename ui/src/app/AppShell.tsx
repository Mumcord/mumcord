import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type CSSProperties,
  type ReactNode,
} from "react";
import { useTranslation } from "react-i18next";
import { SkipLinks } from "../a11y";
import { ResizeHandle } from "./ResizeHandle";
import { isMembersShortcut, loadMembersCollapsed, saveMembersCollapsed } from "./membersCollapse";
import { useColumnWidths, useNarrowWindow } from "./useColumnWidths";
import "./shell.css";

export interface ShellSlots {
  servers: ReactNode;
  channels: ReactNode;
  userPanel: ReactNode;
  chatStatus: ReactNode;
  chat: ReactNode;
  members: ReactNode;
  /** A page that covers everything right of the server bar (the public server browser). */
  page?: ReactNode;
}

/** Element ids of the shell regions, used as skip-link targets. */
const SHELL_IDS = {
  channels: "channels",
  chat: "chat",
  members: "members",
  membersContent: "members-content",
} as const;

/**
 * Four-column main window frame (server bar, channels + user panel, chat, members). Purely
 * presentational: features fill the slots. The member column can be collapsed to give the chat the
 * full width (issue #165); its state is remembered across restarts. Narrow windows always show it
 * as an overlay instead, opened from a button in the chat. The channel and member columns are
 * resizable by their inner border (QA.29).
 */
export function AppShell({ slots, inert = false }: { slots: ShellSlots; inert?: boolean }) {
  const { t } = useTranslation("app");
  const [collapsed, setCollapsed] = useState(loadMembersCollapsed);
  const [overlayOpen, setOverlayOpen] = useState(false);
  const narrow = useNarrowWindow();
  const [wasNarrow, setWasNarrow] = useState(narrow);
  // A window narrowed below the breakpoint starts with the overlay closed (auto-collapse).
  if (wasNarrow !== narrow) {
    setWasNarrow(narrow);
    if (narrow) setOverlayOpen(false);
  }
  const membersShown = narrow ? overlayOpen : !collapsed;
  const shellRef = useRef<HTMLDivElement>(null);
  const serverBarRef = useRef<HTMLElement>(null);
  const membersRef = useRef<HTMLElement>(null);
  const revealRef = useRef<HTMLButtonElement>(null);
  const restoreFocus = useRef(false);
  const [revealWidth, setRevealWidth] = useState(0);
  const widths = useColumnWidths(shellRef, serverBarRef);

  const toggleMembers = useCallback(() => {
    // Remember to move the focus before the focused element leaves with the column.
    if (membersShown && membersRef.current?.contains(document.activeElement)) {
      restoreFocus.current = true;
    }
    if (narrow) {
      setOverlayOpen((open) => !open);
      return;
    }
    setCollapsed((value) => {
      const next = !value;
      saveMembersCollapsed(next);
      return next;
    });
  }, [narrow, membersShown]);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (inert || event.defaultPrevented || !isMembersShortcut(event)) return;
      event.preventDefault();
      toggleMembers();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => {
      window.removeEventListener("keydown", onKeyDown);
    };
  }, [inert, toggleMembers]);

  // Collapsing takes the focused element with it: the reveal button takes the focus instead.
  const wasShown = useRef(membersShown);
  useEffect(() => {
    const focusedInside = membersRef.current?.contains(document.activeElement) ?? false;
    if (wasShown.current && !membersShown && (restoreFocus.current || focusedInside)) {
      revealRef.current?.focus();
    }
    restoreFocus.current = false;
    wasShown.current = membersShown;
  }, [membersShown]);

  // The chat header leaves room for the reveal button whatever its language's text is wide.
  useLayoutEffect(() => {
    const button = revealRef.current;
    if (!button) {
      setRevealWidth(0);
      return;
    }
    const measure = () => {
      setRevealWidth(button.offsetWidth);
    };
    measure();
    if (typeof ResizeObserver !== "function") return;
    const observer = new ResizeObserver(measure);
    observer.observe(button);
    return () => {
      observer.disconnect();
    };
  }, [membersShown]);

  // Custom properties are not part of React's style type.
  const style = {
    "--channels-width": `${String(widths.shown.channels)}rem`,
    "--members-width": `${String(widths.shown.members)}rem`,
    "--members-column-width": `${String(collapsed ? 0 : widths.shown.members)}rem`,
    "--members-reveal-width": `${String(revealWidth)}px`,
  } as CSSProperties;

  return (
    <div
      ref={shellRef}
      className="shell"
      style={style}
      data-members-collapsed={collapsed}
      data-members-open={overlayOpen}
      data-members-reveal={!membersShown}
      data-page-open={slots.page ? true : undefined}
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
      {slots.page ? <div className="shell__page">{slots.page}</div> : null}
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
        {!membersShown && (
          <button
            ref={revealRef}
            type="button"
            className="shell__members-toggle"
            aria-expanded={false}
            aria-controls={SHELL_IDS.membersContent}
            aria-keyshortcuts="Control+Shift+M Meta+Shift+M"
            onClick={toggleMembers}
          >
            <span className="shell__members-chevron" aria-hidden="true" />
            {t("members.show")}
          </button>
        )}
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
      <aside
        ref={membersRef}
        id={SHELL_IDS.members}
        className="shell__members"
        aria-label={t("regions.members")}
        inert={!membersShown}
      >
        {membersShown && (
          <button
            type="button"
            className="shell__members-hide"
            aria-expanded
            aria-controls={SHELL_IDS.membersContent}
            aria-keyshortcuts="Control+Shift+M Meta+Shift+M"
            aria-label={t("members.hide")}
            data-tooltip={t("members.hide")}
            onClick={toggleMembers}
          >
            <span className="shell__members-chevron" aria-hidden="true" />
          </button>
        )}
        <div id={SHELL_IDS.membersContent} className="shell__members-content">
          {slots.members}
        </div>
      </aside>
    </div>
  );
}
