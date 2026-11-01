import { Fragment, useEffect, useId, useRef, type KeyboardEvent, type RefObject } from "react";
import { useTranslation } from "react-i18next";
import { announce } from "../../a11y/announcer";
import { useIsMacos } from "../../api/useIsMacos";
import {
  entryLabel,
  groupResults,
  matchRanges,
  searchSettings,
  type SearchResult,
} from "./settingsIndex";

interface SettingsSearchProps {
  query: string;
  onQueryChange: (query: string) => void;
  onPick: (result: SearchResult) => void;
  inputRef: RefObject<HTMLInputElement | null>;
  /** Title keys of cards that are not offered on this system. */
  hiddenCards?: readonly string[];
}

const resultKey = (result: SearchResult) =>
  result.kind === "section"
    ? `section:${result.section}`
    : `${result.entry.section}:${result.entry.group}:${result.entry.control ?? ""}`;

/** `text` with the parts that match the query marked. */
function Highlighted({ text, query }: { text: string; query: string }) {
  const ranges = matchRanges(text, query);
  return (
    <>
      {ranges.map(([start, end], index) => (
        <Fragment key={start}>
          {text.slice(ranges[index - 1]?.[1] ?? 0, start)}
          <mark className="settings-search__match">{text.slice(start, end)}</mark>
        </Fragment>
      ))}
      {text.slice(ranges.at(-1)?.[1] ?? 0)}
    </>
  );
}

/**
 * Search field above the group list. While it holds text, the matching groups and settings,
 * bucketed by group with a count, replace the group list; picking one opens its group and focuses
 * the control.
 */
export function SettingsSearch({
  query,
  onQueryChange,
  onPick,
  inputRef,
  hiddenCards,
}: SettingsSearchProps) {
  const { t } = useTranslation("settings");
  const macos = useIsMacos();
  const id = useId();
  const listRef = useRef<HTMLDivElement>(null);
  const searching = query.trim() !== "";
  const results = searching ? searchSettings(query, t, hiddenCards) : [];
  const count = results.length;

  useEffect(() => {
    if (searching) announce(t("search.count", { count }));
  }, [searching, count, t]);

  const buttons = () => Array.from(listRef.current?.querySelectorAll("button") ?? []);

  const onInputKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key === "Escape" && query !== "") {
      // Only clears; a second Escape closes the settings as usual.
      event.preventDefault();
      event.stopPropagation();
      onQueryChange("");
    } else if (event.key === "Enter" && results[0]) {
      event.preventDefault();
      onPick(results[0]);
    } else if (event.key === "ArrowDown") {
      event.preventDefault();
      buttons()[0]?.focus();
    }
  };

  const onResultKeyDown = (event: KeyboardEvent<HTMLButtonElement>) => {
    const all = buttons();
    const index = all.indexOf(event.currentTarget);
    if (event.key === "ArrowDown") {
      event.preventDefault();
      all[Math.min(index + 1, all.length - 1)]?.focus();
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      if (index === 0) inputRef.current?.focus();
      else all[index - 1]?.focus();
    } else if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      onQueryChange("");
      inputRef.current?.focus();
    }
  };

  return (
    <div role="search" className="settings-search">
      <label htmlFor={id} className="visually-hidden">
        {t("search.label")}
      </label>
      <div className="settings-search__field">
        <input
          ref={inputRef}
          id={id}
          type="search"
          className="settings-search__input"
          placeholder={t("search.label")}
          autoComplete="off"
          spellCheck={false}
          aria-keyshortcuts={macos ? "Meta+F" : "Control+F"}
          value={query}
          onChange={(event) => {
            onQueryChange(event.target.value);
          }}
          onKeyDown={onInputKeyDown}
        />
        {!searching && (
          <kbd className="settings-search__shortcut" aria-hidden="true">
            {macos ? "⌘F" : t("search.shortcut")}
          </kbd>
        )}
      </div>
      {searching &&
        (count === 0 ? (
          <p className="settings-search__empty">{t("search.empty", { query: query.trim() })}</p>
        ) : (
          <div
            ref={listRef}
            role="group"
            className="settings-search__results"
            aria-label={t("search.results")}
          >
            {groupResults(results).map((group) => {
              const name = t(`sections.${group.section}`);
              const headingId = `${id}-${group.section}`;
              return (
                <section key={group.section} aria-labelledby={headingId}>
                  <h3 id={headingId} className="settings-search__group">
                    {name}{" "}
                    <span className="settings-search__count">
                      {t("search.groupCount", { count: group.results.length })}
                    </span>
                  </h3>
                  <ul className="settings-search__list">
                    {group.results.map((result) => (
                      <li key={resultKey(result)}>
                        <button
                          type="button"
                          className="settings-search__result"
                          onKeyDown={onResultKeyDown}
                          onClick={() => {
                            onPick(result);
                          }}
                        >
                          {result.kind === "section" ? (
                            <Highlighted text={name} query={query} />
                          ) : (
                            <>
                              <Highlighted text={entryLabel(result.entry, t)} query={query} />
                              {result.entry.control !== undefined && (
                                <span className="settings-search__card">
                                  <span className="visually-hidden">, </span>
                                  <Highlighted text={t(result.entry.group)} query={query} />
                                </span>
                              )}
                            </>
                          )}
                        </button>
                      </li>
                    ))}
                  </ul>
                </section>
              );
            })}
          </div>
        ))}
    </div>
  );
}
