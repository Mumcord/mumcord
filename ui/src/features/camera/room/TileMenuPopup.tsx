/**
 * The tile menu itself (spec 08 §5.3, mockup 10): Pin for me (P), Show large (F), Hide {name}'s
 * video (H) and Quality for me (Auto / Data saver / Best) with the footer; on the self-view Hide
 * self-view and Background instead of hide and quality. It opens below the ⋯ button, inside the
 * camera room so it stays visible in full screen; arrow keys move, the letter keys pick, Escape or
 * Tab closes and returns the focus.
 */
import { useEffect, useId, useLayoutEffect, useRef, type KeyboardEvent } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import type { CameraTileQuality } from "../../../api";
import { RoomIcon, type RoomIconName } from "./icons";
import type { TileKey } from "./layout";
import { closeTileMenu, setQuality, toggleHidden, toggleLarge, togglePin } from "./TileMenuActions";
import type { TileModel } from "./useRoomModel";

/** Gap kept between the menu and the window edge. */
const EDGE = 8;
/** The menu overlaps the ⋯ button's lower right a little, as in mockup 10. */
const OVERLAP = 4;

const QUALITIES = [
  { value: "auto", label: "tileMenu.qualityAuto", hint: "tileMenu.qualityAutoHint" },
  { value: "dataSaver", label: "tileMenu.qualityDataSaver", hint: "tileMenu.qualityDataSaverHint" },
  { value: "best", label: "tileMenu.qualityBest", hint: "tileMenu.qualityBestHint" },
] as const satisfies readonly { value: CameraTileQuality; label: string; hint: string }[];

interface Action {
  label: string;
  icon: RoomIconName;
  /** The letter key, shown as a hint and working inside the menu too. */
  shortcut?: string;
  run: () => void;
}

export interface TileMenuPopupProps {
  serverId: number;
  tile: TileModel;
  large: TileKey | null;
  label: string;
  opener: HTMLElement;
  onBackground: (() => void) | null;
}

export function TileMenuPopup({
  serverId,
  tile,
  large,
  label,
  opener,
  onBackground,
}: TileMenuPopupProps) {
  const { t } = useTranslation("camera");
  const ref = useRef<HTMLDivElement>(null);
  const titleId = useId();
  const self = tile.state === "self";

  const actions: Action[] = [
    {
      label: t(tile.pinned ? "tileMenu.unpin" : "tileMenu.pin"),
      icon: "pin",
      shortcut: "P",
      run: () => {
        togglePin(tile);
      },
    },
    {
      label: t(large === tile.key ? "tileMenu.leaveLarge" : "tileMenu.showLarge"),
      icon: "fullscreen",
      shortcut: "F",
      run: () => {
        toggleLarge(tile, large);
      },
    },
  ];
  if (self) {
    actions.push({
      label: t("room.hideSelf"),
      icon: "eyeOff",
      shortcut: "H",
      run: () => {
        toggleHidden(serverId, tile);
      },
    });
    if (onBackground !== null)
      actions.push({ label: t("tileMenu.selfBackground"), icon: "background", run: onBackground });
  } else {
    const hidden = tile.state === "hidden";
    actions.push({
      label: t(hidden ? "tileMenu.showVideo" : "tileMenu.hideVideo", { name: tile.name }),
      icon: hidden ? "eye" : "eyeOff",
      shortcut: "H",
      run: () => {
        toggleHidden(serverId, tile);
      },
    });
  }

  const close = () => {
    closeTileMenu();
    if (opener.isConnected) opener.focus();
  };
  const pick = (run: () => void) => {
    close();
    run();
  };

  useEffect(() => {
    const dismiss = () => {
      closeTileMenu();
    };
    const onPointerDown = (event: PointerEvent) => {
      const target = event.target;
      if (!(target instanceof Node)) return;
      // The ⋯ button toggles the menu itself.
      if (ref.current?.contains(target) || opener.contains(target)) return;
      dismiss();
    };
    document.addEventListener("pointerdown", onPointerDown, true);
    window.addEventListener("blur", dismiss);
    window.addEventListener("resize", dismiss);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown, true);
      window.removeEventListener("blur", dismiss);
      window.removeEventListener("resize", dismiss);
    };
  }, [opener]);

  useLayoutEffect(() => {
    const element = ref.current;
    if (element === null) return;
    // Opened from the tile with the keyboard: anchor at the tile's ⋯ button all the same.
    const anchor = opener.querySelector<HTMLElement>(".cam-tile__more") ?? opener;
    const rect = anchor.getBoundingClientRect();
    const left = Math.max(
      EDGE,
      Math.min(rect.left + OVERLAP, window.innerWidth - element.offsetWidth - EDGE),
    );
    const below = rect.bottom - OVERLAP;
    const top =
      below + element.offsetHeight <= window.innerHeight - EDGE
        ? below
        : Math.max(EDGE, rect.top + OVERLAP - element.offsetHeight);
    element.style.left = `${String(left)}px`;
    element.style.top = `${String(top)}px`;
    element.querySelector<HTMLElement>("[role^='menuitem']")?.focus();
  }, [opener]);

  function onKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    const items = [...(ref.current?.querySelectorAll<HTMLElement>("[role^='menuitem']") ?? [])];
    const index = items.findIndex((item) => item === document.activeElement);
    const focusAt = (next: number) => {
      items[(next + items.length) % items.length]?.focus();
    };
    const letter = event.key.toUpperCase();
    const shortcut =
      event.ctrlKey || event.altKey || event.metaKey
        ? undefined
        : actions.find((action) => action.shortcut === letter);
    if (shortcut !== undefined) pick(shortcut.run);
    else if (event.key === "ArrowDown") focusAt(index + 1);
    else if (event.key === "ArrowUp") focusAt(index - 1);
    else if (event.key === "Home") focusAt(0);
    else if (event.key === "End") focusAt(items.length - 1);
    else if (event.key === "Escape" || event.key === "Tab") close();
    else return;
    event.preventDefault();
    event.stopPropagation();
  }

  const host = opener.closest<HTMLElement>(".cam-room") ?? document.body;
  return createPortal(
    <div
      ref={ref}
      className="cam-menu"
      role="menu"
      aria-label={label}
      tabIndex={-1}
      onKeyDown={onKeyDown}
    >
      {actions.map((action) => (
        <button
          key={action.icon}
          type="button"
          role="menuitem"
          tabIndex={-1}
          className="cam-menu__item"
          aria-keyshortcuts={action.shortcut}
          onClick={() => {
            pick(action.run);
          }}
        >
          <RoomIcon name={action.icon} />
          {action.label}
          {action.shortcut !== undefined && <kbd aria-hidden="true">{action.shortcut}</kbd>}
        </button>
      ))}
      {!self && (
        <>
          <hr className="cam-menu__sep" />
          <p id={titleId} className="cam-menu__title">
            {t("tileMenu.quality")}
          </p>
          <div role="group" aria-labelledby={titleId} className="cam-menu__group">
            {QUALITIES.map((quality) => {
              const checked = tile.quality === quality.value;
              return (
                <button
                  key={quality.value}
                  type="button"
                  role="menuitemradio"
                  tabIndex={-1}
                  aria-checked={checked}
                  className="cam-menu__item"
                  onClick={() => {
                    pick(() => {
                      setQuality(serverId, tile, quality.value);
                    });
                  }}
                >
                  {checked && <RoomIcon name="check" />}
                  {t(quality.label)}
                  <span className="cam-menu__hint">{t(quality.hint)}</span>
                </button>
              );
            })}
          </div>
          <p className="cam-menu__foot">{t("tileMenu.footer")}</p>
        </>
      )}
    </div>,
    host,
  );
}
