import { useEffect, useId, useRef } from "react";
import { useTranslation } from "react-i18next";

export interface ChannelSearchProps {
  value: string;
  onChange: (value: string) => void;
  onClear: () => void;
  /** Enter: open the first match. */
  onSubmit: () => void;
}

/** Ctrl+K and Ctrl+F (Cmd on macOS) focus the field; the app uses neither for anything else. */
const isFocusShortcut = (event: KeyboardEvent) =>
  (event.ctrlKey || event.metaKey) &&
  !event.altKey &&
  !event.shiftKey &&
  (event.key.toLowerCase() === "k" || event.key.toLowerCase() === "f");

/** Search field above the channel tree (QA.27); filtering happens in `ChannelTree`. */
export function ChannelSearch({ value, onChange, onClear, onSubmit }: ChannelSearchProps) {
  const { t } = useTranslation("channels");
  const id = useId();
  const inputRef = useRef<HTMLInputElement>(null);
  const label = t("search.label");

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (!isFocusShortcut(event)) return;
      event.preventDefault();
      inputRef.current?.focus();
      inputRef.current?.select();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => {
      window.removeEventListener("keydown", onKeyDown);
    };
  }, []);

  return (
    <div role="search" className="channel-search">
      <label htmlFor={id} className="visually-hidden">
        {label}
      </label>
      <input
        ref={inputRef}
        id={id}
        type="search"
        className="channel-search__input"
        placeholder={label}
        autoComplete="off"
        spellCheck={false}
        aria-keyshortcuts="Control+K Control+F"
        value={value}
        onChange={(event) => {
          onChange(event.target.value);
        }}
        onKeyDown={(event) => {
          if (event.key === "Escape" && value !== "") {
            event.preventDefault();
            event.stopPropagation();
            onClear();
          } else if (event.key === "Enter") {
            event.preventDefault();
            onSubmit();
          }
        }}
      />
      {value !== "" && (
        <button
          type="button"
          className="channel-search__clear"
          aria-label={t("search.clear")}
          title={t("search.clear")}
          onClick={() => {
            onClear();
            inputRef.current?.focus();
          }}
        >
          <span aria-hidden="true">×</span>
        </button>
      )}
    </div>
  );
}
