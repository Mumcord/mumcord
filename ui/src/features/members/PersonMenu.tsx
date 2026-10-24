/**
 * Context menu on a person (FR-1.18, QA.32): View profile, local mute, volume slider and
 * private message. Reuses the profile card's volume slider and local-mute action, so both
 * places store the same per-certificate settings. Yourself only gets "View profile".
 */
import {
  useEffect,
  useLayoutEffect,
  useRef,
  useSyncExternalStore,
  type KeyboardEvent,
} from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import { getApi, toEngineError } from "../../api";
import { useServerState } from "../channels/serverStore";
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

const ITEMS = '[role="menuitem"], input[type="range"]';

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
  const state = useServerState();
  const user = findPerson(state.users, open.session, open.name);
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

  const item = (id: string, label: string, onSelect: () => void) => (
    <button
      key={id}
      type="button"
      role="menuitem"
      tabIndex={-1}
      className="server-menu__item"
      onClick={onSelect}
    >
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
        item("profile", t("menu.viewProfile"), () => {
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
          className="server-menu__item"
        >
          {t("menu.viewProfile")}
        </button>
      )}
      {user && !isSelf && (
        <>
          {item("mute", user.localMute ? t("menu.unmute") : t("menu.mute"), () => {
            closePersonMenu();
            getApi().personSetLocalMute(user.session, !user.localMute).catch(report);
            open.opener?.focus();
          })}
          <div className="person-menu__volume">
            <VolumeSlider user={user} onError={report} />
          </div>
          {item("message", t("card.sendPrivateMessage"), () => {
            closePersonMenu();
            openPrivateConversation(user);
          })}
        </>
      )}
    </div>
  );
}
