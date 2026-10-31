/**
 * Small line icons of the audio player and music cards (issues #260, #262). Decorative: every
 * button carrying one has its own label.
 */
import type { ReactNode } from "react";

function Icon({ size = 16, children }: { size?: number | undefined; children: ReactNode }) {
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

export function PlayIcon({ size }: { size?: number | undefined }) {
  return (
    <Icon size={size}>
      <path d="M7 4.5v15l12-7.5z" fill="currentColor" />
    </Icon>
  );
}

export function PauseIcon({ size }: { size?: number | undefined }) {
  return (
    <Icon size={size}>
      <path d="M8 5v14M16 5v14" strokeWidth="3" />
    </Icon>
  );
}

export function StopIcon({ size }: { size?: number | undefined }) {
  return (
    <Icon size={size}>
      <rect x="6" y="6" width="12" height="12" rx="1.5" fill="currentColor" />
    </Icon>
  );
}

export function ExternalIcon({ size }: { size?: number | undefined }) {
  return (
    <Icon size={size}>
      <path d="M14 4h6v6M20 4l-9 9M18 14v5a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1h5" />
    </Icon>
  );
}

export function DownloadIcon({ size }: { size?: number | undefined }) {
  return (
    <Icon size={size}>
      <path d="M12 4v11M7 10l5 5 5-5M5 20h14" />
    </Icon>
  );
}

export function VolumeIcon({ muted, size }: { muted: boolean; size?: number | undefined }) {
  return (
    <Icon size={size}>
      <path d="M4 9.5h3.5L12 5.5v13l-4.5-4H4z" fill="currentColor" />
      {muted ? (
        <path d="M16 9.5l5 5M21 9.5l-5 5" />
      ) : (
        <path d="M15.5 9a4 4 0 0 1 0 6M18.5 6.5a7.5 7.5 0 0 1 0 11" />
      )}
    </Icon>
  );
}

export function NoteIcon({ size }: { size?: number | undefined }) {
  return (
    <Icon size={size}>
      <path d="M9 18V6l11-2v12" />
      <circle cx="6.5" cy="18" r="2.5" />
      <circle cx="17.5" cy="16" r="2.5" />
    </Icon>
  );
}

export function GlobeIcon({ size }: { size?: number | undefined }) {
  return (
    <Icon size={size}>
      <circle cx="12" cy="12" r="9" />
      <path d="M3 12h18M12 3c2.5 2.5 3.5 5.5 3.5 9s-1 6.5-3.5 9c-2.5-2.5-3.5-5.5-3.5-9s1-6.5 3.5-9z" />
    </Icon>
  );
}

export function AlertIcon({ size }: { size?: number | undefined }) {
  return (
    <Icon size={size}>
      <circle cx="12" cy="12" r="9" />
      <path d="M12 7.5v5.5M12 16.5v.01" />
    </Icon>
  );
}
