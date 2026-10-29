import { useVirtualizer } from "@tanstack/react-virtual";
import { useEffect, useId, useMemo, useRef, useState, type KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";
import { announce } from "../../a11y";
import { getApi } from "../../api";
import { ContextMenu, type MenuPosition } from "../../components/ContextMenu";
import { Select } from "../../components/Select";
import { openInBrowser } from "../chat/openLink";
import { SAFE_LINK } from "../chat/sanitize";
import {
  entryKey,
  entryText,
  filterEntries,
  licenceOptions,
  type NoticeEntry,
  type Notices,
} from "./notices";
import "../settings/settings.css";
import "./licences.css";

const SOURCE_URL = "https://github.com/Mumcord/mumcord";
const ALL = "";

type LinkMenu = MenuPosition & { url: string };

export interface LicencesViewProps {
  /** `null` while loading. */
  notices: Notices | null;
  failed: boolean;
}

/**
 * The third-party licences (issue #138): one collapsed row per component that expands to its
 * licence text, with search, a licence filter and expand/collapse all. Only the rows in view
 * are rendered, as there are several hundred.
 */
export function LicencesView({ notices, failed }: LicencesViewProps) {
  const { t } = useTranslation("settings");
  const titleId = useId();
  const [query, setQuery] = useState("");
  const [licence, setLicence] = useState(ALL);
  const [expanded, setExpanded] = useState<ReadonlySet<string>>(new Set());
  const [linkMenu, setLinkMenu] = useState<LinkMenu | null>(null);

  const entries = notices?.entries;
  const options = useMemo(() => licenceOptions(entries ?? []), [entries]);
  const shown = useMemo(
    () => filterEntries(entries ?? [], query, licence === ALL ? null : licence),
    [entries, query, licence],
  );

  const openLinkMenu = (url: string, opener: HTMLElement) => {
    const rect = opener.getBoundingClientRect();
    setLinkMenu({ url, x: rect.left, y: rect.bottom, opener });
  };

  const toggle = (key: string) => {
    setExpanded((old) => {
      const next = new Set(old);
      if (!next.delete(key)) next.add(key);
      return next;
    });
  };

  return (
    <main className="lic-window" aria-labelledby={titleId}>
      <header className="lic-header">
        <h1 id={titleId} className="settings-section__title">
          {t("licences.title")}
        </h1>
        <p className="settings-hint">
          {t("licences.own")}{" "}
          <LinkButton url={SOURCE_URL} label={t("licences.sourceCode")} onOpen={openLinkMenu} />
        </p>
      </header>
      {failed ? (
        <p role="alert" className="settings-error">
          {t("licences.loadFailed")}
        </p>
      ) : notices === null ? (
        <p className="settings-hint">{t("about.loading")}</p>
      ) : (
        <>
          <div className="lic-toolbar">
            <input
              type="search"
              className="settings-input lic-search"
              aria-label={t("licences.search")}
              placeholder={t("licences.search")}
              value={query}
              onChange={(event) => {
                setQuery(event.target.value);
              }}
            />
            <LicenceFilter
              value={licence}
              options={[
                { value: ALL, label: t("licences.allLicences") },
                ...options.map((o) => ({ value: o.id, label: `${o.id} (${String(o.count)})` })),
              ]}
              onChange={setLicence}
            />
            <div className="settings-actions">
              <button
                type="button"
                className="settings-button"
                onClick={() => {
                  setExpanded(new Set(shown.map(entryKey)));
                }}
              >
                {t("licences.expandAll")}
              </button>
              <button
                type="button"
                className="settings-button"
                onClick={() => {
                  setExpanded(new Set());
                }}
              >
                {t("licences.collapseAll")}
              </button>
            </div>
            <p className="settings-hint lic-count" role="status">
              {t("licences.shown", { shown: shown.length, total: notices.entries.length })}
            </p>
          </div>
          {shown.length === 0 ? (
            <p className="settings-hint">{t("licences.empty")}</p>
          ) : (
            <LicenceList
              entries={shown}
              texts={notices.texts}
              expanded={expanded}
              onToggle={toggle}
              onOpenLink={openLinkMenu}
            />
          )}
        </>
      )}
      {linkMenu ? (
        <ContextMenu
          menu={linkMenu}
          label={t("linkMenu.label", { ns: "chat" })}
          onClose={() => {
            setLinkMenu(null);
          }}
          items={[
            {
              label: t("linkMenu.copy", { ns: "chat" }),
              onSelect: () => {
                copy(linkMenu.url, t("linkMenu.copied", { ns: "chat" }), t("licences.copyFailed"));
              },
            },
            {
              label: t("linkMenu.open", { ns: "chat" }),
              onSelect: () => {
                openInBrowser(linkMenu.url);
              },
            },
          ]}
        />
      ) : null}
    </main>
  );
}

function copy(text: string, done: string, failed: string) {
  getApi()
    .copyText(text)
    .then(
      () => {
        announce(done);
      },
      (error: unknown) => {
        console.error("Copying failed", error);
        announce(failed, "assertive");
      },
    );
}

function LicenceFilter(props: {
  value: string;
  options: { value: string; label: string }[];
  onChange: (value: string) => void;
}) {
  const { t } = useTranslation("settings");
  const id = useId();
  return (
    <div className="settings-control-row">
      <label htmlFor={id}>{t("licences.filter")}</label>
      <Select id={id} value={props.value} options={props.options} onChange={props.onChange} />
    </div>
  );
}

/** A link that opens the link menu (copy or open in the browser) instead of navigating. */
function LinkButton(props: {
  url: string;
  label?: string;
  onOpen: (url: string, opener: HTMLElement) => void;
}) {
  if (!SAFE_LINK.test(props.url)) return <span className="lic-url">{props.url}</span>;
  return (
    <button
      type="button"
      className="lic-link"
      onClick={(event) => {
        props.onOpen(props.url, event.currentTarget);
      }}
    >
      {props.label ?? props.url}
    </button>
  );
}

function LicenceList(props: {
  entries: readonly NoticeEntry[];
  texts: Readonly<Record<string, string>>;
  expanded: ReadonlySet<string>;
  onToggle: (key: string) => void;
  onOpenLink: (url: string, opener: HTMLElement) => void;
}) {
  const { entries } = props;
  const scrollRef = useRef<HTMLDivElement>(null);
  const [focusIndex, setFocusIndex] = useState<number | null>(null);
  // The compiler skips this component: the virtualizer's functions must not be memoized.
  // eslint-disable-next-line react-hooks/incompatible-library
  const virtualizer = useVirtualizer({
    count: entries.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => 44,
    overscan: 8,
    getItemKey: (index) => {
      const entry = entries[index];
      return entry ? entryKey(entry) : index;
    },
  });
  const items = virtualizer.getVirtualItems();

  // Arrow keys may move to a row that is not rendered yet: it is focused once it is.
  useEffect(() => {
    if (focusIndex === null) return;
    const toggle = scrollRef.current?.querySelector<HTMLElement>(
      `[data-toggle-index="${String(focusIndex)}"]`,
    );
    if (toggle) {
      toggle.focus();
      setFocusIndex(null);
    }
  }, [focusIndex, items]);

  const onKeyDown = (event: KeyboardEvent<HTMLButtonElement>, index: number) => {
    const last = entries.length - 1;
    const target =
      event.key === "ArrowDown"
        ? Math.min(index + 1, last)
        : event.key === "ArrowUp"
          ? Math.max(index - 1, 0)
          : event.key === "Home"
            ? 0
            : event.key === "End"
              ? last
              : null;
    if (target === null) return;
    event.preventDefault();
    virtualizer.scrollToIndex(target);
    setFocusIndex(target);
  };

  return (
    <div ref={scrollRef} className="lic-list">
      <div className="lic-list__inner" style={{ height: virtualizer.getTotalSize() }}>
        {items.map((item) => {
          const entry = entries[item.index];
          if (!entry) return null;
          const key = entryKey(entry);
          return (
            <div
              key={key}
              ref={virtualizer.measureElement}
              data-index={item.index}
              className="lic-row"
              style={{ transform: `translateY(${String(item.start)}px)` }}
            >
              <LicenceRow
                entry={entry}
                index={item.index}
                text={props.expanded.has(key) ? entryText(entry, props.texts) : null}
                onToggle={() => {
                  props.onToggle(key);
                }}
                onKeyDown={onKeyDown}
                onOpenLink={props.onOpenLink}
              />
            </div>
          );
        })}
      </div>
    </div>
  );
}

function LicenceRow(props: {
  entry: NoticeEntry;
  index: number;
  /** The licence text while the row is expanded, `null` while it is collapsed. */
  text: string | null;
  onToggle: () => void;
  onKeyDown: (event: KeyboardEvent<HTMLButtonElement>, index: number) => void;
  onOpenLink: (url: string, opener: HTMLElement) => void;
}) {
  const { t } = useTranslation("settings");
  const { entry, text } = props;
  const buttonId = useId();
  const panelId = useId();
  const open = text !== null;
  return (
    <>
      <h2 className="lic-row__heading">
        <button
          type="button"
          id={buttonId}
          className="lic-row__toggle"
          aria-expanded={open}
          aria-controls={panelId}
          data-toggle-index={props.index}
          onClick={props.onToggle}
          onKeyDown={(event) => {
            props.onKeyDown(event, props.index);
          }}
        >
          <span className="lic-row__chevron" aria-hidden="true" />
          <span className="lic-row__name">
            {entry.version === "" ? entry.name : `${entry.name} ${entry.version}`}
          </span>
          <span className="lic-row__licence">— {entry.license}</span>
          <span className={`lic-tag lic-tag--${entry.source}`}>
            {t(`licences.sources.${entry.source}`)}
          </span>
        </button>
      </h2>
      <div id={panelId} className="lic-row__panel" hidden={!open}>
        {open && (
          <>
            {entry.description !== undefined && (
              <p className="settings-hint">{entry.description}</p>
            )}
            <div className="lic-row__actions">
              {entry.repository !== null && (
                <span className="lic-row__repo">
                  {t("licences.repository")}{" "}
                  <LinkButton url={entry.repository} onOpen={props.onOpenLink} />
                </span>
              )}
              <button
                type="button"
                className="settings-button"
                onClick={() => {
                  copy(text, t("licences.copied"), t("licences.copyFailed"));
                }}
              >
                {t("licences.copy")}
              </button>
            </div>
            <pre
              className="lic-text"
              // Focusable so keyboard users can scroll a long text.
              // eslint-disable-next-line jsx-a11y/no-noninteractive-tabindex
              tabIndex={0}
              aria-label={t("licences.textOf", { name: entry.name })}
            >
              {text}
            </pre>
          </>
        )}
      </div>
    </>
  );
}
