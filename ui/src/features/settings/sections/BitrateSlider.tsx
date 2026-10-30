import { useEffect, useId, useState } from "react";
import { useTranslation } from "react-i18next";
import { getApi } from "../../../api";
import { useThrottledSend } from "../../../components/useThrottledSend";
import type { SettingsSectionProps } from "./types";

/** Lowest bitrate the slider offers (FR-2.64). */
export const BITRATE_MIN_KBPS = 16;
/** Opus' own limit; the range while no voice server tells a lower maximum. */
const BITRATE_OPUS_MAX_KBPS = 510;
export const BITRATE_THROTTLE_MS = 100;

/**
 * Own bitrate cap (FR-2.64): from 16 kbit/s to the highest bitrate the voice server allows; the far
 * right is "Max" (the full maximum). Independent of "Music quality"; defaults to 96 kbit/s. Applied live while dragging.
 */
export function BitrateSlider({
  settings,
  update,
  limitLabel,
}: SettingsSectionProps & {
  /** Compact variant (quick panel): shows the range ends as marks and keeps the hint for screen readers only. */
  limitLabel?: string;
}) {
  const { t } = useTranslation("settings");
  const id = useId();
  const { schedule } = useThrottledSend(BITRATE_THROTTLE_MS);
  const [serverMax, setServerMax] = useState<number | null>(null);
  const [dragging, setDragging] = useState<number | null>(null);

  useEffect(() => {
    let active = true;
    const api = getApi();
    api.bitrateMax().then(
      (kbps) => {
        if (active) setServerMax(kbps);
      },
      (error: unknown) => {
        console.error("Loading the bitrate maximum failed", error);
      },
    );
    const unsubscribe = api.on("bitrateMaxChanged", ({ kbps }) => {
      setServerMax(kbps);
    });
    return () => {
      active = false;
      unsubscribe();
    };
  }, []);

  const max = Math.max(serverMax ?? BITRATE_OPUS_MAX_KBPS, BITRATE_MIN_KBPS);
  const stored = settings.bitrateKbps;
  const value = Math.min(dragging ?? stored ?? max, max);
  const atMax = value >= max;
  const shown = atMax ? t("audio.bitrateMax") : t("audio.bitrateValue", { kbps: value });

  return (
    <div className="settings-field overlay-settings__slider">
      <label htmlFor={id}>{t("audio.bitrate")}</label>
      <input
        id={id}
        type="range"
        min={BITRATE_MIN_KBPS}
        max={max}
        step={1}
        value={value}
        aria-valuetext={shown}
        aria-describedby={`${id}-hint`}
        onChange={(event) => {
          const next = Number(event.target.value);
          setDragging(next);
          schedule(() => {
            update({ bitrateKbps: next >= max ? null : next });
          });
        }}
        onPointerUp={() => {
          setDragging(null);
        }}
        onBlur={() => {
          setDragging(null);
        }}
      />
      <output htmlFor={id}>{shown}</output>
      {limitLabel !== undefined ? (
        <div className="overlay-settings__marks" aria-hidden="true">
          <span>{BITRATE_MIN_KBPS}</span>
          {serverMax !== null ? <span>{limitLabel}</span> : null}
        </div>
      ) : null}
      <p
        id={`${id}-hint`}
        className={limitLabel !== undefined ? "settings-hint visually-hidden" : "settings-hint"}
      >
        {serverMax === null
          ? t("audio.bitrateMaxUnknown")
          : t("audio.bitrateMaxKnown", { kbps: serverMax })}
      </p>
    </div>
  );
}
