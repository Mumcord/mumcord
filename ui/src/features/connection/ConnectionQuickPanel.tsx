import { useId } from "react";
import { useTranslation } from "react-i18next";
import { Switch } from "../settings/components/Switch";
import { BitrateSlider } from "../settings/sections/BitrateSlider";
import { useSettings } from "../settings/useSettings";
import { estimateBandwidth } from "./bandwidth";
import { AudioHealthRating } from "./AudioHealthRating";
import { windowMinutes } from "./audioHealth";
import { useConnectionStats } from "./useConnectionStats";

/**
 * Whether the engine has a low-latency setting yet (issue #156). Until it does, the row is shown
 * disabled so the panel layout is final.
 */
function lowLatencyAvailable(): boolean {
  return false;
}

/**
 * "Voice quality" quick panel: the bitrate cap and Music quality from Settings > Audio (the same
 * stored values, saved at once), the bandwidth estimate and a way into the full audio settings.
 * The audio health rating stays visible at the top.
 */
export function ConnectionQuickPanel({ onOpenVoiceSettings }: { onOpenVoiceSettings: () => void }) {
  const { t } = useTranslation("connection");
  const { t: ts } = useTranslation("settings");
  const { settings, update } = useSettings();
  const state = useConnectionStats(true);
  const meterId = useId();
  const stats = state.status === "ready" ? state.stats : null;
  const bandwidth = stats ? estimateBandwidth(stats) : null;
  const limit = stats?.serverMaxBandwidthKbps ?? null;
  const lowLatency = lowLatencyAvailable();

  return (
    <div className="connection-quick">
      {stats ? (
        <>
          <p className="connection-quick__status">
            {t("quick.status", {
              latency:
                stats.rttMs === null
                  ? t("none")
                  : t("latencyValue", { value: Math.round(stats.rttMs) }),
              loss: t("lossValue", { value: stats.lossPercent }),
            })}
          </p>
          {stats.audio ? (
            <AudioHealthRating
              rating={stats.audio.rating}
              issue={stats.audio.issue}
              minutes={windowMinutes(stats.audio.windowSecs)}
            />
          ) : null}
        </>
      ) : null}
      {settings ? (
        <>
          <BitrateSlider settings={settings} update={update} />
          {stats ? (
            <p className="settings-hint">
              {t("quick.effective", { value: stats.sendBitrateKbps })}
            </p>
          ) : null}
          <Switch
            label={ts("audio.musicQuality")}
            checked={settings.musicQuality}
            onChange={(musicQuality) => {
              update({ musicQuality });
            }}
          />
        </>
      ) : null}
      <Switch
        label={t("quick.lowLatency")}
        description={lowLatency ? t("quick.lowLatencyHint") : t("quick.lowLatencyComingSoon")}
        checked={false}
        disabled={!lowLatency}
        onChange={() => undefined}
      />
      {bandwidth ? (
        <div className="connection-quick__bandwidth">
          <span id={meterId}>
            {t("quick.bandwidth", { value: Math.round(bandwidth.kbps) })}
            {limit === null ? "" : ` / ${t("bitrateValue", { value: limit })}`}
          </span>
          {bandwidth.share !== null ? (
            <meter
              aria-labelledby={meterId}
              min={0}
              max={1}
              low={0.7}
              high={0.9}
              optimum={0}
              value={bandwidth.share}
            />
          ) : null}
          <p className="settings-hint">
            {t("quick.bandwidthHint", { value: Math.round(bandwidth.lowLatencyExtraKbps) })}
          </p>
        </div>
      ) : null}
      <button type="button" className="connection-quick__link" onClick={onOpenVoiceSettings}>
        {t("quick.voiceSettings")}
      </button>
    </div>
  );
}
