/**
 * The small line icons of the media views (issue #163). They are decorative: every button that
 * shows one carries its own accessible name, so the icons are hidden from screen readers.
 */
import type { ReactNode } from "react";

const PATHS = {
  play: "M5 3.5v9l7.5-4.5z",
  pause: "M4.5 3h2.5v10H4.5zM9 3h2.5v10H9z",
  previous: "M4 3v10M13 3.5v9L6 8z",
  next: "M12 3v10M3 3.5v9L10 8z",
  volume: "M2.5 6h2.5l3.5-3v10L5 10H2.5zM11 5.5a3.5 3.5 0 0 1 0 5M12.8 3.5a6.3 6.3 0 0 1 0 9",
  muted: "M2.5 6h2.5l3.5-3v10L5 10H2.5zM11 6l3.5 4M14.5 6L11 10",
  close: "M4 4l8 8M12 4l-8 8",
  more: "M3.5 8h.01M8 8h.01M12.5 8h.01",
  crown: "M2.5 11.5h11M3 10l-.5-6 3 2.5L8 3l2.5 3.5 3-2.5-.5 6z",
  link: "M7 9a3 3 0 0 0 4.2 0l2-2a3 3 0 0 0-4.2-4.2l-.8.8M9 7a3 3 0 0 0-4.2 0l-2 2a3 3 0 0 0 4.2 4.2l.8-.8",
  grip: "M6 3.5h.01M10 3.5h.01M6 8h.01M10 8h.01M6 12.5h.01M10 12.5h.01",
  mini: "M2 3.5h12v9H2zM8.5 8h4v3h-4z",
  open: "M9.5 2.5h4v4M13.5 2.5L8 8M7 3.5H3.5v9h9V9",
  leave: "M6.5 2.5h-3v11h3M10 5l3 3-3 3M13 8H6.5",
  popIn: "M13.5 2.5L8 8M8 4.5V8h3.5M7 3.5H3.5v9h9V9",
  pin: "M6 2.5h4M7 2.5v4L4.5 9h7L9 6.5v-4M8 9v4.5",
} as const;

export type MediaIconName = keyof typeof PATHS;

/** Filled shapes; the rest are drawn as lines. */
const FILLED: ReadonlySet<MediaIconName> = new Set(["play", "pause"]);
/** Dots need a thicker round stroke to be visible. */
const DOTS: ReadonlySet<MediaIconName> = new Set(["more", "grip"]);

export function MediaIcon({ name, size = 16 }: { name: MediaIconName; size?: number }): ReactNode {
  const filled = FILLED.has(name);
  return (
    <svg
      className="media-icon"
      viewBox="0 0 16 16"
      width={size}
      height={size}
      aria-hidden="true"
      focusable="false"
    >
      <path
        d={PATHS[name]}
        fill={filled ? "currentColor" : "none"}
        stroke="currentColor"
        strokeWidth={DOTS.has(name) ? 2.6 : filled ? 1 : 1.5}
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}
