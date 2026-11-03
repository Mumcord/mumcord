import { useEffect, useRef, type KeyboardEvent, type PointerEvent, type ReactNode } from "react";
import { COLUMN_MIN_REM, widthForDrag, widthForKey, type ResizableColumn } from "./columnWidths";
import "./resizeHandle.css";

export interface ResizeHandleProps {
  /** "vertical" sits between two columns, "horizontal" between a pane above and one below. */
  orientation?: "vertical" | "horizontal";
  /** Which handle this is, for styling and the whole-document drag cursor. */
  name: string;
  /** Accessible name, e.g. "Resize channel list". */
  label: string;
  /** Id of the pane this handle resizes. */
  controls: string;
  /** Value, range and spoken value for assistive technology. */
  aria: { now: number; min: number; max: number; text?: string };
  /** The value a drag starts from; handed back to `onDrag`. */
  start: number;
  /** A drag began. */
  onDragStart?: () => void;
  /**
   * The pointer moved `deltaPx` along the handle's axis (right or down is positive) since the drag
   * started at `start`; `done` is set on release. Applied at most once per animation frame.
   */
  onDrag: (start: number, deltaPx: number, altKey: boolean, done: boolean) => void;
  /** A key press; true when the handle used it. */
  onKey: (key: string) => boolean;
  /** Double-click: back to the default. */
  onReset: () => void;
  /** Extra content inside the handle, e.g. a read-out (hidden from assistive technology). */
  children?: ReactNode;
}

function rootFontPx(): number {
  return parseFloat(getComputedStyle(document.documentElement).fontSize) || 16;
}

const round = (rem: number) => Math.round(rem * 10) / 10;

/**
 * The draggable, focusable border between two panes (ARIA window splitter pattern). Pointer moves
 * are applied at most once per animation frame so dragging never queues more layouts than are
 * painted.
 */
export function ResizeHandle({
  orientation = "vertical",
  name,
  label,
  controls,
  aria,
  start,
  onDragStart,
  onDrag,
  onKey,
  onReset,
  children,
}: ResizeHandleProps) {
  const drag = useRef<{ from: number; start: number; at: number; alt: boolean } | null>(null);
  const frame = useRef(0);
  const vertical = orientation === "vertical";
  // The whole-document cursor and selection lock while dragging, per direction (resizeHandle.css).
  const flag = vertical ? "data-column-resizing" : "data-row-resizing";

  useEffect(
    () => () => {
      cancelAnimationFrame(frame.current);
      document.documentElement.removeAttribute(flag);
    },
    [flag],
  );

  const position = (event: PointerEvent<HTMLDivElement>) =>
    vertical ? event.clientX : event.clientY;

  const onPointerDown = (event: PointerEvent<HTMLDivElement>) => {
    if (event.button !== 0) return;
    // Keeps the browser from starting a text selection under the pointer.
    event.preventDefault();
    event.currentTarget.setPointerCapture(event.pointerId);
    event.currentTarget.focus();
    document.documentElement.setAttribute(flag, name);
    const at = position(event);
    drag.current = { from: at, start, at, alt: event.altKey };
    onDragStart?.();
  };

  const onPointerMove = (event: PointerEvent<HTMLDivElement>) => {
    const state = drag.current;
    if (!state) return;
    state.at = position(event);
    state.alt = event.altKey;
    if (frame.current) return;
    frame.current = requestAnimationFrame(() => {
      frame.current = 0;
      const now = drag.current;
      if (!now) return;
      onDrag(now.start, now.at - now.from, now.alt, false);
    });
  };

  const endDrag = (event: PointerEvent<HTMLDivElement>) => {
    const state = drag.current;
    if (!state) return;
    drag.current = null;
    document.documentElement.removeAttribute(flag);
    cancelAnimationFrame(frame.current);
    frame.current = 0;
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
    // A cancelled pointer has no meaningful position; keep the last one seen.
    const cancelled = event.type === "pointercancel";
    const at = cancelled ? state.at : position(event);
    onDrag(state.start, at - state.from, cancelled ? state.alt : event.altKey, true);
  };

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (onKey(event.key)) event.preventDefault();
  };

  /* eslint-disable jsx-a11y/no-noninteractive-element-interactions, jsx-a11y/no-noninteractive-tabindex --
     A focusable separator is an interactive widget (ARIA window splitter pattern); the plugin
     treats every separator as static. */
  return (
    <div
      className="resize-handle"
      data-column={vertical ? name : undefined}
      data-orientation={orientation}
      role="separator"
      aria-orientation={orientation}
      aria-label={label}
      aria-controls={controls}
      aria-valuenow={aria.now}
      aria-valuemin={aria.min}
      aria-valuemax={aria.max}
      aria-valuetext={aria.text}
      tabIndex={0}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={endDrag}
      onPointerCancel={endDrag}
      onKeyDown={onKeyDown}
      onDoubleClick={onReset}
    >
      {children}
    </div>
  );
  /* eslint-enable jsx-a11y/no-noninteractive-element-interactions, jsx-a11y/no-noninteractive-tabindex */
}

export interface ColumnResizeHandleProps {
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

/** The border of a resizable side column (FR-1.15, QA.29), with its width in rem. */
export function ColumnResizeHandle({
  column,
  label,
  controls,
  value,
  max,
  onResize,
  onCommit,
  onReset,
}: ColumnResizeHandleProps) {
  return (
    <ResizeHandle
      name={column}
      label={label}
      controls={controls}
      aria={{ now: round(value), min: COLUMN_MIN_REM, max: round(max) }}
      start={value}
      onDrag={(start, deltaPx, _alt, done) => {
        onResize(widthForDrag(column, start, deltaPx, rootFontPx(), max));
        if (done) onCommit();
      }}
      onKey={(key) => {
        const next = widthForKey(key, column, value, max);
        if (next === null) return false;
        onResize(next);
        onCommit();
        return true;
      }}
      onReset={onReset}
    />
  );
}
