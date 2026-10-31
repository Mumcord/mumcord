/**
 * The frame the two media views of issue #163 share: the picture, a top overlay saying who and what,
 * and a bottom overlay with the controls in the same order for both. The overlays fade in on hover,
 * keyboard focus or pause, and stay visible when reduced motion is on (`media.css`).
 */
import type { ReactNode, RefObject } from "react";
import "./media.css";

export interface MediaFrameProps {
  /** Element that full screen takes (F). */
  ref: RefObject<HTMLElement | null>;
  /** Accessible name of the region, e.g. "Watch together – Northern Lights over a Quiet Lake". */
  label: string;
  /** Top overlay: title, who added it, who is in control, the sync pill. */
  header: ReactNode;
  /** The picture area. */
  picture: ReactNode;
  /** Bottom overlay controls, in the shared order, before full screen. */
  controls: ReactNode;
  /** Accessible name of the control row. */
  toolbarLabel: string;
  /** Keeps the controls visible while the party or the share is paused (issue #163). */
  paused?: boolean;
  /** The keys, shown on first use. */
  hint?: ReactNode;
  fullscreen: boolean;
  onFullscreen: () => void;
  fullscreenLabel: string;
  exitFullscreenLabel: string;
}

export function MediaFrame({
  ref,
  label,
  header,
  picture,
  controls,
  toolbarLabel,
  paused = false,
  hint,
  fullscreen,
  onFullscreen,
  fullscreenLabel,
  exitFullscreenLabel,
}: MediaFrameProps) {
  return (
    <section ref={ref} className="media-frame" aria-label={label} data-paused={paused || undefined}>
      <div className="media-frame__picture">{picture}</div>
      <header className="media-frame__top">{header}</header>
      <div className="media-frame__bottom" role="toolbar" aria-label={toolbarLabel}>
        {controls}
        <button
          type="button"
          className="button media-frame__icon"
          aria-label={fullscreen ? exitFullscreenLabel : fullscreenLabel}
          data-tooltip={fullscreen ? exitFullscreenLabel : fullscreenLabel}
          aria-pressed={fullscreen}
          aria-keyshortcuts="F"
          onClick={onFullscreen}
        >
          <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true" focusable="false">
            <path
              d={
                fullscreen
                  ? "M6 2v4H2M10 2v4h4M6 14v-4H2M10 14v-4h4"
                  : "M2 6V2h4M14 6V2h-4M2 10v4h4M14 10v4h-4"
              }
              fill="none"
              stroke="currentColor"
              strokeWidth="1.6"
              strokeLinecap="round"
              strokeLinejoin="round"
            />
          </svg>
        </button>
      </div>
      {hint !== undefined && <p className="media-frame__hint">{hint}</p>}
    </section>
  );
}
