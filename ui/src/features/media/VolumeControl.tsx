import type { CSSProperties } from "react";
/**
 * Volume for a media view (issue #163): a slider with the level, plus a live meter where the source
 * can report one (the shared sound of a screen share). M mutes, and the meter keeps showing that the
 * sound is there.
 */
import { MediaIcon } from "./icons";

export interface VolumeControlProps {
  label: string;
  /** 0–1. */
  level: number;
  onSet: (level: number) => void;
  /** Live level 0–1 where the engine reports one; the bar then shows a meter. */
  meter?: number | null;
}

export function VolumeControl({ label, level, onSet, meter = null }: VolumeControlProps) {
  const percent = Math.round(level * 100);
  return (
    <div className="volume" style={{ "--played": `${String(percent)}%` } as CSSProperties}>
      <span className="volume__icon" aria-hidden="true">
        <MediaIcon name={percent === 0 ? "muted" : "volume"} />
      </span>
      <input
        type="range"
        className="volume__input"
        aria-label={label}
        value={percent}
        min={0}
        max={100}
        step={1}
        aria-valuenow={percent}
        aria-valuetext={`${String(percent)} %`}
        onChange={(event) => {
          onSet(Number(event.target.value) / 100);
        }}
      />
      {meter !== null && (
        <span
          className="volume__meter"
          aria-hidden="true"
          style={{ width: `${String(Math.min(100, Math.round(meter * 100)))}%` }}
        />
      )}
    </div>
  );
}
