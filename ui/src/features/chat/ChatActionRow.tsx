/**
 * "Apps" button left of the image picker (FR-2.65) with a menu of start actions: "Share screen"
 * (or pause, stop and viewers while sharing) and "Watch together" (paste a YouTube link). Shown
 * only in the chat of the channel the user is in by voice; disabled entries show their reason.
 */
import { useEffect, useId, useRef, useState, type KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";
import { useSelectedConversation, useServerState, useShownServerId } from "../channels";
import { useEmbedsAllowed } from "../../app/preferences";
import { useOpenPrivacySettings } from "../../app/view";
import { getApi } from "../../api";
import { Viewers, shareBlockText, useOwnShare, useShareActions } from "../screenshare";
import { WatchLinkDialog, useWatchParty } from "../watchparty";
import "./chatActionRow.css";

const APPS_ICON = "M5 5h5v5H5zM14 5h5v5h-5zM5 14h5v5H5zM14 14h5v5h-5z";

export function ChatActionRow() {
  const { t } = useTranslation("chat");
  const { t: tShare } = useTranslation("screenshare");
  const { t: tP2p } = useTranslation("p2p");
  const { t: tParty } = useTranslation("watchparty");
  const serverId = useShownServerId();
  const { conversation } = useSelectedConversation();
  const { users, selfSession, connected } = useServerState();
  const party = useWatchParty(serverId);
  const embeds = useEmbedsAllowed();
  const openPrivacy = useOpenPrivacySettings();
  const [open, setOpen] = useState(false);
  const [menuOpen, setMenuOpen] = useState(false);
  const wrapper = useRef<HTMLDivElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const share = useOwnShare();
  const shareActions = useShareActions(share);
  const menuId = useId();
  const shareErrorId = useId();

  useEffect(() => {
    if (!menuOpen) return;
    wrapper.current?.querySelector<HTMLElement>('[role="menuitem"]')?.focus();
    const onPointerDown = (event: PointerEvent) => {
      if (!(event.target instanceof Node && wrapper.current?.contains(event.target))) {
        setMenuOpen(false);
      }
    };
    document.addEventListener("pointerdown", onPointerDown, true);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown, true);
    };
  }, [menuOpen]);

  const ownChannel = selfSession === null ? undefined : users.get(selfSession)?.channelId;
  if (
    serverId === null ||
    conversation?.kind !== "channel" ||
    ownChannel === undefined ||
    conversation.channelId !== ownChannel
  ) {
    return null;
  }

  // Watch together loads YouTube, so it follows the "link previews and embeds" privacy switch.
  const embedsOff = connected && !embeds;
  const reason = !connected ? t("actions.notConnected") : embedsOff ? tParty("embedsOff") : null;
  const partyRunning = party?.party.channelId === ownChannel;

  const closeMenu = (returnFocus: boolean) => {
    setMenuOpen(false);
    if (returnFocus) trigger.current?.focus();
  };

  const onMenuKey = (event: KeyboardEvent<HTMLDivElement>) => {
    const items = [...event.currentTarget.querySelectorAll<HTMLElement>('[role="menuitem"]')];
    const index = items.findIndex((item) => item === document.activeElement);
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
      case "Home":
        focusAt(0);
        break;
      case "End":
        focusAt(items.length - 1);
        break;
      case "Escape":
      case "Tab":
        closeMenu(true);
        break;
      default:
        return;
    }
    event.preventDefault();
    event.stopPropagation();
  };

  const sharing = share.own;
  const shareReason = share.block === null ? null : shareBlockText(tShare, tP2p, share.block);
  const watchLabel = partyRunning ? t("actions.addToPlaylist") : tParty("watchTogether");

  const item = (
    label: string,
    reasonText: string | null,
    run: () => void,
    extra?: { onFocus?: () => void },
  ) => (
    <button
      type="button"
      role="menuitem"
      tabIndex={-1}
      className="server-menu__item chat-apps__item"
      aria-disabled={reasonText !== null || undefined}
      onFocus={extra?.onFocus}
      onClick={() => {
        if (reasonText !== null) return;
        closeMenu(true);
        run();
      }}
    >
      {label}
      {reasonText !== null && <span className="chat-apps__reason">{reasonText}</span>}
    </button>
  );

  return (
    <div className="chat-apps" ref={wrapper}>
      <button
        ref={trigger}
        type="button"
        className="chat-composer__icon chat-apps__button"
        aria-haspopup="menu"
        aria-expanded={menuOpen}
        aria-controls={menuOpen ? menuId : undefined}
        aria-label={sharing === null ? t("actions.apps") : t("actions.appsLive")}
        data-tooltip={t("actions.apps")}
        onClick={() => {
          setMenuOpen((open) => !open);
        }}
      >
        <svg viewBox="0 0 24 24" width="18" height="18" aria-hidden="true" focusable="false">
          <path d={APPS_ICON} fill="none" stroke="currentColor" strokeWidth="2" />
        </svg>
        {sharing !== null && <span className="chat-apps__live" aria-hidden="true" />}
      </button>
      {menuOpen && (
        <div
          id={menuId}
          className="server-menu chat-apps__menu"
          role="menu"
          aria-label={t("actions.label")}
          tabIndex={-1}
          onKeyDown={onMenuKey}
        >
          {sharing === null ? (
            item(
              shareActions.starting ? tShare("choosing") : tShare("share"),
              shareReason,
              () => void shareActions.start(),
              { onFocus: () => void share.recheck() },
            )
          ) : (
            <>
              {share.serverId !== null &&
                item(
                  sharing.paused ? tShare("resumeSharing") : tShare("pauseSharing"),
                  null,
                  () => {
                    if (share.serverId === null) return;
                    getApi()
                      .screenShareSetPaused(share.serverId, !sharing.paused)
                      .catch((error: unknown) => {
                        console.error("Changing the screen share failed", error);
                      });
                  },
                )}
              {item(tShare("stop"), null, shareActions.stop)}
              {share.serverId !== null && (
                <div
                  role="group"
                  aria-label={tShare("viewersLabel")}
                  className="chat-apps__viewers"
                >
                  <Viewers serverId={share.serverId} own={sharing} />
                </div>
              )}
            </>
          )}
          {item(watchLabel, reason, () => {
            setOpen(true);
          })}
          {embedsOff && openPrivacy !== null && item(tParty("openPrivacy"), null, openPrivacy)}
        </div>
      )}
      {shareActions.failed !== null && (
        <p id={shareErrorId} className="chat-actions__error visually-hidden" role="alert">
          {tShare(shareActions.failed)}
        </p>
      )}
      {open && (
        <WatchLinkDialog
          serverId={serverId}
          onClose={() => {
            setOpen(false);
          }}
        />
      )}
    </div>
  );
}
