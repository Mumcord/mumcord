import { useEffect, useId, useLayoutEffect, useRef, useState, type KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";
import { announce } from "../../a11y";
import { useIsMacos } from "../../api/useIsMacos";
import { Aaron } from "./Aaron";
import { placeGuide, type Box, type Placement } from "./placement";
import {
  closeTour,
  nextTip,
  previousTip,
  startTour,
  TIPS,
  useTour,
  type TipId,
  type Tour,
} from "./tour";
import "./tips.css";

/** How long Aaron talks when a new step appears. */
const TALK_MS = 1800;
/** Room between the target and the spotlight's ring, so the ring does not cut into it. */
const SPOT_PADDING = 4;
/** Room between the spotlight's ring and the window edges. */
const SPOT_INSET = 2;

function samePlacement(a: Placement, b: Placement): boolean {
  return (
    a.left === b.left &&
    a.top === b.top &&
    a.pose === b.pose &&
    a.mascot === b.mascot &&
    a.align === b.align
  );
}

function sameBox(a: Box | null, b: Box | null): boolean {
  if (a === null || b === null) return a === b;
  return a.left === b.left && a.top === b.top && a.width === b.width && a.height === b.height;
}

/**
 * The spotlight around a target: a little larger than the target, but kept inside the window so
 * its ring stays whole around columns that touch the window edges.
 */
function spotlight(target: Box, viewport: { width: number; height: number }): Box {
  const left = Math.max(SPOT_INSET, target.left - SPOT_PADDING);
  const top = Math.max(SPOT_INSET, target.top - SPOT_PADDING);
  const right = Math.min(viewport.width - SPOT_INSET, target.left + target.width + SPOT_PADDING);
  const bottom = Math.min(viewport.height - SPOT_INSET, target.top + target.height + SPOT_PADDING);
  return { left, top, width: right - left, height: bottom - top };
}

/** The first visible element marked for the tip; several (e.g. two message bars) may exist. */
function findTarget(id: string): Element | null {
  for (const element of document.querySelectorAll(`[data-tour="${id}"]`)) {
    const box = element.getBoundingClientRect();
    if (box.width > 0 && box.height > 0) return element;
  }
  return null;
}

/**
 * Aaron with a speech bubble: the first-start offer, then one tip at a time (#637). For each tip
 * he floats next to the element marked `data-tour="<tip>"` and points at it; without one (the
 * offer, the last tip, or nothing connected yet) he waits in the lower right corner. During the
 * tour the rest of the window is dimmed and the target sits in a ringed spotlight. Not modal:
 * the dim layer lets clicks through and the app stays usable while it is open.
 */
export function TipsHost() {
  const { t } = useTranslation(["tips", "channels"]);
  const tour = useTour();
  const macos = useIsMacos();
  const titleId = useId();
  const bodyId = useId();
  const primary = useRef<HTMLButtonElement>(null);
  const guide = useRef<HTMLDivElement>(null);
  const mascot = useRef<HTMLDivElement>(null);
  const [placement, setPlacement] = useState<Placement | null>(null);
  // The target's box, or null when the tip has none; only used during the tour.
  const [spot, setSpot] = useState<Box | null>(null);
  // The step after which Aaron fell silent. Every step is a new tour object, so he talks again
  // when the same tip comes back.
  const [quiet, setQuiet] = useState<Tour | null>(null);

  const tip: TipId | null = tour.kind === "tip" ? (TIPS[tour.index] ?? "end") : null;
  const target = tip === "end" ? null : tip;

  // Floats next to the target, measured again whenever the window or anything in it scrolls or
  // changes size. Runs before paint, so the guide never flashes at its old place.
  useLayoutEffect(() => {
    if (tour.kind === "hidden") return;
    const measure = () => {
      const box = guide.current;
      const face = mascot.current;
      if (!box || !face) return;
      const element = target === null ? null : findTarget(target);
      const targetBox: Box | null = element ? element.getBoundingClientRect() : null;
      const viewport = { width: window.innerWidth, height: window.innerHeight };
      const next = placeGuide(
        targetBox,
        {
          width: box.offsetWidth,
          height: box.offsetHeight,
          mascotWidth: face.offsetWidth,
          mascotHeight: face.offsetHeight,
        },
        viewport,
      );
      setPlacement((previous) => (previous && samePlacement(previous, next) ? previous : next));
      // A target that is off-screen or has no size gets no spotlight, like Aaron ignores it.
      const nextSpot = targetBox && next.pose !== "idle" ? spotlight(targetBox, viewport) : null;
      setSpot((previous) => (sameBox(previous, nextSpot) ? previous : nextSpot));
    };
    measure();
    // Also catches the bubble's text wrapping differently and the target growing or moving
    // within the layout, which no window event reports.
    const observer = typeof ResizeObserver === "function" ? new ResizeObserver(measure) : null;
    if (guide.current) observer?.observe(guide.current);
    if (target !== null) {
      const element = findTarget(target);
      if (element) observer?.observe(element);
    }
    window.addEventListener("resize", measure);
    window.addEventListener("scroll", measure, true);
    return () => {
      observer?.disconnect();
      window.removeEventListener("resize", measure);
      window.removeEventListener("scroll", measure, true);
    };
  }, [tour, target]);

  // He talks for a moment whenever a new step appears.
  useEffect(() => {
    if (tour.kind === "hidden") return;
    const timer = window.setTimeout(() => {
      setQuiet(tour);
    }, TALK_MS);
    return () => {
      window.clearTimeout(timer);
    };
  }, [tour]);

  // The offer pops up on its own after the wizard, so screen readers hear it too.
  useEffect(() => {
    if (tour.kind === "offer") announce(`${t("greeting.title")} ${t("greeting.body")}`);
  }, [tour.kind, t]);

  // Keyboard users land on the bubble's main button for every new step.
  useEffect(() => {
    if (tour.kind !== "hidden") primary.current?.focus();
  }, [tour]);

  if (tour.kind === "hidden") return null;

  // Every control of the bubble closes it on Escape; focus is always on one of them.
  const onKeyDown = (event: KeyboardEvent) => {
    if (event.key !== "Escape") return;
    event.stopPropagation();
    closeTour();
  };

  let title: string;
  let body: string;
  let progress: string | null = null;
  let actions;
  if (tour.kind === "offer") {
    title = t("greeting.title");
    body = t("greeting.body");
    actions = (
      <>
        <button type="button" className="button" onKeyDown={onKeyDown} onClick={closeTour}>
          {t("greeting.later")}
        </button>
        <button
          ref={primary}
          type="button"
          className="button button--primary"
          onKeyDown={onKeyDown}
          onClick={startTour}
        >
          {t("greeting.start")}
        </button>
      </>
    );
  } else {
    const id: TipId = tip ?? "end";
    const last = tour.index === TIPS.length - 1;
    const searchKey = macos ? "⌘K" : t("channels:search.shortcut");
    title = t(`tips.${id}.title`);
    body = t(`tips.${id}.body`, { key: searchKey });
    progress = t("progress", { current: tour.index + 1, total: TIPS.length });
    actions = (
      <>
        <button type="button" className="button" onKeyDown={onKeyDown} onClick={closeTour}>
          {t("skip")}
        </button>
        {tour.index > 0 && (
          <button type="button" className="button" onKeyDown={onKeyDown} onClick={previousTip}>
            {t("back")}
          </button>
        )}
        <button
          ref={primary}
          type="button"
          className="button button--primary"
          onKeyDown={onKeyDown}
          onClick={nextTip}
        >
          {last ? t("done") : t("next")}
        </button>
      </>
    );
  }

  const classes = ["tips"];
  if (placement?.mascot === "left") classes.push("tips--mascot-left");
  if (placement?.align === "start") classes.push("tips--top");
  const style = placement
    ? { left: `${String(placement.left)}px`, top: `${String(placement.top)}px` }
    : undefined;
  const spotStyle = spot
    ? {
        left: `${String(spot.left)}px`,
        top: `${String(spot.top)}px`,
        width: `${String(spot.width)}px`,
        height: `${String(spot.height)}px`,
      }
    : undefined;

  return (
    <>
      {tour.kind === "tip" && (
        // Pure decoration: hidden from screen readers, never focusable, lets clicks through.
        <div
          className={spot ? "tips-dim tips-dim--spot" : "tips-dim"}
          style={spotStyle}
          aria-hidden="true"
          data-testid="tips-dim"
        />
      )}
      <div
        ref={guide}
        className={classes.join(" ")}
        style={style}
        role="dialog"
        aria-label={t("label")}
        aria-describedby={`${titleId} ${bodyId}`}
      >
        <div className="tips__bubble">
          <button
            type="button"
            className="tips__close"
            aria-label={t("close")}
            onKeyDown={onKeyDown}
            onClick={closeTour}
          >
            <span aria-hidden="true">×</span>
          </button>
          {progress !== null && <p className="tips__progress">{progress}</p>}
          <h2 id={titleId} className="tips__title">
            {title}
          </h2>
          <p id={bodyId} className="tips__body">
            {body}
          </p>
          <div className="tips__actions">{actions}</div>
        </div>
        <div ref={mascot} className="tips__mascot">
          <Aaron pose={placement?.pose ?? "idle"} talking={quiet !== tour} />
        </div>
      </div>
    </>
  );
}
