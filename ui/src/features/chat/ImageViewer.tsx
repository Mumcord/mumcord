/**
 * Full-window image viewer (FR-1.56): fit to window, zoom around the pointer (wheel, pinch, + / −),
 * pan by dragging, 0 for fit, double-click for 100 %, ← / → through the conversation's images,
 * Escape to close, "Save image" and, for external images, "Open in browser". Thumbnails sent by
 * Mumcord users are replaced by the full image once it arrives from the sender (FR-1.60c). The view is one CSS
 * transform written once per animation frame, so a storm of wheel events costs one update.
 */
import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type KeyboardEvent,
  type PointerEvent,
} from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import { getApi } from "../../api";
import { BetaBadge } from "../../components/BetaBadge";
import { pickSavePath } from "../settings/fileDialogs";
import { removeFavorite, saveFavorite, useGifFavorites } from "./favorites";
import { useFullImage, type FullImageSource } from "./fullImage";
import { openInBrowser } from "./openLink";
import {
  clampView,
  fitView,
  isFit,
  wheelFactor,
  zoomAround,
  ZOOM_STEP,
  type Size,
  type View,
} from "./viewerMath";

export interface ViewerImage {
  /** `data:` URL of the already loaded image. */
  src: string;
  /** Address of an external image; only these offer "Open in browser". */
  url?: string | undefined;
  name: string;
  /** Set when `src` is a thumbnail whose full image can be fetched from the sender. */
  full?: FullImageSource | undefined;
}

interface ImageViewerProps {
  images: ViewerImage[];
  index: number;
  onIndexChange: (index: number) => void;
  onClose: () => void;
}

const FOCUSABLE = "button:not([disabled])";

/** File extension for a `data:image/...` URL. */
function extensionOf(src: string): string {
  const match = /^data:image\/(png|jpeg|gif|webp)/.exec(src);
  if (!match) return "png";
  return match[1] === "jpeg" ? "jpg" : (match[1] ?? "png");
}

/** Suggested file name: the external file's own name, else "image". */
function suggestedName(image: ViewerImage, extension: string): string {
  const last = image.name.replace(/[\\/:*?"<>|]/g, "_").trim();
  const base = last === "" ? "image" : last;
  return base.toLowerCase().endsWith(`.${extension}`) ? base : `${base}.${extension}`;
}

/** Loading progress of the full image, or that only the thumbnail is available. */
function FullImageHint({ state }: { state: ReturnType<typeof useFullImage> }) {
  const { t } = useTranslation("chat");
  if (state.status === "loading") {
    // The badge sits outside the live region so screen readers don't repeat it on every progress tick.
    return (
      <p className="image-viewer__hint">
        <span role="status">
          {state.progress === null
            ? t("viewer.fullLoading")
            : t("viewer.fullProgress", { percent: Math.floor(state.progress * 100) })}
        </span>{" "}
        <BetaBadge />
      </p>
    );
  }
  if (state.status === "failed") {
    return (
      <p className="image-viewer__hint" role="status">
        {t("viewer.fullUnavailable")}
      </p>
    );
  }
  return null;
}

export function ImageViewer({ images, index, onIndexChange, onClose }: ImageViewerProps) {
  const { t } = useTranslation("chat");
  const current = images[index];
  const full = useFullImage(current?.full);
  // The thumbnail stays until the full image is there, and when it cannot be had.
  const shown = full.status === "done" ? full.src : current?.src;
  const favorites = useGifFavorites();
  const dialogRef = useRef<HTMLDivElement>(null);
  const stageRef = useRef<HTMLDivElement>(null);
  const imgRef = useRef<HTMLImageElement>(null);
  const natural = useRef<Size>({ width: 0, height: 0 });
  const view = useRef<View>({ scale: 1, x: 0, y: 0 });
  const frame = useRef<number | null>(null);
  const pointers = useRef(new Map<number, { x: number; y: number }>());
  const drag = useRef<{ x: number; y: number } | null>(null);
  // A drag that ends on the empty stage must not count as a click that closes the viewer.
  const moved = useRef(false);
  const [percent, setPercent] = useState(100);
  const [panning, setPanning] = useState(false);
  const [zoomed, setZoomed] = useState(false);
  const [status, setStatus] = useState<{ text: string; failed: boolean } | null>(null);

  const stageSize = useCallback((): Size => {
    const rect = stageRef.current?.getBoundingClientRect();
    return { width: rect?.width ?? 0, height: rect?.height ?? 0 };
  }, []);

  const flush = useCallback(() => {
    frame.current = null;
    const { scale, x, y } = view.current;
    const img = imgRef.current;
    if (img)
      img.style.transform = `translate(${String(x)}px, ${String(y)}px) scale(${String(scale)})`;
    setPercent(Math.round(scale * 100));
    setZoomed(!isFit(view.current, stageSize(), natural.current));
  }, [stageSize]);

  // One write per frame, however many wheel / pointer events arrive.
  const apply = useCallback(
    (next: View) => {
      view.current = next;
      frame.current ??= requestAnimationFrame(flush);
    },
    [flush],
  );

  const fit = useCallback(() => {
    apply(fitView(stageSize(), natural.current));
  }, [apply, stageSize]);

  // Zoom around the stage centre (keys, buttons) or a point relative to it (wheel, pinch).
  const zoomBy = useCallback(
    (factor: number, around = { x: 0, y: 0 }) => {
      apply(zoomAround(view.current, factor, around, stageSize(), natural.current));
    },
    [apply, stageSize],
  );

  const toggleActualSize = useCallback(
    (around: { x: number; y: number }) => {
      const size = stageSize();
      const now = view.current;
      if (Math.abs(now.scale - 1) < 1e-6) fit();
      else apply(zoomAround(now, 1 / now.scale, around, size, natural.current));
    },
    [apply, fit, stageSize],
  );

  const relativeToCentre = useCallback((clientX: number, clientY: number) => {
    const rect = stageRef.current?.getBoundingClientRect();
    if (!rect) return { x: 0, y: 0 };
    return { x: clientX - rect.left - rect.width / 2, y: clientY - rect.top - rect.height / 2 };
  }, []);

  // A new image starts fitted (its size is known once it has loaded).
  useLayoutEffect(() => {
    natural.current = { width: 0, height: 0 };
    view.current = { scale: 1, x: 0, y: 0 };
    setStatus(null);
    const img = imgRef.current;
    if (img?.complete && img.naturalWidth > 0) {
      natural.current = { width: img.naturalWidth, height: img.naturalHeight };
    }
    fit();
  }, [shown, fit]);

  useEffect(() => {
    function onResize() {
      const size = stageSize();
      if (isFit(view.current, size, natural.current) || natural.current.width === 0) fit();
      else apply(clampView(view.current, size, natural.current));
    }
    window.addEventListener("resize", onResize);
    return () => {
      window.removeEventListener("resize", onResize);
      if (frame.current !== null) cancelAnimationFrame(frame.current);
      // Effects run twice in development; a stale handle would block every later frame.
      frame.current = null;
    };
  }, [apply, fit, stageSize]);

  // Native listener: React's wheel handler is passive and could not stop the page from scrolling.
  useEffect(() => {
    const stage = stageRef.current;
    if (!stage) return;
    function onWheel(event: WheelEvent) {
      event.preventDefault();
      zoomBy(
        wheelFactor(event.deltaY, event.deltaMode, event.ctrlKey),
        relativeToCentre(event.clientX, event.clientY),
      );
    }
    stage.addEventListener("wheel", onWheel, { passive: false });
    return () => {
      stage.removeEventListener("wheel", onWheel);
    };
  }, [relativeToCentre, zoomBy]);

  useEffect(() => {
    dialogRef.current?.focus();
  }, []);

  function onKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    if (event.ctrlKey || event.metaKey || event.altKey) return;
    switch (event.key) {
      case "Escape":
        onClose();
        break;
      case "+":
      case "=":
        zoomBy(ZOOM_STEP);
        break;
      case "-":
      case "_":
        zoomBy(1 / ZOOM_STEP);
        break;
      case "0":
        fit();
        break;
      case "ArrowLeft":
        if (index > 0) onIndexChange(index - 1);
        break;
      case "ArrowRight":
        if (index < images.length - 1) onIndexChange(index + 1);
        break;
      case "Tab": {
        const focusable = [...(dialogRef.current?.querySelectorAll<HTMLElement>(FOCUSABLE) ?? [])];
        const first = focusable[0];
        const last = focusable[focusable.length - 1];
        if (!first || !last) break;
        if (
          event.shiftKey &&
          (document.activeElement === first || document.activeElement === dialogRef.current)
        ) {
          last.focus();
        } else if (!event.shiftKey && document.activeElement === last) {
          first.focus();
        } else {
          return;
        }
        break;
      }
      default:
        return;
    }
    event.preventDefault();
    // The chat list behind the viewer must not see these keys.
    event.stopPropagation();
  }

  function onPointerDown(event: PointerEvent<HTMLDivElement>) {
    event.currentTarget.setPointerCapture(event.pointerId);
    pointers.current.set(event.pointerId, { x: event.clientX, y: event.clientY });
    drag.current = { x: event.clientX, y: event.clientY };
    moved.current = false;
    setPanning(true);
  }

  function onPointerMove(event: PointerEvent<HTMLDivElement>) {
    const known = pointers.current.get(event.pointerId);
    if (!known) return;
    const others = [...pointers.current.entries()].filter(([id]) => id !== event.pointerId);
    const other = others[0]?.[1];
    if (other) {
      // Two fingers: the change of their distance zooms around their midpoint.
      const before = Math.hypot(known.x - other.x, known.y - other.y);
      const after = Math.hypot(event.clientX - other.x, event.clientY - other.y);
      if (before > 0 && after > 0) {
        zoomBy(
          after / before,
          relativeToCentre((event.clientX + other.x) / 2, (event.clientY + other.y) / 2),
        );
      }
    } else if (drag.current) {
      if (Math.hypot(event.clientX - drag.current.x, event.clientY - drag.current.y) > 0)
        moved.current = true;
      const now = view.current;
      apply(
        clampView(
          {
            ...now,
            x: now.x + event.clientX - drag.current.x,
            y: now.y + event.clientY - drag.current.y,
          },
          stageSize(),
          natural.current,
        ),
      );
      drag.current = { x: event.clientX, y: event.clientY };
    }
    pointers.current.set(event.pointerId, { x: event.clientX, y: event.clientY });
  }

  function onPointerEnd(event: PointerEvent<HTMLDivElement>) {
    pointers.current.delete(event.pointerId);
    const remaining = [...pointers.current.values()][0];
    drag.current = remaining ?? null;
    if (!remaining) setPanning(false);
  }

  async function save() {
    if (!current || shown === undefined) return;
    const extension = extensionOf(shown);
    try {
      const path = await pickSavePath(
        suggestedName(current, extension),
        { name: t("viewer.fileFilter"), extensions: [extension] },
        t("viewer.save"),
      );
      if (path === null) return;
      await getApi().saveImage(path, shown);
      setStatus({ text: t("viewer.saved"), failed: false });
    } catch (error) {
      console.error("Saving the image failed", error);
      setStatus({ text: t("viewer.saveFailed"), failed: true });
    }
  }

  if (!current) return null;
  const button = (label: string, onClick: () => void, text: string) => (
    <button
      type="button"
      className="image-viewer__button"
      aria-label={label}
      data-tooltip={label}
      onClick={onClick}
    >
      {text}
    </button>
  );

  return createPortal(
    // The backdrop only offers a pointer shortcut for Escape, which every keyboard user has.
    // eslint-disable-next-line jsx-a11y/click-events-have-key-events, jsx-a11y/no-static-element-interactions
    <div
      className="image-viewer__backdrop"
      onClick={(event) => {
        if (moved.current) return;
        if (event.target === event.currentTarget || event.target === stageRef.current) onClose();
      }}
    >
      {/* Keys are handled for the whole dialog, wherever focus is inside it. */}
      {/* eslint-disable-next-line jsx-a11y/no-noninteractive-element-interactions */}
      <div
        ref={dialogRef}
        className="image-viewer"
        role="dialog"
        aria-modal="true"
        aria-label={t("viewer.label")}
        tabIndex={-1}
        onKeyDown={onKeyDown}
      >
        <div className="image-viewer__bar">
          <span className="image-viewer__counter" aria-live="polite">
            {images.length > 1
              ? t("viewer.counter", { current: index + 1, total: images.length })
              : ""}
          </span>
          <span className="image-viewer__zoom" aria-hidden="true">
            {percent} %
          </span>
          {button(
            t("viewer.zoomOut"),
            () => {
              zoomBy(1 / ZOOM_STEP);
            },
            "−",
          )}
          {button(
            t("viewer.zoomIn"),
            () => {
              zoomBy(ZOOM_STEP);
            },
            "+",
          )}
          {button(t("viewer.fit"), fit, "⤢")}
          {button(t("viewer.save"), () => void save(), "⬇")}
          {current.url !== undefined
            ? button(
                favorites.includes(current.url) ? t("favorites.remove") : t("favorites.save"),
                () => {
                  if (current.url === undefined) return;
                  if (favorites.includes(current.url)) removeFavorite(current.url);
                  else saveFavorite(current.url);
                },
                favorites.includes(current.url) ? "★" : "☆",
              )
            : null}
          {current.url !== undefined
            ? button(
                t("viewer.openInBrowser"),
                () => {
                  if (current.url !== undefined) openInBrowser(current.url);
                },
                "↗",
              )
            : null}
          {button(t("viewer.close"), onClose, "✕")}
        </div>
        {status ? (
          <p className="image-viewer__status" role={status.failed ? "alert" : "status"}>
            {status.text}
          </p>
        ) : null}
        <FullImageHint state={full} />
        {/* Pointer drag and the wheel have keyboard equivalents (+ − 0 and the buttons). */}
        {/* eslint-disable-next-line jsx-a11y/no-static-element-interactions */}
        <div
          ref={stageRef}
          className={`image-viewer__stage${panning ? " image-viewer__stage--panning" : ""}`}
          data-zoomed={zoomed}
          onPointerDown={onPointerDown}
          onPointerMove={onPointerMove}
          onPointerUp={onPointerEnd}
          onPointerCancel={onPointerEnd}
          onDoubleClick={(event) => {
            toggleActualSize(relativeToCentre(event.clientX, event.clientY));
          }}
        >
          <img
            ref={imgRef}
            className="image-viewer__img"
            src={shown}
            alt={current.name}
            draggable={false}
            onLoad={(event) => {
              natural.current = {
                width: event.currentTarget.naturalWidth,
                height: event.currentTarget.naturalHeight,
              };
              fit();
            }}
          />
        </div>
        {images.length > 1
          ? [
              <button
                key="prev"
                type="button"
                className="image-viewer__nav image-viewer__nav--prev"
                aria-label={t("viewer.previous")}
                disabled={index === 0}
                onClick={() => {
                  onIndexChange(index - 1);
                }}
              >
                ‹
              </button>,
              <button
                key="next"
                type="button"
                className="image-viewer__nav image-viewer__nav--next"
                aria-label={t("viewer.next")}
                disabled={index === images.length - 1}
                onClick={() => {
                  onIndexChange(index + 1);
                }}
              >
                ›
              </button>,
            ]
          : null}
      </div>
    </div>,
    document.body,
  );
}
