import { useEffect, useId, useRef, useState, type KeyboardEvent } from "react";
import "./select.css";
import { filterOptions, type SearchableOption } from "./searchableOptions";

interface SearchableSelectProps {
  /** Id of the text field, so a visible `<label htmlFor>` names it. */
  id?: string;
  value: string;
  options: readonly SearchableOption[];
  onChange: (value: string) => void;
  /** Shown in the empty field while searching. */
  placeholder: string;
  /** Announced when nothing matches what was typed. */
  noMatch: string;
  className?: string;
}

/**
 * Themed dropdown with a search field (editable combobox with a listbox popup, ARIA 1.2). Typing
 * filters the list, arrow keys and Home/End move through it, Enter chooses, Escape first closes
 * the list and only then reaches the surrounding dialog.
 */
export function SearchableSelect({
  id,
  value,
  options,
  onChange,
  placeholder,
  noMatch,
  className,
}: SearchableSelectProps) {
  const baseId = useId();
  const listId = `${baseId}-list`;
  const optionId = (index: number) => `${baseId}-option-${String(index)}`;
  const selected = options.find((option) => option.value === value);
  const [open, setOpen] = useState(false);
  // null while the field shows the chosen option rather than a search.
  const [query, setQuery] = useState<string | null>(null);
  const [active, setActive] = useState(0);
  const wrapperRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const matches = filterOptions(options, query ?? "");

  useEffect(() => {
    if (!open) return;
    const close = (event: PointerEvent) => {
      if (!wrapperRef.current?.contains(event.target as Node)) {
        setOpen(false);
        setQuery(null);
      }
    };
    document.addEventListener("pointerdown", close);
    return () => {
      document.removeEventListener("pointerdown", close);
    };
  }, [open]);

  useEffect(() => {
    // jsdom has no scrollIntoView, so the call is optional despite the DOM typings.
    // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition
    if (open) document.getElementById(optionId(active))?.scrollIntoView?.({ block: "nearest" });
    // optionId only depends on baseId
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, active]);

  const show = () => {
    setActive(
      Math.max(
        0,
        matches.findIndex((option) => option.value === value),
      ),
    );
    setOpen(true);
  };

  const close = () => {
    setOpen(false);
    setQuery(null);
  };

  const choose = (option: SearchableOption | undefined) => {
    close();
    if (option && option.value !== value) onChange(option.value);
  };

  const move = (index: number) => {
    setActive(Math.min(matches.length - 1, Math.max(0, index)));
  };

  const onKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.altKey || event.ctrlKey || event.metaKey) return;
    let handled = true;
    switch (event.key) {
      case "ArrowDown":
        if (open) move(active + 1);
        else show();
        break;
      case "ArrowUp":
        if (open) move(active - 1);
        else show();
        break;
      case "Home":
      case "End":
        // While searching, Home/End move the caret in the field as usual.
        if (open && query === null) move(event.key === "Home" ? 0 : matches.length - 1);
        else handled = false;
        break;
      case "Enter":
        if (open) choose(matches[active]);
        else show();
        break;
      case "Escape":
        if (open || query !== null) close();
        else handled = false;
        break;
      default:
        handled = false;
    }
    if (handled) {
      event.preventDefault();
      event.stopPropagation();
    }
  };

  const activeOption = open ? matches[active] : undefined;
  return (
    <div ref={wrapperRef} className={`select select--searchable ${className ?? ""}`.trim()}>
      <div className="select__button select__field">
        <input
          ref={inputRef}
          id={id}
          type="text"
          role="combobox"
          className="select__input"
          autoComplete="off"
          spellCheck={false}
          aria-autocomplete="list"
          aria-expanded={open}
          aria-controls={listId}
          aria-activedescendant={activeOption ? optionId(active) : undefined}
          lang={query === null ? selected?.lang : undefined}
          placeholder={placeholder}
          value={query ?? selected?.label ?? ""}
          onChange={(event) => {
            setQuery(event.target.value);
            setActive(0);
            setOpen(true);
          }}
          onFocus={(event) => {
            // Typing replaces the shown name instead of appending to it.
            event.target.select();
          }}
          onClick={() => {
            if (!open) show();
          }}
          onKeyDown={onKeyDown}
          onBlur={close}
        />
        <button
          type="button"
          className="select__toggle"
          tabIndex={-1}
          aria-hidden="true"
          // Keeps focus in the field.
          onMouseDown={(event) => {
            event.preventDefault();
          }}
          onClick={() => {
            if (open) close();
            else {
              inputRef.current?.focus();
              show();
            }
          }}
        >
          <span className="select__chevron" />
        </button>
      </div>
      {open && matches.length > 0 && (
        <div
          id={listId}
          role="listbox"
          tabIndex={-1}
          className="select__list"
          // Keeps focus in the field while the pointer picks an option.
          onMouseDown={(event) => {
            event.preventDefault();
          }}
        >
          {matches.map((option, index) => (
            // Keyboard handling lives on the combobox field (aria-activedescendant pattern).
            // eslint-disable-next-line jsx-a11y/click-events-have-key-events
            <div
              key={option.value}
              id={optionId(index)}
              role="option"
              tabIndex={-1}
              lang={option.lang}
              aria-selected={option.value === value}
              className={`select__option${index === active ? " select__option--active" : ""}`}
              onMouseMove={() => {
                setActive(index);
              }}
              onClick={() => {
                choose(option);
              }}
            >
              {option.label}
            </div>
          ))}
        </div>
      )}
      <div className="select__status" role="status">
        {open && matches.length === 0 ? noMatch : ""}
      </div>
    </div>
  );
}
