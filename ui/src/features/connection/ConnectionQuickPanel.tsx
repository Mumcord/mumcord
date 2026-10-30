import { useId } from "react";
import { useTranslation } from "react-i18next";
import { Switch } from "../settings/components/Switch";
import { BitrateSlider } from "../settings/sections/BitrateSlider";
import { useSettings } from "../settings/useSettings";
import { estimateBandwidth } from "./bandwidth";
import { AudioRatingBadge } from "./AudioHealthRating";
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
      <header className="connection-quick__header">
        {stats ? (
          <p className="connection-quick__status">
            {t("quick.status", {
              latency:
                stats.rttMs === null
                  ? t("none")
                  : t("latencyValue", { value: Math.round(stats.rttMs) }),
              loss: t("lossValue", { value: stats.lossPercent }),
            })}
          </p>
        ) : null}
        {stats?.audio ? (
          <AudioRatingBadge rating={stats.audio.rating} issue={stats.audio.issue} />
        ) : null}
        <p className="settings-hint connection-quick__applies">{t("quick.applies")}</p>
      </header>
      {settings ? (
        <section className="connection-quick__section">
          <BitrateSlider settings={settings} update={update} />
          {stats ? (
            <p className="connection-quick__effective">
              <span>{t("quick.effectiveLabel")}</span>
              <strong>{t("bitrateValue", { value: stats.sendBitrateKbps })}</strong>
            </p>
          ) : null}
        </section>
      ) : null}
      {settings ? (
        <section className="connection-quick__section">
          <Switch
            label={ts("audio.musicQuality")}
            description={ts("audio.musicQualityHint")}
            checked={settings.musicQuality}
            onChange={(musicQuality) => {
              update({ musicQuality });
            }}
          />
        </section>
      ) : null}
      <section className="connection-quick__section">
        <Switch
          label={t("quick.lowLatency")}
          description={lowLatency ? t("quick.lowLatencyHint") : t("quick.lowLatencyComingSoon")}
          checked={false}
          disabled={!lowLatency}
          onChange={() => undefined}
        />
      </section>
      {bandwidth ? (
        <section className="connection-quick__section connection-quick__bandwidth">
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
        </section>
      ) : null}
      <footer className="connection-quick__footer">
        <button type="button" className="connection-quick__link" onClick={onOpenVoiceSettings}>
          {t("quick.voiceSettings")}
        </button>
        <span className="settings-hint">{t("quick.keys")}</span>
      </footer>
    </div>
  );
}
