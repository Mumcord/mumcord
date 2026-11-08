/**
 * Context menu on a person (FR-1.18, QA.32): send message (#141 chat request when needed), join
 * their channel, add or remove friend (FR-1.64), view profile, revoke chat consent, local mute and
 * volume slider. Reuses the profile card's volume slider and local-mute action, so both
 * places store the same per-certificate settings. Yourself only gets "View profile".
 *
 * Epic 4, issue #253 adds the "Admin ▸" sub-menu after the normal items (FR-10.6, FR-10.7): Kick…
 * and Ban…, in red, only with the right the server grants on the root channel (FR-10.1, FR-10.2),
 * and never for yourself. The other admin items come with their own issues.
 *
 * Epic 4, issue #255 adds the two move items: "Move to my channel" (only while the person sits
 * elsewhere) and "Move to…", which opens the channel picker. Both need the move right in the
 * person's channel, and "Move to…" takes the people picked in the tree along.
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
import { getApi, toEngineError, type ModerateInput } from "../../api";
import { shownServerId, useServerState } from "../channels/serverStore";
import { WHISPER_ICON_PATHS } from "../whisper/whisperIcons";
import { createWhisperTarget } from "../whisper/whisperStore";
import { hasMumcordFeature } from "../channels/mumcord";
import { setFriend, useFriend } from "../friends";
import { openChatRequestDialog } from "../requests/chatRequestDialogStore";
import { openRevokeConsentDialog } from "../requests/revokeConsentStore";
import { useAcceptedConsent, usePmRequests } from "../requests/requestsStore";
import { blockTargetFor, openBlockDialog, unblock, useBlock } from "../blocking";
import { BLOCK_PATH } from "../blocking/BlockIcon";
import { ContextMenu, type MenuPosition, type MenuItem } from "../../components/ContextMenu";
import { canBan, canKick } from "../admin/permissions";
import type { PersonActionMode } from "../admin/personActionStore";
import { openPersonAction } from "../admin/personActionStore";
import "../userpanel/quicksettings.css";
import "./members.css";
import { joinTarget } from "./joinTarget";
import { findPerson } from "./personLookup";
import { VolumeSlider } from "./ProfileCard";
import { openPrivateConversation } from "./privateMessage";
import { openMovePeople } from "../channels/movePeople";
import { peopleToMove, sharedChannelOf } from "../channels/peopleSelection";
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
import { requestPluginMenu, usePluginMenuEntries } from "../plugins/dialog";

/** Gap kept between the menu and the window edge. */
const EDGE = 8;

const ITEMS = '[role="menuitem"], [role="menuitemcheckbox"], input[type="range"]';

/** Shield for the admin sub-menu; the meaning also sits in its label (FR-2.16). */
const ADMIN_ICON = "M12 3l8 3v6c0 4.5-3.5 8-8 9-4.5-1-8-4.5-8-9V6z";
/**
 * Moving people (issue #255): an arrow down onto a line for "to my channel", an arrow into a bar
 * for "to another channel".
 */
const MOVE_TO_ME_ICON = "M12 3v12M8 11l4 4 4-4M4 21h16";
const MOVE_TO_ICON = "M3 12h11M10 8l4 4-4 4M19 4v16";
/** Leaving the server: a door with an arrow out, so a kick does not rest on its red colour alone. */
const KICK_ICON = "M9 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h4M16 17l5-5-5-5M21 12H9";
/** Moderation icons (issue #257, mockup "admin-menus"): crossed mic, crossed headphones, a star. */
const MUTE_ICON =
  "M12 2a3 3 0 0 0-3 3v6a3 3 0 0 0 4.6 2.5M15 9V5a3 3 0 0 0-.9-2.1M5 10v1a7 7 0 0 0 11.5 5.4M19 10v1a7 7 0 0 1-.4 2.3M12 18v4M3 3l18 18";
const DEAFEN_ICON =
  "M3 18v-6a9 9 0 0 1 14.4-7.2M21 12v6M21 18a2 2 0 0 1-2 2h-1v-6h3M3 18a2 2 0 0 0 2 2h1v-6H3M3 3l18 18";
const UNSUPPRESS_ICON =
  "M12 2a3 3 0 0 0-3 3v6a3 3 0 0 0 6 0V5a3 3 0 0 0-3-3M19 10v1a7 7 0 0 1-14 0v-1M12 18v4";
const STAR_ICON = "M12 3l2.8 5.7 6.2.9-4.5 4.4 1.1 6.2L12 17.3l-5.6 2.9 1.1-6.2L3 9.6l6.2-.9L12 3";
const RESET_COMMENT_ICON =
  "M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2zM9 8l6 6M15 8l-6 6";
const RESET_AVATAR_ICON = "M3 3h18v18H3zM9 9l6 6M15 9l-6 6";

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
  const { t: tWhisper } = useTranslation("whisper");
  const state = useServerState();
  const user = findPerson(state.users, open.session, open.name);
  const friend = useFriend(user?.certHash ?? null);
  const { sent } = usePmRequests(state.serverId);
  const consent = useAcceptedConsent(state.serverId, user?.certHash ?? null);
  const block = useBlock(state.serverId, user?.certHash ?? null, user?.name ?? "");
  const pluginEntries = usePluginMenuEntries(user?.session);
  const ref = useRef<HTMLDivElement>(null);
  // The menu closes once the join succeeds; a failure keeps it open to show why.
  const [joinError, setJoinError] = useState<string | null>(null);
  // The "Admin ▸" sub-menu (FR-10.6): it opens from its item and keeps the person menu open.
  const [adminMenu, setAdminMenu] = useState<MenuPosition | null>(null);
  const tAdmin = useTranslation("admin").t;
  const submenuOpen = adminMenu !== null;

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
      if (event.target instanceof Node && ref.current?.contains(event.target)) return;
      // The Admin sub-menu sits in its own portal; a press on it must not close the person menu,
      // or the sub-menu unmounts before its click and Kick… or Ban… does nothing (issue #507).
      if (submenuOpen && event.target instanceof Element && event.target.closest(".server-menu"))
        return;
      dismiss();
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
  }, [submenuOpen]);

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
  // Plugin entries (FR-6.40) only for people in the user's own channel.
  const ownChannel =
    state.selfSession === null ? undefined : state.users.get(state.selfSession)?.channelId;
  const inOwnChannel = user && !isSelf && ownChannel !== undefined && user.channelId === ownChannel;
  // Moving needs the move right where the person is; the server checks the target as well (#255).
  const movable =
    user !== undefined &&
    !isSelf &&
    state.channels.get(user.channelId)?.permissions.moveUsers === true;
  const elsewhere = movable && ownChannel !== undefined && user.channelId !== ownChannel;
  const moveToMyChannel = () => {
    if (!user || state.serverId === null || ownChannel === undefined) return;
    closePersonMenu();
    // A full or forbidden target is refused by the server, which says so once in a notice.
    getApi()
      .personMove(state.serverId, user.session, ownChannel)
      .catch((cause: unknown) => {
        console.error("Moving a person to the own channel failed", toEngineError(cause).code);
      });
  };
  const openMovePicker = () => {
    if (!user) return;
    const sessions = peopleToMove(user.session);
    // Focus first: the dialog puts it back here when it closes.
    open.opener?.focus();
    closePersonMenu();
    openMovePeople({ sessions, fromChannelId: sharedChannelOf(state.users, sessions) });
  };

  // The move items come first in Admin ▸ (FR-10.7, issue #255): moving needs the move right where
  // the person is, and "Move to my channel" only makes sense while they sit elsewhere.
  const moveItems: MenuItem[] =
    user !== undefined && !isSelf && movable
      ? [
          ...(elsewhere
            ? [
                {
                  label: t("menu.moveToMe"),
                  icon: MOVE_TO_ME_ICON,
                  onSelect: moveToMyChannel,
                },
              ]
            : []),
          { label: t("menu.moveTo"), icon: MOVE_TO_ICON, onSelect: openMovePicker },
        ]
      : [];

  // Kick… and Ban… only with the right the server granted on the root channel (FR-10.1, FR-10.2),
  // never for yourself (issue #253), and never as a disabled item (FR-10.2).
  const serverId = shownServerId();
  const openAction = (mode: PersonActionMode) => {
    if (!user) return;
    closePersonMenu();
    openPersonAction(
      serverId,
      user.session,
      user.name,
      state.channels.get(user.channelId)?.name ?? "",
      mode,
    );
  };
  // Moderation (issue #257): server mute, deafen, un-suppress and priority speaker need Mute others
  // in the person's channel; resets need Reset profiles in Root (FR-10.7, FR-10.15, FR-10.19).
  // The standard server refuses them for people in a temporary channel, so they are not offered.
  const personChannel = user ? state.channels.get(user.channelId) : undefined;
  const rights = personChannel?.temporary === true ? undefined : personChannel?.permissions;
  const rootRights = state.channels.get(0)?.permissions;
  const moderate = (action: ModerateInput) => {
    if (!user) return;
    getApi()
      .personModerate(serverId, user.session, action)
      .catch((error: unknown) => {
        console.error("A moderation action failed", toEngineError(error).message);
      });
  };
  const moderation: MenuItem[] =
    user && rights?.muteDeafen === true
      ? [
          {
            label: tAdmin("moderate.mute"),
            icon: MUTE_ICON,
            checked: user.serverMute,
            onSelect: () => {
              moderate({ kind: "mute", on: !user.serverMute });
            },
          },
          {
            label: tAdmin("moderate.deafen"),
            icon: DEAFEN_ICON,
            checked: user.serverDeaf,
            onSelect: () => {
              moderate({ kind: "deafen", on: !user.serverDeaf });
            },
          },
          ...(user.suppressed
            ? [
                {
                  label: tAdmin("moderate.unsuppress"),
                  icon: UNSUPPRESS_ICON,
                  onSelect: () => {
                    moderate({ kind: "unsuppress" });
                  },
                },
              ]
            : []),
          {
            label: tAdmin("moderate.priority"),
            icon: STAR_ICON,
            checked: user.prioritySpeaker,
            onSelect: () => {
              moderate({ kind: "prioritySpeaker", on: !user.prioritySpeaker });
            },
          },
        ]
      : [];
  const resets: MenuItem[] =
    user && !isSelf && rootRights?.resetUserContent === true
      ? [
          ...(user.hasComment
            ? [
                {
                  label: tAdmin("moderate.resetComment"),
                  icon: RESET_COMMENT_ICON,
                  onSelect: () => {
                    openAction("resetComment");
                  },
                },
              ]
            : []),
          ...(user.hasAvatar
            ? [
                {
                  label: tAdmin("moderate.resetAvatar"),
                  icon: RESET_AVATAR_ICON,
                  onSelect: () => {
                    openAction("resetAvatar");
                  },
                },
              ]
            : []),
        ]
      : [];
  const removal: MenuItem[] =
    user && !isSelf
      ? [
          ...moveItems,
          ...(canKick(serverId)
            ? [
                {
                  label: tAdmin("menuKick"),
                  icon: KICK_ICON,
                  danger: true,
                  onSelect: () => {
                    openAction("kick");
                  },
                },
              ]
            : []),
          ...(canBan(serverId)
            ? [
                {
                  label: tAdmin("menuBan"),
                  icon: BLOCK_PATH,
                  danger: true,
                  onSelect: () => {
                    openAction("ban");
                  },
                },
              ]
            : []),
        ]
      : [];
  // Kick and ban sit last, in red, behind a divider (FR-10.6).
  const adminItems: MenuItem[] = [
    ...moderation,
    ...resets,
    ...removal.map((item, index) =>
      index === 0 && moderation.length + resets.length > 0
        ? { ...item, separatorBefore: true }
        : item,
    ),
  ];

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
      {user &&
        !isSelf &&
        (state.channels.get(user.channelId)?.permissions.whisper === false
          ? disabledItem(
              "whisper",
              <path d={WHISPER_ICON_PATHS.whisper} />,
              tWhisper("menu.whisper"),
              tWhisper("menu.noPermission"),
            )
          : item(
              "whisper",
              <path d={WHISPER_ICON_PATHS.whisper} />,
              tWhisper("menu.whisper"),
              () => {
                closePersonMenu();
                createWhisperTarget({
                  name: user.name,
                  receivers: { kind: "people", names: [user.name] },
                });
              },
            ))}
      {inOwnChannel &&
        pluginEntries.map(({ plugin, label }) =>
          item(`plugin-${plugin}`, <PluginIcon />, label, () => {
            closePersonMenu();
            requestPluginMenu(plugin, user.session, open.opener ?? undefined);
          }),
        )}
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
          <div role="separator" className="person-menu__separator" />
          {/* Last, apart from the rest: the label and icon say it is drastic, not only the colour. */}
          <button
            type="button"
            role="menuitem"
            tabIndex={-1}
            className="server-menu__item server-menu__item--danger person-menu__item"
            data-testid={block ? "person-unblock" : "person-block"}
            onClick={() => {
              closePersonMenu();
              if (block) {
                unblock(block.id).catch((cause: unknown) => {
                  console.error("Unblocking failed", toEngineError(cause).code);
                });
                open.opener?.focus();
                return;
              }
              const target =
                state.serverId === null
                  ? null
                  : blockTargetFor(
                      state.serverId,
                      user.name,
                      user.certHash,
                      user.session,
                      open.opener,
                    );
              if (target) openBlockDialog(target);
            }}
          >
            <MenuIcon>
              <path d={BLOCK_PATH} />
            </MenuIcon>
            {block ? t("block.unblock") : t("block.menu")}
          </button>
          {adminItems.length > 0 && (
            <>
              <div role="separator" className="person-menu__separator" />
              {/* The sub-menu of the admin items (FR-10.6, FR-10.7); the arrow says it opens. */}
              <button
                type="button"
                role="menuitem"
                tabIndex={-1}
                aria-haspopup="menu"
                aria-expanded={adminMenu !== null}
                className="server-menu__item person-menu__item"
                onClick={(event) => {
                  const opener = event.currentTarget;
                  const rect = opener.getBoundingClientRect();
                  setAdminMenu({ x: rect.right + 4, y: rect.top, opener });
                }}
              >
                <MenuIcon>
                  <path d={ADMIN_ICON} />
                </MenuIcon>
                <span className="quick-settings__text">
                  <span className="quick-settings__label">{tAdmin("menuAdmin")}</span>
                </span>
                <svg
                  className="server-menu__submenu"
                  viewBox="0 0 24 24"
                  width="16"
                  height="16"
                  aria-hidden="true"
                  focusable="false"
                >
                  <path d="M9 6l6 6-6 6" fill="none" stroke="currentColor" strokeWidth="2" />
                </svg>
              </button>
            </>
          )}
        </>
      )}
      {adminMenu !== null && adminItems.length > 0 && (
        <ContextMenu
          menu={adminMenu}
          label={tAdmin("menuAdmin")}
          items={adminItems}
          onClose={() => {
            setAdminMenu(null);
          }}
        />
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

function PluginIcon() {
  return (
    <path d="M10 3H6a2 2 0 0 0-2 2v4h2a2 2 0 1 1 0 4H4v6a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-4h-2a2 2 0 1 1 0-4h2V5a2 2 0 0 0-2-2h-4a2 2 0 1 0-4 0z" />
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
