/**
 * The frame the media views share: a rounded card with the picture, a top overlay saying who and what,
 * and a bottom overlay with the seek bar and the controls in the same order for both. The overlays are
 * the shared media overlay look (`mediaOverlay.css`), so a chat card's player looks the same (issue
 * #419): they sit on the picture and fade in on hover, keyboard focus or pause.
 */
import type { ReactNode, RefObject } from "react";
import { MediaIcon } from "./icons";
import "./media.css";
import "./mediaOverlay.css";

export interface MediaFrameProps {
  /** Element that full screen takes (F). */
  ref: RefObject<HTMLElement | null>;
  /** Accessible name of the region, e.g. "Watch together – Northern Lights over a Quiet Lake". */
  label: string;
  /**
   * Top overlay: title, who added it, who is in control, the sync pill. Left out while the
   * combined card's header next to the camera strip carries it (mockups 08–09).
   */
  header?: ReactNode;
  /** The picture area. */
  picture: ReactNode;
  /** Bottom overlay controls, in the shared order, before full screen. */
  controls: ReactNode;
  /** A full-width row above the controls, e.g. the seek bar of the watch party. */
  seek?: ReactNode;
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
  seek,
  toolbarLabel,
  paused = false,
  hint,
  fullscreen,
  onFullscreen,
  fullscreenLabel,
  exitFullscreenLabel,
}: MediaFrameProps) {
  return (
    <section
      ref={ref}
      className="media-frame media-overlay"
      aria-label={label}
      data-paused={paused || undefined}
      data-fullscreen={fullscreen || undefined}
    >
      <div className="media-frame__picture">{picture}</div>
      {header !== undefined && <header className="media-overlay__top">{header}</header>}
      {hint !== undefined && <p className="media-frame__hint">{hint}</p>}
      <div className="media-overlay__bar">
        {seek !== undefined && <div className="media-frame__seek">{seek}</div>}
        <div className="media-overlay__controls" role="toolbar" aria-label={toolbarLabel}>
          {controls}
          <button
            type="button"
            className="button media-overlay__icon"
            aria-label={fullscreen ? exitFullscreenLabel : fullscreenLabel}
            data-tooltip={fullscreen ? exitFullscreenLabel : fullscreenLabel}
            aria-pressed={fullscreen}
            aria-keyshortcuts="F"
            onClick={onFullscreen}
          >
            <MediaIcon name={fullscreen ? "exitFullscreen" : "fullscreen"} />
          </button>
        </div>
      </div>
    </section>
  );
}
