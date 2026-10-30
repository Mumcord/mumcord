import { useId } from "react";
import { useTranslation } from "react-i18next";
import { Switch } from "../settings/components/Switch";
import { BitrateSlider } from "../settings/sections/BitrateSlider";
import { useSettings } from "../settings/useSettings";
import { estimateBandwidth } from "./bandwidth";
import { InfoTip } from "../../components/InfoTip";
import { useServers } from "../servers";
import { AudioRatingBadge, AudioRatingReason } from "./AudioHealthRating";
import { LatencyBreakdown, LatencySummary } from "../settings/sections/LatencyEstimate";
import { useConnectionStats } from "./useConnectionStats";

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
  const more = (topic: string) => t("quick.moreInfo", { topic });
  const servers = useServers();
  const link = servers.connection.state === "disconnected" ? null : servers.connection.serverId;
  const saved = servers.servers.find((server) => server.id === link);
  const serverName = saved ? saved.label || saved.address : "";
  const stats = state.status === "ready" ? state.stats : null;
  const lowLatency = settings?.lowLatency ?? false;
  const bandwidth = stats ? estimateBandwidth(stats, lowLatency) : null;
  const limit = stats?.serverMaxBandwidthKbps ?? null;
  const withLowLatency = bandwidth ? bandwidth.kbps + bandwidth.lowLatencyExtraKbps : 0;
  const bandwidthHint = lowLatency
    ? t(
        limit === null
          ? "quick.bandwidthLowLatencyNoLimit"
          : (bandwidth?.kbps ?? 0) <= limit
            ? "quick.bandwidthLowLatencyWithin"
            : "quick.bandwidthLowLatencyOver",
        { value: Math.round(bandwidth?.lowLatencyExtraKbps ?? 0) },
      )
    : t(
        limit === null
          ? "quick.bandwidthHintNoLimit"
          : withLowLatency <= limit
            ? "quick.bandwidthHintWithin"
            : "quick.bandwidthHintOver",
        { value: Math.round(withLowLatency) },
      );
  const warning = [
    bandwidth
      ? t("quick.warningBandwidth", {
          on: Math.round(bandwidth.kbps),
          off: Math.round(bandwidth.kbps - bandwidth.lowLatencyExtraKbps),
        })
      : null,
    t("quick.warningWifi"),
    t("quick.warningHeadphones"),
    t("quick.warningPtt"),
  ]
    .filter((line) => line !== null)
    .join(" ");

  return (
    <div className="connection-quick">
      <header className="connection-quick__header">
        <div className="connection-quick__headline">
          <span className="connection-quick__title">
            {t("quick.title")}
            <InfoTip label={more(t("quick.title"))} text={t("quick.applies")} />
          </span>
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
        {serverName ? <p className="settings-hint">{serverName}</p> : null}
      </header>
      {settings ? (
        <section className="connection-quick__section">
          <BitrateSlider
            settings={settings}
            update={update}
            tipLabel={more(ts("audio.bitrate"))}
            compact
          />
          {stats ? (
            <p className="connection-quick__effective">
              <span>
                {t("quick.effectiveShort")}
                <InfoTip label={more(t("quick.effectiveShort"))} text={t("quick.effectiveTip")} />
              </span>
              <strong>{t("bitrateValue", { value: stats.sendBitrateKbps })}</strong>
            </p>
          ) : null}
        </section>
      ) : null}
      {settings ? (
        <section className="connection-quick__section">
          <Switch
            label={ts("audio.musicQuality")}
            tip={{
              label: more(ts("audio.musicQuality")),
              text: lowLatency ? t("quick.musicIncluded") : ts("audio.musicQualityHint"),
            }}
            checked={lowLatency || settings.musicQuality}
            disabled={lowLatency}
            onChange={(musicQuality) => {
              update({ musicQuality });
            }}
          />
        </section>
      ) : null}
      {settings ? (
        <section className="connection-quick__section">
          <Switch
            label={t("quick.lowLatency")}
            badge={t("quick.beta")}
            tip={{ label: more(t("quick.lowLatency")), text: t("quick.lowLatencyHint") }}
            checked={settings.lowLatency}
            onChange={(on) => {
              update({ lowLatency: on });
            }}
          />
          {stats?.latency ? <LatencySummary estimate={stats.latency} /> : null}
          {lowLatency && stats?.latency ? <LatencyBreakdown estimate={stats.latency} /> : null}
          {lowLatency ? (
            <p className="connection-quick__warning">
              {ts("audio.lowLatencyWarningTitle")}
              <InfoTip label={more(ts("audio.lowLatencyWarningTitle"))} text={warning} />
            </p>
          ) : null}
        </section>
      ) : null}
      {bandwidth ? (
        <section className="connection-quick__section connection-quick__bandwidth">
          <p className="connection-quick__effective">
            <span id={meterId}>
              {t("quick.bandwidthLabel")}
              <InfoTip label={more(t("quick.bandwidthLabel"))} text={bandwidthHint} />
            </span>
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
