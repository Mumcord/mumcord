/** Eye glyph, same 16px stroke style as the person status icons; the pupil fills while followed. */
export function EyeIcon() {
  return (
    <svg
      className="follow-eye"
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
      <path d="M2 12s3.6-7 10-7 10 7 10 7-3.6 7-10 7-10-7-10-7" />
      <circle className="follow-eye__pupil" cx="12" cy="12" r="3" />
    </svg>
  );
}
