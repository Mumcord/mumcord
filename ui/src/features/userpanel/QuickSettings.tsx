/**
 * Quick-settings menu on the settings (gear) button: the few switches people flip often, applied
 * and saved right away like in the full settings (FR-1.66, QA.47). Same look and keys as the
 * other context menus.
 */
import { useEffect, useLayoutEffect, useRef, useState, type KeyboardEvent } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import { getApi, type Theme } from "../../api";
import { useSettings } from "../settings/useSettings";

/** Gap kept between the menu and the window edge. */
const EDGE = 8;

const ITEM = '[role="menuitemcheckbox"]:not([aria-disabled]), [role="menuitemradio"]';

const THEMES: readonly Theme[] = ["system", "dark", "light"];

export interface QuickSettingsState {
  /** Viewport position of the pointer (or of the button for keyboard use). */
  x: number;
  y: number;
  /** Gets the focus back when the menu closes. */
  opener: HTMLElement | null;
}

export function QuickSettings({
  menu,
  onClose,
}: {
  menu: QuickSettingsState;
  onClose: () => void;
}) {
  const { t } = useTranslation("userpanel");
  const { settings, update } = useSettings();
  const [supported, setSupported] = useState(true);
  const ref = useRef<HTMLDivElement>(null);
  const ready = settings !== null;

  const close = (returnFocus: boolean) => {
    onClose();
    if (returnFocus && menu.opener?.isConnected) menu.opener.focus();
  };

  useEffect(() => {
    let active = true;
    getApi()
      .nowPlayingStatus()
      .then(
        (status) => {
          if (active) setSupported(status.supported);
        },
        (error: unknown) => {
          console.error("Reading the now-playing status failed", error);
        },
      );
    return () => {
      active = false;
    };
  }, []);

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
  }, [menu, ready, supported]);

  // Focus the first item once the settings have loaded and the items are enabled.
  useEffect(() => {
    if (ready) ref.current?.querySelector<HTMLElement>(ITEM)?.focus();
  }, [ready]);

  function onKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    const items = [...(ref.current?.querySelectorAll<HTMLElement>(ITEM) ?? [])];
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

  const checkbox = (label: string, checked: boolean, onToggle: () => void, disabled = false) => (
    <button
      type="button"
      role="menuitemcheckbox"
      aria-checked={checked}
      aria-disabled={disabled || undefined}
      tabIndex={-1}
      className="server-menu__item"
      onClick={disabled ? undefined : onToggle}
    >
      {label}
    </button>
  );

  return createPortal(
    <div
      ref={ref}
      className="server-menu"
      role="menu"
      aria-label={t("quickSettings.label")}
      tabIndex={-1}
      onKeyDown={onKeyDown}
    >
      {settings ? (
        <>
          {checkbox(t("quickSettings.linkPreviews"), settings.linkPreviews, () => {
            update({ linkPreviews: !settings.linkPreviews });
          })}
          {checkbox(
            supported
              ? t("quickSettings.nowPlaying")
              : `${t("quickSettings.nowPlaying")} (${t("quickSettings.unsupported")})`,
            supported && settings.nowPlaying,
            () => {
              update({ nowPlaying: !settings.nowPlaying });
            },
            !supported,
          )}
          <div role="group" aria-label={t("quickSettings.theme")}>
            {THEMES.map((theme) => (
              <button
                key={theme}
                type="button"
                role="menuitemradio"
                aria-checked={settings.theme === theme}
                tabIndex={-1}
                className="server-menu__item"
                onClick={() => {
                  update({ theme });
                }}
              >
                {t(`quickSettings.theme_${theme}`)}
              </button>
            ))}
          </div>
        </>
      ) : null}
    </div>,
    document.body,
  );
}
