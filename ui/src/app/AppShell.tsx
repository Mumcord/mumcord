import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type ReactNode,
} from "react";
import { useTranslation } from "react-i18next";
import { SkipLinks } from "../a11y";
import { onUiHotkey } from "../features/hotkeys";
import { ResizeHandle } from "./ResizeHandle";
import {
  isMembersShortcut,
  loadMembersCollapsed,
  MembersColumnContext,
  saveMembersCollapsed,
  type MembersColumn,
} from "./membersCollapse";
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
} as const;

/**
 * Four-column main window frame (server bar, channels + user panel, chat, members). Purely
 * presentational: features fill the slots. The member column can be hidden to give the chat the
 * full width (issue #165, variant C); its state is remembered across restarts. Narrow windows show
 * it as an overlay below the chat header instead. The toggle is the members button in the chat
 * header, which reaches the shell through `MembersColumnContext`. The channel and member columns
 * are resizable by their inner border (QA.29).
 */
export function AppShell({
  slots,
  inert = false,
  focus = false,
  mediaPanel = false,
}: {
  slots: ShellSlots;
  inert?: boolean;
  /** Focus mode of a media view: only the picture (issue #163, Z or Esc leaves it). */
  focus?: boolean;
  /** A watch party or screen share panel fills the member column (issue #163). */
  mediaPanel?: boolean;
}) {
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
  const buttonRef = useRef<HTMLButtonElement>(null);
  const restoreFocus = useRef(false);
  const [overlayTop, setOverlayTop] = useState(0);
  const overlayShown = narrow && overlayOpen;
  const widths = useColumnWidths(shellRef, serverBarRef, mediaPanel);

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
      if (inert || event.defaultPrevented) return;
      if (overlayShown && event.key === "Escape") {
        event.preventDefault();
        toggleMembers();
        return;
      }
      if (!isMembersShortcut(event)) return;
      event.preventDefault();
      toggleMembers();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => {
      window.removeEventListener("keydown", onKeyDown);
    };
  }, [inert, overlayShown, toggleMembers]);

  // The global hotkey also works while the window is in the background (#64).
  useEffect(() => onUiHotkey("toggleMemberColumn", toggleMembers), [toggleMembers]);

  // Hiding takes the focused element with it: the members button takes the focus instead.
  const wasShown = useRef(membersShown);
  useEffect(() => {
    const focusedInside = membersRef.current?.contains(document.activeElement) ?? false;
    if (wasShown.current && !membersShown && (restoreFocus.current || focusedInside)) {
      buttonRef.current?.focus();
    }
    restoreFocus.current = false;
    wasShown.current = membersShown;
  }, [membersShown]);

  // The overlay opens below the chat header, which keeps the members button reachable.
  useLayoutEffect(() => {
    if (!overlayShown) return;
    const measure = () => {
      const shell = shellRef.current;
      const header = shell?.querySelector(".chat-header");
      if (!shell || !header) {
        setOverlayTop(0);
        return;
      }
      setOverlayTop(header.getBoundingClientRect().bottom - shell.getBoundingClientRect().top);
    };
    measure();
    window.addEventListener("resize", measure);
    return () => {
      window.removeEventListener("resize", measure);
    };
  }, [overlayShown]);

  const column = useMemo<MembersColumn>(
    () => ({
      shown: membersShown,
      overlay: narrow,
      controls: SHELL_IDS.members,
      toggle: toggleMembers,
      buttonRef,
    }),
    [membersShown, narrow, toggleMembers],
  );

  // Custom properties are not part of React's style type.
  const style = {
    "--channels-width": `${String(widths.shown.channels)}rem`,
    "--members-width": `${String(widths.shown.members)}rem`,
    "--members-column-width": `${String(collapsed ? 0 : widths.shown.members)}rem`,
    "--members-overlay-top": `${String(overlayTop)}px`,
  } as CSSProperties;

  return (
    <div
      ref={shellRef}
      className="shell"
      style={style}
      data-members-collapsed={!narrow && collapsed}
      data-members-open={overlayShown}
      data-page-open={slots.page ? true : undefined}
      data-focus={focus || undefined}
      inert={inert}
    >
      <MembersColumnContext value={column}>
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
          {slots.members}
        </aside>
        {overlayShown && (
          <div className="shell__members-scrim" aria-hidden="true" onClick={toggleMembers} />
        )}
      </MembersColumnContext>
    </div>
  );
}
