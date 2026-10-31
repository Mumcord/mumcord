/**
 * Warning triangle for the identity manager. Drawn, not a "⚠" character: a text glyph would be
 * picked up as untranslated UI text, and screen readers get the message from the surrounding
 * `role="alert"` text.
 */
export function WarningIcon({ className }: { className?: string }) {
  return (
    <svg
      className={className}
      viewBox="0 0 24 24"
      width="16"
      height="16"
      aria-hidden="true"
      focusable="false"
    >
      <path
        d="M12 3.5 2.5 20h19L12 3.5Z"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.8"
        strokeLinejoin="round"
      />
      <path
        d="M12 9.5v4.6"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.8"
        strokeLinecap="round"
      />
      <circle cx="12" cy="17.1" r="1.1" fill="currentColor" />
    </svg>
  );
}
