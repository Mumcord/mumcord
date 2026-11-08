import { useEffect, useLayoutEffect, useRef, useState, type RefObject } from "react";
import { useTranslation } from "react-i18next";
import { Aaron, type AaronPose } from "../tips/Aaron";
import type { WizardStepId } from "./wizardFlow";
import { GUIDE_TARGETS, guideSpot, poseFor, type Box } from "./wizardGuideLayout";

/** Aaron's rendered size; keep in sync with `.wizard-guide__aaron` in wizard.css. */
const AARON_SIZE = 160;
/** His pointing hand sits this far below the top of his figure. */
const ARM_Y = AARON_SIZE * 0.72;
/** How long he talks when a new step appears. */
const TALK_MS = 1600;
/** Space between the target and Aaron when he steps out of his column towards it. */
const GAP = 16;
/** Marks the field Aaron points at, so wizard.css can make it glow. */
const TARGET_ATTRIBUTE = "data-guide-target";

function prefersReducedMotion(): boolean {
  return (
    typeof window.matchMedia === "function" &&
    window.matchMedia("(prefers-reduced-motion: reduce)").matches
  );
}

interface Placement {
  pose: AaronPose;
  offset: number;
  shift: number;
}

/**
 * Everything of the step next to the target between `top` and `bottom`: the siblings of the
 * target and of each of its ancestors up to the step. Aaron must not stand over any of them.
 */
function besideTarget(target: Element, root: Element, top: number, bottom: number): Box[] {
  const boxes: Box[] = [];
  for (let element = target; element !== root;) {
    const parent = element.parentElement;
    if (parent === null) break;
    for (const sibling of parent.children) {
      if (sibling === element) continue;
      const box = sibling.getBoundingClientRect();
      if (box.width > 0 && box.height > 0 && box.bottom > top && box.top < bottom) {
        boxes.push(box);
      }
    }
    element = parent;
  }
  return boxes;
}

/**
 * Aaron next to the wizard step (#637): he stands level with the field the step is about, as
 * close to it as the step's other content allows, points at it and makes it glow. Measures the field on step changes, resizes, scrolling and
 * content changes; layout-only, so nothing here re-renders the step.
 */
function useGuidePlacement(
  step: WizardStepId,
  stepRoot: RefObject<HTMLElement | null>,
  column: RefObject<HTMLElement | null>,
  body: RefObject<HTMLElement | null>,
  active: boolean,
): Placement {
  const [placement, setPlacement] = useState<Placement>({ pose: "idle", offset: 0, shift: 0 });

  useLayoutEffect(() => {
    const root = stepRoot.current;
    if (!active || root === null) return;
    const selector = GUIDE_TARGETS[step];
    let marked: Element | null = null;
    const measure = () => {
      const columnElement = column.current;
      if (columnElement === null) return;
      const target = selector === null ? null : root.querySelector(selector);
      const targetBox = target?.getBoundingClientRect() ?? null;
      // Aaron moves within the column's padding box, so his offsets start below its padding.
      const outer = columnElement.getBoundingClientRect();
      const padding = window.getComputedStyle(columnElement);
      const columnBox: Box = {
        left: outer.left,
        right: outer.right,
        top: outer.top + (parseFloat(padding.paddingTop) || 0),
        bottom: outer.bottom - (parseFloat(padding.paddingBottom) || 0),
      };
      const height = body.current?.offsetHeight ?? AARON_SIZE;
      const { offset, shift } = guideSpot(
        targetBox,
        columnBox,
        ARM_Y,
        height,
        GAP,
        (top, bottom) => (target ? besideTarget(target, root, top, bottom) : []),
      );
      const aaronTop = columnBox.top + offset;
      const pose = poseFor(targetBox, {
        left: columnBox.left + shift,
        right: columnBox.right + shift,
        top: aaronTop,
        bottom: aaronTop + AARON_SIZE,
      });
      setPlacement((current) =>
        current.pose === pose && current.offset === offset && current.shift === shift
          ? current
          : { pose, offset, shift },
      );
      if (marked !== target) {
        marked?.removeAttribute(TARGET_ATTRIBUTE);
        target?.setAttribute(TARGET_ATTRIBUTE, "");
        marked = target;
      }
    };
    measure();
    window.addEventListener("resize", measure);
    // Steps scroll inside their own body, and some fill in later (devices, the import list).
    root.addEventListener("scroll", measure, true);
    const mutations = new MutationObserver(measure);
    mutations.observe(root, { childList: true, subtree: true });
    const resizes = typeof ResizeObserver === "function" ? new ResizeObserver(measure) : null;
    resizes?.observe(root);
    return () => {
      window.removeEventListener("resize", measure);
      root.removeEventListener("scroll", measure, true);
      mutations.disconnect();
      resizes?.disconnect();
      marked?.removeAttribute(TARGET_ATTRIBUTE);
    };
  }, [step, stepRoot, column, body, active]);

  return placement;
}

/** He talks for a moment whenever the step changes; with reduced motion he stays still. */
function useTalkOnStep(step: WizardStepId): boolean {
  const [still] = useState(prefersReducedMotion);
  const [quietStep, setQuietStep] = useState<WizardStepId | null>(null);
  useEffect(() => {
    const timer = window.setTimeout(() => {
      setQuietStep(step);
    }, TALK_MS);
    return () => {
      window.clearTimeout(timer);
    };
  }, [step]);
  return !still && quietStep !== step;
}

interface WizardGuideProps {
  step: WizardStepId;
  /** The element that holds the current step; Aaron's target is searched inside it. */
  stepRoot: RefObject<HTMLElement | null>;
}

export function WizardGuide({ step, stepRoot }: WizardGuideProps) {
  const { t } = useTranslation("wizard");
  const [hidden, setHidden] = useState(false);
  const toggled = useRef(false);
  const column = useRef<HTMLElement>(null);
  const body = useRef<HTMLDivElement>(null);
  const toggle = useRef<HTMLButtonElement>(null);
  const { pose, offset, shift } = useGuidePlacement(step, stepRoot, column, body, !hidden);
  const talking = useTalkOnStep(step);

  // The button that replaces the pressed one takes the focus, so keyboard users stay in place.
  useEffect(() => {
    if (toggled.current) toggle.current?.focus();
  }, [hidden]);

  function setVisible(visible: boolean) {
    toggled.current = true;
    setHidden(!visible);
  }

  if (hidden) {
    return (
      <aside
        ref={column}
        className="wizard-guide wizard-guide--hidden"
        aria-label={t("guide.label")}
      >
        <button
          ref={toggle}
          type="button"
          className="wizard-link wizard-small"
          onClick={() => {
            setVisible(true);
          }}
        >
          {t("guide.show")}
        </button>
      </aside>
    );
  }

  return (
    <aside ref={column} className="wizard-guide" aria-label={t("guide.label")}>
      <div
        ref={body}
        className="wizard-guide__body"
        style={{ transform: `translate(${String(shift)}px, ${String(offset)}px)` }}
      >
        <Aaron className="wizard-guide__aaron" pose={pose} talking={talking} />
        <div className="wizard-guide__bubble">
          <div className="wizard-guide__text">
            <p className="wizard-guide__title">{t(`guide.titles.${step}`)}</p>
            <p className="wizard-guide__hint">{t(`guide.hints.${step}`)}</p>
          </div>
          <div className="wizard-guide__footer">
            <span className="wizard-muted wizard-small" aria-hidden="true">
              {t("guide.from")}
            </span>
            <button
              ref={toggle}
              type="button"
              className="button wizard-guide__hide"
              onClick={() => {
                setVisible(false);
              }}
            >
              {t("guide.hide")}
            </button>
          </div>
        </div>
      </div>
    </aside>
  );
}
