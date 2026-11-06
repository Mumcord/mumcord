/** Camera line icon (mockups 01, 02, 05); crossed out while the camera is off. Decorative. */
export function CameraIcon({ size, off = false }: { size: number; off?: boolean }) {
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
      <path d="M15 10l5-3v10l-5-3z" />
      <rect x="2" y="6" width="13" height="12" rx="2" />
      {off && <path d="M2 2l20 20" />}
    </svg>
  );
}
