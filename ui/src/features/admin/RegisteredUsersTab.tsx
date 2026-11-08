/**
 * The Registered users tab of the Server administration window (Epic 4, issue #256, FR-10.52,
 * FR-10.53; the mockups `registered-users-dark` and `registered-users-light`).
 *
 * Search and the three filters on top, a sortable table with the name, the user id, when the
 * account was last seen and where, and a detail panel beside the selected row: the name field,
 * last seen, the groups that name this account, "Ban certificate…" when the server showed one, and
 * "Unregister…" as the one dangerous action.
 *
 * Only the rows on screen are drawn, so a server with thousands of accounts scrolls smoothly
 * (FR-10.53, §9). Arrow keys walk the rows, Enter opens the panel's name field, and every action
 * says what it did out loud for a screen reader.
 *
 * A change goes to the server for one account at a time, when the user saves, and the window then
 * shows the list the server read back — because this server takes a rename onto a held name
 * without saying so (issue #256), the name that was typed is never shown as if it stood.
 */
import {
  useEffect,
  useEffectEvent,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
} from "react";
import { useTranslation } from "react-i18next";
import { useVirtualizer } from "@tanstack/react-virtual";
import { announce } from "../../a11y";
import type { RegisteredUserEntry } from "../../api";
import { ConfirmDialog } from "../servers/ConfirmDialog";
import { initials } from "../servers/format";
import { registeredErrorText } from "./errors";
import {
  countsOf,
  filterRegistered,
  isInactive,
  lastSeenText,
  searchRegistered,
  serverReports,
  sortRegistered,
  type RegisteredColumn,
  type RegisteredFilter,
  type RegisteredSort,
} from "./registeredFormat";
import {
  draftOf,
  dropChanges,
  load,
  pendingCount,
  pendingRename,
  saveChanges,
  select,
  setDraft,
  stageUnregister,
  undoUnregister,
  type RegisteredWindow,
} from "./registeredStore";
import "./admin.css";

/** All columns the tab can show; the two an old server does not report are dropped (FR-10.5). */
const COLUMNS: { id: RegisteredColumn; label: string }[] = [
  { id: "name", label: "columns.name" },
  { id: "userId", label: "columns.id" },
  { id: "lastSeen", label: "columns.lastSeen" },
  { id: "lastChannel", label: "columns.lastChannel" },
];

interface RegisteredUsersTabProps {
  state: RegisteredWindow;
  /** Whether the connection is still up; a dead one leaves the tab read-only (FR-10.58). */
  readOnly: boolean;
  /** The Ban right, which alone lets the panel show a certificate at all (FR-10.65). */
  canBan: boolean;
  /** Id of the line that describes the window while this tab is shown. */
  describedBy?: string | undefined;
  /** Closes the window, asking first when changes are queued. */
  onRequestClose: () => void;
  /** Opens the add-ban form with this account's name and fingerprint (FR-10.51). */
  onBanCertificate: (entry: RegisteredUserEntry) => void;
}

export function RegisteredUsersTab({
  state,
  readOnly,
  canBan,
  describedBy,
  onRequestClose,
  onBanCertificate,
}: RegisteredUsersTabProps) {
  const { t, i18n } = useTranslation(["admin", "common"]);
  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState<RegisteredFilter>("all");
  const [sort, setSort] = useState<RegisteredSort>({ column: "name", ascending: true });
  const [confirmClose, setConfirmClose] = useState(false);
  /** The account whose "Unregister…" is being confirmed (FR-10.55, FR-10.52). */
  const [confirming, setConfirming] = useState<RegisteredUserEntry | null>(null);
  const [focusRequest, setFocusRequest] = useState({ userId: -1, nonce: 0, field: false });
  const rowRefs = useRef(new Map<number, HTMLDivElement>());
  const nameRef = useRef<HTMLInputElement>(null);
  const lastFocusNonce = useRef(0);
  const scrollRef = useRef<HTMLDivElement>(null);
  // Minutes are enough for "3 days ago", and one value per render keeps the cells, the filters
  // and the 90-day rule agreeing.
  const now = Math.floor(Date.now() / 60_000) * 60_000;

  const base = state.entries;
  const counts = useMemo(() => countsOf(base, now), [base, now]);
  const rows = useMemo(
    () => sortRegistered(searchRegistered(filterRegistered(base, filter, now), query), sort),
    [base, filter, query, sort, now],
  );
  const filtering = query.trim() !== "" || filter !== "all";
  const reports = base.length === 0 || base.some(serverReports);
  const columns = reports
    ? COLUMNS
    : COLUMNS.filter((column) => column.id !== "lastSeen" && column.id !== "lastChannel");
  const selected =
    state.selected === null ? null : (base.find((e) => e.userId === state.selected) ?? null);
  const rename = pendingRename(state);
  const pending = pendingCount(state);
  const keeping =
    state.kept === null ? null : (base.find((entry) => entry.userId === state.kept) ?? null);

  // The compiler skips this component: the virtualizer's functions must not be memoized.
  // eslint-disable-next-line react-hooks/incompatible-library
  const virtualizer = useVirtualizer({
    count: rows.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => 44,
    overscan: 8,
    getItemKey: (index) => rows[index]?.userId ?? index,
  });
  // Without a measured scroll box (a headless test, or a hidden state) nothing can be virtualized,
  // so the rows are drawn as they are.
  const items =
    virtualizer.getVirtualItems().length > 0
      ? virtualizer.getVirtualItems()
      : rows.map((entry, index) => ({ index, key: entry.userId, size: 44 }));

  const focusRow = (userId: number, field: boolean) => {
    select(userId);
    setFocusRequest((current) => ({ userId, nonce: current.nonce + 1, field }));
  };

  // Move the focus to the row it belongs to, also after the list re-rendered or scrolled.
  useEffect(() => {
    if (focusRequest.userId < 0 || focusRequest.nonce === lastFocusNonce.current) return;
    const index = rows.findIndex((row) => row.userId === focusRequest.userId);
    if (index < 0) return;
    lastFocusNonce.current = focusRequest.nonce;
    virtualizer.scrollToIndex(index, { align: "auto" });
    const frame = requestAnimationFrame(() => {
      // Enter goes straight to the name field, so typing starts without another click.
      if (focusRequest.field) nameRef.current?.focus();
      else rowRefs.current.get(focusRequest.userId)?.focus();
    });
    return () => {
      cancelAnimationFrame(frame);
    };
  }, [focusRequest, rows, virtualizer]);

  // A new list keeps the focus where it was: on the same account, or on the first one.
  const afterRowsChanged = useEffectEvent(() => {
    const active = selected?.userId ?? null;
    // Only a row keeps its place; a click on the search field or a header stays where it was.
    const listHadFocus = document.activeElement?.closest("[role='row']") !== null;
    if (active !== null && !rows.some((row) => row.userId === active)) select(null);
    if (!listHadFocus) return;
    const target = rows.find((row) => row.userId === active) ?? rows[0];
    if (target) focusRow(target.userId, false);
  });
  useEffect(() => {
    afterRowsChanged();
  }, [rows]);

  const requestClose = () => {
    if (pending > 0) setConfirmClose(true);
    else onRequestClose();
  };

  const save = async () => {
    await saveChanges();
    announce(t("admin:announcedRegisteredSaved"));
  };

  /** Arrow keys walk the list; Enter opens the row's name field. */
  const onListKeyDown = (event: ReactKeyboardEvent<HTMLDivElement>) => {
    if (
      event.target !== event.currentTarget &&
      event.key !== "ArrowUp" &&
      event.key !== "ArrowDown"
    ) {
      return;
    }
    if (rows.length === 0) return;
    const index = selected === null ? -1 : rows.indexOf(selected);
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      const step = event.key === "ArrowDown" ? 1 : -1;
      const next = rows[Math.min(rows.length - 1, Math.max(0, index + step))];
      if (next) focusRow(next.userId, false);
      return;
    }
    if (event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      const row = selected ?? rows[0];
      if (row) focusRow(row.userId, true);
    }
  };

  const seen = (entry: RegisteredUserEntry) =>
    lastSeenText(entry, i18n.language, now, {
      online: t("admin:onlineNow"),
      unknown: t("admin:lastSeenUnknown"),
    });

  const queued = (entry: RegisteredUserEntry) =>
    state.removals.includes(entry.userId) || rename?.userId === entry.userId;

  return (
    <>
      <div className="registered">
        <div className="registered__main">
          {readOnly ? null : (
            <>
              <div className="admin-toolbar">
                <label className="admin-field">
                  <span>{t("admin:registeredSearch")}</span>
                  <input
                    type="search"
                    value={query}
                    onChange={(event) => {
                      setQuery(event.target.value);
                    }}
                  />
                </label>
                <div className="admin-chips" role="group" aria-label={t("admin:filter")}>
                  {(["all", "online", "inactive"] as const).map((option) => (
                    <button
                      key={option}
                      type="button"
                      className="button button--small"
                      aria-pressed={filter === option}
                      onClick={() => {
                        setFilter(option);
                      }}
                    >
                      {`${
                        option === "all"
                          ? t("admin:filterRegisteredAll")
                          : option === "online"
                            ? t("admin:filterRegisteredOnline")
                            : t("admin:filterRegisteredInactive")
                      } (${String(counts[option])})`}
                    </button>
                  ))}
                </div>
                <button
                  type="button"
                  className="button"
                  onClick={() => void load()}
                  disabled={state.loading}
                >
                  {t("admin:refresh")}
                </button>
              </div>
              <p className="admin-count" role="status" id={describedBy}>
                {t("admin:shownOfTotalUsers", { shown: rows.length, total: counts.all })}
              </p>
            </>
          )}

          {state.error ? (
            <p className="admin-error" role="status">
              {registeredErrorText(t, state.error)}{" "}
              <button type="button" className="button button--small" onClick={() => void load()}>
                {t("common:retry")}
              </button>
            </p>
          ) : null}

          {keeping ? (
            <p className="admin-notice admin-notice--danger" role="status">
              {t("admin:renameKept", { name: keeping.name })}
            </p>
          ) : null}

          {state.saved && state.kept === null ? (
            <p className="admin-notice" role="status">
              {t("admin:registeredSaved", {
                renamed: state.saved.renamed,
                unregistered: state.saved.unregistered,
              })}
            </p>
          ) : null}

          <div className="admin-scroll registered__scroll" ref={scrollRef}>
            <div
              className="admin-table admin-table--registered"
              role="table"
              aria-label={`${t("admin:registeredTab")} (${String(counts.all)})`}
              aria-busy={state.loading}
            >
              <div className="admin-row admin-row--head" role="row">
                {columns.map((column) => (
                  <div
                    key={column.id}
                    className="admin-cell admin-cell--head"
                    role="columnheader"
                    aria-sort={
                      sort.column === column.id
                        ? sort.ascending
                          ? "ascending"
                          : "descending"
                        : "none"
                    }
                  >
                    <button
                      type="button"
                      className="admin-sort"
                      onClick={() => {
                        setSort((current) => ({
                          column: column.id,
                          ascending: current.column === column.id ? !current.ascending : true,
                        }));
                      }}
                    >
                      {t(`admin:${column.label}`)}
                    </button>
                  </div>
                ))}
                <div className="admin-cell admin-cell--head" role="columnheader">
                  <span className="visually-hidden">{t("admin:actions")}</span>
                </div>
              </div>

              {state.loading && rows.length === 0 ? (
                <>
                  <p className="visually-hidden" role="status">
                    {t("common:loading")}
                  </p>
                  {[0, 1, 2, 3].map((line) => (
                    <div key={line} className="admin-row admin-row--skeleton" aria-hidden="true">
                      {columns.map((column) => (
                        <div key={column.id} className="admin-cell">
                          <span className="admin-skeleton" />
                        </div>
                      ))}
                      <div className="admin-cell" />
                    </div>
                  ))}
                </>
              ) : null}

              {!state.loading && rows.length === 0 ? (
                <p className="admin-empty">
                  {filtering ? t("admin:emptySearchUsers") : t("admin:emptyUsers")}
                </p>
              ) : null}

              {items.map((item) => {
                const entry = rows[item.index];
                if (!entry) return null;
                const active = selected?.userId === entry.userId;
                const removing = state.removals.includes(entry.userId);
                return (
                  <div
                    key={item.key}
                    ref={(element) => {
                      if (element) rowRefs.current.set(entry.userId, element);
                      else rowRefs.current.delete(entry.userId);
                    }}
                    className={`admin-row${active ? " admin-row--active" : ""}${
                      queued(entry) ? " admin-row--pending" : ""
                    }`}
                    role="row"
                    aria-selected={active}
                    // The list is one tab stop: while nothing is opened, the first row is it, so
                    // the arrow keys have somewhere to start (FR-10.53).
                    tabIndex={active || (selected === null && item.index === 0) ? 0 : -1}
                    data-inactive={isInactive(entry, now) || undefined}
                    data-removed={removing || undefined}
                    style={{ height: `${String(item.size)}px` }}
                    onFocus={() => {
                      select(entry.userId);
                    }}
                    onClick={() => {
                      select(entry.userId);
                    }}
                    onKeyDown={onListKeyDown}
                  >
                    <div className="admin-cell admin-cell--name" role="cell">
                      <span
                        className={`registered__mark${entry.online ? " registered__mark--online" : ""}`}
                        aria-hidden="true"
                      >
                        {initials(entry.name)}
                      </span>
                      <span
                        className="admin-cell__text"
                        data-tooltip={entry.name || undefined}
                        data-tooltip-overflow
                      >
                        {entry.name || t("admin:noName")}
                      </span>
                      {queued(entry) ? (
                        <span className="admin-badge admin-badge--pending">
                          {t("admin:pending")}
                        </span>
                      ) : null}
                    </div>
                    <div className="admin-cell admin-cell--id" role="cell">
                      {String(entry.userId)}
                    </div>
                    {reports ? (
                      <>
                        <div className="admin-cell" role="cell">
                          <span className={entry.online ? "registered__online" : undefined}>
                            {seen(entry)}
                          </span>
                        </div>
                        <div className="admin-cell" role="cell">
                          <span className="admin-cell__text">{entry.lastChannel ?? "—"}</span>
                        </div>
                      </>
                    ) : null}
                    <div className="admin-cell admin-cell--actions" role="cell">
                      <button
                        type="button"
                        className="button button--small admin-row__more"
                        aria-label={t("admin:changeNameOf", {
                          name: entry.name || t("admin:noName"),
                        })}
                        tabIndex={active ? 0 : -1}
                        disabled={readOnly}
                        onClick={() => {
                          focusRow(entry.userId, true);
                        }}
                      >
                        ⋯
                      </button>
                    </div>
                  </div>
                );
              })}
            </div>
          </div>
        </div>

        {selected ? (
          <aside
            className="registered__panel"
            aria-label={t("admin:detailsOf", { name: selected.name || t("admin:noName") })}
          >
            <h3 className="registered__panel-name">{selected.name || t("admin:noName")}</h3>
            <p className="registered__panel-line">
              {t("admin:userIdLine", {
                id: String(selected.userId),
                state: selected.online ? t("admin:online") : t("admin:offline"),
              })}
            </p>
            <label className="admin-field">
              <span>{t("admin:name")}</span>
              <input
                ref={nameRef}
                type="text"
                value={draftOf(state, selected)}
                maxLength={100}
                disabled={readOnly}
                onChange={(event) => {
                  setDraft(event.target.value);
                }}
              />
            </label>
            <p className="registered__help">
              {t("admin:renameHelp", { name: selected.name || t("admin:noName") })}
            </p>
            <p className="registered__row">
              <span className="registered__row-label">{t("admin:columns.lastSeen")}</span>
              <span>
                {seen(selected)}
                {selected.lastChannel ? ` · ${selected.lastChannel}` : ""}
              </span>
            </p>
            {state.groups && state.groups.length > 0 ? (
              <p className="registered__row">
                <span className="registered__row-label">{t("admin:groups")}</span>
                <span className="registered__groups">
                  {state.groups.map((group) => (
                    <span key={group} className="registered__group">
                      {group}
                    </span>
                  ))}
                </span>
              </p>
            ) : null}
            {canBan && selected.certHash ? (
              <button
                type="button"
                className="button registered__ban"
                disabled={readOnly}
                onClick={() => {
                  onBanCertificate(selected);
                }}
              >
                {t("admin:banCertificate")}
              </button>
            ) : null}
            {state.removals.includes(selected.userId) ? (
              <button
                type="button"
                className="button registered__undo"
                disabled={readOnly || state.busy}
                onClick={() => {
                  undoUnregister(selected.userId);
                }}
              >
                {t("admin:keepRegistration")}
              </button>
            ) : (
              <button
                type="button"
                className="button button--danger registered__unregister"
                disabled={readOnly}
                onClick={() => {
                  setConfirming(selected);
                }}
              >
                {t("admin:unregister")}
              </button>
            )}
            <p className="registered__help">
              {t("admin:unregisterNote", { name: selected.name || t("admin:noName") })}
            </p>
          </aside>
        ) : null}
      </div>

      <div className="admin-footer">
        <span className="admin-footer__count">
          {pending === 0 ? null : (
            <>
              <span className="admin-badge">{String(pending)}</span>{" "}
              {t("admin:changesNotSavedUsers")}
            </>
          )}
        </span>
        <span className="admin-footer__note">{t("admin:perUserSave")}</span>
        <div className="admin-footer__buttons">
          <button
            type="button"
            className="button"
            onClick={requestClose}
            disabled={pending > 0 ? false : state.busy}
          >
            {t("common:close")}
          </button>
          <button
            type="button"
            className="button button--primary"
            onClick={() => void save()}
            disabled={readOnly || pending === 0 || state.busy}
          >
            {t("admin:save")}
          </button>
        </div>
      </div>

      {confirming ? (
        <ConfirmDialog
          title={t("admin:unregisterQuestion", {
            name: confirming.name || t("admin:noName"),
          })}
          message={t("admin:unregisterDetails")}
          confirmLabel={t("admin:unregister")}
          danger
          onConfirm={() => {
            stageUnregister(confirming.userId);
            setConfirming(null);
            announce(t("admin:announcedStaged"));
          }}
          onClose={() => {
            setConfirming(null);
          }}
        />
      ) : null}

      {confirmClose ? (
        <ConfirmDialog
          title={t("admin:discardTitle", { count: pending })}
          message={t("admin:discardNote")}
          confirmLabel={t("admin:discardAndClose")}
          danger
          onConfirm={() => {
            dropChanges();
            setConfirmClose(false);
            onRequestClose();
          }}
          onClose={() => {
            setConfirmClose(false);
          }}
        />
      ) : null}
    </>
  );
}
