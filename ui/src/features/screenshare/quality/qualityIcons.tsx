/** Icons of the live bar's Quality menu and notices, drawn like the other screen share icons. */
import type { ReactNode } from "react";

function Icon({ size = 14, children }: { size?: number; children: ReactNode }) {
  return (
    <svg viewBox="0 0 24 24" width={size} height={size} aria-hidden="true" focusable="false">
      <g
        fill="none"
        stroke="currentColor"
        strokeWidth="2"
        strokeLinecap="round"
        strokeLinejoin="round"
      >
        {children}
      </g>
    </svg>
  );
}

/** The "Quality" button. */
export function GaugeIcon() {
  return (
    <Icon>
      <path d="M12 14l4-4M3.3 17a10 10 0 1 1 17.4 0" />
    </Icon>
  );
}

/** The checked row of a menu. */
export function CheckIcon() {
  return (
    <Icon>
      <path d="M20 6 9 17l-5-5" />
    </Icon>
  );
}

/** A notice that needs the sharer's attention. */
export function WarningIcon() {
  return (
    <Icon size={18}>
      <path d="M10.3 3.9 1.8 18a2 2 0 0 0 1.7 3h17a2 2 0 0 0 1.7-3L13.7 3.9a2 2 0 0 0-3.4 0zM12 9v4M12 17h.01" />
    </Icon>
  );
}
