/**
 * The Server administration state, ban list (Epic 4, issue #254, FR-10.50 ff.; polish in #436).
 *
 * Follows the mockup: search and filters on top, the list with sortable columns, one form for
 * adding and changing an entry, and one save that sends the whole list. The save first re-reads
 * the server's list, so a change someone else made meanwhile is asked about before anything is
 * sent (FR-10.66). Fingerprints are shortened and addresses masked, and only this window shows
 * them (FR-10.51, FR-10.65). Only the rows that are visible are drawn, so a list of thousands
 * scrolls smoothly (FR-10.53). Arrow keys move through the rows; the window keeps the focus where
 * the user was when the list changes.
 */
import {
  useEffect,
  useEffectEvent,
  useId,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
} from "react";
import { useTranslation } from "react-i18next";
import { useVirtualizer } from "@tanstack/react-virtual";
import { announce } from "../../a11y";
import { getApi, type BanChangeInput, type BanEntry } from "../../api";
import { ConfirmDialog } from "../servers/ConfirmDialog";
import { Dialog } from "../servers/Dialog";
import { displayName, initials } from "../servers/format";
import { useServers } from "../servers/store";
import { errorText } from "./errors";
import {
  endTime,
  filterBans,
  maskAddress,
  plain,
  searchBans,
  shortHash,
  sortBans,
} from "./banFormat";
import {
  changeKey,
  closeServerAdmin,
  discardChanges,
  load,
  pendingFor,
  reveal,
  saveChanges,
  stageChange,
  useServerAdmin,
  type BanWindow,
} from "./banStore";
import { BanEditDialog, type BanDraft } from "./BanEditDialog";
import { ConflictDialog } from "./ConflictDialog";
import "./admin.css";

const COLUMNS: { id: SortColumn; label: string }[] = [
  { id: "name", label: "columns.name" },
  { id: "certHash", label: "columns.certificate" },
  { id: "address", label: "columns.ipAddress" },
  { id: "reason", label: "columns.reason" },
  { id: "start", label: "columns.banned" },
  { id: "ends", label: "columns.ends" },
];

type SortColumn = "name" | "certHash" | "address" | "reason" | "start" | "ends";
type SortState = { column: SortColumn; ascending: boolean };
type Filter = "all" | "active" | "expired";
type Form = { kind: "add" } | { kind: "edit"; entry: BanEntry };
type SavedCounts = { added: number; edited: number; removed: number };

export function ServerAdminWindowHost() {
  const state = useServerAdmin();
  return state ? <ServerAdminWindow state={state} /> : null;
}

/** The loaded entries with the pending changes, as one list before filter, search and sort. */
function mergedEntries(state: BanWindow): BanEntry[] {
  const merged = (entry: BanEntry): BanEntry => {
    const change = state.changes.get(entry.key);
    if (change?.kind !== "edit") return entry;
    return { ...entry, reason: change.reason, durationSecs: change.duration };
  };
  const added = [...state.changes.values()]
    .filter((change) => change.kind === "add")
    .map((change) => ({
      key: changeKey(change),
      name: change.name,
      certHash: change.hash,
      address: change.address,
      mask: change.mask,
      reason: change.reason,
      start: new Date().toISOString(),
      durationSecs: change.duration,
      expired: false,
    }));
  return [...state.entries.map(merged), ...added];
}

function countsOf(changes: BanWindow["changes"]): SavedCounts {
  const counts = { added: 0, edited: 0, removed: 0 };
  for (const change of changes.values()) {
    if (change.kind === "add") counts.added += 1;
    else if (change.kind === "edit") counts.edited += 1;
    else counts.removed += 1;
  }
  return counts;
}

function ServerAdminWindow({ state }: { state: BanWindow }) {
  const { t, i18n } = useTranslation(["admin", "common", "servers"]);
  const servers = useServers();
  const server = servers.servers.find((s) => s.id === state.serverId);
  const serverName = server ? displayName(server) : t("servers:unknownServer");
  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState<Filter>("all");
  const [sort, setSort] = useState<SortState>({ column: "name", ascending: true });
  const [selected, setSelected] = useState<string | null>(null);
  const [form, setForm] = useState<Form | null>(null);
  const [removing, setRemoving] = useState<BanEntry | null>(null);
  const [confirmClose, setConfirmClose] = useState(false);
  const [saved, setSaved] = useState<SavedCounts | null>(null);
  /** Changes the ending connection took with it, so the window can say so (issue #436). */
  const [dropped, setDropped] = useState(0);
  const [focusRequest, setFocusRequest] = useState({ key: "", nonce: 0 });
  const rowRefs = useRef(new Map<string, HTMLDivElement>());
  const lastFocusNonce = useRef(0);
  const scrollRef = useRef<HTMLDivElement>(null);
  const descId = useId();

  // A connection that ends turns the window read-only and keeps nothing unsaved (FR-10.58).
  const connection = servers.connections.get(state.serverId);
  const connected = !servers.ready || connection?.state === "connected";
  const readOnly = !connected;

  const base = useMemo(() => mergedEntries(state), [state]);
  const counts = useMemo(
    () => ({
      all: base.length,
      active: base.filter((entry) => !entry.expired).length,
      expired: base.filter((entry) => entry.expired).length,
    }),
    [base],
  );
  const rows = useMemo(
    () => sortBans(searchBans(filterBans(base, filter), query), sort),
    [base, filter, query, sort],
  );
  const filtering = query.trim() !== "" || filter !== "all";

  // The compiler skips this component: the virtualizer's functions must not be memoized.
  // eslint-disable-next-line react-hooks/incompatible-library
  const virtualizer = useVirtualizer({
    count: rows.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => 44,
    overscan: 8,
    getItemKey: (index) => rows[index]?.key ?? index,
  });
  // Without a measured scroll box (a headless test, or a hidden state) nothing can be
  // virtualized, so the rows are drawn as they are.
  const items =
    virtualizer.getVirtualItems().length > 0
      ? virtualizer.getVirtualItems()
      : rows.map((entry, index) => ({ index, key: entry.key, size: 44 }));

  const selectedEntry = rows.find((entry) => entry.key === selected) ?? rows[0] ?? null;

  const date = (value: string | null) =>
    value === null
      ? "—"
      : new Intl.DateTimeFormat(i18n.language, { dateStyle: "medium" }).format(new Date(value));

  const endsText = (entry: BanEntry) => {
    if (entry.expired) return t("admin:endsExpired");
    if (entry.durationSecs === 0) return t("admin:endsPermanent");
    const end = endTime(entry);
    return end === null ? t("admin:endsUnknown") : date(end.toISOString());
  };

  /** What a row is called in the label of its own buttons. */
  const rowName = (entry: BanEntry) =>
    plain(entry.name) || `${maskAddress(entry.address, entry.mask)}/${String(entry.mask)}`;

  const focusRow = (key: string) => {
    setSelected(key);
    setFocusRequest((current) => ({ key, nonce: current.nonce + 1 }));
  };

  // Move the focus to the row it belongs to, also after the list re-rendered or scrolled.
  useEffect(() => {
    if (focusRequest.key === "" || focusRequest.nonce === lastFocusNonce.current) return;
    const index = rows.findIndex((row) => row.key === focusRequest.key);
    if (index < 0) return;
    lastFocusNonce.current = focusRequest.nonce;
    virtualizer.scrollToIndex(index, { align: "auto" });
    const frame = requestAnimationFrame(() => {
      rowRefs.current.get(focusRequest.key)?.focus();
    });
    return () => {
      cancelAnimationFrame(frame);
    };
  }, [focusRequest, rows, virtualizer]);

  // A new list keeps the focus where it was: on the same entry, or on the first one (issue #436).
  const afterRowsChanged = useEffectEvent(() => {
    const active = selectedEntry?.key ?? null;
    // Only a row keeps its place; a click on the search field or a column header must stay there.
    const listHadFocus = document.activeElement?.closest("[role='row']") !== null;
    if (active !== null && !rows.some((row) => row.key === active)) {
      setSelected(rows[0]?.key ?? null);
    }
    if (!listHadFocus) return;
    const target = rows.find((row) => row.key === active) ?? rows[0];
    if (target) focusRow(target.key);
  });
  useEffect(() => {
    afterRowsChanged();
  }, [rows]);

  // Nothing collected here survives the connection: say so instead of losing it silently.
  const changesDropped = useEffectEvent((count: number) => {
    setDropped(count);
    discardChanges();
    announce(t("admin:announcedDiscarded", { count }));
  });
  useEffect(() => {
    if (readOnly && state.changes.size > 0) changesDropped(state.changes.size);
  }, [readOnly, state.changes]);

  const requestClose = () => {
    if (state.changes.size > 0) setConfirmClose(true);
    else closeServerAdmin();
  };

  const stage = (change: BanChangeInput) => {
    stageChange(change);
    setSaved(null);
    setForm(null);
    announce(t("admin:announcedStaged"));
  };

  const save = async () => {
    const countsNow = countsOf(state.changes);
    const outcome = await saveChanges(false);
    if (outcome?.kind !== "saved") return;
    setSaved(countsNow);
    announce(t("admin:announcedSaved"));
  };

  /** Arrow keys walk the list; Enter changes the row the focus is on. */
  const onListKeyDown = (event: ReactKeyboardEvent<HTMLDivElement>) => {
    if (
      event.target !== event.currentTarget &&
      event.key !== "ArrowUp" &&
      event.key !== "ArrowDown"
    ) {
      return;
    }
    if (rows.length === 0) return;
    const index = selectedEntry === null ? -1 : rows.indexOf(selectedEntry);
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      const step = event.key === "ArrowDown" ? 1 : -1;
      const next = rows[Math.min(rows.length - 1, Math.max(0, index + step))];
      if (next) focusRow(next.key);
      return;
    }
    if ((event.key === "Enter" || event.key === " ") && event.target === event.currentTarget) {
      event.preventDefault();
      if (selectedEntry) setForm({ kind: "edit", entry: selectedEntry });
    }
  };

  const pendingEdit = (entry: BanEntry): BanDraft => {
    const change = state.changes.get(entry.key);
    return change?.kind === "edit"
      ? change
      : { kind: "edit", key: entry.key, reason: entry.reason, duration: entry.durationSecs };
  };

  return (
    <Dialog
      title={t("admin:windowTitle", { server: serverName })}
      titleLead={
        <span className="admin-title-mark" aria-hidden="true">
          {initials(serverName)}
        </span>
      }
      describedBy={descId}
      onClose={requestClose}
      closeLabel={t("common:close")}
      className="admin-window"
    >
      <div className="admin-tabs">
        <span className="admin-tabs__tab">{`${t("admin:bansTab")} (${String(counts.all)})`}</span>
      </div>

      {readOnly ? (
        <p id={descId} className="admin-notice admin-notice--danger" role="status">
          <span>{t("admin:disconnected", { server: serverName })}</span>
          {dropped === 0 ? null : <span> {t("admin:disconnectedLost")}</span>}
        </p>
      ) : (
        <>
          <div className="admin-toolbar">
            <label className="admin-field">
              <span>{t("admin:searchPlaceholder")}</span>
              <input
                type="search"
                value={query}
                onChange={(event) => {
                  setQuery(event.target.value);
                }}
              />
            </label>
            <div className="admin-chips" role="group" aria-label={t("admin:filter")}>
              {(["all", "active", "expired"] as const).map((option) => (
                <button
                  key={option}
                  type="button"
                  className="button button--small"
                  aria-pressed={filter === option}
                  onClick={() => {
                    setFilter(option);
                  }}
                >
                  {`${t(
                    `admin:filter${option === "all" ? "All" : option === "active" ? "Active" : "Expired"}`,
                  )} (${String(counts[option])})`}
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
          <p id={descId} className="admin-count" role="status">
            {t("admin:shownOfTotal", { shown: rows.length, total: counts.all })}
          </p>
        </>
      )}

      {state.error ? (
        <p className="admin-error" role="status">
          {errorText(t, state.error)}{" "}
          <button type="button" className="button button--small" onClick={() => void load()}>
            {t("common:retry")}
          </button>
        </p>
      ) : null}

      <div className="admin-scroll" ref={scrollRef}>
        <div
          className="admin-table"
          role="table"
          aria-label={`${t("admin:bansTab")} (${String(counts.all)})`}
          aria-busy={state.loading}
        >
          <div className="admin-row admin-row--head" role="row">
            {COLUMNS.map((column) => (
              <div
                key={column.id}
                className="admin-cell admin-cell--head"
                role="columnheader"
                aria-sort={
                  sort.column === column.id ? (sort.ascending ? "ascending" : "descending") : "none"
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
          </div>

          {state.loading && rows.length === 0 ? (
            <>
              <p className="visually-hidden" role="status">
                {t("common:loading")}
              </p>
              {[0, 1, 2, 3].map((line) => (
                <div key={line} className="admin-row admin-row--skeleton" aria-hidden="true">
                  {COLUMNS.map((column) => (
                    <div key={column.id} className="admin-cell">
                      <span className="admin-skeleton" />
                    </div>
                  ))}
                </div>
              ))}
            </>
          ) : null}

          {!state.loading && rows.length === 0 ? (
            <p className="admin-empty">{filtering ? t("admin:emptySearch") : t("admin:empty")}</p>
          ) : null}

          {items.map((item) => {
            const entry = rows[item.index];
            if (!entry) return null;
            const revealed = state.revealed.has(entry.key);
            const name = rowName(entry);
            const active = selectedEntry?.key === entry.key;
            const unsaved = pendingFor(state, entry) !== undefined;
            return (
              <div
                key={item.key}
                ref={(element) => {
                  if (element) rowRefs.current.set(entry.key, element);
                  else rowRefs.current.delete(entry.key);
                }}
                className={`admin-row${active ? " admin-row--active" : ""}${
                  unsaved ? " admin-row--pending" : ""
                }`}
                role="row"
                aria-selected={active}
                tabIndex={active ? 0 : -1}
                data-expired={entry.expired || undefined}
                style={{ height: `${String(item.size)}px` }}
                onFocus={() => {
                  setSelected(entry.key);
                }}
                onClick={() => {
                  setSelected(entry.key);
                }}
                onDoubleClick={() => {
                  if (!readOnly) setForm({ kind: "edit", entry });
                }}
                onKeyDown={onListKeyDown}
              >
                <div className="admin-cell admin-cell--name" role="cell">
                  <span
                    className="admin-cell__text"
                    data-tooltip={plain(entry.name) || undefined}
                    data-tooltip-overflow
                  >
                    {plain(entry.name) || t("admin:noName")}
                  </span>
                  {unsaved ? (
                    <span className="admin-badge admin-badge--pending">{t("admin:pending")}</span>
                  ) : null}
                </div>
                <div className="admin-cell admin-cell--actions" role="cell">
                  <span className="admin-cell__text">{shortHash(entry.certHash) || "—"}</span>
                  <button
                    type="button"
                    className="button button--small"
                    aria-label={t("admin:copyFingerprintOf", { name })}
                    tabIndex={active ? 0 : -1}
                    data-tooltip={t("admin:copyFullHint")}
                    disabled={!entry.certHash || readOnly}
                    onClick={() => void getApi().copyText(entry.certHash ?? "")}
                  >
                    {t("admin:copy")}
                  </button>
                </div>
                <div className="admin-cell admin-cell--actions" role="cell">
                  <span className="admin-cell__text">
                    {revealed ? entry.address : maskAddress(entry.address, entry.mask)}
                    {`/${String(entry.mask)}`}
                  </span>
                  <button
                    type="button"
                    className="button button--small"
                    aria-pressed={revealed}
                    aria-label={
                      revealed
                        ? t("admin:hideAddressOf", { name })
                        : t("admin:showAddressOf", { name })
                    }
                    tabIndex={active ? 0 : -1}
                    disabled={readOnly}
                    onClick={() => {
                      reveal(entry.key, !revealed);
                    }}
                  >
                    {revealed ? t("admin:hide") : t("admin:show")}
                  </button>
                  <button
                    type="button"
                    className="button button--small"
                    aria-label={t("admin:copyAddressOf", { name })}
                    tabIndex={active ? 0 : -1}
                    data-tooltip={t("admin:copyFullHint")}
                    disabled={readOnly}
                    onClick={() => void getApi().copyText(entry.address)}
                  >
                    {t("admin:copy")}
                  </button>
                </div>
                <div className="admin-cell" role="cell">
                  <span
                    className="admin-cell__text"
                    data-tooltip={plain(entry.reason) || undefined}
                  >
                    {plain(entry.reason) || "—"}
                  </span>
                </div>
                <div className="admin-cell" role="cell">
                  {date(entry.start)}
                </div>
                <div className="admin-cell" role="cell">
                  <span className={entry.expired ? "admin-ends-mark" : undefined}>
                    {endsText(entry)}
                  </span>
                </div>
              </div>
            );
          })}
        </div>
      </div>

      {saved ? (
        <p className="admin-notice" role="status">
          {t("admin:savedSummary", {
            added: saved.added,
            edited: saved.edited,
            removed: saved.removed,
          })}
        </p>
      ) : null}

      {readOnly ? null : (
        <div className="admin-footer">
          <div className="admin-footer__actions">
            <button
              type="button"
              className="button button--primary"
              onClick={() => {
                setForm({ kind: "add" });
              }}
            >
              + {t("admin:addBan")}
            </button>
            <button
              type="button"
              className="button"
              disabled={selectedEntry === null}
              onClick={() => {
                if (selectedEntry) setForm({ kind: "edit", entry: selectedEntry });
              }}
            >
              {t("admin:edit")}
            </button>
            <button
              type="button"
              className="button button--danger"
              disabled={selectedEntry === null}
              onClick={() => {
                if (selectedEntry) setRemoving(selectedEntry);
              }}
            >
              {t("admin:remove")}
            </button>
          </div>
          <span className="admin-footer__count">
            <span className="admin-badge">{String(state.changes.size)}</span>{" "}
            {t("admin:changesNotSaved")}
          </span>
          <div className="admin-footer__buttons">
            <button
              type="button"
              className="button"
              onClick={() => {
                discardChanges();
                setSaved(null);
              }}
              disabled={state.changes.size === 0}
            >
              {t("admin:discard")}
            </button>
            <button
              type="button"
              className="button button--primary"
              onClick={() => void save()}
              disabled={state.changes.size === 0}
            >
              {t("admin:saveBanList")}
            </button>
          </div>
        </div>
      )}

      {readOnly ? (
        <div className="admin-footer">
          <span className="admin-footer__count" />
          <button type="button" className="button button--primary" onClick={closeServerAdmin}>
            {t("common:close")}
          </button>
        </div>
      ) : null}

      {form ? (
        <BanEditDialog
          serverName={serverName}
          change={
            form.kind === "add"
              ? {
                  kind: "add",
                  name: null,
                  hash: null,
                  address: "",
                  mask: 32,
                  reason: null,
                  duration: 0,
                }
              : pendingEdit(form.entry)
          }
          entry={form.kind === "edit" ? form.entry : null}
          onStage={stage}
          onClose={() => {
            setForm(null);
          }}
        />
      ) : null}

      {removing ? (
        <ConfirmDialog
          title={t("admin:unbanQuestion", { name: rowName(removing) })}
          message={`${t("admin:unbanDetails", {
            reason: plain(removing.reason) || t("admin:unbanReasonMissing"),
            ends:
              removing.expired || removing.durationSecs === 0
                ? endsText(removing)
                : t("admin:unbanUntil", { date: endsText(removing) }),
          })} ${t("admin:unbanNote", { name: rowName(removing) })}`}
          confirmLabel={t("admin:unban")}
          danger
          onConfirm={() => {
            stageChange({ kind: "remove", key: removing.key });
            setSaved(null);
            setRemoving(null);
            announce(t("admin:announcedStaged"));
          }}
          onClose={() => {
            setRemoving(null);
          }}
        />
      ) : null}

      {confirmClose ? (
        <ConfirmDialog
          title={t("admin:discardTitle", { count: state.changes.size })}
          message={t("admin:discardNote")}
          confirmLabel={t("admin:discardAndClose")}
          danger
          onConfirm={() => {
            setConfirmClose(false);
            closeServerAdmin();
          }}
          onClose={() => {
            setConfirmClose(false);
          }}
        />
      ) : null}

      {state.conflict ? <ConflictDialog /> : null}
    </Dialog>
  );
}
