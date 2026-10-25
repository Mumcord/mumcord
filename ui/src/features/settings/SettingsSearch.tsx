import { useEffect, useId, useRef, type KeyboardEvent, type RefObject } from "react";
import { useTranslation } from "react-i18next";
import { announce } from "../../a11y/announcer";
import { entryLabel, searchSettings, type SearchResult } from "./settingsIndex";

interface SettingsSearchProps {
  query: string;
  onQueryChange: (query: string) => void;
  onPick: (result: SearchResult) => void;
  inputRef: RefObject<HTMLInputElement | null>;
}

const resultKey = (result: SearchResult) =>
  result.kind === "section"
    ? `section:${result.section}`
    : `${result.entry.section}:${result.entry.group ?? ""}:${result.entry.control ?? ""}`;

/**
 * Search field above the section list. While it holds text, the matching sections and settings
 * replace the section list; picking one opens its section and focuses the control.
 */
export function SettingsSearch({ query, onQueryChange, onPick, inputRef }: SettingsSearchProps) {
  const { t } = useTranslation("settings");
  const id = useId();
  const listRef = useRef<HTMLUListElement>(null);
  const searching = query.trim() !== "";
  const results = searching ? searchSettings(query, t) : [];
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
      <input
        ref={inputRef}
        id={id}
        type="search"
        className="settings-search__input"
        placeholder={t("search.label")}
        autoComplete="off"
        spellCheck={false}
        aria-keyshortcuts="Control+F"
        value={query}
        onChange={(event) => {
          onQueryChange(event.target.value);
        }}
        onKeyDown={onInputKeyDown}
      />
      {searching &&
        (count === 0 ? (
          <p className="settings-search__empty">{t("search.empty", { query: query.trim() })}</p>
        ) : (
          <ul ref={listRef} className="settings-search__results" aria-label={t("search.results")}>
            {results.map((result) => {
              const section = t(
                `sections.${result.kind === "section" ? result.section : result.entry.section}`,
              );
              return (
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
                      section
                    ) : (
                      <>
                        <span className="settings-search__section">{section}</span> ›{" "}
                        {entryLabel(result.entry, t)}
                      </>
                    )}
                  </button>
                </li>
              );
            })}
          </ul>
        ))}
    </div>
  );
}
