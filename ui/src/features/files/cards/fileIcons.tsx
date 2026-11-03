/** Line icons of the file cards and dialogs (mockups 2 – 7, 12, 13 of #348). */
import type { ReactNode } from "react";

function Icon({ size = 16, children }: { size?: number | undefined; children: ReactNode }) {
  return (
    <svg
      viewBox="0 0 24 24"
      width={size}
      height={size}
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      focusable="false"
    >
      {children}
    </svg>
  );
}

interface SizeProps {
  size?: number | undefined;
}

export function DocumentIcon({ size }: SizeProps) {
  return (
    <Icon size={size}>
      <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
      <path d="M14 2v6h6M8 13h8M8 17h5" />
    </Icon>
  );
}

export function ProgramIcon({ size }: SizeProps) {
  return (
    <Icon size={size}>
      <rect x="3" y="4" width="18" height="16" rx="2" />
      <path d="M3 8h18M7 12l3 2-3 2M12 16h4" />
    </Icon>
  );
}

export function CopyIcon({ size }: SizeProps) {
  return (
    <Icon size={size}>
      <rect x="9" y="9" width="12" height="12" rx="2" />
      <path d="M5 15H4a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1h10a1 1 0 0 1 1 1v1" />
    </Icon>
  );
}

export function DownloadIcon({ size }: SizeProps) {
  return (
    <Icon size={size}>
      <path d="M12 3v12M7 10l5 5 5-5M5 21h14" />
    </Icon>
  );
}

export function LockIcon({ size }: SizeProps) {
  return (
    <Icon size={size}>
      <rect x="4" y="11" width="16" height="10" rx="2" />
      <path d="M8 11V7a4 4 0 0 1 8 0v4" />
    </Icon>
  );
}

export function CloseIcon({ size }: SizeProps) {
  return (
    <Icon size={size}>
      <path d="M6 6l12 12M18 6 6 18" />
    </Icon>
  );
}

export function FolderIcon({ size }: SizeProps) {
  return (
    <Icon size={size}>
      <path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" />
    </Icon>
  );
}

export function CheckIcon({ size }: SizeProps) {
  return (
    <Icon size={size}>
      <path d="M5 12l5 5 9-10" />
    </Icon>
  );
}

export function StopIcon({ size }: SizeProps) {
  return (
    <Icon size={size}>
      <rect x="6" y="6" width="12" height="12" rx="2" />
    </Icon>
  );
}

export function WarningIcon({ size }: SizeProps) {
  return (
    <Icon size={size}>
      <path d="M10.3 3.9 1.8 18a2 2 0 0 0 1.7 3h17a2 2 0 0 0 1.7-3L13.7 3.9a2 2 0 0 0-3.4 0zM12 9v4M12 17h.01" />
    </Icon>
  );
}

export function ClockIcon({ size }: SizeProps) {
  return (
    <Icon size={size}>
      <circle cx="12" cy="12" r="9" />
      <path d="M12 7v5l3 2" />
    </Icon>
  );
}

export function BlockedIcon({ size }: SizeProps) {
  return (
    <Icon size={size}>
      <circle cx="12" cy="12" r="10" />
      <path d="M4.9 4.9l14.2 14.2" />
    </Icon>
  );
}
