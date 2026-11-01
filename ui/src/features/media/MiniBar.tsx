/**
 * The mini bar both media views use when their view is closed (issue #163): a compact row at the
 * top of the chat with the picture's thumbnail, the title, how far it is, the state dot, play or
 * pause, "Open" back to the full view and close. Everything keeps working from here.
 */
import type { ReactNode } from "react";
import "./media.css";

export interface MiniBarProps {
  /** Accessible name of the bar. */
  label: string;
  title: ReactNode;
  /** Thumbnail of the video or the shared picture. */
  thumbnail: ReactNode;
  /** How far the party is, as "7:12 / 17:30". */
  progress?: ReactNode;
  /** State dot: sync for the party, live for a share. */
  status: ReactNode;
  actions?: ReactNode;
  openLabel: string;
  onOpen: () => void;
  closeLabel: string;
  onClose: () => void;
}

export function MiniBar({
  label,
  title,
  thumbnail,
  progress,
  status,
  actions,
  openLabel,
  onOpen,
  closeLabel,
  onClose,
}: MiniBarProps) {
  return (
    <div className="mini-bar" role="group" aria-label={label}>
      <span className="mini-bar__thumb" aria-hidden="true">
        {thumbnail}
      </span>
      <span className="mini-bar__text">
        <span
          className="mini-bar__title"
          data-tooltip={typeof title === "string" ? title : undefined}
        >
          {title}
        </span>
        {progress !== undefined && <span className="mini-bar__progress">{progress}</span>}
      </span>
      <span className="mini-bar__status">{status}</span>
      {actions}
      <button type="button" className="button" data-tooltip={openLabel} onClick={onOpen}>
        {openLabel}
      </button>
      <button
        type="button"
        className="button"
        aria-label={closeLabel}
        data-tooltip={closeLabel}
        onClick={onClose}
      >
        <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true" focusable="false">
          <path
            d="M4 4l8 8M12 4l-8 8"
            fill="none"
            stroke="currentColor"
            strokeWidth="1.6"
            strokeLinecap="round"
          />
        </svg>
      </button>
    </div>
  );
}
