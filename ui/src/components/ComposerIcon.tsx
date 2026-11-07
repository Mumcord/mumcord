/**
 * Line icon of the message input bar (issue #596). Every button in the bar draws its icon with
 * the same 24-unit grid, the same 20 px box and the same stroke, so the row reads as one set.
 * Decorative: the button around it names itself.
 */
export function ComposerIcon({
  d,
  size = 20,
}: {
  /** One path, or several when the icon is made of more than one stroke. */
  d: string | readonly string[];
  /** Icon box in pixels; the bar's own icons all use the default. */
  size?: number;
}) {
  const paths = typeof d === "string" ? [d] : d;
  return (
    <svg
      viewBox="0 0 24 24"
      width={size}
      height={size}
      aria-hidden="true"
      focusable="false"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.75"
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      {paths.map((path) => (
        <path key={path} d={path} />
      ))}
    </svg>
  );
}
