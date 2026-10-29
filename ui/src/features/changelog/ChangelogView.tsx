import { useId, useMemo, useRef, useState, type KeyboardEvent, type MouseEvent } from "react";
import { useTranslation } from "react-i18next";
import { announce } from "../../a11y";
import { getApi } from "../../api";
import { ContextMenu, type MenuPosition } from "../../components/ContextMenu";
import { openInBrowser } from "../chat/openLink";
import { SAFE_LINK } from "../chat/sanitize";
import {
  currentVersion,
  filterChangelog,
  groupKind,
  INTERNAL,
  isDevBuild,
  itemHtml,
  summaryOf,
  visibleVersions,
  type ChangelogSection,
  type ChangelogVersion,
} from "./changelog";
import "../settings/settings.css";
import "./changelog.css";

type LinkMenu = MenuPosition & { url: string };

export interface ChangelogViewProps {
  /** `null` while loading. */
  versions: ChangelogVersion[] | null;
  /** The running build's display version; `null` while loading. */
  appVersion: string | null;
  failed: boolean;
  onClose: () => void;
}

/**
 * "What's new" (issue #145): one collapsible section per version, newest first, with the
 * current version open. Search narrows the versions to the matching entries.
 */
export function ChangelogView({ versions, appVersion, failed, onClose }: ChangelogViewProps) {
  const { t, i18n } = useTranslation("settings");
  const titleId = useId();
  const [query, setQuery] = useState("");
  const [linkMenu, setLinkMenu] = useState<LinkMenu | null>(null);
  const listRef = useRef<HTMLDivElement>(null);

  const all = useMemo(
    () => (versions === null || appVersion === null ? [] : visibleVersions(versions, appVersion)),
    [versions, appVersion],
  );
  const current = appVersion === null ? null : currentVersion(all, appVersion);
  const [expanded, setExpanded] = useState<ReadonlySet<string> | null>(null);
  const [internalOpen, setInternalOpen] = useState<ReadonlySet<string>>(new Set());
  // Until the user changes anything, the current version is the open one.
  const open = expanded ?? new Set(current === null ? [] : [current]);
  const shown = useMemo(() => filterChangelog(all, query), [all, query]);

  const search = (value: string) => {
    setQuery(value);
    const matching = value.trim() === "" ? [] : filterChangelog(all, value).map((v) => v.version);
    setExpanded(value.trim() === "" ? null : new Set(matching));
    setInternalOpen(new Set(matching));
  };

  const toggle = (set: ReadonlySet<string>, key: string) => {
    const next = new Set(set);
    if (!next.delete(key)) next.add(key);
    return next;
  };

  // Links inside entries open the link menu (copy or open in the browser) instead of navigating.
  const onListClick = (event: MouseEvent<HTMLDivElement>) => {
    if (!(event.target instanceof Element)) return;
    const anchor = event.target.closest("a");
    if (anchor === null) return;
    event.preventDefault();
    const href = anchor.getAttribute("href");
    if (href !== null && SAFE_LINK.test(href)) openLinkMenu(href, anchor);
  };

  const openLinkMenu = (url: string, opener: HTMLElement) => {
    const rect = opener.getBoundingClientRect();
    setLinkMenu({ url, x: rect.left, y: rect.bottom, opener });
  };

  const onToggleKey = (event: KeyboardEvent<HTMLButtonElement>) => {
    const toggles = [
      ...(listRef.current?.querySelectorAll<HTMLButtonElement>("[data-version-toggle]") ?? []),
    ];
    const index = toggles.indexOf(event.currentTarget);
    const last = toggles.length - 1;
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
    toggles[target]?.focus();
  };

  return (
    <main className="cl-window" aria-labelledby={titleId}>
      <header className="cl-header">
        <img className="cl-logo" src={`${import.meta.env.BASE_URL}favicon.png`} alt="" />
        <div className="cl-header__text">
          <h1 id={titleId} className="cl-title">
            {t("changelog.title", { version: appVersion ?? "" })}
          </h1>
          <p className="settings-hint">
            {t("changelog.subtitle")}
            {/* The entries themselves are not translated. */}
            {!i18n.language.startsWith("en") && ` ${t("changelog.englishNote")}`}
          </p>
        </div>
        <button type="button" className="settings-button" onClick={onClose}>
          {t("close", { ns: "common" })}
        </button>
      </header>
      {failed ? (
        <p role="alert" className="settings-error">
          {t("changelog.loadFailed")}
        </p>
      ) : versions === null || appVersion === null ? (
        <p className="settings-hint">{t("about.loading")}</p>
      ) : (
        <>
          <div className="cl-toolbar">
            <input
              type="search"
              className="settings-input cl-search"
              aria-label={t("changelog.search")}
              placeholder={t("changelog.search")}
              value={query}
              onChange={(event) => {
                search(event.target.value);
              }}
            />
            <div className="settings-actions">
              <button
                type="button"
                className="settings-button"
                onClick={() => {
                  setExpanded(new Set(shown.map((v) => v.version)));
                }}
              >
                {t("changelog.expandAll")}
              </button>
              <button
                type="button"
                className="settings-button"
                onClick={() => {
                  setExpanded(new Set());
                  setInternalOpen(new Set());
                }}
              >
                {t("changelog.collapseAll")}
              </button>
            </div>
            <p className="cl-count" role="status">
              {query.trim() === "" ? "" : t("changelog.found", { count: shown.length })}
            </p>
          </div>
          {shown.length === 0 ? (
            <p className="settings-hint">{t("changelog.empty")}</p>
          ) : (
            // Clicks on the links inside the entries; the links themselves are keyboard reachable.
            // eslint-disable-next-line jsx-a11y/click-events-have-key-events, jsx-a11y/no-static-element-interactions
            <div ref={listRef} className="cl-list" onClick={onListClick}>
              {shown.map((version) => (
                <VersionEntry
                  key={version.version}
                  version={version}
                  current={version.version === current}
                  dev={isDevBuild(appVersion)}
                  open={open.has(version.version)}
                  internalOpen={internalOpen.has(version.version)}
                  onToggle={() => {
                    setExpanded(toggle(open, version.version));
                  }}
                  onToggleInternal={() => {
                    setInternalOpen((old) => toggle(old, version.version));
                  }}
                  onKeyDown={onToggleKey}
                  onOpenLink={openLinkMenu}
                />
              ))}
            </div>
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
                copyLink(
                  linkMenu.url,
                  t("linkMenu.copied", { ns: "chat" }),
                  t("licences.copyFailed"),
                );
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

function copyLink(text: string, done: string, failed: string) {
  getApi()
    .copyText(text)
    .then(
      () => {
        announce(done);
      },
      (error: unknown) => {
        console.error("Copying a link failed", error);
        announce(failed, "assertive");
      },
    );
}

function formatDate(date: string, language: string): string {
  // The changelog dates are calendar days; read them as UTC so no time zone shifts the day.
  const parsed = new Date(`${date}T00:00:00Z`);
  if (Number.isNaN(parsed.getTime())) return date;
  return new Intl.DateTimeFormat(language, { dateStyle: "long", timeZone: "UTC" }).format(parsed);
}

function VersionEntry(props: {
  version: ChangelogVersion;
  current: boolean;
  dev: boolean;
  open: boolean;
  internalOpen: boolean;
  onToggle: () => void;
  onToggleInternal: () => void;
  onKeyDown: (event: KeyboardEvent<HTMLButtonElement>) => void;
  onOpenLink: (url: string, opener: HTMLElement) => void;
}) {
  const { t, i18n } = useTranslation("settings");
  const { version, open } = props;
  const panelId = useId();
  const summary = open ? null : summaryOf(version);
  const groups = version.sections.filter((s) => s.title !== INTERNAL);
  const internal = version.sections.find((s) => s.title === INTERNAL);
  return (
    <section className={`cl-version${props.current ? " cl-version--current" : ""}`}>
      <h2 className="cl-version__heading">
        <button
          type="button"
          className="cl-version__toggle"
          aria-expanded={open}
          aria-controls={panelId}
          data-version-toggle=""
          onClick={props.onToggle}
          onKeyDown={props.onKeyDown}
        >
          <span className="cl-chevron" aria-hidden="true" />
          <span className="cl-version__number">
            {version.unreleased && version.version === "Unreleased"
              ? t("changelog.unreleased")
              : version.version}
          </span>
          {props.current && (
            <span className="cl-badge cl-badge--current">{t("changelog.current")}</span>
          )}
          {version.unreleased && props.dev && (
            <span className="cl-badge cl-badge--dev">{t("changelog.inDevelopment")}</span>
          )}
          {version.date !== null && (
            <span className="cl-version__date">{formatDate(version.date, i18n.language)}</span>
          )}
          {summary !== null && <span className="cl-version__summary">{summary}</span>}
        </button>
      </h2>
      <div id={panelId} className="cl-version__panel" hidden={!open}>
        {open && (
          <>
            {version.summary !== undefined && (
              <div
                className="cl-summary"
                // Sanitized in itemHtml.
                dangerouslySetInnerHTML={{ __html: itemHtml(version.summary) }}
              />
            )}
            {groups.map((section, index) => (
              <Group key={`${section.title} ${String(index)}`} section={section} />
            ))}
            {internal !== undefined && (
              <InternalGroup
                section={internal}
                open={props.internalOpen}
                onToggle={props.onToggleInternal}
              />
            )}
            {version.url !== undefined && SAFE_LINK.test(version.url) && (
              <p className="cl-release">
                <button
                  type="button"
                  className="cl-link"
                  onClick={(event) => {
                    event.stopPropagation();
                    if (version.url !== undefined)
                      props.onOpenLink(version.url, event.currentTarget);
                  }}
                >
                  {t("changelog.releasePage")}
                </button>
              </p>
            )}
          </>
        )}
      </div>
    </section>
  );
}

function GroupIcon({ title }: { title: string }) {
  const kind = groupKind(title);
  const glyph = { added: "+", changed: "↻", fixed: "✓", removed: "−", internal: "⚙", other: "•" }[
    kind
  ];
  return (
    <span className={`cl-group__icon cl-group--${kind}`} aria-hidden="true">
      {glyph}
    </span>
  );
}

function Items({ items, kind, id }: { items: string[]; kind: string; id?: string }) {
  return (
    <ul id={id} className={`cl-items cl-group--${kind}`}>
      {items.map((item, index) => (
        <li
          // Entries are plain text that can repeat; their position is their identity.
          key={index}
          // Sanitized in itemHtml.
          dangerouslySetInnerHTML={{ __html: itemHtml(item) }}
        />
      ))}
    </ul>
  );
}

function GroupCount({ count }: { count: number }) {
  const { t } = useTranslation("settings");
  return (
    <span className="cl-group__count">
      <span aria-hidden="true">{count}</span>
      <span className="visually-hidden">{t("changelog.entries", { count })}</span>
    </span>
  );
}

function Group({ section }: { section: ChangelogSection }) {
  const kind = groupKind(section.title);
  return (
    <div className="cl-group">
      {section.title !== "" && (
        <h3 className={`cl-group__title cl-group--${kind}`}>
          <GroupIcon title={section.title} />
          <span className="cl-group__name">{section.title}</span>
          <GroupCount count={section.items.length} />
        </h3>
      )}
      <Items items={section.items} kind={kind} />
    </div>
  );
}

function InternalGroup(props: { section: ChangelogSection; open: boolean; onToggle: () => void }) {
  const { t } = useTranslation("settings");
  const listId = useId();
  const { section } = props;
  return (
    <div className="cl-group cl-group--internal-block">
      <h3 className="cl-group__title cl-group--internal">
        <button
          type="button"
          className="cl-internal__toggle"
          aria-expanded={props.open}
          aria-controls={listId}
          onClick={props.onToggle}
        >
          <span className="cl-chevron" aria-hidden="true" />
          <GroupIcon title={section.title} />
          <span className="cl-group__name">{section.title}</span>
          <GroupCount count={section.items.length} />
          <span className="cl-group__hint">{t("changelog.forDevelopers")}</span>
        </button>
      </h3>
      {props.open ? (
        <Items items={section.items} kind="internal" id={listId} />
      ) : (
        <div id={listId} hidden />
      )}
    </div>
  );
}
