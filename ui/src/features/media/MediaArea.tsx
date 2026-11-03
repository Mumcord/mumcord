/**
 * The resizable media area (spec 07 §4.9, issue #386): wraps the big view of the screen share, the
 * watch party and later the camera room, and puts the horizontal handle between it and the chat.
 * Without a chosen height the view looks exactly as before ("Auto"). The handle only exists while
 * the big view is in the main window: not in full screen, not for a popped-out view, and not when
 * the chat column is too short to share (FR-7.61, FR-7.71, FR-7.72).
 */
import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import { announce } from "../../a11y";
import {
  heightForDrag,
  heightForKey,
  mediaLimits,
  percentOf,
  shownHeight,
  type MediaLimits,
  type MediaRoom,
} from "../../app/mediaHeight";
import { ResizeHandle } from "../../app/ResizeHandle";
import { leaveCinema } from "./mediaView";
import { setMediaDrag, setMediaHeight, useMediaResize } from "./mediaResize";

const MEDIA_AREA_ID = "media-area";

/** Snap, reset and collapse take this long when the system allows motion (FR-7.73). */
const ANIMATION_MS = 120;
const MOTION_QUERY = "(prefers-reduced-motion: no-preference)";

function motionAllowed(): boolean {
  return typeof window.matchMedia === "function" && window.matchMedia(MOTION_QUERY).matches;
}

function rootFontPx(): number {
  return parseFloat(getComputedStyle(document.documentElement).fontSize) || 16;
}

interface Measured extends MediaRoom {
  /** Height of the picture as it is shown now. */
  picture: number;
  /** Height the picture has in Auto at the current width (16:9, at most 58 % of the window). */
  auto: number;
}

/**
 * Measures the chat column around the area. Null while there is nothing to resize: no frame (the
 * view is popped out), full screen, or no layout at all (hidden window, test DOM).
 */
function measure(area: HTMLElement, handle: HTMLElement | null): Measured | null {
  const column = area.parentElement;
  const picture = area.querySelector<HTMLElement>(".media-frame__picture");
  if (column === null || picture === null || document.fullscreenElement !== null) return null;
  if (column.clientHeight === 0) return null;
  const remPx = rootFontPx();
  const top = column.getBoundingClientRect().top;
  const pictureHeight = picture.getBoundingClientRect().height;
  // Without the handle (it is hidden) the area's own bottom is where the chat starts.
  const bottom = (handle ?? area).getBoundingClientRect().bottom;
  const holder = picture.closest(".media-frame")?.parentElement ?? area;
  const style = getComputedStyle(holder);
  const width =
    holder.clientWidth -
    (parseFloat(style.paddingLeft) || 0) -
    (parseFloat(style.paddingRight) || 0);
  return {
    column: column.clientHeight,
    // A hidden handle still needs its 0.75 rem once it shows.
    overhead: bottom - top - pictureHeight + (handle === null ? 0.75 * remPx : 0),
    remPx,
    picture: pictureHeight,
    auto: (Math.min(width, (window.innerHeight * 0.58 * 16) / 9) * 9) / 16,
  };
}

const sameRoom = (a: Measured | null, b: Measured | null) =>
  a === b ||
  (a !== null &&
    b !== null &&
    Math.abs(a.column - b.column) < 0.5 &&
    Math.abs(a.overhead - b.overhead) < 0.5 &&
    Math.abs(a.picture - b.picture) < 0.5 &&
    Math.abs(a.auto - b.auto) < 0.5 &&
    a.remPx === b.remPx);

export function MediaArea({ children }: { children: ReactNode }) {
  const { t } = useTranslation("common", { keyPrefix: "media.resize" });
  const areaRef = useRef<HTMLDivElement>(null);
  const handleRef = useRef<HTMLDivElement>(null);
  const [room, setRoom] = useState<Measured | null>(null);
  const [frame, setFrame] = useState<Element | null>(null);
  const resize = useMediaResize();
  /** Height the drag currently asks for, before limits; drives the collapse read-out. */
  const [raw, setRaw] = useState<number | null>(null);
  /** A short animated phase (reset to Auto or minimising) showing this height in pixels. */
  const [phase, setPhase] = useState<{ px: number; to: "auto" | "collapse" } | null>(null);
  const dragFrom = useRef(resize.chosen);
  const speak = useRef(false);

  useLayoutEffect(() => {
    const area = areaRef.current;
    if (area === null) return;
    let pending = 0;
    const update = () => {
      setFrame(area.querySelector(".media-frame"));
      const next = measure(area, handleRef.current);
      setRoom((prev) => (sameRoom(prev, next) ? prev : next));
    };
    const schedule = () => {
      if (pending) return;
      pending = requestAnimationFrame(() => {
        pending = 0;
        update();
      });
    };
    update();
    // The chat column (window size), the area (bars and views come and go) and the handle (its
    // height is in rem, so it follows the text size) each change the room (FR-7.71).
    const observer = typeof ResizeObserver === "function" ? new ResizeObserver(schedule) : null;
    if (area.parentElement) observer?.observe(area.parentElement);
    observer?.observe(area);
    window.addEventListener("resize", schedule);
    document.addEventListener("fullscreenchange", schedule);
    return () => {
      observer?.disconnect();
      window.removeEventListener("resize", schedule);
      document.removeEventListener("fullscreenchange", schedule);
      cancelAnimationFrame(pending);
    };
  }, []);

  // Re-observes the handle whenever it appears.
  const handleShown = room !== null && mediaLimits(room) !== null;
  useEffect(() => {
    const handle = handleRef.current;
    if (handle === null || typeof ResizeObserver !== "function") return;
    const observer = new ResizeObserver(() => {
      const area = areaRef.current;
      if (area === null) return;
      const next = measure(area, handle);
      setRoom((prev) => (sameRoom(prev, next) ? prev : next));
    });
    observer.observe(handle);
    return () => {
      observer.disconnect();
    };
  }, [handleShown]);

  const limits: MediaLimits | null = room === null ? null : mediaLimits(room);
  const sized =
    room !== null && limits !== null && resize.chosen !== "auto"
      ? shownHeight(resize.chosen, room, limits)
      : null;
  const shownPx = phase?.px ?? sized;
  const currentPx = sized ?? room?.picture ?? 0;
  const percent = room === null ? 0 : percentOf(currentPx, room);
  const valueText = t("valueText", { percent });

  // Value changes from the keyboard or a finished drag are spoken (FR-7.68).
  useEffect(() => {
    if (!speak.current || room === null) return;
    speak.current = false;
    announce(valueText);
  }, [valueText, room]);

  // Ends an animated phase after its 120 ms.
  useEffect(() => {
    if (phase === null) return;
    const timer = window.setTimeout(() => {
      setPhase(null);
      if (phase.to === "collapse") leaveCinema();
    }, ANIMATION_MS);
    return () => {
      window.clearTimeout(timer);
    };
  }, [phase]);

  const minimise = () => {
    // The height chosen before this drag stays, so "Open" comes back to it (FR-7.69).
    setMediaHeight(dragFrom.current);
    announce(t("minimised"));
    if (motionAllowed()) {
      setMediaDrag(false);
      setPhase({ px: 0, to: "collapse" });
      return;
    }
    setMediaDrag(false);
    leaveCinema();
  };

  const reset = () => {
    speak.current = true;
    if (motionAllowed() && room !== null && sized !== null) {
      setPhase({ px: room.auto, to: "auto" });
    }
    setMediaHeight("auto", { save: true, animate: true });
  };

  const handle =
    room !== null && limits !== null ? (
      <ResizeHandle
        orientation="horizontal"
        name="media"
        label={t("label")}
        controls={MEDIA_AREA_ID}
        aria={{
          now: percent,
          min: percentOf(limits.min, room),
          max: percentOf(limits.max, room),
          text: valueText,
        }}
        start={currentPx}
        onDragStart={() => {
          dragFrom.current = resize.chosen;
          setMediaDrag(true);
        }}
        onDrag={(start, deltaPx, altKey, done) => {
          const result = heightForDrag(start + deltaPx, room, limits, altKey);
          if (!done) {
            setRaw(start + deltaPx);
            setMediaHeight(result.px / room.column, { animate: result.snapped });
            setMediaDrag(true, result.collapse);
            return;
          }
          setRaw(null);
          if (result.collapse) {
            minimise();
            return;
          }
          setMediaDrag(false);
          // A plain click (or a double-click's first half) leaves the height as it was.
          if (deltaPx === 0) {
            setMediaHeight(dragFrom.current);
            return;
          }
          speak.current = true;
          setMediaHeight(result.px / room.column, { save: true, animate: result.snapped });
        }}
        onKey={(key) => {
          const next = heightForKey(key, currentPx, room, limits);
          if (next === null) return false;
          if (next === "auto") {
            reset();
            return true;
          }
          speak.current = true;
          setMediaHeight(next / room.column, {
            save: true,
            animate: key !== "ArrowUp" && key !== "ArrowDown",
          });
          return true;
        }}
        onReset={reset}
      >
        <Readout
          percent={resize.collapsing && raw !== null ? percentOf(Math.max(raw, 0), room) : percent}
          note={
            !resize.dragging
              ? t("keys")
              : resize.collapsing
                ? t("release")
                : currentPx >= limits.max - 0.5
                  ? t("chatMinimum")
                  : t("ofWindow")
          }
          accent={resize.dragging}
        />
      </ResizeHandle>
    ) : null;

  return (
    <>
      <div
        ref={areaRef}
        id={MEDIA_AREA_ID}
        className="media-area"
        data-sized={shownPx !== null || undefined}
        data-animate={phase !== null || resize.animate || undefined}
        data-auto-width={
          (phase !== null && (phase.to === "auto" || resize.chosen === "auto")) || undefined
        }
        style={
          shownPx === null ? undefined : { ["--media-height" as string]: `${String(shownPx)}px` }
        }
      >
        {children}
      </div>
      {handle !== null && (
        <div
          ref={handleRef}
          className="media-area__handle"
          data-dragging={resize.dragging || undefined}
        >
          {handle}
        </div>
      )}
      {resize.collapsing &&
        frame !== null &&
        createPortal(
          <div className="media-area__collapse">
            <span>{t("releaseHint")}</span>
          </div>,
          frame,
        )}
    </>
  );
}

function Readout({ percent, note, accent }: { percent: number; note: string; accent: boolean }) {
  return (
    <span className="media-area__readout" data-accent={accent || undefined} aria-hidden="true">
      <strong>{percent}%</strong>
      <span>{note}</span>
    </span>
  );
}
