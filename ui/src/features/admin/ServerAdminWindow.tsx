/**
 * The Server administration state, ban list (Epic 4, issue #254, FR-10.50 ff.).
 *
 * Follows mockup `ban-list-dark`/`ban-list-light`: search and filter on top, the list with
 * sortable columns, edit in place for one entry (reason and end, or unban), and one save that
 * sends the whole list. The save first re-reads the server's list, so a change someone else made
 * meanwhile is asked about before anything is sent (FR-10.66). Fingerprints are shortened and
 * addresses masked, and only this window shows them (FR-10.51, FR-10.65). Only the rows that are
 * visible are drawn, so a list of thousands scrolls smoothly (FR-10.53).
 */
import { useId, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useVirtualizer } from "@tanstack/react-virtual";
import { announce } from "../../a11y";
import { getApi, type BanChangeInput, type BanEntry } from "../../api";
import { ConfirmDialog } from "../servers/ConfirmDialog";
import { Dialog } from "../servers/Dialog";
import { displayName } from "../servers/format";
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
import { BanEditDialog } from "./BanEditDialog";
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

const END_CHIPS: { secs: number; key: string }[] = [
  { secs: 86_400, key: "chipExtendDay" },
  { secs: 0, key: "chipPermanent" },
];

type SortColumn = "name" | "certHash" | "address" | "reason" | "start" | "ends";
type SortState = { column: SortColumn; ascending: boolean };

export function ServerAdminWindowHost() {
  const state = useServerAdmin();
  return state ? <ServerAdminWindow state={state} /> : null;
}

/** The rows the table shows: the loaded entries with the pending changes, searched and sorted. */
function rowsOf(
  state: BanWindow,
  query: string,
  filter: "all" | "active" | "expired",
  sort: SortState,
): BanEntry[] {
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
  return sortBans(
    searchBans(filterBans([...state.entries.map(merged), ...added], filter), query),
    sort,
  );
}

function ServerAdminWindow({ state }: { state: BanWindow }) {
  const { t, i18n } = useTranslation(["admin", "common", "servers"]);
  const servers = useServers();
  const server = servers.servers.find((s) => s.id === state.serverId);
  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState<"all" | "active" | "expired">("all");
  const [sort, setSort] = useState<SortState>({ column: "name", ascending: true });
  const [selected, setSelected] = useState<string | null>(null);
  const [adding, setAdding] = useState(false);
  const [removing, setRemoving] = useState<BanEntry | null>(null);
  const scrollRef = useRef<HTMLDivElement>(null);
  const descId = useId();

  const rows = useMemo(() => rowsOf(state, query, filter, sort), [state, query, filter, sort]);
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

  const selectedEntry = rows.find((entry) => entry.key === selected);
  const pending = selectedEntry ? pendingFor(state, selectedEntry) : undefined;

  const save = async () => {
    const outcome = await saveChanges(false);
    if (outcome?.kind === "saved") announce(t("admin:announcedSaved"));
  };

  return (
    <Dialog
      title={t("admin:windowTitle")}
      describedBy={descId}
      onClose={closeServerAdmin}
      className="admin-window"
    >
      <p id={descId} className="admin-server">
        {server ? displayName(server) : t("servers:unknownServer")}
      </p>

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
              {t(
                `admin:filter${option === "all" ? "All" : option === "active" ? "Active" : "Expired"}`,
              )}
            </button>
          ))}
        </div>
        <button
          type="button"
          className="button button--primary"
          onClick={() => {
            setAdding(true);
          }}
        >
          + {t("admin:addBan")}
        </button>
        <button
          type="button"
          className="button button--small"
          onClick={() => void load()}
          disabled={state.loading}
        >
          {t("admin:refresh")}
        </button>
      </div>

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
          aria-label={`${t("admin:bansTab")} (${String(rows.length)})`}
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
            <div className="admin-cell admin-cell--head" role="columnheader">
              {t("admin:edit")}
            </div>
          </div>

          {rows.length === 0 ? (
            <p className="admin-empty">{state.loading ? t("common:loading") : t("admin:empty")}</p>
          ) : null}

          {items.map((item) => {
            const entry = rows[item.index];
            if (!entry) return null;
            const revealed = state.revealed.has(entry.key);
            return (
              <div
                key={item.key}
                className={`admin-row${pendingFor(state, entry) ? " admin-row--pending" : ""}`}
                role="row"
                style={{ height: `${String(item.size)}px` }}
              >
                <div className="admin-cell" role="cell">
                  {plain(entry.name) || t("admin:noName")}
                </div>
                <div className="admin-cell" role="cell">
                  <span data-tooltip={entry.certHash ?? undefined} data-tooltip-overflow>
                    {shortHash(entry.certHash) || "—"}
                  </span>{" "}
                  <button
                    type="button"
                    className="button button--small"
                    aria-label={t("admin:copyCertificate")}
                    disabled={!entry.certHash}
                    onClick={() => void getApi().copyText(entry.certHash ?? "")}
                  >
                    {t("admin:copy")}
                  </button>
                </div>
                <div className="admin-cell" role="cell">
                  <span data-tooltip={revealed ? entry.address : undefined} data-tooltip-overflow>
                    {revealed ? entry.address : maskAddress(entry.address, entry.mask)}
                    {`/${String(entry.mask)}`}
                  </span>{" "}
                  <button
                    type="button"
                    className="button button--small"
                    aria-pressed={revealed}
                    onClick={() => {
                      reveal(entry.key, !revealed);
                    }}
                  >
                    {revealed ? t("admin:hide") : t("admin:show")}
                  </button>
                </div>
                <div className="admin-cell" role="cell">
                  {plain(entry.reason) || "—"}
                </div>
                <div className="admin-cell" role="cell">
                  {date(entry.start)}
                </div>
                <div className="admin-cell" role="cell">
                  {endsText(entry)}
                </div>
                <div className="admin-cell admin-cell--actions" role="cell">
                  <button
                    type="button"
                    className="button button--small"
                    aria-pressed={selected === entry.key}
                    onClick={() => {
                      setSelected(entry.key);
                    }}
                  >
                    {t("admin:edit")}
                  </button>
                </div>
              </div>
            );
          })}
        </div>
      </div>

      {selectedEntry ? (
        <EditPanel
          entry={selectedEntry}
          pending={pending}
          onUnban={(entry) => {
            setRemoving(entry);
          }}
        />
      ) : null}

      <div className="admin-footer">
        <span className="admin-footer__count">
          <span className="admin-badge">{String(state.changes.size)}</span>{" "}
          {t("admin:changesNotSaved")}
        </span>
        <button
          type="button"
          className="button"
          onClick={discardChanges}
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

      {adding ? (
        <BanEditDialog
          change={{
            kind: "add",
            name: null,
            hash: null,
            address: "",
            mask: 32,
            reason: null,
            duration: 0,
          }}
          entry={null}
          onStage={(change) => {
            stageChange(change);
            setAdding(false);
            announce(t("admin:announcedStaged"));
          }}
          onClose={() => {
            setAdding(false);
          }}
        />
      ) : null}

      {removing ? (
        <ConfirmDialog
          title={t("admin:unbanQuestion", { name: plain(removing.name) || t("admin:noName") })}
          message={t("admin:unbanNote", { name: plain(removing.name) || t("admin:noName") })}
          confirmLabel={t("admin:unban")}
          danger
          onConfirm={() => {
            stageChange({ kind: "remove", key: removing.key });
            setRemoving(null);
            announce(t("admin:announcedStaged"));
          }}
          onClose={() => {
            setRemoving(null);
          }}
        />
      ) : null}

      {state.conflict ? <ConflictDialog /> : null}
    </Dialog>
  );
}

/** Edit in place: reason, end, or unban for the selected entry (FR-10.51). */
function EditPanel({
  entry,
  pending,
  onUnban,
}: {
  entry: BanEntry;
  pending: BanChangeInput | undefined;
  onUnban: (entry: BanEntry) => void;
}) {
  const { t, i18n } = useTranslation(["admin"]);
  const [reason, setReason] = useState(
    pending?.kind === "edit" ? (pending.reason ?? "") : (entry.reason ?? ""),
  );
  const [duration, setDuration] = useState(
    pending?.kind === "edit" ? pending.duration : entry.durationSecs,
  );
  const [custom, setCustom] = useState(false);
  const reasonId = useId();
  const [now] = useState(() => Date.now());
  const end = duration === 0 ? null : new Date(now + duration * 1000);

  const stage = (nextReason: string | null, nextDuration: number) => {
    stageChange({ kind: "edit", key: entry.key, reason: nextReason, duration: nextDuration });
  };

  return (
    <section
      className="admin-edit"
      role="group"
      aria-label={`${t("admin:editPanel")} · ${plain(entry.name) || t("admin:noName")}`}
    >
      <h2 className="admin-edit__title">
        {t("admin:editPanel")} · {plain(entry.name) || t("admin:noName")}
      </h2>
      <div className="admin-edit__grid">
        <div className="admin-form__field">
          <label htmlFor={reasonId}>{t("admin:reason")}</label>
          <input
            id={reasonId}
            type="text"
            value={reason}
            onChange={(event) => {
              setReason(event.target.value);
            }}
            onBlur={() => {
              stage(reason.trim() === "" ? null : reason.trim(), duration);
            }}
          />
        </div>
        <div className="admin-form__field">
          <span>{t("admin:columns.ends")}</span>
          <div className="admin-chips">
            {END_CHIPS.map((chip) => (
              <button
                key={chip.key}
                type="button"
                className="button button--small"
                aria-pressed={!custom && duration === chip.secs}
                onClick={() => {
                  setCustom(false);
                  setDuration(chip.secs);
                  stage(reason.trim() === "" ? null : reason.trim(), chip.secs);
                }}
              >
                {t(`admin:${chip.key}`)}
              </button>
            ))}
            <button
              type="button"
              className="button button--small"
              aria-pressed={custom}
              onClick={() => {
                setCustom(true);
              }}
            >
              {t("admin:chipCustom")}
            </button>
          </div>
          {custom ? (
            <input
              type="date"
              aria-label={t("admin:endsOnDate")}
              onChange={(event) => {
                const when = event.target.valueAsNumber;
                if (Number.isNaN(when)) return;
                stage(
                  reason.trim() === "" ? null : reason.trim(),
                  Math.max(0, Math.round((when - Date.now()) / 1000)),
                );
              }}
            />
          ) : null}
          <span className="admin-form__help">
            {end === null
              ? t("admin:endsPermanent")
              : t("admin:endsOn", {
                  date: new Intl.DateTimeFormat(i18n.language, { dateStyle: "long" }).format(end),
                })}
          </span>
        </div>
      </div>
      <div className="admin-edit__unban">
        <button
          type="button"
          className="button button--danger"
          onClick={() => {
            onUnban(entry);
          }}
        >
          {t("admin:unban")}
        </button>
        <span>{t("admin:unbanNote", { name: plain(entry.name) || t("admin:noName") })}</span>
      </div>
    </section>
  );
}
