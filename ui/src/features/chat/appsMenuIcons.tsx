/** Line icons of the built-in Apps menu items (mockup 06). Decorative: the label names the item. */
import type { ReactNode } from "react";

const SHAPES = {
  screen: (
    <>
      <rect x="2" y="3" width="20" height="14" rx="2" />
      <path d="M8 21h8M12 17v4" />
    </>
  ),
  watch: <polygon points="6 3 20 12 6 21 6 3" />,
} as const;

export function AppsMenuIcon({ name }: { name: keyof typeof SHAPES }): ReactNode {
  return (
    <span className="chat-apps__builtin">
      <svg
        viewBox="0 0 24 24"
        width="16"
        height="16"
        fill="none"
        stroke="currentColor"
        strokeWidth="2"
        strokeLinecap="round"
        strokeLinejoin="round"
        aria-hidden="true"
        focusable="false"
      >
        {SHAPES[name]}
      </svg>
    </span>
  );
}
