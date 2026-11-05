/** Line icons of the camera preview dialog (mockups 03, 04). Decorative: their text is beside them. */
import type { ReactNode } from "react";

function Icon({
  size,
  children,
  strokeWidth = 2,
}: {
  size: number;
  children: ReactNode;
  strokeWidth?: number;
}) {
  return (
    <svg viewBox="0 0 24 24" width={size} height={size} aria-hidden="true" focusable="false">
      <g
        fill="none"
        stroke="currentColor"
        strokeWidth={strokeWidth}
        strokeLinecap="round"
        strokeLinejoin="round"
      >
        {children}
      </g>
    </svg>
  );
}

export function LockIcon({ size }: { size: number }) {
  return (
    <Icon size={size}>
      <rect x="5" y="11" width="14" height="10" rx="2" />
      <path d="M8 11V7a4 4 0 0 1 8 0v4" />
    </Icon>
  );
}

export function CheckIcon({ size }: { size: number }) {
  return (
    <Icon size={size}>
      <path d="M20 6 9 17l-5-5" />
    </Icon>
  );
}

export function NoneIcon() {
  return (
    <Icon size={20}>
      <circle cx="12" cy="12" r="9" />
      <path d="M5.6 5.6l12.8 12.8" />
    </Icon>
  );
}

export function BlurIcon() {
  return (
    <Icon size={20}>
      <circle cx="12" cy="8" r="3" />
      <path d="M6 20a6 6 0 0 1 12 0" />
      <path d="M2 4h.01M2 12h.01M2 20h.01M22 4h.01M22 12h.01M22 20h.01" strokeWidth="3" />
    </Icon>
  );
}

export function ImageIcon() {
  return (
    <Icon size={20}>
      <path d="M4 5h16v14H4zM4 16l5-5 4 4 3-3 4 4M15 9h.01" />
    </Icon>
  );
}

export function UploadIcon() {
  return (
    <Icon size={16}>
      <path d="M12 21V9M7 14l5-5 5 5M5 3h14" />
    </Icon>
  );
}

export function LoadIcon() {
  return (
    <Icon size={14}>
      <path d="M12 3v4M12 17v4M3 12h4M17 12h4M5.6 5.6l2.8 2.8M15.6 15.6l2.8 2.8M5.6 18.4l2.8-2.8M15.6 8.4l2.8-2.8" />
    </Icon>
  );
}

export function CameraIcon() {
  return (
    <Icon size={14}>
      <path d="M15 10l5-3v10l-5-3z" />
      <rect x="2" y="6" width="13" height="12" rx="2" />
    </Icon>
  );
}
