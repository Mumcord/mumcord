interface LevelMeterProps {
  label: string;
  /** 0.0 (silence) to 1.0 (full scale). */
  level: number;
}

/** Microphone level bar. */
export function LevelMeter({ label, level }: LevelMeterProps) {
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
    </div>
  );
}
