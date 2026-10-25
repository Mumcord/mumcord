/**
 * Context menu on a person (FR-1.18, QA.32): View profile, local mute, friend (FR-1.64), volume
 * slider and private message. Reuses the profile card's volume slider and local-mute action, so both
 * places store the same per-certificate settings. Yourself only gets "View profile".
 */
import {
  useEffect,
  useLayoutEffect,
  useRef,
  useSyncExternalStore,
  type KeyboardEvent,
  type ReactNode,
} from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import { getApi, toEngineError } from "../../api";
import { useServerState } from "../channels/serverStore";
import { setFriend, useFriend } from "../friends";
import "../userpanel/quicksettings.css";
import "./members.css";
import { findPerson } from "./personLookup";
import { VolumeSlider } from "./ProfileCard";
import { openPrivateConversation } from "./privateMessage";
import {
  closePersonMenu,
  currentPersonMenu,
  subscribePersonMenu,
  type OpenPersonMenu,
} from "./personMenuStore";
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
  onViewProfile: (session: number, opener: HTMLElement | null) => void;
}) {
  const { t } = useTranslation("members");
  const { t: tFriends } = useTranslation("friends");
  const state = useServerState();
  const user = findPerson(state.users, open.session, open.name);
  const friend = useFriend(user?.certHash ?? null);
  const ref = useRef<HTMLDivElement>(null);

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

  return (
    <div
      ref={ref}
      className="server-menu person-menu"
      role="menu"
      aria-label={t("menu.label", { name: user?.name ?? open.name })}
      tabIndex={-1}
      onKeyDown={onKeyDown}
    >
      {user ? (
        item("profile", <IdentityCardIcon />, t("menu.viewProfile"), () => {
          closePersonMenu();
          onViewProfile(user.session, open.opener);
        })
      ) : (
        // Not on the server: there is no profile to open and nothing to mute.
        <button
          type="button"
          role="menuitem"
          aria-disabled="true"
          tabIndex={-1}
          className="server-menu__item person-menu__item"
        >
          <MenuIcon>
            <IdentityCardIcon />
          </MenuIcon>
          {t("menu.viewProfile")}
        </button>
      )}
      {user && !isSelf && (
        <>
          {item("message", <ChatBubbleIcon />, t("card.sendPrivateMessage"), () => {
            closePersonMenu();
            openPrivateConversation(user);
          })}
          <div role="separator" className="person-menu__separator" />
          {/* Same switch row as the quick settings; the menu stays open while it toggles. */}
          <button
            type="button"
            role="menuitemcheckbox"
            aria-checked={user.localMute}
            tabIndex={-1}
            className="quick-settings__row"
            onClick={() => {
              getApi().personSetLocalMute(user.session, !user.localMute).catch(report);
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
          {/* People without a certificate can't be recognised again, so they can't be friends. */}
          <button
            type="button"
            role="menuitemcheckbox"
            aria-checked={friend !== undefined}
            aria-disabled={user.certHash === null}
            tabIndex={-1}
            className="quick-settings__row"
            onClick={() => {
              if (user.certHash === null) return;
              setFriend(user, friend === undefined).catch(report);
            }}
          >
            <MenuIcon>
              <path d="M12 21s-7-4.5-9.5-9A5.5 5.5 0 0 1 12 6a5.5 5.5 0 0 1 9.5 6c-2.5 4.5-9.5 9-9.5 9z" />
            </MenuIcon>
            <span className="quick-settings__text">
              <span className="quick-settings__label">{tFriends("friend")}</span>
              {user.certHash === null && (
                <span className="quick-settings__hint">{tFriends("noCertificate")}</span>
              )}
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
