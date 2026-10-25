/**
 * Context menu on an inline image: save it as a GIF favorite (or remove it) and open it in the
 * browser (FR-1.52, QA.43). Same look and keys as the other context menus.
 */
import { useEffect, useLayoutEffect, useRef, type KeyboardEvent } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import { removeFavorite, saveFavorite, useGifFavorites } from "./favorites";
import { openInBrowser } from "./openLink";

/** Gap kept between the menu and the window edge. */
const EDGE = 8;

export interface ImageMenuState {
  url: string;
  name: string;
  /** Viewport position of the pointer (or of the image for keyboard use). */
  x: number;
  y: number;
  /** Gets the focus back when the menu closes. */
  opener: HTMLElement | null;
}

export function ImageMenu({ menu, onClose }: { menu: ImageMenuState; onClose: () => void }) {
  const { t } = useTranslation("chat");
  const favorites = useGifFavorites();
  const saved = favorites.includes(menu.url);
  const ref = useRef<HTMLDivElement>(null);

  const close = (returnFocus: boolean) => {
    onClose();
    if (returnFocus && menu.opener?.isConnected) menu.opener.focus();
  };

  useEffect(() => {
    const dismiss = () => {
      onClose();
    };
    const onPointerDown = (event: PointerEvent) => {
      if (!(event.target instanceof Node && ref.current?.contains(event.target))) dismiss();
    };
    document.addEventListener("pointerdown", onPointerDown, true);
    window.addEventListener("blur", dismiss);
    window.addEventListener("resize", dismiss);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown, true);
      window.removeEventListener("blur", dismiss);
      window.removeEventListener("resize", dismiss);
    };
  }, [onClose]);

  useLayoutEffect(() => {
    const element = ref.current;
    if (!element) return;
    const left = Math.max(EDGE, Math.min(menu.x, window.innerWidth - element.offsetWidth - EDGE));
    const top = Math.max(EDGE, Math.min(menu.y, window.innerHeight - element.offsetHeight - EDGE));
    element.style.left = `${String(left)}px`;
    element.style.top = `${String(top)}px`;
    element.querySelector<HTMLElement>('[role="menuitem"]')?.focus();
  }, [menu]);

  function onKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    const items = [...(ref.current?.querySelectorAll<HTMLElement>('[role="menuitem"]') ?? [])];
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
        close(true);
        break;
      default:
        return;
    }
    event.preventDefault();
    event.stopPropagation();
  }

  const item = (label: string, onSelect: () => void) => (
    <button
      type="button"
      role="menuitem"
      tabIndex={-1}
      className="server-menu__item"
      onClick={() => {
        close(true);
        onSelect();
      }}
    >
      {label}
    </button>
  );

  return createPortal(
    <div
      ref={ref}
      className="server-menu"
      role="menu"
      aria-label={t("imageMenu.label", { name: menu.name })}
      tabIndex={-1}
      onKeyDown={onKeyDown}
    >
      {saved
        ? item(t("favorites.remove"), () => {
            removeFavorite(menu.url);
          })
        : item(t("favorites.save"), () => {
            saveFavorite(menu.url);
          })}
      {item(t("viewer.openInBrowser"), () => {
        openInBrowser(menu.url);
      })}
    </div>,
    document.body,
  );
}
