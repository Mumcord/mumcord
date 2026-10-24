/**
 * Overlapping avatars of a channel's people on their own line under its row (QA.14): as many as
 * fit the line, up to five (QA.29), plus a "+N" chip.
 * Read as one image whose label names the shown people, so the treeitem's visible text stays a run
 * of its accessible name (the chip's digits and the initials are CSS-generated, WCAG 2.5.3).
 */
import { memo, useLayoutEffect, useRef, useState, type RefObject } from "react";
import { useTranslation } from "react-i18next";
import type { User } from "../../api";
import { Avatar } from "../members/Avatar";
import { stackFit } from "./tree";
import "./AvatarStack.css";

export interface StackPerson {
  user: User;
  talking: boolean;
  src: string | null;
}

export interface AvatarStackProps {
  /** The people to show, front first (see `stackPeople`); trimmed to what fits the line. */
  people: readonly StackPerson[];
  /** Everyone in the channel, shown or not. */
  total: number;
  /** Id of the stack image, so the channel row can include its label in its name. */
  id?: string;
}

type WidthListener = () => void;
const widthListeners = new Map<Element, WidthListener>();
let rowObserver: ResizeObserver | null = null;

/*
 * One observer for all stacks: a big server shows many at once. ResizeObserver reports at most
 * once per frame, and the count only changes (and re-renders) when a row crosses a threshold.
 */
function observeWidth(element: Element, listener: WidthListener): () => void {
  if (typeof ResizeObserver === "undefined") return () => undefined;
  rowObserver ??= new ResizeObserver((entries) => {
    for (const entry of entries) widthListeners.get(entry.target)?.();
  });
  widthListeners.set(element, listener);
  rowObserver.observe(element);
  return () => {
    widthListeners.delete(element);
    rowObserver?.unobserve(element);
  };
}

/** Content width of an element (without padding), in px. */
function contentWidth(element: Element): number {
  const style = getComputedStyle(element);
  return (
    element.clientWidth -
    (parseFloat(style.paddingLeft) || 0) -
    (parseFloat(style.paddingRight) || 0)
  );
}

/**
 * Width (rem) the stack may use on its line: the line's content box minus whatever else sits on
 * it (the "Show all" toggle) and the gaps.
 */
function availableRem(stack: HTMLElement, line: Element): number {
  const gap = parseFloat(getComputedStyle(line).columnGap) || 0;
  let used = 0;
  for (const child of line.children) {
    if (child !== stack) used += child.getBoundingClientRect().width + gap;
  }
  const remPx = parseFloat(getComputedStyle(document.documentElement).fontSize) || 16;
  return (contentWidth(line) - used) / remPx;
}

/**
 * How many of `total` avatars fit on the stack's line, or null until the line has a measurable
 * width (never where there is no layout, such as tests); then every given person is shown.
 */
function useLineFit(ref: RefObject<HTMLElement | null>, total: number): number | null {
  const [fit, setFit] = useState<number | null>(null);
  useLayoutEffect(() => {
    const stack = ref.current;
    const line = stack?.parentElement;
    if (!stack || !line) return;
    const apply = () => {
      if (!(contentWidth(line) > 0)) return;
      setFit(stackFit(availableRem(stack, line), total));
    };
    // Measured once right away so the first paint already shows the right count.
    apply();
    return observeWidth(line, apply);
  }, [ref, total]);
  return fit;
}

function AvatarStackView({ id, people: given, total }: AvatarStackProps) {
  const { t } = useTranslation("channels");
  const ref = useRef<HTMLSpanElement>(null);
  const fit = useLineFit(ref, total);
  const people = fit === null ? given : given.slice(0, fit);
  const rest = total - people.length;
  const names = people
    .map((p) => (p.talking ? t("tree.stack.talking", { name: p.user.name }) : p.user.name))
    .join(", ");
  const label =
    rest > 0
      ? t("tree.stack.some", { shown: people.length, total, names, rest })
      : t("tree.stack.all", { count: total, names });
  return (
    <span ref={ref} id={id} className="avatar-stack" role="img" aria-label={label}>
      {people.map((p) => (
        <span key={p.user.session} className="avatar-stack__item">
          <Avatar name={p.user.name} src={p.src} talking={p.talking} size="sm" />
        </span>
      ))}
      {rest > 0 && <span className="avatar-stack__more" data-more={`+${String(rest)}`} />}
    </span>
  );
}

const samePeople = (a: AvatarStackProps, b: AvatarStackProps) =>
  a.id === b.id &&
  a.total === b.total &&
  a.people.length === b.people.length &&
  a.people.every((p, i) => {
    const q = b.people[i];
    return q !== undefined && p.user === q.user && p.talking === q.talking && p.src === q.src;
  });

/** Re-renders only when the shown people, their talking state or images change. */
export const AvatarStack = memo(AvatarStackView, samePeople);
