/**
 * The mini bar both media views use when their view is closed (issue #163): a compact row at the
 * top of the chat with the picture's thumbnail, the title, how far it is, the state dot, play or
 * pause, "Open" back to the full view and close. Everything keeps working from here.
 */
import type { ReactNode } from "react";
import { MediaIcon } from "./icons";
import "./media.css";

export interface MiniBarProps {
  /** Accessible name of the bar. */
  label: string;
  title: ReactNode;
  /** Thumbnail of the video or the shared picture. */
  thumbnail: ReactNode;
  /** How far the party is, as "7:12 / 17:30". */
  progress?: ReactNode;
  /** How far it is as 0–1, drawn as a thin line next to the progress where it is known. */
  fraction?: number;
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
  fraction,
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
        {progress !== undefined && (
          <span className="mini-bar__progress">
            {fraction !== undefined && (
              <span className="mini-bar__track" aria-hidden="true">
                <span
                  className="mini-bar__fill"
                  style={{ width: `${(Math.min(1, Math.max(0, fraction)) * 100).toFixed(1)}%` }}
                />
              </span>
            )}
            {progress}
          </span>
        )}
      </span>
      <span className="mini-bar__status">{status}</span>
      {actions}
      <button type="button" className="button mini-bar__button" onClick={onOpen}>
        <MediaIcon name="open" size={14} />
        {openLabel}
      </button>
      <button
        type="button"
        className="button mini-bar__icon"
        aria-label={closeLabel}
        data-tooltip={closeLabel}
        onClick={onClose}
      >
        <MediaIcon name="close" size={14} />
      </button>
    </div>
  );
}
