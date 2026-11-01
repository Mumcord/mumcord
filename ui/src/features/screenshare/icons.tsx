/**
 * Small line icons of the screen share views (issue #163) that the shared media icons do not
 * have. Decorative: the chip or button carrying one has its own text or label.
 */
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

/** Resolution chip. */
export function MonitorIcon() {
  return (
    <Icon>
      <rect x="3" y="4" width="18" height="12" rx="2" />
      <path d="M8 20h8M12 16v4" />
    </Icon>
  );
}

/** Bitrate chip. */
export function SignalIcon() {
  return (
    <Icon>
      <path d="M5 20v-3M10 20v-7M15 20v-11M20 20V5" />
    </Icon>
  );
}

/** A direct connection. */
export function BoltIcon() {
  return (
    <Icon>
      <path d="M13 3 5 14h6l-1 7 8-11h-6z" />
    </Icon>
  );
}

/** A connection through the relay. */
export function RelayIcon() {
  return (
    <Icon>
      <circle cx="5" cy="17" r="2" />
      <circle cx="12" cy="8" r="2" />
      <circle cx="19" cy="17" r="2" />
      <path d="m6.5 15.5 4-5.5M13.5 10l4 5.5" />
    </Icon>
  );
}

/** "Change window". */
export function WindowIcon() {
  return (
    <Icon>
      <rect x="3" y="4" width="18" height="16" rx="2" />
      <path d="M3 9h18" />
    </Icon>
  );
}

/** The keys hint. */
export function KeyboardIcon() {
  return (
    <Icon>
      <rect x="2" y="6" width="20" height="12" rx="2" />
      <path d="M6 10h.01M10 10h.01M14 10h.01M18 10h.01M8 14h8" />
    </Icon>
  );
}

/** Stream details. */
export function InfoCircleIcon() {
  return (
    <Icon>
      <circle cx="12" cy="12" r="9" />
      <path d="M12 11v5M12 8h.01" />
    </Icon>
  );
}

/** Opens the viewers popover. */
export function ChevronDownIcon() {
  return (
    <Icon size={12}>
      <path d="m6 9 6 6 6-6" />
    </Icon>
  );
}
