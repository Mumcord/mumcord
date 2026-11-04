/** Line icons of the share start dialog (mockup 01). Decorative: their lines carry the text. */
import type { ReactNode } from "react";

function Icon({ size, children }: { size: number; children: ReactNode }) {
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

/** The "Viewers get" box. */
export function ScreenIcon() {
  return (
    <Icon size={18}>
      <rect x="2" y="3" width="20" height="14" rx="2" />
      <path d="M8 21h8M12 17v4" />
    </Icon>
  );
}

/** Sent once through the relay, which passes it on. */
export function FanoutIcon() {
  return (
    <Icon size={14}>
      <circle cx="12" cy="5" r="2.5" />
      <circle cx="5" cy="19" r="2.5" />
      <circle cx="12" cy="19" r="2.5" />
      <circle cx="19" cy="19" r="2.5" />
      <path d="M12 7.5v9M10.5 7 6 16.8M13.5 7l4.5 9.8" />
    </Icon>
  );
}

/** One copy per viewer from the sharer's computer. */
export function CopiesIcon() {
  return (
    <Icon size={14}>
      <rect x="8" y="8" width="13" height="13" rx="2" />
      <path d="M16 8V5a2 2 0 0 0-2-2H5a2 2 0 0 0-2 2v9a2 2 0 0 0 2 2h3" />
    </Icon>
  );
}

/** The note under the codec choice. */
export function InfoIcon() {
  return (
    <Icon size={14}>
      <circle cx="12" cy="12" r="10" />
      <path d="M12 16v-4M12 8h.01" />
    </Icon>
  );
}
