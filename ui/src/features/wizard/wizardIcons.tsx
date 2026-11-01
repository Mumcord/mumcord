import type { ReactNode } from "react";
import type { SetupPreset } from "../../api";

/** Line icons of the wizard; decorative, the text next to them carries the meaning. */
function Icon({ children }: { children: ReactNode }) {
  return (
    <svg
      className="wizard-icon"
      viewBox="0 0 24 24"
      width="24"
      height="24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.8"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      focusable="false"
    >
      {children}
    </svg>
  );
}

export function CheckIcon() {
  return (
    <Icon>
      <path d="M5 12.5l4.5 4.5L19 7.5" />
    </Icon>
  );
}

const STYLE_ICONS: Record<SetupPreset, ReactNode> = {
  everythingOn: (
    <path d="M12 3.5l2.6 5.3 5.9.9-4.3 4.1 1 5.8-5.2-2.7-5.2 2.7 1-5.8-4.3-4.1 5.9-.9z" />
  ),
  balanced: (
    <>
      <path d="M12 4v16M7 20h10M5 7h14" />
      <path d="M5 7l-3 6a3 3 0 0 0 6 0zM19 7l-3 6a3 3 0 0 0 6 0z" />
    </>
  ),
  maximumPrivacy: (
    <>
      <path d="M12 3l7 3v5c0 4.5-3 8-7 10-4-2-7-5.5-7-10V6z" />
      <path d="M9 12l2 2 4-4" />
    </>
  ),
  custom: <path d="M4 7h10M18 7h2M4 17h4M12 17h8M14 5v4M8 15v4" />,
};

export function StyleIcon({ style }: { style: SetupPreset }) {
  return <Icon>{STYLE_ICONS[style]}</Icon>;
}
