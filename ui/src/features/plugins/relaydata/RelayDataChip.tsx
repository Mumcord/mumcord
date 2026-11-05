import { useTranslation } from "react-i18next";
import { useRelayAvailability } from "./useRelayAvailability";
import "./relaydata.css";

interface RelayDataChipProps {
  serverId: number | null;
  /**
   * `false` when the relay could carry the data but this plugin does not use it (switch or
   * permission off, FR-9.63): the chip then shows the Mumble connection.
   */
  usable?: boolean;
}

/**
 * The stage's connection chip (FR-9.48, FR-9.52): "Relay · end-to-end encrypted" or "Mumble
 * connection · slower". Shared with the stage header. The relay's rate limit is in the tooltip.
 */
export function RelayDataChip({ serverId, usable = true }: RelayDataChipProps) {
  const { t } = useTranslation("pluginStage");
  const { t: tRelay } = useTranslation("pluginRelay");
  const availability = useRelayAvailability(serverId);
  if (availability === null) return null;
  if (availability.kind === "available" && usable) {
    return (
      <span
        className="relay-chip relay-chip--ok"
        data-tooltip={
          availability.kbps === null
            ? undefined
            : tRelay("serverAllowsRate", { rate: availability.kbps })
        }
      >
        {t("connection.relay")}
      </span>
    );
  }
  return (
    <span
      className="relay-chip relay-chip--warn"
      data-tooltip={availability.kind === "available" ? undefined : t("noRelayTitle")}
    >
      {t("connection.mumble")}
    </span>
  );
}
