/**
 * Follow mode of the packet list: it scrolls to new packets like a log viewer, pauses when the
 * user scrolls up and counts what arrives meanwhile. Separate from pausing the capture.
 */
import { useEffect, useLayoutEffect, useRef, useState, type UIEvent } from "react";
import type { Packet } from "./packets";

/** How far from the end still counts as "at the bottom" (fractional scroll positions). */
const BOTTOM_SLACK_PX = 24;

interface ScrollGeometry {
  scrollTop: number;
  clientHeight: number;
  scrollHeight: number;
}

export function isAtBottom(el: ScrollGeometry): boolean {
  return el.scrollHeight - el.scrollTop - el.clientHeight <= BOTTOM_SLACK_PX;
}

/** Scrolls to the end and returns the position the browser settled on. */
function scrollToEnd(el: HTMLElement): number {
  el.scrollTop = el.scrollHeight;
  return el.scrollTop;
}

/** Packets newer than the one the user saw last (`null`: none seen yet). */
export function countUnseen(packets: readonly Packet[], seenId: number | null): number {
  return seenId === null ? packets.length : packets.filter((p) => p.id > seenId).length;
}

export interface FollowScroll {
  /** Callback ref for the scrolling container. */
  attachScroller: (element: HTMLDivElement | null) => void;
  onScroll: (event: UIEvent<HTMLDivElement>) => void;
  follow: boolean;
  unseen: number;
  setFollow: (on: boolean) => void;
  /** Stops following without scrolling. */
  pause: () => void;
  /** Scrolls to the newest packet and follows from there. */
  jump: () => void;
}

/**
 * `packets` is the list as shown, oldest first. The scroll position is set directly, without
 * animation, which is cheap at high packet rates and needs no reduced-motion special case.
 */
export function useFollowScroll(packets: readonly Packet[]): FollowScroll {
  const [scroller, setScroller] = useState<HTMLDivElement | null>(null);
  const [follow, setFollowState] = useState(true);
  const [seenId, setSeenId] = useState<number | null>(null);
  // Where our own scrolling ended: the scroll event it causes arrives later, possibly after more
  // packets were added, and must not be mistaken for the user scrolling up.
  const ownScroll = useRef<number | null>(null);
  const newestId = packets[packets.length - 1]?.id ?? null;

  useLayoutEffect(() => {
    if (follow && scroller) ownScroll.current = scrollToEnd(scroller);
  }, [follow, newestId, scroller]);

  // A list that gets shorter (a smaller window) keeps its scroll position, which would leave the
  // newest packet out of view until the next one arrives.
  useEffect(() => {
    if (!follow || !scroller || typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(() => {
      ownScroll.current = scrollToEnd(scroller);
    });
    observer.observe(scroller);
    return () => {
      observer.disconnect();
    };
  }, [follow, scroller]);

  const setFollow = (on: boolean) => {
    if (!on) setSeenId(newestId);
    setFollowState(on);
  };

  const onScroll = (event: UIEvent<HTMLDivElement>) => {
    const el = event.currentTarget;
    if (follow && ownScroll.current !== null && Math.abs(el.scrollTop - ownScroll.current) < 1) {
      return;
    }
    const atBottom = isAtBottom(el);
    if (atBottom !== follow) setFollow(atBottom);
  };

  return {
    attachScroller: setScroller,
    onScroll,
    follow,
    unseen: follow ? 0 : countUnseen(packets, seenId),
    setFollow,
    pause: () => {
      if (follow) setFollow(false);
    },
    jump: () => {
      setFollow(true);
    },
  };
}
