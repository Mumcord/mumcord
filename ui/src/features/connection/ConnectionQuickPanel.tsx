import { useId } from "react";
import { useTranslation } from "react-i18next";
import { Switch } from "../settings/components/Switch";
import { BitrateSlider } from "../settings/sections/BitrateSlider";
import { useSettings } from "../settings/useSettings";
import { estimateBandwidth } from "./bandwidth";
import { useServers } from "../servers";
import { AudioRatingBadge, AudioRatingReason } from "./AudioHealthRating";
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
  const servers = useServers();
  const link = servers.connection.state === "disconnected" ? null : servers.connection.serverId;
  const saved = servers.servers.find((server) => server.id === link);
  const serverName = saved ? saved.label || saved.address : "";
  const stats = state.status === "ready" ? state.stats : null;
  const bandwidth = stats ? estimateBandwidth(stats) : null;
  const limit = stats?.serverMaxBandwidthKbps ?? null;
  const lowLatency = lowLatencyAvailable();
  const withLowLatency = bandwidth ? bandwidth.kbps + bandwidth.lowLatencyExtraKbps : 0;

  return (
    <div className="connection-quick">
      <header className="connection-quick__header">
        <div className="connection-quick__headline">
          <span className="connection-quick__title">{t("quick.title")}</span>
          <span className="connection-quick__summary">
            {stats?.audio ? (
              <AudioRatingBadge rating={stats.audio.rating} issue={stats.audio.issue} compact />
            ) : null}
            {stats ? (
              <span>
                {t("quick.status", {
                  latency:
                    stats.rttMs === null
                      ? t("none")
                      : t("latencyValue", { value: Math.round(stats.rttMs) }),
                  loss: t("lossValue", { value: stats.lossPercent }),
                })}
              </span>
            ) : null}
          </span>
        </div>
        {stats?.audio?.rating !== "good" ? (
          <AudioRatingReason issue={stats?.audio?.issue ?? null} />
        ) : null}
        <p className="settings-hint connection-quick__applies">
          {serverName ? `${serverName} · ` : ""}
          {t("quick.applies")}
        </p>
      </header>
      {settings ? (
        <section className="connection-quick__section">
          <BitrateSlider
            settings={settings}
            update={update}
            limitLabel={
              stats?.serverMaxBandwidthKbps != null
                ? `${t("serverLimit")} ${String(stats.serverMaxBandwidthKbps)}`
                : t("serverLimit")
            }
          />
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
          <p className="connection-quick__effective">
            <span id={meterId}>{t("quick.bandwidthLabel")}</span>
            <strong>
              {limit === null
                ? t("quick.bandwidthValueNoLimit", { value: Math.round(bandwidth.kbps) })
                : t("quick.bandwidthValue", { value: Math.round(bandwidth.kbps), limit })}
            </strong>
          </p>
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
            {t(
              limit === null
                ? "quick.bandwidthHintNoLimit"
                : withLowLatency <= limit
                  ? "quick.bandwidthHintWithin"
                  : "quick.bandwidthHintOver",
              { value: Math.round(withLowLatency) },
            )}
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
