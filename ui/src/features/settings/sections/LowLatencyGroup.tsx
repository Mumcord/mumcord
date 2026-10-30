import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { getApi } from "../../../api";
import type { LatencyEstimate } from "../../../bindings/commands";
import { Switch } from "../components/Switch";
import { LatencyBreakdown, LatencySummary } from "./LatencyEstimate";
import type { SettingsSectionProps } from "./types";

const POLL_MS = 2000;

/** Re-reads the estimate while mounted; it moves with the ping and the measured processing. */
function useLatencyEstimate(lowLatency: boolean): LatencyEstimate | null {
  const [estimate, setEstimate] = useState<LatencyEstimate | null>(null);
  useEffect(() => {
    let active = true;
    const load = () => {
      getApi()
        .latencyEstimate()
        .then(
          (loaded) => {
            if (active) setEstimate(loaded);
          },
          (error: unknown) => {
            console.error("Loading the latency estimate failed", error);
          },
        );
    };
    load();
    const timer = setInterval(load, POLL_MS);
    return () => {
      active = false;
      clearInterval(timer);
    };
  }, [lowLatency]);
  return estimate;
}

/** Low-latency switch with its trade-off warning and the live latency estimate. */
export function LowLatencyGroup({ settings, update }: SettingsSectionProps) {
  const { t } = useTranslation("settings");
  const estimate = useLatencyEstimate(settings.lowLatency);
  return (
    <>
      <Switch
        label={t("audio.lowLatency")}
        badge={t("audio.lowLatencyBeta")}
        description={t("audio.lowLatencyHint")}
        checked={settings.lowLatency}
        onChange={(lowLatency) => {
          update({ lowLatency });
        }}
      />
      {settings.lowLatency && (
        <div role="note" className="latency-warning">
          <p className="latency-warning__title">{t("audio.lowLatencyWarningTitle")}</p>
          <ul>
            <li>{t("audio.lowLatencyBandwidth")}</li>
            <li>{t("audio.lowLatencyWifi")}</li>
            <li>{t("audio.lowLatencyHeadphones")}</li>
            <li>{t("audio.lowLatencyPtt")}</li>
          </ul>
        </div>
      )}
      {estimate !== null && (
        <div aria-live="off">
          <LatencySummary estimate={estimate} />
          <LatencyBreakdown estimate={estimate} />
        </div>
      )}
    </>
  );
}
