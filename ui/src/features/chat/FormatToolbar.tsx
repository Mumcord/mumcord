/**
 * Formatting buttons above the message input (FR-1.54). One Tab stop with arrow-key navigation
 * (WAI-ARIA toolbar pattern); clicking a button never takes the focus away from the input.
 */
import { useRef, useState, type KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";
import { FORMATS, SHORTCUTS, type Format } from "./formatting";

const ICONS: Record<Format, string> = {
  bold: "M7 5h6a3.5 3.5 0 0 1 0 7H7zM7 12h7a3.5 3.5 0 0 1 0 7H7z",
  italic: "M10 5h8M6 19h8M14 5l-4 14",
  strike:
    "M5 12h14M16 7.5C15.4 6 14 5 12 5c-2.5 0-4 1.3-4 3M8 16.5c.6 1.5 2 2.5 4 2.5 2.5 0 4-1.3 4-3",
  code: "M9 8l-4 4 4 4M15 8l4 4-4 4",
  codeBlock: "M4 5h16v14H4zM10 10l-2 2 2 2M14 10l2 2-2 2",
  quote: "M5 8h5v5c0 3-2 4.5-5 5M14 8h5v5c0 3-2 4.5-5 5",
  link: "M10 14a4 4 0 0 0 5.7 0l3-3a4 4 0 0 0-5.7-5.7l-1 1M14 10a4 4 0 0 0-5.7 0l-3 3a4 4 0 0 0 5.7 5.7l1-1",
};

/** "Control+Shift+E" → "Ctrl+Shift+E" for tooltips. */
const shortcutLabel = (shortcut: string) => shortcut.replace("Control", "Ctrl");

export interface FormatToolbarProps {
  onFormat: (format: Format) => void;
  disabled: boolean;
}

export function FormatToolbar({ onFormat, disabled }: FormatToolbarProps) {
  const { t } = useTranslation("chat");
  const [active, setActive] = useState(0);
  const buttons = useRef<(HTMLButtonElement | null)[]>([]);

  function onKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    const last = FORMATS.length - 1;
    const next =
      event.key === "ArrowRight"
        ? (active + 1) % FORMATS.length
        : event.key === "ArrowLeft"
          ? (active + last) % FORMATS.length
          : event.key === "Home"
            ? 0
            : event.key === "End"
              ? last
              : null;
    if (next === null) return;
    event.preventDefault();
    setActive(next);
    buttons.current[next]?.focus();
  }

  return (
    <div
      role="toolbar"
      aria-label={t("format.toolbar")}
      className="chat-format"
      onKeyDown={onKeyDown}
    >
      {FORMATS.map((format, index) => {
        const label = t(`format.${format}`);
        return (
          <button
            key={format}
            ref={(element) => {
              buttons.current[index] = element;
            }}
            type="button"
            className="chat-format__button"
            aria-label={label}
            aria-keyshortcuts={SHORTCUTS[format]}
            data-tooltip={`${label} (${shortcutLabel(SHORTCUTS[format])})`}
            tabIndex={index === active ? 0 : -1}
            disabled={disabled}
            // Keeps the caret and selection in the input while clicking.
            onMouseDown={(event) => {
              event.preventDefault();
            }}
            onClick={() => {
              setActive(index);
              onFormat(format);
            }}
          >
            <svg viewBox="0 0 24 24" width="16" height="16" aria-hidden="true" focusable="false">
              <path
                d={ICONS[format]}
                fill="none"
                stroke="currentColor"
                strokeWidth="2"
                strokeLinecap="round"
                strokeLinejoin="round"
              />
            </svg>
          </button>
        );
      })}
    </div>
  );
}
