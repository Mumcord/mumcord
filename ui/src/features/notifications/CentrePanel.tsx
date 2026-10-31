import { useEffect, useId, useLayoutEffect, useRef, useState, type CSSProperties } from "react";
import { useTranslation } from "react-i18next";
import { openSettingsSection } from "../settings/settingsOpener";
import { dndSource, useDnd } from "../dnd/useDnd";
import { displayName } from "../servers/format";
import { useServers } from "../servers/store";
import { CentreEntry } from "./CentreEntry";
import {
  FILTERS,
  filterCounts,
  serverOf,
  visibleItems,
  type CentreFilter,
  type CentreItem,
} from "./centreModel";
import { CENTRE_PANEL_ID, focusBell } from "./centreShortcut";
import { markAllRead, pruneItems, setCentreOpen } from "./centreStore";
import { useCentreItems } from "./useCentreItems";

/** How often "2 min ago" is brought up to date. */
const CLOCK_MS = 30_000;

function useNow(): number {
  const [now, setNow] = useState(Date.now);
  useEffect(() => {
    const timer = setInterval(() => {
      setNow(Date.now());
    }, CLOCK_MS);
    return () => {
      clearInterval(timer);
    };
  }, []);
  return now;
}

/** Below the bell, right edges aligned; top right of the window when the bell is not shown. */
function usePlacement(): CSSProperties {
  const [style, setStyle] = useState<CSSProperties>({});
  useLayoutEffect(() => {
    const place = () => {
      const bell = document.querySelector<HTMLElement>("[data-centre-bell]");
      if (!bell) {
        setStyle({});
        return;
      }
      const rect = bell.getBoundingClientRect();
      setStyle({
        top: `${String(rect.bottom + 8)}px`,
        right: `${String(window.innerWidth - rect.right)}px`,
      });
    };
    place();
    window.addEventListener("resize", place);
    return () => {
      window.removeEventListener("resize", place);
    };
  }, []);
  return style;
}

function close(returnFocus: boolean): void {
  setCentreOpen(false);
  if (returnFocus) focusBell();
}

/** Moves the focus between the entries with the arrow keys, Home and End. */
function moveFocus(panel: HTMLElement, key: string): boolean {
  const entries = [...panel.querySelectorAll<HTMLElement>(".centre-entry__main")];
  if (entries.length === 0) return false;
  const current = entries.findIndex((entry) => entry.contains(document.activeElement));
  let next: number;
  if (key === "Home") next = 0;
  else if (key === "End") next = entries.length - 1;
  else if (key === "ArrowDown") next = current < 0 ? 0 : Math.min(current + 1, entries.length - 1);
  else if (key === "ArrowUp") next = current < 0 ? entries.length - 1 : Math.max(current - 1, 0);
  else return false;
  entries[next]?.focus();
  return true;
}

/**
 * The notification centre panel (#143, FR-3.105–3.107): everything that wants attention across
 * all connected servers, newest first, with filters, "Mark all as read" and the Do Not Disturb
 * state. A non-modal dialog: Escape or a click outside closes it.
 */
export function CentrePanel() {
  const { t } = useTranslation("notifications");
  const titleId = useId();
  const panelRef = useRef<HTMLElement>(null);
  const items = useCentreItems();
  const [filter, setFilter] = useState<CentreFilter>("all");
  const now = useNow();
  const { servers } = useServers();
  const dnd = dndSource(useDnd().state) !== null;
  const placement = usePlacement();
  const counts = filterCounts(items);
  const shown = visibleItems(items, filter);
  const serverName = (item: CentreItem) => {
    const serverId = serverOf(item);
    const saved = servers.find((s) => s.id === serverId);
    return saved ? displayName(saved) : "";
  };

  useEffect(() => {
    pruneItems();
    const panel = panelRef.current;
    if (!panel) return;
    const first = panel.querySelector<HTMLElement>(".centre-entry__main");
    (first ?? panel).focus();
    const onPointerDown = (event: PointerEvent) => {
      const target = event.target as Node;
      const bell = document.querySelector("[data-centre-bell]");
      if (panel.contains(target) || bell?.contains(target)) return;
      close(false);
    };
    // The arrow keys, wherever the focus is inside the panel.
    const onKeyDown = (event: KeyboardEvent) => {
      if (moveFocus(panel, event.key)) event.preventDefault();
    };
    // Escape on the document: "Mark all as read" disables the focused button, which drops the
    // focus to the page body, outside the panel.
    const onEscape = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || event.defaultPrevented) return;
      event.preventDefault();
      close(true);
    };
    document.addEventListener("pointerdown", onPointerDown);
    document.addEventListener("keydown", onEscape);
    panel.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown);
      document.removeEventListener("keydown", onEscape);
      panel.removeEventListener("keydown", onKeyDown);
    };
  }, []);

  return (
    <section
      ref={panelRef}
      id={CENTRE_PANEL_ID}
      className="centre"
      role="dialog"
      aria-modal="false"
      aria-labelledby={titleId}
      tabIndex={-1}
      style={placement}
      data-testid="notification-centre"
    >
      <div className="centre__head">
        <h2 id={titleId} className="centre__title">
          {t("centre.title")}
        </h2>
        <button
          type="button"
          className="centre__mark-all"
          disabled={items.every((item) => item.read)}
          onClick={() => {
            markAllRead(items.filter((item) => item.kind === "request").map((item) => item.id));
          }}
        >
          {t("centre.markAllRead")}
        </button>
      </div>
      <div className="centre__filters" role="group" aria-label={t("centre.filterLabel")}>
        {FILTERS.map((id) => (
          <button
            key={id}
            type="button"
            className="centre__filter"
            aria-pressed={filter === id}
            data-testid={`centre-filter-${id}`}
            onClick={() => {
              setFilter(id);
            }}
          >
            {t(`centre.filter.${id}`)}
            <span className="centre__filter-count">
              <span className="visually-hidden">, </span>
              {counts[id]}
            </span>
          </button>
        ))}
      </div>
      {shown.length === 0 ? (
        <p className="centre__empty">{t("centre.empty")}</p>
      ) : (
        <ul className="centre__list" aria-labelledby={titleId}>
          {shown.map((item) => (
            <CentreEntry key={item.id} item={item} now={now} serverName={serverName(item)} />
          ))}
        </ul>
      )}
      <div className="centre__foot">
        <span className="centre__dnd" data-on={dnd || undefined}>
          {dnd ? t("centre.dndOn") : t("centre.dndOff")}
        </span>
        <button
          type="button"
          className="centre__settings"
          onClick={() => {
            close(false);
            openSettingsSection("sounds");
          }}
        >
          {t("centre.settings")}
        </button>
      </div>
    </section>
  );
}
