/**
 * Context menu on a person (FR-1.18, QA.32): send message (#141 chat request when needed), join
 * their channel, add or remove friend (FR-1.64), view profile, revoke chat consent, local mute and
 * volume slider. Reuses the profile card's volume slider and local-mute action, so both
 * places store the same per-certificate settings. Yourself only gets "View profile".
 */
import {
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  useSyncExternalStore,
  type KeyboardEvent,
  type ReactNode,
} from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import { getApi, toEngineError } from "../../api";
import { shownServerId, useServerState } from "../channels/serverStore";
import { hasMumcordFeature } from "../channels/mumcord";
import { setFriend, useFriend } from "../friends";
import { openChatRequestDialog } from "../requests/chatRequestDialogStore";
import { openRevokeConsentDialog } from "../requests/revokeConsentStore";
import { useAcceptedConsent, usePmRequests } from "../requests/requestsStore";
import "../userpanel/quicksettings.css";
import "./members.css";
import { joinTarget } from "./joinTarget";
import { findPerson } from "./personLookup";
import { VolumeSlider } from "./ProfileCard";
import { openPrivateConversation } from "./privateMessage";
import {
  closePersonMenu,
  currentPersonMenu,
  subscribePersonMenu,
  type OpenPersonMenu,
} from "./personMenuStore";
import { joinChannel } from "../userpanel/readOnly";
import { requestConnectionExpanded } from "../connection/connectionExpanded";
import { connectionLevel, useUserStats } from "../connection/userStatsStore";
import { useProfileCard } from "./useProfileCard";

/** Gap kept between the menu and the window edge. */
const EDGE = 8;

const ITEMS = '[role="menuitem"], [role="menuitemcheckbox"], input[type="range"]';

/** Decorative: the entry's label is the accessible name, so the icon stays hidden. */
function MenuIcon({ children }: { children: ReactNode }) {
  return (
    <svg
      className="person-menu__icon"
      viewBox="0 0 24 24"
      width="18"
      height="18"
      aria-hidden="true"
      focusable="false"
    >
      <g
        fill="none"
        stroke="currentColor"
        strokeWidth="2"
        strokeLinecap="round"
        strokeLinejoin="round"
      >
        {children}
      </g>
    </svg>
  );
}

/** Renders the open person menu and the profile card it can open; mount once. */
export function PersonMenuHost() {
  const open = useSyncExternalStore(subscribePersonMenu, currentPersonMenu);
  const { openProfile, profileCard } = useProfileCard();
  // Nothing may stay open once the host is gone.
  useEffect(() => closePersonMenu, []);
  return (
    <>
      {open && createPortal(<PersonMenu open={open} onViewProfile={openProfile} />, document.body)}
      {profileCard}
    </>
  );
}

function PersonMenu({
  open,
  onViewProfile,
}: {
  open: OpenPersonMenu;
  onViewProfile: (session: number, opener: HTMLElement | null, focusConnection?: boolean) => void;
}) {
  const { t } = useTranslation("members");
  const { t: tFriends } = useTranslation("friends");
  const { t: tChannels } = useTranslation("channels");
  const state = useServerState();
  const user = findPerson(state.users, open.session, open.name);
  const friend = useFriend(user?.certHash ?? null);
  const { sent } = usePmRequests(state.serverId);
  const consent = useAcceptedConsent(state.serverId, user?.certHash ?? null);
  const ref = useRef<HTMLDivElement>(null);
  // The menu closes once the join succeeds; a failure keeps it open to show why.
  const [joinError, setJoinError] = useState<string | null>(null);

  const close = (returnFocus: boolean) => {
    closePersonMenu();
    if (returnFocus && open.opener?.isConnected) open.opener.focus();
  };

  // Without a stored name there is nothing to show for a person who left.
  const gone = !user && open.name === undefined;
  useEffect(() => {
    if (gone) closePersonMenu();
  }, [gone]);

  useEffect(() => {
    const dismiss = () => {
      closePersonMenu();
    };
    const onPointerDown = (event: PointerEvent) => {
      if (!(event.target instanceof Node && ref.current?.contains(event.target))) dismiss();
    };
    // Containers scrolling by themselves (the chat list following a new message) keep it open;
    // the page scrolling or the user using the wheel elsewhere closes it.
    const onScroll = (event: Event) => {
      if (event.target === document || event.target instanceof Window) dismiss();
    };
    const onWheel = (event: Event) => {
      if (!(event.target instanceof Node && ref.current?.contains(event.target))) dismiss();
    };
    document.addEventListener("pointerdown", onPointerDown, true);
    window.addEventListener("scroll", onScroll, true);
    window.addEventListener("wheel", onWheel, { capture: true, passive: true });
    window.addEventListener("blur", dismiss);
    window.addEventListener("resize", dismiss);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown, true);
      window.removeEventListener("scroll", onScroll, true);
      window.removeEventListener("wheel", onWheel, true);
      window.removeEventListener("blur", dismiss);
      window.removeEventListener("resize", dismiss);
    };
  }, []);

  // Clamp to the viewport once the size is known; focus the first item.
  useLayoutEffect(() => {
    const element = ref.current;
    if (!element) return;
    const left = Math.max(EDGE, Math.min(open.x, window.innerWidth - element.offsetWidth - EDGE));
    const top = Math.max(EDGE, Math.min(open.y, window.innerHeight - element.offsetHeight - EDGE));
    element.style.left = `${String(left)}px`;
    element.style.top = `${String(top)}px`;
    element.querySelector<HTMLElement>('[role="menuitem"]')?.focus();
  }, [open]);

  if (gone) return null;
  const isSelf = user?.session === state.selfSession;

  const join = user && !isSelf ? joinTarget(state, user) : null;

  // Chat requests (issue #141) reach Mumcord users who announced them, and only make sense for
  // people who may not write yet: not friends, no accepted conversation, no accepted request.
  const sentRequest = user ? sent.find((r) => r.session === user.session) : undefined;
  const alreadyAccepted = sentRequest?.accepted === true || consent !== undefined;
  const chatRequest =
    user && !isSelf && friend === undefined && !alreadyAccepted
      ? hasMumcordFeature(user, "chat-request")
        ? sentRequest
          ? "pending"
          : "offer"
        : null
      : null;

  const report = (cause: unknown) => {
    console.error("Changing a person's local settings failed", toEngineError(cause).code);
  };

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const items = [...(ref.current?.querySelectorAll<HTMLElement>(ITEMS) ?? [])];
    const index = items.findIndex((item) => item === document.activeElement);
    const onSlider = document.activeElement instanceof HTMLInputElement;
    const focusAt = (next: number) => {
      items[(next + items.length) % items.length]?.focus();
    };
    switch (event.key) {
      case "ArrowDown":
        focusAt(index + 1);
        break;
      case "ArrowUp":
        focusAt(index - 1);
        break;
      // Home/End belong to the slider while it has focus.
      case "Home":
        if (onSlider) return;
        focusAt(0);
        break;
      case "End":
        if (onSlider) return;
        focusAt(items.length - 1);
        break;
      case "Escape":
      case "Tab":
        close(true);
        break;
      default:
        return;
    }
    event.preventDefault();
    event.stopPropagation();
  };

  const item = (id: string, icon: ReactNode, label: string, onSelect: () => void) => (
    <button
      key={id}
      type="button"
      role="menuitem"
      tabIndex={-1}
      className="server-menu__item person-menu__item"
      onClick={onSelect}
    >
      <MenuIcon>{icon}</MenuIcon>
      {label}
    </button>
  );

  // Shown but not selectable; the visible reason is its accessible description, not its name.
  const disabledItem = (id: string, icon: ReactNode, label: string, reason: string | null) => (
    <button
      key={id}
      type="button"
      role="menuitem"
      aria-disabled="true"
      aria-labelledby={`person-menu-${id}-label`}
      aria-describedby={reason === null ? undefined : `person-menu-${id}-reason`}
      tabIndex={-1}
      className="server-menu__item person-menu__item"
    >
      <MenuIcon>{icon}</MenuIcon>
      <span className="quick-settings__text">
        <span id={`person-menu-${id}-label`} className="quick-settings__label">
          {label}
        </span>
        {reason !== null && (
          <span id={`person-menu-${id}-reason`} className="quick-settings__hint">
            {reason}
          </span>
        )}
      </span>
    </button>
  );

  return (
    <div
      ref={ref}
      className="server-menu person-menu"
      role="menu"
      aria-label={t("menu.label", { name: user?.name ?? open.name })}
      tabIndex={-1}
      onKeyDown={onKeyDown}
    >
      {user && (
        <div role="none" className="person-menu__header">
          <span className="person-menu__name">{user.name}</span>
          <MenuConnectionLine session={user.session} />
        </div>
      )}
      {user && !isSelf && (
        <>
          {chatRequest === "pending"
            ? disabledItem(
                "message",
                <ChatBubbleIcon />,
                t("menu.sendMessage"),
                t("menu.chatRequestPending"),
              )
            : item("message", <ChatBubbleIcon />, t("menu.sendMessage"), () => {
                closePersonMenu();
                // Without an accepted conversation a Mumcord user first gets a chat request (#141).
                if (chatRequest === "offer") {
                  openChatRequestDialog({
                    serverId: shownServerId(),
                    session: user.session,
                    name: user.name,
                    opener: open.opener,
                  });
                } else {
                  openPrivateConversation(user);
                }
              })}
          {join &&
            (join.blocked === null
              ? item(
                  "join",
                  <EnterChannelIcon />,
                  t("menu.join", { channel: join.channel.name }),
                  () => {
                    setJoinError(null);
                    joinChannel(join.channel.id).then(
                      () => {
                        closePersonMenu();
                      },
                      (cause: unknown) => {
                        const { code } = toEngineError(cause);
                        if (code !== "permissionDenied" && code !== "notConnected") {
                          console.error("Joining a person's channel failed", code);
                        }
                        setJoinError(
                          code === "permissionDenied"
                            ? tChannels("errors.joinDenied")
                            : code === "notConnected"
                              ? tChannels("errors.notConnected")
                              : tChannels("errors.joinFailed"),
                        );
                      },
                    );
                  },
                )
              : disabledItem(
                  "join",
                  <EnterChannelIcon />,
                  t("menu.join", { channel: join.channel.name }),
                  join.blocked === "full" ? t("menu.joinFull") : t("menu.joinLocked"),
                ))}
          {joinError && (
            <p role="alert" className="channels-error">
              {joinError}
            </p>
          )}
          {/* People without a certificate can't be recognised again, so they can't be friends. */}
          {user.certHash === null
            ? disabledItem("friend", <HeartIcon />, tFriends("add"), tFriends("noCertificate"))
            : item(
                "friend",
                <HeartIcon />,
                friend === undefined ? tFriends("add") : tFriends("remove"),
                () => {
                  setFriend(user, friend === undefined).catch(report);
                },
              )}
        </>
      )}
      {user
        ? item("profile", <IdentityCardIcon />, t("menu.viewProfile"), () => {
            closePersonMenu();
            onViewProfile(user.session, open.opener);
          })
        : // Not on the server: there is no profile to open and nothing to mute.
          disabledItem("profile", <IdentityCardIcon />, t("menu.viewProfile"), null)}
      {user &&
        item("connection", <SignalIcon />, t("menu.connectionInfo"), () => {
          closePersonMenu();
          requestConnectionExpanded();
          onViewProfile(user.session, open.opener, true);
        })}
      {consent &&
        !isSelf &&
        item("revokeConsent", <RevokeIcon />, t("menu.revokeConsent"), () => {
          closePersonMenu();
          openRevokeConsentDialog({ conversation: consent, opener: open.opener });
        })}
      {user && !isSelf && (
        <>
          <div role="separator" className="person-menu__separator" />
          {/* Same switch row as the quick settings; the menu stays open while it toggles. */}
          <button
            type="button"
            role="menuitemcheckbox"
            aria-checked={user.localMute}
            tabIndex={-1}
            className="quick-settings__row"
            onClick={() => {
              getApi()
                .personSetLocalMute(shownServerId(), user.session, !user.localMute)
                .catch(report);
            }}
          >
            <MenuIcon>
              <path d="M11 5L6 9H2v6h4l5 4zM22 9l-6 6M16 9l6 6" />
            </MenuIcon>
            <span className="quick-settings__text">
              <span className="quick-settings__label">{t("menu.mute")}</span>
            </span>
            <span className="quick-settings__switch" aria-hidden="true" />
          </button>
          <div className="person-menu__volume">
            <MenuIcon>
              <path d="M11 5L6 9H2v6h4l5 4zM15.5 8.5a5 5 0 0 1 0 7M19 5a10 10 0 0 1 0 14" />
            </MenuIcon>
            <VolumeSlider user={user} onError={report} />
          </div>
        </>
      )}
    </div>
  );
}

/** Ping and transport next to the name; the server only returns ping where it may be shown (FR-2.25a). */
function MenuConnectionLine({ session }: { session: number }) {
  const { t } = useTranslation("members");
  const stats = useUserStats(session);
  if (stats?.pingMs == null) return null;
  const transport = stats.transport === null ? null : stats.transport === "udp" ? "UDP" : "TCP";
  const text = [t("quality.ms", { value: Math.round(stats.pingMs) }), transport]
    .filter((part) => part !== null)
    .join(" · ");
  return (
    <span className="person-menu__connection" data-level={connectionLevel(stats)}>
      <svg viewBox="0 0 24 24" width="14" height="14" aria-hidden="true" focusable="false">
        <path
          d="M4 20v-3M9 20v-7M14 20v-11M19 20V4"
          fill="none"
          stroke="currentColor"
          strokeWidth="2.5"
          strokeLinecap="round"
        />
      </svg>
      <span className="person-menu__connection-text">{text}</span>
    </span>
  );
}

function SignalIcon() {
  return <path d="M4 20v-3M9 20v-7M14 20v-11M19 20V4" />;
}

/* Icon shapes match the pinned profile card, so the same action looks the same everywhere. */
function IdentityCardIcon() {
  return (
    <>
      <rect x="3" y="5" width="18" height="14" rx="2" />
      <circle cx="9" cy="11" r="2" />
      <path d="M14 10h4M14 14h3" />
    </>
  );
}

function ChatBubbleIcon() {
  return <path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z" />;
}

function RevokeIcon() {
  return (
    <path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2zM9 7l6 6M15 7l-6 6" />
  );
}

function HeartIcon() {
  return (
    <path d="M12 21s-7-4.5-9.5-9A5.5 5.5 0 0 1 12 6a5.5 5.5 0 0 1 9.5 6c-2.5 4.5-9.5 9-9.5 9z" />
  );
}

function EnterChannelIcon() {
  return <path d="M15 3h4a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2h-4M10 17l5-5-5-5M15 12H3" />;
}
