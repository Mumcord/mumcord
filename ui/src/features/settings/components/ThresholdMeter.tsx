import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { getApi } from "../../../api";
import { LevelMeter } from "./LevelMeter";

/** Lowest and highest threshold; matches the engine's range and the meter's −60…0 dBFS scale. */
const THRESHOLD_MIN_DBFS = -60;
const THRESHOLD_MAX_DBFS = 0;

/** Position of `dbfs` on the 0–1 meter scale used by the `micLevel` event. */
function dbfsToMeter(dbfs: number): number {
  return (dbfs - THRESHOLD_MIN_DBFS) / (THRESHOLD_MAX_DBFS - THRESHOLD_MIN_DBFS);
}

interface ThresholdMeterProps {
  value: number;
  onChange: (dbfs: number) => void;
}

/**
 * Live microphone level with the voice-activation threshold: a slider drawn over the meter
 * (FR-1.57). The level flows while connected; otherwise the button runs the engine's short
 * microphone test, which is cut off before playback, only to feed the meter.
 */
export function ThresholdMeter({ value, onChange }: ThresholdMeterProps) {
  const { t } = useTranslation("settings");
  const [level, setLevel] = useState(0);
  const [measuring, setMeasuring] = useState(false);
  const measuringRef = useRef(false);

  useEffect(() => {
    const offLevel = getApi().on("micLevel", (payload) => {
      setLevel(payload.level ?? 0);
    });
    const offPhase = getApi().on("micTestPhaseChanged", (payload) => {
      if (!measuringRef.current || payload.phase === "recording") return;
      measuringRef.current = false;
      setMeasuring(false);
      setLevel(0);
      // Playback of the recording is not wanted here.
      if (payload.phase === "playing") {
        getApi()
          .micTestStop()
          .catch((error: unknown) => {
            console.error("Stopping the level measurement failed", error);
          });
      }
    });
    return () => {
      offLevel();
      offPhase();
      if (measuringRef.current) {
        measuringRef.current = false;
        getApi()
          .micTestStop()
          .catch((error: unknown) => {
            console.error("Stopping the level measurement failed", error);
          });
      }
    };
  }, []);

  const measure = () => {
    measuringRef.current = true;
    setMeasuring(true);
    getApi()
      .micTestStart()
      .catch((error: unknown) => {
        console.error("Starting the level measurement failed", error);
        measuringRef.current = false;
        setMeasuring(false);
      });
  };

  return (
    <div className="settings-field-row">
      <div className="settings-threshold">
        <LevelMeter label={t("voice.level")} level={level} threshold={dbfsToMeter(value)} />
        <input
          type="range"
          className="settings-threshold__slider"
          aria-label={t("voice.threshold")}
          aria-valuetext={t("voice.thresholdValue", { value })}
          min={THRESHOLD_MIN_DBFS}
          max={THRESHOLD_MAX_DBFS}
          step={1}
          value={value}
          onChange={(event) => {
            onChange(Number(event.target.value));
          }}
        />
      </div>
      <p className="settings-value">{t("voice.thresholdValue", { value })}</p>
      <p className="settings-hint">{t("voice.thresholdHint")}</p>
      <div className="settings-actions">
        <button type="button" className="settings-button" disabled={measuring} onClick={measure}>
          {measuring ? t("voice.measuring") : t("voice.measure")}
        </button>
      </div>
    </div>
  );
}
