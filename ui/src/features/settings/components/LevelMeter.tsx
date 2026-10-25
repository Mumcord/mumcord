interface LevelMeterProps {
  label: string;
  /** 0.0 (silence) to 1.0 (full scale). */
  level: number;
  /** Optional threshold marker, 0.0–1.0 on the same scale. */
  threshold?: number;
}

/** Microphone level bar. */
export function LevelMeter({ label, level, threshold }: LevelMeterProps) {
  const percent = Math.round(Math.min(1, Math.max(0, level)) * 100);
  return (
    <div
      className="settings-meter"
      role="meter"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={percent}
    >
      <div className="settings-meter__fill" style={{ width: `${String(percent)}%` }} />
      {threshold !== undefined && (
        <div
          className="settings-meter__threshold"
          style={{ left: `${String(Math.round(Math.min(1, Math.max(0, threshold)) * 100))}%` }}
        />
      )}
    </div>
  );
}
