import { useCallback, useEffect, useId, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { announce } from "../../a11y";
import { getApi, type PublicServer, type SavedServer } from "../../api";
import { Select } from "../../components/Select";
import { addServer, requestConnect, useServers } from "../servers/store";
import { closeServerBrowser } from "./browserStore";
import {
  NO_FILTERS,
  PING_LIMITS,
  countriesOf,
  isEmpty,
  isFull,
  nextSort,
  pingOf,
  pingTone,
  serverKey,
  shownServers,
  type Filters,
  type PingState,
  type Sort,
  type SortKey,
} from "./model";
import { PingScheduler } from "./pingScheduler";
import "./serverbrowser.css";

type ListState =
  { kind: "loading" } | { kind: "error" } | { kind: "ready"; servers: PublicServer[] };

/** Without IntersectionObserver (tests) the first rows count as on screen. */
const FALLBACK_VISIBLE = 20;

function savedMatch(saved: readonly SavedServer[], server: PublicServer): SavedServer | undefined {
  return saved.find(
    (s) => s.address.toLowerCase() === server.host.toLowerCase() && s.port === server.port,
  );
}

/**
 * The public server browser (issue #68, FR-3.91, FR-3.92): the list the official Mumble client
 * shows, loaded only now that the user opened it. Search, filters and sorting run locally; only
 * the rows on screen are pinged, a few at a time. Everything in the list is untrusted and shown as
 * plain text.
 */
export function ServerBrowser() {
  const { t } = useTranslation("serverbrowser");
  const titleId = useId();
  const { servers: saved, identityName } = useServers();
  const [list, setList] = useState<ListState>({ kind: "loading" });
  const [pings, setPings] = useState<ReadonlyMap<string, PingState>>(new Map());
  const [filters, setFilters] = useState<Filters>(NO_FILTERS);
  const [sort, setSort] = useState<Sort>({ key: "name", ascending: true });
  const [busy, setBusy] = useState<string | null>(null);
  const [failed, setFailed] = useState(false);
  const [loads, setLoads] = useState(0);

  const [scheduler] = useState(
    () =>
      new PingScheduler(
        (host, port) => getApi().publicServerPing(host, port),
        (key, answer) => {
          setPings((current) =>
            new Map(current).set(
              key,
              answer ? { kind: "answered", ...answer } : { kind: "unreachable" },
            ),
          );
        },
      ),
  );
  useEffect(() => {
    scheduler.start();
    return () => {
      scheduler.stop();
    };
  }, [scheduler]);

  // The list loads only here, when the page opens or Refresh is pressed (FR-3.92).
  useEffect(() => {
    let current = true;
    getApi()
      .publicServersList()
      .then((servers) => {
        if (current) setList({ kind: "ready", servers });
      })
      .catch((error: unknown) => {
        console.error("Loading the public server list failed", error);
        if (current) setList({ kind: "error" });
      });
    return () => {
      current = false;
    };
  }, [loads]);

  const refresh = () => {
    setList({ kind: "loading" });
    setPings(new Map());
    scheduler.reset();
    setLoads((n) => n + 1);
  };

  const servers = useMemo(() => (list.kind === "ready" ? list.servers : []), [list]);
  const shown = useMemo(
    () => shownServers(servers, pings, filters, sort),
    [servers, pings, filters, sort],
  );
  const countries = useMemo(() => countriesOf(servers), [servers]);

  // Rows on screen, kept in a ref so the observer callback always sees the current order.
  const onScreen = useRef(new Set<string>());
  const shownRef = useRef(shown);
  const pingVisible = useCallback(() => {
    scheduler.setVisible(shownRef.current.filter((s) => onScreen.current.has(serverKey(s))));
  }, [scheduler]);
  const observer = useRef<IntersectionObserver | null>(null);
  useEffect(() => {
    if (typeof IntersectionObserver !== "function") return;
    const watcher = new IntersectionObserver((entries) => {
      for (const entry of entries) {
        const key = (entry.target as HTMLElement).dataset.key;
        if (key === undefined) continue;
        if (entry.isIntersecting) onScreen.current.add(key);
        else onScreen.current.delete(key);
      }
      pingVisible();
    });
    observer.current = watcher;
    return () => {
      watcher.disconnect();
      observer.current = null;
    };
  }, [pingVisible]);
  useEffect(() => {
    shownRef.current = shown;
    if (observer.current) pingVisible();
    else scheduler.setVisible(shown.slice(0, FALLBACK_VISIBLE));
  }, [shown, pingVisible, scheduler]);

  const observeRow = useCallback((row: HTMLTableRowElement | null) => {
    const watcher = observer.current;
    if (!row || !watcher) return;
    watcher.observe(row);
    return () => {
      watcher.unobserve(row);
      if (row.dataset.key !== undefined) onScreen.current.delete(row.dataset.key);
    };
  }, []);

  // Opening the page moves the focus to its search field, so typing filters right away.
  const searchRef = useRef<HTMLInputElement>(null);
  useEffect(() => {
    searchRef.current?.focus();
  }, []);

  // Escape inside the page closes it; a list box or dialog that handles Escape itself prevents it.
  const pageRef = useRef<HTMLElement>(null);
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || event.defaultPrevented) return;
      if (event.target instanceof Node && pageRef.current?.contains(event.target)) {
        closeServerBrowser();
      }
    };
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("keydown", onKeyDown);
    };
  }, []);

  const save = async (server: PublicServer): Promise<SavedServer> =>
    savedMatch(saved, server) ??
    (await addServer({
      label: server.name,
      address: server.host,
      port: server.port,
      username: identityName,
      password: { kind: "keep" },
    }));

  const act = (server: PublicServer, connect: boolean) => {
    setBusy(serverKey(server));
    setFailed(false);
    save(server)
      .then((entry) => {
        if (connect) {
          closeServerBrowser();
          requestConnect(entry.id);
        } else {
          announce(t("announce.saved", { server: server.name }));
        }
      })
      .catch((error: unknown) => {
        console.error("Saving a public server failed", error);
        setFailed(true);
      })
      .finally(() => {
        setBusy(null);
      });
  };

  const setFilter = (patch: Partial<Filters>) => {
    setFilters((current) => ({ ...current, ...patch }));
  };

  const sortHeader = (key: SortKey) => {
    const active = sort.key === key;
    return (
      <th
        scope="col"
        aria-sort={active ? (sort.ascending ? "ascending" : "descending") : "none"}
        className={`server-browser__col server-browser__col--${key}`}
      >
        <button
          type="button"
          className="server-browser__sort"
          onClick={() => {
            setSort(nextSort(sort, key));
          }}
        >
          {t(`columns.${key}`)}
          {active && <span aria-hidden="true">{sort.ascending ? " ▲" : " ▼"}</span>}
        </button>
      </th>
    );
  };

  return (
    <section ref={pageRef} className="server-browser" aria-labelledby={titleId}>
      <header className="server-browser__header">
        <h1 id={titleId}>{t("title")}</h1>
        <p className="server-browser__subtitle" role="status">
          {list.kind === "loading"
            ? t("loading")
            : list.kind === "error"
              ? null
              : t("subtitle", { shown: shown.length, total: servers.length })}
        </p>
      </header>

      <div className="server-browser__search-row">
        <input
          ref={searchRef}
          type="search"
          className="server-browser__search"
          aria-label={t("search")}
          placeholder={t("search")}
          value={filters.query}
          onChange={(event) => {
            setFilter({ query: event.target.value });
          }}
        />
        <button
          type="button"
          className="button server-browser__refresh"
          onClick={refresh}
          disabled={list.kind === "loading"}
        >
          <span aria-hidden="true">↻ </span>
          {t("refresh")}
        </button>
      </div>

      <div className="server-browser__filters" role="group" aria-label={t("filters.label")}>
        <span className="server-browser__filters-label" aria-hidden="true">
          {t("filters.label")}
        </span>
        <Select
          className="server-browser__filter"
          ariaLabel={t("filters.country")}
          value={filters.country ?? ""}
          options={[
            { value: "", label: t("filters.anyCountry") },
            ...countries.map((c) => ({ value: c.code, label: `${c.name} (${c.code})` })),
          ]}
          onChange={(value) => {
            setFilter({ country: value === "" ? null : value });
          }}
        />
        <Select
          className="server-browser__filter"
          ariaLabel={t("filters.ping")}
          value={filters.maxPing === null ? "" : String(filters.maxPing)}
          options={[
            { value: "", label: t("filters.anyPing") },
            ...PING_LIMITS.map((ms) => ({
              value: String(ms),
              label: t("filters.pingBelow", { ms }),
            })),
          ]}
          onChange={(value) => {
            setFilter({ maxPing: value === "" ? null : Number(value) });
          }}
        />
        <button
          type="button"
          className="server-browser__toggle"
          aria-pressed={filters.hideEmpty}
          onClick={() => {
            setFilter({ hideEmpty: !filters.hideEmpty });
          }}
        >
          {t("filters.hideEmpty")}
        </button>
        <button
          type="button"
          className="server-browser__toggle"
          aria-pressed={filters.hideFull}
          onClick={() => {
            setFilter({ hideFull: !filters.hideFull });
          }}
        >
          {t("filters.hideFull")}
        </button>
        <span className="server-browser__sort-label">
          {t("sortedBy", { column: t(`columns.${sort.key}`) })}
        </span>
      </div>

      {failed && (
        <p className="server-browser__error" role="alert">
          {t("saveFailed")}
        </p>
      )}

      {list.kind === "error" ? (
        <div className="server-browser__empty" role="alert">
          <p>{t("loadFailed")}</p>
          <button type="button" className="button button--primary" onClick={refresh}>
            {t("retry")}
          </button>
        </div>
      ) : list.kind === "ready" && shown.length === 0 ? (
        <p className="server-browser__empty">{t("noMatch")}</p>
      ) : (
        <div className="server-browser__table-wrap">
          <table className="server-browser__table" aria-busy={list.kind === "loading"}>
            <thead>
              <tr>
                {sortHeader("name")}
                {sortHeader("country")}
                {sortHeader("ping")}
                {sortHeader("users")}
                <th scope="col" className="server-browser__col--actions">
                  <span className="visually-hidden">{t("columns.actions")}</span>
                </th>
              </tr>
            </thead>
            <tbody>
              {shown.map((server) => {
                const key = serverKey(server);
                const ping = pingOf(pings, server);
                const savedEntry = savedMatch(saved, server);
                return (
                  <tr key={key} ref={observeRow} data-key={key} className="server-browser__row">
                    <th scope="row" className="server-browser__name">
                      <span className="server-browser__name-line">
                        <span className="server-browser__server-name">{server.name}</span>
                        {isEmpty(ping) && (
                          <span className="server-browser__badge">{t("badge.empty")}</span>
                        )}
                        {isFull(ping) && (
                          <span className="server-browser__badge">{t("badge.full")}</span>
                        )}
                      </span>
                      <span className="server-browser__host">
                        {server.port === 64738
                          ? server.host
                          : `${server.host}:${String(server.port)}`}
                      </span>
                    </th>
                    <td>
                      {server.countryCode !== "" && (
                        <span className="server-browser__country" data-tooltip={server.country}>
                          <span className="server-browser__country-code" aria-hidden="true">
                            {server.countryCode}
                          </span>
                          <span className="visually-hidden">
                            {server.country || server.countryCode}
                          </span>
                        </span>
                      )}
                    </td>
                    <td>
                      <PingCell ping={ping} />
                    </td>
                    <td className="server-browser__users">
                      {ping.kind === "answered" ? (
                        t("users", { users: ping.users, max: ping.maxUsers })
                      ) : (
                        <>
                          <span aria-hidden="true">–</span>
                          <span className="visually-hidden">{t("unknown")}</span>
                        </>
                      )}
                    </td>
                    <td className="server-browser__actions">
                      <button
                        type="button"
                        className="button button--primary server-browser__connect"
                        aria-label={t("connectTo", { server: server.name })}
                        disabled={busy !== null}
                        onClick={() => {
                          act(server, true);
                        }}
                      >
                        {t("connect")}
                      </button>
                      <button
                        type="button"
                        className="button server-browser__favourite"
                        aria-label={
                          savedEntry
                            ? t("savedAs", { server: server.name })
                            : t("favouriteServer", { server: server.name })
                        }
                        disabled={busy !== null || savedEntry !== undefined}
                        onClick={() => {
                          act(server, false);
                        }}
                      >
                        <span aria-hidden="true">{savedEntry ? "★ " : "☆ "}</span>
                        {savedEntry ? t("saved") : t("favourite")}
                      </button>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}

      <p className="server-browser__note">{t("note")}</p>
    </section>
  );
}

function PingCell({ ping }: { ping: PingState }) {
  const { t } = useTranslation("serverbrowser");
  switch (ping.kind) {
    case "pending":
      return (
        <span className="server-browser__ping server-browser__ping--pending">
          <span aria-hidden="true">…</span>
          <span className="visually-hidden">{t("measuring")}</span>
        </span>
      );
    case "unreachable":
      return (
        <span className="server-browser__ping server-browser__ping--none">{t("noAnswer")}</span>
      );
    case "answered":
      return (
        <span className={`server-browser__ping server-browser__ping--${pingTone(ping.pingMs)}`}>
          <span className="server-browser__ping-dot" aria-hidden="true" />
          {t("ms", { ms: ping.pingMs })}
        </span>
      );
  }
}
