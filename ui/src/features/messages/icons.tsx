/** Line icons of the private messages overview, drawn on a 24x24 grid like the context menu's. */
export const CHAT_ICON = "M4 5h16v11H9l-5 4z";
export const COMPOSE_ICON = "M4 20h4L19 9l-4-4L4 16zM14 6l4 4";
export const SEARCH_ICON = "M10.5 4a6.5 6.5 0 1 0 0 13 6.5 6.5 0 0 0 0-13zM15.5 15.5L20 20";
export const CHECK_ICON = "M5 12l5 5 9-10";
export const PIN_ICON = "M9 4h6M10 4v6l-3 4h10l-3-4V4M12 14v6";
export const BELL_ICON = "M6 16V11a6 6 0 0 1 12 0v5l2 2H4zM10 20h4";
export const BELL_OFF_ICON = "M6 16V11a6 6 0 0 1 9-5.2M18 11v5l2 2H8M10 20h4M4 4l16 16";
export const PERSON_ICON = "M12 4a4 4 0 1 0 0 8 4 4 0 0 0 0-8zM4 20a8 8 0 0 1 16 0";
export const BLOCK_ICON = "M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zM5.6 5.6l12.8 12.8";
export const TRASH_ICON = "M4 7h16M9 7V4h6v3M6 7l1 13h10l1-13";

export function Icon({ path, className }: { path: string; className?: string }) {
  return (
    <svg
      className={className}
      viewBox="0 0 24 24"
      width="14"
      height="14"
      aria-hidden="true"
      focusable="false"
    >
      <path
        d={path}
        fill="none"
        stroke="currentColor"
        strokeWidth="2"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}
