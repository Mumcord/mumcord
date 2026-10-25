/**
 * Quick-settings menu on the settings (gear) button: the few switches people flip often, applied
 * and saved right away like in the full settings (FR-1.66, QA.47). Same look and keys as the
 * other context menus.
 */
import {
  useEffect,
  useId,
  useLayoutEffect,
  useRef,
  useState,
  type KeyboardEvent,
  type ReactNode,
} from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import { getApi, type Theme } from "../../api";
import { useSettings } from "../settings/useSettings";
import "./quicksettings.css";

/** Gap kept between the menu and the window edge. */
const EDGE = 8;

const ITEM =
  '[role="menuitemcheckbox"]:not([aria-disabled]), [role="menuitemradio"], [role="menuitem"]';

const THEMES: readonly Theme[] = ["system", "dark", "light"];

const ICON = {
  link: "M10 14a4 4 0 0 0 5.7 0l3-3a4 4 0 0 0-5.7-5.7l-1 1M14 10a4 4 0 0 0-5.7 0l-3 3a4 4 0 0 0 5.7 5.7l1-1",
  music: "M9 18V5l12-2v13M9 18a3 3 0 1 1-6 0a3 3 0 1 1 6 0M21 16a3 3 0 1 1-6 0a3 3 0 1 1 6 0",
  game: "M6 11h4M8 9v4M15 12h.01M18 10h.01M17.3 5H6.7a4 4 0 0 0-3.9 3.2l-1.5 7A2.5 2.5 0 0 0 5.9 17l1.4-2h9.4l1.4 2a2.5 2.5 0 0 0 4.6-1.8l-1.5-7A4 4 0 0 0 17.3 5z",
  system: "M3 5h18v11H3zM8 20h8M12 16v4",
  dark: "M21 12.8A9 9 0 1 1 11.2 3a7 7 0 0 0 9.8 9.8z",
  light:
    "M12 8a4 4 0 1 0 0 8a4 4 0 0 0 0-8zM12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4",
  settings: "M5 12h14M13 6l6 6l-6 6",
} as const;

function Glyph({ d }: { d: string }) {
  return (
    <svg
      className="quick-settings__glyph"
      viewBox="0 0 24 24"
      width="18"
      height="18"
      aria-hidden="true"
      focusable="false"
    >
      <path
        d={d}
        fill="none"
        stroke="currentColor"
        strokeWidth="2"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}

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
  onOpenSettings,
}: {
  menu: QuickSettingsState;
  onClose: () => void;
  onOpenSettings: () => void;
}) {
  const { t } = useTranslation("userpanel");
  const { settings, update } = useSettings();
  const [supported, setSupported] = useState(true);
  const [gamingSupported, setGamingSupported] = useState(true);
  const ref = useRef<HTMLDivElement>(null);
  const titleId = useId();
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
    getApi()
      .nowGamingStatus()
      .then(
        (status) => {
          if (active) setGamingSupported(status.supported);
        },
        (error: unknown) => {
          console.error("Reading the now-gaming status failed", error);
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
      case "ArrowRight":
      case "ArrowLeft": {
        // Inside the theme control the horizontal arrows step through the options.
        const current = items[index];
        if (current?.getAttribute("role") !== "menuitemradio") return;
        const radios = items.filter((item) => item.getAttribute("role") === "menuitemradio");
        const at = radios.indexOf(current);
        const step = event.key === "ArrowRight" ? 1 : -1;
        radios[(at + step + radios.length) % radios.length]?.focus();
        break;
      }
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

  const row = (
    icon: string,
    label: string,
    hint: string,
    checked: boolean,
    onToggle: () => void,
    disabled = false,
  ): ReactNode => (
    <button
      type="button"
      role="menuitemcheckbox"
      aria-checked={checked}
      aria-disabled={disabled || undefined}
      tabIndex={-1}
      className="quick-settings__row"
      onClick={disabled ? undefined : onToggle}
    >
      <Glyph d={icon} />
      <span className="quick-settings__text">
        <span className="quick-settings__label">{label}</span>
        <span className="quick-settings__hint">{hint}</span>
      </span>
      <span className="quick-settings__switch" aria-hidden="true" />
    </button>
  );

  return createPortal(
    <div
      ref={ref}
      className="quick-settings"
      role="menu"
      aria-labelledby={titleId}
      tabIndex={-1}
      onKeyDown={onKeyDown}
    >
      <div id={titleId} className="quick-settings__title">
        {t("quickSettings.label")}
      </div>
      {settings ? (
        <>
          {row(
            ICON.link,
            t("quickSettings.linkPreviews"),
            t("quickSettings.linkPreviewsHint"),
            settings.linkPreviews,
            () => {
              update({ linkPreviews: !settings.linkPreviews });
            },
          )}
          {row(
            ICON.music,
            t("quickSettings.nowPlaying"),
            supported ? t("quickSettings.nowPlayingHint") : t("quickSettings.unsupported"),
            supported && settings.nowPlaying,
            () => {
              update({ nowPlaying: !settings.nowPlaying });
            },
            !supported,
          )}
          {row(
            ICON.game,
            t("quickSettings.nowGaming"),
            gamingSupported ? t("quickSettings.nowGamingHint") : t("quickSettings.unavailable"),
            gamingSupported && settings.nowGaming,
            () => {
              update({ nowGaming: !settings.nowGaming });
            },
            !gamingSupported,
          )}
          <div className="quick-settings__theme">
            <span className="quick-settings__label">{t("quickSettings.theme")}</span>
            <div
              role="group"
              aria-label={t("quickSettings.theme")}
              className="quick-settings__segments"
            >
              {THEMES.map((theme) => (
                <button
                  key={theme}
                  type="button"
                  role="menuitemradio"
                  aria-checked={settings.theme === theme}
                  tabIndex={-1}
                  className="quick-settings__segment"
                  onClick={() => {
                    update({ theme });
                  }}
                >
                  <Glyph d={ICON[theme]} />
                  {t(`quickSettings.theme_${theme}`)}
                </button>
              ))}
            </div>
          </div>
          <button
            type="button"
            role="menuitem"
            tabIndex={-1}
            className="quick-settings__footer"
            onClick={() => {
              onClose();
              onOpenSettings();
            }}
          >
            {t("quickSettings.allSettings")}
            <Glyph d={ICON.settings} />
          </button>
        </>
      ) : null}
    </div>,
    document.body,
  );
}
