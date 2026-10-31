import { useEffect, useId, useMemo, useRef, useState, type KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";
import { useIsMacos } from "../../api/useIsMacos";
import type { MenuPosition } from "../../components/ContextMenu";
import {
  sameConversation,
  selectConversation,
  showServer,
  useSelectedConversation,
  useShownServerId,
} from "../channels";
import { useAllPmRequests } from "../requests";
import { ConversationMenu } from "./ConversationMenu";
import { ConversationRow } from "./ConversationRow";
import { CHAT_ICON, COMPOSE_ICON, Icon, SEARCH_ICON } from "./icons";
import { NewConversationDialog } from "./NewConversationDialog";
import { matches, unreadConversations, type Filter, type Row } from "./overviewModel";
import { RequestsSection, type PendingRequest } from "./RequestsSection";
import { timeLabel } from "./timeLabel";
import { useNow, useOverviewRows } from "./useOverviewRows";
import "./messages.css";

/** Ctrl+K and Ctrl+F (Cmd on macOS) focus the search, like the channel search they replace. */
const isFocusShortcut = (event: globalThis.KeyboardEvent) =>
  (event.ctrlKey || event.metaKey) &&
  !event.altKey &&
  !event.shiftKey &&
  (event.key.toLowerCase() === "k" || event.key.toLowerCase() === "f");

/** Message requests of every connected server, newest first. */
function usePendingRequests(): PendingRequest[] {
  const all = useAllPmRequests();
  return useMemo(
    () =>
      [...all]
        .flatMap(([serverId, r]) => r.incoming.map((request) => ({ serverId, request })))
        .sort(
          (a, b) =>
            Date.parse(b.request.receivedAt) - Date.parse(a.request.receivedAt) ||
            // Same millisecond: the later request (higher id) is the newer one.
            b.request.id - a.request.id,
        ),
    [all],
  );
}

/**
 * The private messages overview (issue #173), in the channel column: every conversation of every
 * connected server in one list, message requests on top, search, filters and quick actions.
 */
export function MessagesOverview() {
  const { t, i18n } = useTranslation("chat");
  const macos = useIsMacos();
  const rows = useOverviewRows();
  const requests = usePendingRequests();
  const now = useNow();
  const shown = useShownServerId();
  const { conversation: selected } = useSelectedConversation();
  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState<Filter>("all");
  const [activeId, setActiveId] = useState<string | null>(null);
  const [menu, setMenu] = useState<{ row: Row; at: MenuPosition } | null>(null);
  const [creating, setCreating] = useState(false);
  const [failed, setFailed] = useState(false);
  const searchRef = useRef<HTMLInputElement>(null);
  const buttons = useRef(new Map<string, HTMLButtonElement>());
  const titleId = useId();
  const searchId = useId();

  useEffect(() => {
    const onKeyDown = (event: globalThis.KeyboardEvent) => {
      if (event.defaultPrevented || !isFocusShortcut(event)) return;
      event.preventDefault();
      searchRef.current?.focus();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => {
      window.removeEventListener("keydown", onKeyDown);
    };
  }, []);

  const searched = rows.filter((row) => matches(row, query));
  const visible = filter === "unread" ? searched.filter((row) => row.unread > 0) : searched;
  const listed = filter === "requests" ? [] : visible;
  const shownRequests =
    query.trim() === ""
      ? requests
      : requests.filter(({ request }) =>
          request.name.toLocaleLowerCase().includes(query.trim().toLocaleLowerCase()),
        );
  const pinned = listed.filter((row) => row.pinned);
  const recent = listed.filter((row) => !row.pinned);
  const active = listed.find((row) => row.id === activeId) ?? listed[0];
  const yesterday = t("overview.yesterday");
  const nothing = rows.length === 0 && requests.length === 0;

  const open = (row: Row) => {
    showServer(row.serverId);
    selectConversation(row.conversation);
  };

  const onListKeyDown = (event: KeyboardEvent<HTMLButtonElement>) => {
    const index = listed.findIndex((row) => row.id === active?.id);
    const next =
      event.key === "ArrowDown"
        ? index + 1
        : event.key === "ArrowUp"
          ? index - 1
          : event.key === "Home"
            ? 0
            : event.key === "End"
              ? listed.length - 1
              : null;
    if (next === null) return;
    const row = listed[Math.max(0, Math.min(listed.length - 1, next))];
    if (!row) return;
    event.preventDefault();
    setActiveId(row.id);
    buttons.current.get(row.id)?.focus();
  };

  const renderRows = (list: readonly Row[]) => (
    <ul className="pm-list">
      {list.map((row) => (
        <li key={row.id}>
          <ConversationRow
            row={row}
            time={row.last ? timeLabel(row.last.at, now, i18n.language, yesterday) : ""}
            current={row.serverId === shown && sameConversation(selected, row.conversation)}
            active={row.id === active?.id}
            buttonRef={(element) => {
              if (element) buttons.current.set(row.id, element);
              else buttons.current.delete(row.id);
            }}
            onOpen={() => {
              open(row);
            }}
            onFocus={() => {
              setActiveId(row.id);
            }}
            onNavigate={onListKeyDown}
            onMenu={(event) => {
              const target = event.currentTarget;
              const rect = target.getBoundingClientRect();
              const pointer = "clientX" in event && event.clientX !== 0;
              setFailed(false);
              setMenu({
                row,
                at: {
                  x: pointer ? event.clientX : rect.left + rect.width / 2,
                  y: pointer ? event.clientY : rect.bottom,
                  opener: target,
                },
              });
            }}
          />
        </li>
      ))}
    </ul>
  );

  const filters: { id: Filter; label: string; count: number | null }[] = [
    { id: "all", label: t("overview.filterAll"), count: null },
    { id: "unread", label: t("overview.filterUnread"), count: unreadConversations(searched) },
    { id: "requests", label: t("overview.filterRequests"), count: shownRequests.length },
  ];

  return (
    <section className="pm-overview" aria-labelledby={titleId} data-testid="pm-overview">
      <div className="pm-overview__head">
        <h2 id={titleId} className="pm-overview__title">
          <Icon path={CHAT_ICON} />
          {t("overview.title")}
        </h2>
        <button
          type="button"
          className="pm-overview__new"
          aria-label={t("overview.new")}
          data-tooltip={t("overview.new")}
          data-testid="pm-new"
          onClick={() => {
            setCreating(true);
          }}
        >
          <Icon path={COMPOSE_ICON} />
        </button>
      </div>
      {!nothing && (
        <>
          <div className="pm-search">
            <Icon path={SEARCH_ICON} className="pm-search__icon" />
            <label className="visually-hidden" htmlFor={searchId}>
              {t("overview.search")}
            </label>
            <input
              ref={searchRef}
              id={searchId}
              type="search"
              className="pm-search__input"
              placeholder={t("overview.search")}
              value={query}
              onChange={(event) => {
                setQuery(event.target.value);
              }}
              onKeyDown={(event) => {
                if (event.key === "Escape" && query !== "") {
                  event.preventDefault();
                  setQuery("");
                }
              }}
            />
            <kbd className="pm-search__key" aria-hidden="true">
              {macos ? "⌘K" : t("overview.searchKey")}
            </kbd>
          </div>
          <div className="pm-filters" role="group" aria-label={t("overview.filterLabel")}>
            {filters.map((f) => (
              <button
                key={f.id}
                type="button"
                className="pm-filter"
                aria-pressed={filter === f.id}
                data-testid={`pm-filter-${f.id}`}
                onClick={() => {
                  setFilter(f.id);
                }}
              >
                {f.label}
                {f.count !== null && f.count > 0 && (
                  <span className="pm-filter__count">
                    <span className="visually-hidden">, </span>
                    {f.count}
                  </span>
                )}
              </button>
            ))}
          </div>
        </>
      )}
      {failed && (
        <p role="alert" className="pm-overview__error">
          {t("overview.failed")}
        </p>
      )}
      {/* The arrow keys move between the rows of every section; Tab leaves the list. */}
      <div className="pm-overview__body">
        {filter !== "unread" && (
          <RequestsSection requests={shownRequests} expanded={filter === "requests"} />
        )}
        {pinned.length > 0 && (
          <section className="pm-section" aria-label={t("overview.pinnedTitle")}>
            <h3 className="pm-section__title">{t("overview.pinnedTitle")}</h3>
            {renderRows(pinned)}
          </section>
        )}
        {recent.length > 0 && (
          <section className="pm-section" aria-label={t("overview.recentTitle")}>
            <h3 className="pm-section__title">{t("overview.recentTitle")}</h3>
            {renderRows(recent)}
          </section>
        )}
        {nothing ? (
          <div className="pm-empty">
            <p className="pm-empty__title">{t("overview.emptyTitle")}</p>
            <p className="pm-empty__text">{t("overview.emptyText")}</p>
            <button
              type="button"
              className="button button--primary"
              onClick={() => {
                setCreating(true);
              }}
            >
              {t("overview.new")}
            </button>
          </div>
        ) : (
          (filter === "requests"
            ? shownRequests.length === 0
            : listed.length === 0 && (filter === "unread" || shownRequests.length === 0)) && (
            <p className="pm-empty__text" role="status">
              {filter === "requests" ? t("overview.noRequests") : t("overview.noMatch")}
            </p>
          )
        )}
      </div>
      {menu && (
        <ConversationMenu
          row={menu.row}
          menu={menu.at}
          onClose={() => {
            setMenu(null);
          }}
          onError={() => {
            setFailed(true);
          }}
        />
      )}
      {creating && (
        <NewConversationDialog
          onClose={() => {
            setCreating(false);
          }}
        />
      )}
    </section>
  );
}
