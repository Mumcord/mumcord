interface LevelMeterProps {
  /** Microphone level 0.0–1.0 as sent by the `micLevel` event. */
  level: number;
  label: string;
}

/** Horizontal level bar exposed to assistive technology as a meter (0–100 %). */
export function LevelMeter({ level, label }: LevelMeterProps) {
  const percent = Math.round(Math.min(1, Math.max(0, level)) * 100);
  return (
    <div
      className="wizard-meter"
      role="meter"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={percent}
      aria-valuetext={`${String(percent)} %`}
    >
      <div className="wizard-meter-fill" style={{ width: `${String(percent)}%` }} />
    </div>
  );
}
