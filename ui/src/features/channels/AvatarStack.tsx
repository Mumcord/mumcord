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
import { Icon, VoiceStateIcon } from "./PersonStatus";
import { VOICE_STATE_KEY, voiceState, type VoiceState } from "./statusText";
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

/**
 * Layout facts of one tree, shared by all its stacks. Measuring every stack's line on each change
 * forced a style and layout read per row (a third of a 600 ms freeze after connecting to a big
 * server on a slow CPU). Now the tree's width is observed once, and what a line cannot use (its
 * indent, the "Show all" toggle) is measured once per tree level and toggle label.
 */
interface TreeLayout {
  /** The tree's client width, in px. */
  width: number;
  remPx: number;
  /** Per tree level: px of the tree's width a stack line at that level cannot use. */
  insets: Map<string, number>;
  /** Per set of line siblings (by label): px they take beside the stack, gaps included. */
  siblings: Map<string, number>;
  listeners: Set<() => void>;
}

const layouts = new Map<Element, TreeLayout>();
let treeObserver: ResizeObserver | null = null;

const rootRemPx = () => parseFloat(getComputedStyle(document.documentElement).fontSize) || 16;

/* ResizeObserver reports at most once per frame; stacks only re-render when their count changes. */
function onTreeResize(entries: ResizeObserverEntry[]) {
  for (const entry of entries) {
    const layout = layouts.get(entry.target);
    if (!layout || entry.target.clientWidth === layout.width) continue;
    layout.width = entry.target.clientWidth;
    layout.remPx = rootRemPx();
    layout.insets.clear();
    layout.siblings.clear();
    for (const listener of layout.listeners) listener();
  }
}

/** The shared layout of `tree`, created (and observed) on first use. */
function layoutOf(tree: Element): TreeLayout {
  let layout = layouts.get(tree);
  if (!layout) {
    layout = {
      width: tree.clientWidth,
      remPx: rootRemPx(),
      insets: new Map(),
      siblings: new Map(),
      listeners: new Set(),
    };
    layouts.set(tree, layout);
    if (typeof ResizeObserver !== "undefined") {
      treeObserver ??= new ResizeObserver(onTreeResize);
      treeObserver.observe(tree);
    }
  }
  return layout;
}

/** Calls `listener` when the tree's width changes; the returned function unsubscribes. */
function listen(tree: Element, layout: TreeLayout, listener: () => void): () => void {
  layout.listeners.add(listener);
  return () => {
    layout.listeners.delete(listener);
    if (layout.listeners.size > 0) return;
    layouts.delete(tree);
    treeObserver?.unobserve(tree);
    if (layouts.size > 0) return;
    treeObserver?.disconnect();
    treeObserver = null;
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

/** Width (rem) the stack may use on its line: the tree's width minus indent and line siblings. */
function availableRem(layout: TreeLayout, stack: HTMLElement, line: Element): number {
  const level = line.closest("[aria-level]")?.getAttribute("aria-level") ?? "";
  let inset = layout.insets.get(level);
  if (inset === undefined) {
    inset = layout.width - contentWidth(line);
    layout.insets.set(level, inset);
  }
  const others = Array.from(line.children).filter((child) => child !== stack);
  const key = others
    .map((child) => child.getAttribute("data-label") ?? child.textContent)
    .join("\n");
  let used = layout.siblings.get(key);
  if (used === undefined) {
    const gap = others.length > 0 ? parseFloat(getComputedStyle(line).columnGap) || 0 : 0;
    used = others.reduce((sum, child) => sum + child.getBoundingClientRect().width + gap, 0);
    layout.siblings.set(key, used);
  }
  return (layout.width - inset - used) / layout.remPx;
}

/**
 * How many of `total` avatars fit on the stack's line, or null until the tree has a measurable
 * width (never where there is no layout, such as tests); then every given person is shown.
 */
function useLineFit(ref: RefObject<HTMLElement | null>, total: number): number | null {
  const [fit, setFit] = useState<number | null>(null);
  useLayoutEffect(() => {
    const stack = ref.current;
    const line = stack?.parentElement;
    if (!stack || !line) return;
    // Outside a tree (a lone line), the line itself is the measured box.
    const tree = line.closest("[role='tree']") ?? line;
    const layout = layoutOf(tree);
    const apply = () => {
      if (!(layout.width > 0)) return;
      setFit(stackFit(availableRem(layout, stack, line), total));
    };
    // Applied once right away so the first paint already shows the right count.
    apply();
    return listen(tree, layout, apply);
  }, [ref, total]);
  return fit;
}

type BadgeState = VoiceState | "localMuted";

/**
 * The one status badge on a stacked avatar, standing in for the row's status icons. What the
 * server imposes comes first, then the user's own local mute (it explains silence they caused),
 * then the person's self mute/deafen.
 */
function stackBadge(user: User): BadgeState | null {
  const voice = voiceState(user);
  if (voice === "serverDeafened" || voice === "serverMuted" || voice === "suppressed") return voice;
  if (user.localMute) return "localMuted";
  return voice;
}

function StatusBadge({ state }: { state: BadgeState }) {
  // Red for the server's states and the local mute, grey for the person's own choice.
  const tone = state === "deafened" || state === "muted" ? "self" : state;
  return (
    <span className="avatar-stack__badge" data-state={tone}>
      {state === "localMuted" ? <Icon name="volume-x" /> : <VoiceStateIcon state={state} />}
    </span>
  );
}

function AvatarStackView({ id, people: given, total }: AvatarStackProps) {
  const { t } = useTranslation("channels");
  const ref = useRef<HTMLSpanElement>(null);
  const fit = useLineFit(ref, total);
  const people = fit === null ? given : given.slice(0, fit);
  const rest = total - people.length;
  const names = people
    .map(({ user, talking }) => {
      const parts: string[] = [];
      if (talking) parts.push(t("status.talking"));
      const voice = voiceState(user);
      if (voice) parts.push(t(VOICE_STATE_KEY[voice]));
      if (user.localMute) parts.push(t("status.localMuted"));
      return parts.length > 0
        ? t("tree.stack.person", { name: user.name, status: parts.join(", ") })
        : user.name;
    })
    .join(", ");
  const label =
    rest > 0
      ? t("tree.stack.some", { shown: people.length, total, names, rest })
      : t("tree.stack.all", { count: total, names });
  return (
    <span ref={ref} id={id} className="avatar-stack" role="img" aria-label={label}>
      {people.map((p) => {
        const badge = stackBadge(p.user);
        return (
          <span key={p.user.session} className="avatar-stack__item">
            <Avatar name={p.user.name} src={p.src} talking={p.talking} size="sm" />
            {badge && <StatusBadge state={badge} />}
          </span>
        );
      })}
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
