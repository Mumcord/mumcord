import { useEffect, useId, useRef, useState, type KeyboardEvent } from "react";
import "./select.css";

export interface SelectOption {
  value: string;
  label: string;
}

interface SelectProps {
  /** Id of the control, so a visible `<label htmlFor>` names it. */
  id?: string;
  /** Accessible name when there is no visible label. */
  ariaLabel?: string;
  value: string;
  options: readonly SelectOption[];
  onChange: (value: string) => void;
  disabled?: boolean;
  className?: string;
}

/**
 * Themed dropdown (FR-2.20): a select-only combobox with a listbox popup. Arrow keys, Home/End,
 * type-ahead, Enter/Space to choose, Escape to close without leaving the surrounding dialog.
 */
export function Select({
  id,
  ariaLabel,
  value,
  options,
  onChange,
  disabled = false,
  className,
}: SelectProps) {
  const baseId = useId();
  const listId = `${baseId}-list`;
  const optionId = (index: number) => `${baseId}-option-${String(index)}`;
  const selectedIndex = Math.max(
    0,
    options.findIndex((option) => option.value === value),
  );
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(selectedIndex);
  const wrapperRef = useRef<HTMLDivElement>(null);
  const typed = useRef({ text: "", at: 0 });

  useEffect(() => {
    if (!open) return;
    const close = (event: PointerEvent) => {
      if (!wrapperRef.current?.contains(event.target as Node)) setOpen(false);
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
    setActive(selectedIndex);
    setOpen(true);
  };

  const choose = (index: number) => {
    const option = options[index];
    setOpen(false);
    if (option && option.value !== value) onChange(option.value);
  };

  const move = (index: number) => {
    setActive(Math.min(options.length - 1, Math.max(0, index)));
  };

  /** Index of the next option starting with what was typed in the last second. */
  const typeAhead = (character: string) => {
    const now = Date.now();
    const text =
      (now - typed.current.at > 1000 ? "" : typed.current.text) + character.toLowerCase();
    typed.current = { text, at: now };
    const from = open ? active : selectedIndex;
    const ordered = Array.from(
      { length: options.length },
      (_, offset) => (from + offset + (text.length === 1 ? 1 : 0)) % options.length,
    );
    return ordered.find((index) => options[index]?.label.toLowerCase().startsWith(text));
  };

  const onKeyDown = (event: KeyboardEvent<HTMLButtonElement>) => {
    if (disabled || event.altKey || event.ctrlKey || event.metaKey) return;
    const current = open ? active : selectedIndex;
    let handled = true;
    switch (event.key) {
      case "ArrowDown":
        if (open) move(current + 1);
        else show();
        break;
      case "ArrowUp":
        if (open) move(current - 1);
        else show();
        break;
      case "Home":
        if (open) move(0);
        else onChangeAt(0);
        break;
      case "End":
        if (open) move(options.length - 1);
        else onChangeAt(options.length - 1);
        break;
      case "Enter":
      case " ":
        if (open) choose(active);
        else show();
        break;
      case "Escape":
        if (open) setOpen(false);
        else handled = false;
        break;
      case "Tab":
        if (open) choose(active);
        handled = false;
        break;
      default: {
        if (event.key.length !== 1) {
          handled = false;
          break;
        }
        const match = typeAhead(event.key);
        if (match === undefined) break;
        if (open) setActive(match);
        else onChangeAt(match);
      }
    }
    if (handled) {
      event.preventDefault();
      event.stopPropagation();
    }
  };

  function onChangeAt(index: number) {
    const option = options[index];
    if (option && option.value !== value) onChange(option.value);
  }

  return (
    <div ref={wrapperRef} className={`select ${className ?? ""}`.trim()}>
      <button
        id={id}
        type="button"
        role="combobox"
        className="select__button"
        aria-label={ariaLabel}
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-controls={listId}
        aria-activedescendant={open ? optionId(active) : undefined}
        disabled={disabled}
        onClick={() => {
          if (open) setOpen(false);
          else show();
        }}
        onKeyDown={onKeyDown}
        onBlur={() => {
          setOpen(false);
        }}
      >
        <span className="select__value">{options[selectedIndex]?.label ?? ""}</span>
        <span className="select__chevron" aria-hidden="true" />
      </button>
      {open && (
        <div
          id={listId}
          role="listbox"
          tabIndex={-1}
          className="select__list"
          // Keeps focus on the button while the pointer picks an option.
          onMouseDown={(event) => {
            event.preventDefault();
          }}
        >
          {options.map((option, index) => (
            // Keyboard handling lives on the combobox button (aria-activedescendant pattern).
            // eslint-disable-next-line jsx-a11y/click-events-have-key-events
            <div
              key={option.value}
              id={optionId(index)}
              role="option"
              tabIndex={-1}
              aria-selected={index === selectedIndex}
              className={`select__option${index === active ? " select__option--active" : ""}`}
              onMouseMove={() => {
                setActive(index);
              }}
              onClick={() => {
                choose(index);
              }}
            >
              {option.label}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
