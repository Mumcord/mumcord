/**
 * A floating media widget inside the app window (FR-2.70), e.g. a watched screen share or the
 * watch party. Moved by its title bar, resized from edges and corners (keeping an optional aspect
 * ratio), collapsed to a slim bar and closed. Keyboard: the title bar is focusable, arrow keys
 * move it and Shift+arrow keys resize it. Position and size are remembered per `kind` for the
 * session; the widget clicked last is on top.
 */
import {
  useEffect,
  useRef,
  useState,
  type KeyboardEvent,
  type PointerEvent,
  type ReactNode,
  type Ref,
} from "react";
import { useTranslation } from "react-i18next";
import {
  boundsOf,
  clampRect,
  EDGES,
  KEY_STEP,
  MIN_SIZE,
  moveRect,
  placeInside,
  resizeRect,
  type Edge,
  type Rect,
  type Size,
  type SizeRules,
} from "./floatingWidgetGeometry";
import { bringToFront, setWidgetLayout, useWidgetLayout, useZIndex } from "./floatingWidgetStore";
import "./floatingWidget.css";

/** Title bar height assumed until it was measured once. */
const DEFAULT_CHROME = 40;

export interface FloatingWidgetProps {
  /** Widget type; position and size are remembered per type. */
  kind: string;
  /** Accessible name of the widget. */
  label: string;
  title: ReactNode;
  /** Title of the slim bar while collapsed; defaults to `title`. */
  collapsedTitle?: ReactNode;
  /** Content right after the title, e.g. avatars; pressing it never starts a drag. */
  titleExtra?: ReactNode;
  /** Controls in the title bar, e.g. full screen. */
  actions?: ReactNode;
  /** Controls shown in the slim bar only while collapsed, e.g. play/pause. */
  collapsedActions?: ReactNode;
  /** The close action ("Stop watching"); without it the widget has no close button. */
  close?: { label: string; onClose: () => void };
  /** Width / height of the content below the title bar. */
  aspect?: number;
  /** Fixed height in px below the aspect-bound content, e.g. party controls. */
  extra?: number;
  /** Where the widget opens the first time in this session. */
  initialRect: (window: Size) => Rect;
  ref?: Ref<HTMLElement>;
  className?: string;
  children: ReactNode;
}

function windowSize(): Size {
  return { width: window.innerWidth, height: window.innerHeight };
}

function useWindowSize(): Size {
  const [size, setSize] = useState(windowSize);
  useEffect(() => {
    const onResize = () => {
      setSize(windowSize());
    };
    window.addEventListener("resize", onResize);
    return () => {
      window.removeEventListener("resize", onResize);
    };
  }, []);
  return size;
}

const ARROWS: Record<string, [number, number]> = {
  ArrowLeft: [-1, 0],
  ArrowRight: [1, 0],
  ArrowUp: [0, -1],
  ArrowDown: [0, 1],
};

export function FloatingWidget({
  kind,
  label,
  title,
  collapsedTitle,
  titleExtra,
  actions,
  collapsedActions,
  close,
  aspect,
  extra,
  initialRect,
  ref,
  className,
  children,
}: FloatingWidgetProps) {
  const { t } = useTranslation("common");
  const bounds = boundsOf(useWindowSize());
  const layout = useWidgetLayout(kind, () => ({
    rect: initialRect(windowSize()),
    collapsed: false,
  }));
  const zIndex = useZIndex(kind);
  const barRef = useRef<HTMLElement>(null);
  const [chrome, setChrome] = useState(DEFAULT_CHROME);
  const [gesture, setGesture] = useState<"move" | Edge | null>(null);
  const rules: SizeRules = { min: MIN_SIZE, aspect, chrome, extra };
  const { collapsed } = layout;
  const rect = collapsed
    ? placeInside({ ...clampRect(layout.rect, bounds, rules), height: chrome }, bounds)
    : clampRect(layout.rect, bounds, rules);

  // A widget that opens comes to the front.
  useEffect(() => {
    bringToFront(kind);
  }, [kind]);

  const measureChrome = (): number => {
    const height = barRef.current?.offsetHeight ?? 0;
    const measured = height > 0 ? height : chrome;
    if (measured !== chrome) setChrome(measured);
    return measured;
  };

  const store = (next: Rect) => {
    setWidgetLayout(kind, { rect: next, collapsed });
  };

  const startGesture = (event: PointerEvent, kindOfGesture: "move" | Edge) => {
    if (event.button !== 0) return;
    event.preventDefault();
    const target = event.currentTarget;
    const pointerId = event.pointerId;
    // Keeps the pointer events coming while the pointer is over an iframe or outside the widget.
    // Window listeners below are the fallback where capture is not honoured (WebKitGTK).
    try {
      target.setPointerCapture(pointerId);
    } catch {
      // The pointer is already gone; the window listeners still end the gesture.
    }
    const start = rect;
    const originX = event.clientX;
    const originY = event.clientY;
    const gestureRules = { ...rules, chrome: measureChrome() };
    setGesture(kindOfGesture);
    let latest: { x: number; y: number } | null = null;
    let frame = 0;
    const apply = () => {
      frame = 0;
      if (latest === null) return;
      const dx = latest.x - originX;
      const dy = latest.y - originY;
      latest = null;
      store(
        kindOfGesture === "move"
          ? moveRect(start, dx, dy, bounds)
          : resizeRect(start, kindOfGesture, dx, dy, bounds, gestureRules),
      );
    };
    // One layout update per frame keeps fast drags from falling behind the pointer.
    const onMove = (move: globalThis.PointerEvent) => {
      latest = { x: move.clientX, y: move.clientY };
      frame ||= requestAnimationFrame(apply);
    };
    const onEnd = () => {
      if (frame !== 0) cancelAnimationFrame(frame);
      apply();
      setGesture(null);
      if (target.hasPointerCapture(pointerId)) target.releasePointerCapture(pointerId);
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onEnd);
      window.removeEventListener("pointercancel", onEnd);
      window.removeEventListener("blur", onEnd);
      target.removeEventListener("lostpointercapture", onEnd);
    };
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onEnd);
    window.addEventListener("pointercancel", onEnd);
    window.addEventListener("blur", onEnd);
    target.addEventListener("lostpointercapture", onEnd);
  };

  const onBarPointerDown = (event: PointerEvent<HTMLElement>) => {
    // Buttons in the bar keep their click.
    if (event.target instanceof Element && event.target.closest("button, [data-no-drag]") !== null)
      return;
    startGesture(event, "move");
  };

  const onBarKeyDown = (event: KeyboardEvent<HTMLElement>) => {
    if (event.target !== event.currentTarget) return;
    const arrow = ARROWS[event.key];
    if (arrow === undefined || event.altKey || event.ctrlKey || event.metaKey) return;
    event.preventDefault();
    const [x, y] = arrow;
    if (!event.shiftKey) {
      store(moveRect(rect, x * KEY_STEP, y * KEY_STEP, bounds));
      return;
    }
    if (collapsed) return;
    const edge: Edge = x !== 0 ? "e" : "s";
    const keyRules = { ...rules, chrome: measureChrome() };
    store(resizeRect(rect, edge, x * KEY_STEP, y * KEY_STEP, bounds, keyRules));
  };

  const toggleCollapsed = () => {
    setWidgetLayout(kind, { rect: layout.rect, collapsed: !collapsed });
  };

  /* eslint-disable jsx-a11y/no-static-element-interactions, jsx-a11y/no-noninteractive-tabindex --
     The capture handlers only restack the widget; the focusable title bar is the move/resize
     handle (spec FR-2.70), operated with arrow keys as announced by aria-keyshortcuts. */
  return (
    <section
      ref={ref}
      className={["floating-widget", className].filter(Boolean).join(" ")}
      aria-label={label}
      tabIndex={-1}
      data-collapsed={collapsed}
      data-gesture={gesture ?? undefined}
      style={{
        left: rect.x,
        top: rect.y,
        width: rect.width,
        height: collapsed ? undefined : rect.height,
        zIndex,
      }}
      onPointerDownCapture={() => {
        bringToFront(kind);
      }}
      onFocusCapture={() => {
        bringToFront(kind);
      }}
    >
      <header
        ref={barRef}
        className="floating-widget__bar"
        tabIndex={0}
        aria-label={t("floatingWidget.handle", { name: label })}
        aria-keyshortcuts="ArrowLeft ArrowRight ArrowUp ArrowDown Shift+ArrowLeft Shift+ArrowRight Shift+ArrowUp Shift+ArrowDown"
        onPointerDown={onBarPointerDown}
        onKeyDown={onBarKeyDown}
      >
        <h2
          className="floating-widget__title"
          data-has-extra={titleExtra !== undefined || undefined}
        >
          {collapsed ? (collapsedTitle ?? title) : title}
        </h2>
        {titleExtra !== undefined && (
          <div className="floating-widget__title-extra" data-no-drag>
            {titleExtra}
          </div>
        )}
        {collapsed && collapsedActions}
        {actions}
        <button
          type="button"
          className="button"
          data-tooltip={collapsed ? t("floatingWidget.expand") : t("floatingWidget.collapse")}
          aria-label={collapsed ? t("floatingWidget.expand") : t("floatingWidget.collapse")}
          aria-expanded={!collapsed}
          onClick={toggleCollapsed}
        >
          <span aria-hidden="true">{collapsed ? "▴" : "▾"}</span>
        </button>
        {close && (
          <button
            type="button"
            className="button"
            aria-label={close.label}
            data-tooltip={close.label}
            onClick={close.onClose}
          >
            <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true" focusable="false">
              <path
                d="M4 4l8 8M12 4l-8 8"
                fill="none"
                stroke="currentColor"
                strokeWidth="1.6"
                strokeLinecap="round"
              />
            </svg>
          </button>
        )}
      </header>
      {/* Hidden, not unmounted, so collapsing keeps a stream or video running. */}
      <div className="floating-widget__body" hidden={collapsed}>
        {children}
      </div>
      {!collapsed &&
        EDGES.map((edge) => (
          <div
            key={edge}
            className={`floating-widget__edge floating-widget__edge--${edge}`}
            data-edge={edge}
            aria-hidden="true"
            onPointerDown={(event) => {
              startGesture(event, edge);
            }}
          />
        ))}
    </section>
  );
  /* eslint-enable jsx-a11y/no-static-element-interactions, jsx-a11y/no-noninteractive-tabindex */
}
