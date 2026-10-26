import { useRef, useState, type KeyboardEvent, type MouseEvent } from "react";
import { useTranslation } from "react-i18next";
import type { SwatchMenuRequest, ThemeSwatch } from "./themeSwatch";
import "./themePicker.css";

interface ThemePickerProps {
  /** Visible name of the whole picker. */
  label: string;
  /** First row: the presets (and stand-ins like "Use app theme"). */
  presets: readonly ThemeSwatch[];
  /** Second row under "Your themes": themes the user made or imported. */
  customThemes: readonly ThemeSwatch[];
  selectedId: string;
  onSelect: (id: string) => void;
  /** Shows the "+" circle at the end of "Your themes". */
  onAdd?: () => void;
  /** Right-click or the menu key on a circle. */
  onMenu?: (id: string, request: SwatchMenuRequest) => void;
}

const NEXT_KEYS = new Set(["ArrowRight", "ArrowDown"]);
const PREVIOUS_KEYS = new Set(["ArrowLeft", "ArrowUp"]);

/**
 * Theme picker of round swatches (FR-2.63): each circle shows a gradient of the theme's
 * background, surface and accent with the name below and a ring on the selected one. All circles
 * share one Tab stop; arrow keys move between them, Enter or Space applies the focused one.
 */
export function ThemePicker({
  label,
  presets,
  customThemes,
  selectedId,
  onSelect,
  onAdd,
  onMenu,
}: ThemePickerProps) {
  const { t } = useTranslation("common");
  const ref = useRef<HTMLFieldSetElement>(null);
  const all = [...presets, ...customThemes];
  const selectedIndex = Math.max(
    0,
    all.findIndex((swatch) => swatch.id === selectedId),
  );
  // The circle holding the Tab stop; follows the focus so Tab returns to where the user was.
  const [focusIndex, setFocusIndex] = useState<number | null>(null);
  const count = all.length + (onAdd ? 1 : 0);
  const tabIndex = focusIndex !== null && focusIndex < count ? focusIndex : selectedIndex;

  const onKeyDown = (event: KeyboardEvent<HTMLButtonElement>) => {
    const circles = [
      ...(ref.current?.querySelectorAll<HTMLElement>(".theme-picker__circle") ?? []),
    ];
    const index = circles.findIndex((circle) => circle === document.activeElement);
    if (index < 0) return;
    let next: number;
    if (NEXT_KEYS.has(event.key)) next = (index + 1) % circles.length;
    else if (PREVIOUS_KEYS.has(event.key)) next = (index - 1 + circles.length) % circles.length;
    else if (event.key === "Home") next = 0;
    else if (event.key === "End") next = circles.length - 1;
    else return;
    event.preventDefault();
    setFocusIndex(next);
    circles[next]?.focus();
  };

  const circle = (swatch: ThemeSwatch, index: number) => {
    const selected = swatch.id === selectedId;
    const { background, surface, accent } = swatch.colors;
    const openMenu = (event: MouseEvent<HTMLButtonElement>) => {
      if (!onMenu) return;
      event.preventDefault();
      const button = event.currentTarget;
      // The menu key fires the event without a pointer position; open it at the circle then.
      const keyboard = event.clientX === 0 && event.clientY === 0;
      const rect = button.getBoundingClientRect();
      onMenu(swatch.id, {
        x: keyboard ? rect.left : event.clientX,
        y: keyboard ? rect.bottom : event.clientY,
        opener: button,
      });
    };
    return (
      <li key={swatch.id} className="theme-picker__item">
        <button
          type="button"
          className="theme-picker__circle"
          data-selected={selected || undefined}
          tabIndex={index === tabIndex ? 0 : -1}
          aria-label={t(selected ? "themePicker.swatchSelected" : "themePicker.swatch", {
            name: swatch.name,
          })}
          style={{ background: `linear-gradient(135deg, ${background}, ${surface}, ${accent})` }}
          onFocus={() => {
            setFocusIndex(index);
          }}
          onClick={() => {
            onSelect(swatch.id);
          }}
          onKeyDown={onKeyDown}
          onContextMenu={openMenu}
        />
        <span className="theme-picker__name" aria-hidden="true">
          {swatch.name}
        </span>
      </li>
    );
  };

  const addIndex = all.length;
  return (
    <fieldset ref={ref} className="theme-picker">
      <legend className="theme-picker__legend">{label}</legend>
      <ul className="theme-picker__row">{presets.map((swatch, index) => circle(swatch, index))}</ul>
      {(customThemes.length > 0 || onAdd) && (
        <>
          <p className="theme-picker__heading" aria-hidden="true">
            {t("themePicker.yourThemes")}
          </p>
          <ul className="theme-picker__row" aria-label={t("themePicker.yourThemes")}>
            {customThemes.map((swatch, index) => circle(swatch, presets.length + index))}
            {onAdd && (
              <li className="theme-picker__item">
                <button
                  type="button"
                  className="theme-picker__circle theme-picker__circle--add"
                  tabIndex={addIndex === tabIndex ? 0 : -1}
                  aria-label={t("themePicker.add")}
                  onFocus={() => {
                    setFocusIndex(addIndex);
                  }}
                  onKeyDown={onKeyDown}
                  onClick={onAdd}
                >
                  <span aria-hidden="true">+</span>
                </button>
                <span className="theme-picker__name" aria-hidden="true">
                  {t("themePicker.addShort")}
                </span>
              </li>
            )}
          </ul>
        </>
      )}
    </fieldset>
  );
}
