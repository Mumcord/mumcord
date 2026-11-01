import { useRef, useState, type KeyboardEvent, type PointerEvent, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import type { PluginComponent } from "../../../api";
import { useRender } from "./context";
import { dropIndex } from "./helpers";

/** Most rows a list shows (FR-6.41); the host refuses more. */
export const MAX_ROWS = 50;

interface Props {
  list: Extract<PluginComponent, { type: "list" }>;
  rows: readonly ReactNode[];
}

/**
 * A list of rows (FR-6.41). With a reorder action every row gets a handle: drag it with the
 * pointer, or focus it and press Arrow Up/Down. Each move sends `{from, to}`; the plugin
 * updates the tree itself, so the list never reorders on its own.
 */
export function PluginList({ list, rows }: Props) {
  const { send } = useRender();
  const { t } = useTranslation("pluginSlots");
  const listRef = useRef<HTMLUListElement>(null);
  const [drag, setDrag] = useState<{ from: number; to: number } | null>(null);
  const [announcement, setAnnouncement] = useState("");
  const shown = rows.slice(0, MAX_ROWS);
  const action = list.reorderAction;

  const move = (from: number, to: number) => {
    if (action === null || from === to || to < 0 || to >= shown.length) return;
    send(action, { type: "reorder", from, to });
    setAnnouncement(t("list.moved", { from: from + 1, to: to + 1, total: shown.length }));
  };

  const targetAt = (y: number) => {
    const items = listRef.current?.querySelectorAll<HTMLElement>(":scope > li") ?? [];
    const middles = Array.from(items, (item) => {
      const box = item.getBoundingClientRect();
      return box.top + box.height / 2;
    });
    return dropIndex(middles, y);
  };

  const handleProps = (index: number) => ({
    onPointerDown: (e: PointerEvent<HTMLButtonElement>) => {
      e.currentTarget.setPointerCapture(e.pointerId);
      setDrag({ from: index, to: index });
    },
    onPointerMove: (e: PointerEvent<HTMLButtonElement>) => {
      if (drag !== null) setDrag({ from: drag.from, to: targetAt(e.clientY) });
    },
    onPointerUp: () => {
      if (drag !== null) move(drag.from, drag.to);
      setDrag(null);
    },
    onPointerCancel: () => {
      setDrag(null);
    },
    onKeyDown: (e: KeyboardEvent<HTMLButtonElement>) => {
      const to = e.key === "ArrowUp" ? index - 1 : e.key === "ArrowDown" ? index + 1 : null;
      if (to === null) return;
      e.preventDefault();
      move(index, to);
    },
  });

  return (
    <>
      <ul ref={listRef} className="pr-list" aria-label={list.label === "" ? undefined : list.label}>
        {shown.map((row, index) => (
          <li
            // Rows have no identity of their own; the plugin replaces the whole tree on change.
            key={index}
            className="pr-list__row"
            data-dragging={drag?.from === index ? "" : undefined}
            data-drop-target={
              drag !== null && drag.to === index && drag.from !== index ? "" : undefined
            }
          >
            {action !== null && (
              <button
                type="button"
                className="pr-list__handle"
                aria-label={t("list.handle", { position: index + 1, total: shown.length })}
                aria-keyshortcuts="ArrowUp ArrowDown"
                {...handleProps(index)}
              >
                <span aria-hidden="true">⠿</span>
              </button>
            )}
            <div className="pr-list__content">{row}</div>
          </li>
        ))}
      </ul>
      {action !== null && (
        <span className="visually-hidden" role="status">
          {announcement}
        </span>
      )}
    </>
  );
}
