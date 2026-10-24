import { useEffect, useRef, type KeyboardEvent, type PointerEvent } from "react";
import { COLUMN_MIN_REM, widthForDrag, widthForKey, type ResizableColumn } from "./columnWidths";
import "./resizeHandle.css";

export interface ResizeHandleProps {
  column: ResizableColumn;
  /** Accessible name, e.g. "Resize channel list". */
  label: string;
  /** Id of the column this handle resizes. */
  controls: string;
  /** Width shown now, in rem. */
  value: number;
  /** Largest width allowed now, in rem. */
  max: number;
  /** A new width while dragging or from the keyboard (already clamped). */
  onResize: (rem: number) => void;
  /** The user finished changing the width; time to remember it. */
  onCommit: () => void;
  /** Double-click: back to the default width. */
  onReset: () => void;
}

function rootFontPx(): number {
  return parseFloat(getComputedStyle(document.documentElement).fontSize) || 16;
}

const round = (rem: number) => Math.round(rem * 10) / 10;

/**
 * The draggable, focusable border of a side column (window splitter pattern). Pointer moves are
 * applied at most once per animation frame so dragging never queues more layouts than are painted.
 */
export function ResizeHandle({
  column,
  label,
  controls,
  value,
  max,
  onResize,
  onCommit,
  onReset,
}: ResizeHandleProps) {
  const drag = useRef<{ startX: number; startWidth: number; remPx: number; x: number } | null>(
    null,
  );
  const frame = useRef(0);

  useEffect(
    () => () => {
      cancelAnimationFrame(frame.current);
      delete document.documentElement.dataset.columnResizing;
    },
    [],
  );

  const onPointerDown = (event: PointerEvent<HTMLDivElement>) => {
    if (event.button !== 0) return;
    // Keeps the browser from starting a text selection under the pointer.
    event.preventDefault();
    event.currentTarget.setPointerCapture(event.pointerId);
    event.currentTarget.focus();
    // Whole-document cursor and no text selection while dragging, even over other elements.
    document.documentElement.dataset.columnResizing = column;
    drag.current = {
      startX: event.clientX,
      startWidth: value,
      remPx: rootFontPx(),
      x: event.clientX,
    };
  };

  const onPointerMove = (event: PointerEvent<HTMLDivElement>) => {
    const state = drag.current;
    if (!state) return;
    state.x = event.clientX;
    if (frame.current) return;
    frame.current = requestAnimationFrame(() => {
      frame.current = 0;
      const current = drag.current;
      if (!current) return;
      onResize(
        widthForDrag(column, current.startWidth, current.x - current.startX, current.remPx, max),
      );
    });
  };

  const endDrag = (event: PointerEvent<HTMLDivElement>) => {
    const state = drag.current;
    if (!state) return;
    drag.current = null;
    delete document.documentElement.dataset.columnResizing;
    cancelAnimationFrame(frame.current);
    frame.current = 0;
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
    // A cancelled pointer has no meaningful position; keep the last one seen.
    const x = event.type === "pointercancel" ? state.x : event.clientX;
    onResize(widthForDrag(column, state.startWidth, x - state.startX, state.remPx, max));
    onCommit();
  };

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const next = widthForKey(event.key, column, value, max);
    if (next === null) return;
    event.preventDefault();
    onResize(next);
    onCommit();
  };

  /* eslint-disable jsx-a11y/no-noninteractive-element-interactions, jsx-a11y/no-noninteractive-tabindex --
     A focusable separator is an interactive widget (ARIA window splitter pattern); the plugin
     treats every separator as static. */
  return (
    <div
      className="resize-handle"
      data-column={column}
      role="separator"
      aria-orientation="vertical"
      aria-label={label}
      aria-controls={controls}
      aria-valuenow={round(value)}
      aria-valuemin={COLUMN_MIN_REM}
      aria-valuemax={round(max)}
      tabIndex={0}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={endDrag}
      onPointerCancel={endDrag}
      onKeyDown={onKeyDown}
      onDoubleClick={onReset}
    />
  );
  /* eslint-enable jsx-a11y/no-noninteractive-element-interactions, jsx-a11y/no-noninteractive-tabindex */
}
