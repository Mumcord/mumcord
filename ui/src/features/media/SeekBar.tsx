/**
 * The seek bar of the cinema view (issue #163, FR-3.124): the played part, the buffered part where
 * the player knows it, and a marker where each other viewer is. It is a slider, so the keyboard can
 * move it (← and → jump 5 s); the markers carry the person's name as text, not only as a dot.
 */
import { clockTime } from "./format";

export interface SeekMarker {
  session: number;
  /** Where that person is, in milliseconds. */
  positionMs: number;
  /** Their name, for the tooltip and the screen reader. */
  name: string;
}

export interface SeekBarProps {
  label: string;
  positionMs: number;
  durationMs: number;
  /** How much the player has, or `null` where it does not say. */
  bufferedMs: number | null;
  markers: readonly SeekMarker[];
  onSeek: (positionMs: number) => void;
}

/** Without a known length the bar only shows what is played. */
export function SeekBar({
  label,
  positionMs,
  durationMs,
  bufferedMs,
  markers,
  onSeek,
}: SeekBarProps) {
  const duration = Math.max(1, durationMs);
  // Until the player says how long the video is, nothing can be shown as played.
  const played = durationMs > 0 ? Math.min(positionMs, duration) : 0;
  const buffered = Math.min(Math.max(bufferedMs ?? positionMs, played), duration);
  const percent = (ms: number) => `${((ms / duration) * 100).toFixed(2)}%`;
  const time = clockTime((durationMs > 0 ? played : positionMs) / 1000);
  const total = durationMs > 0 ? clockTime(durationMs / 1000) : null;

  return (
    <div className="seek-bar">
      <div className="seek-bar__buffer" aria-hidden="true" style={{ width: percent(buffered) }} />
      <div className="seek-bar__played" aria-hidden="true" style={{ width: percent(played) }} />
      <input
        type="range"
        className="seek-bar__input"
        aria-label={label}
        value={played}
        min={0}
        max={duration}
        step={1000}
        aria-valuemin={0}
        aria-valuemax={duration}
        aria-valuenow={played}
        aria-valuetext={total === null ? time : `${time} / ${total}`}
        onChange={(event) => {
          onSeek(Number(event.target.value));
        }}
      />
      {/* Without a known length there is nowhere to put the other viewers. */}
      {(durationMs > 0 ? markers : []).map((marker) => (
        <span
          key={marker.session}
          className="seek-bar__marker"
          data-tooltip={`${marker.name} · ${clockTime(marker.positionMs / 1000)}`}
          style={{ left: percent(Math.min(marker.positionMs, duration)) }}
        >
          <span className="visually-hidden">{`${marker.name} ${clockTime(marker.positionMs / 1000)}`}</span>
        </span>
      ))}
    </div>
  );
}
