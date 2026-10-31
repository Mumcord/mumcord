import "./connectionQuality.css";
import { useTranslation } from "react-i18next";
import type { UserStats } from "../../bindings/commands";
import { useEffect, useId, useState, type ReactNode } from "react";
import { useShownServerId } from "../channels/serverStore";
import { P2pLinkText } from "../p2p/P2pLinkText";
import { useP2pLink } from "../p2p/useP2pLink";
import { peekConnectionExpandedRequest, takeConnectionExpandedRequest } from "./connectionExpanded";
import { connectionLevel, useUserStats } from "./userStatsStore";

/** Client and OS text comes from the other client; shown as plain text, capped. */
const MAX_TEXT = 64;

const cap = (text: string): string => text.slice(0, MAX_TEXT);

/** Client line, e.g. "Mumcord 1.2.0"; the release string already carries the version. */
function clientText(stats: UserStats): string | null {
  const text = stats.clientRelease ?? stats.clientVersion;
  return text ? cap(text) : null;
}

function osText(stats: UserStats): string | null {
  if (!stats.os) return null;
  return cap(stats.osVersion ? `${stats.os} ${stats.osVersion}` : stats.os);
}

/**
 * Connection section of a person (FR-2.25b): one row with the quality level, expanding to ping,
 * packet loss, bandwidth, UDP/TCP, client and OS (FR-2.25), plus how an open peer-to-peer
 * connection runs (FR-4.19). Fields the server did not return are left out.
 */
export function ConnectionQuality({
  session,
  serverId,
}: {
  session: number;
  serverId?: number | undefined;
}) {
  const { t } = useTranslation("members");
  const { t: tp } = useTranslation("p2p");
  const stats = useUserStats(session, serverId);
  const shown = useShownServerId();
  const link = useP2pLink(serverId ?? shown, session);
  const [expanded, setExpanded] = useState(peekConnectionExpandedRequest);
  useEffect(() => {
    takeConnectionExpandedRequest();
  }, []);
  const detailsId = useId();
  const level = connectionLevel(stats);
  const rows: [string, ReactNode][] = [];
  if (stats) {
    if (stats.pingMs !== null)
      rows.push([t("quality.ping"), t("quality.ms", { value: Math.round(stats.pingMs) })]);
    if (stats.lossPercent !== null)
      rows.push([t("quality.loss"), t("quality.percent", { value: stats.lossPercent.toFixed(1) })]);
    if (stats.latePercent !== null)
      rows.push([t("quality.late"), t("quality.percent", { value: stats.latePercent.toFixed(1) })]);
    if (stats.bandwidthKbps !== null)
      rows.push([t("quality.bandwidth"), t("quality.kbps", { value: stats.bandwidthKbps })]);
    if (stats.transport !== null)
      rows.push([t("quality.transport"), stats.transport === "udp" ? "UDP" : "TCP"]);
    const client = clientText(stats);
    if (client) rows.push([t("quality.client"), client]);
    const os = osText(stats);
    if (os) rows.push([t("quality.os"), os]);
  }
  if (link) rows.push([tp("link.label"), <P2pLinkText link={link} />]);
  const open = expanded && rows.length > 0;
  return (
    <div className="connection-quality" data-level={level}>
      <button
        type="button"
        className="connection-quality__toggle"
        aria-expanded={open}
        aria-controls={open ? detailsId : undefined}
        aria-disabled={rows.length === 0}
        onClick={() => {
          if (rows.length > 0) setExpanded(!expanded);
        }}
      >
        <svg viewBox="0 0 24 24" width="16" height="16" aria-hidden="true" focusable="false">
          <path
            d="M4 20v-3M9 20v-7M14 20v-11M19 20V4"
            fill="none"
            stroke="currentColor"
            strokeWidth="2.5"
            strokeLinecap="round"
            className="connection-quality__icon"
          />
        </svg>
        {t("quality.summary", { level: t(`quality.level.${level}`) })}
        <svg
          className="connection-quality__chevron"
          viewBox="0 0 24 24"
          width="14"
          height="14"
          aria-hidden="true"
          focusable="false"
        >
          <path
            d="M9 6l6 6l-6 6"
            fill="none"
            stroke="currentColor"
            strokeWidth="2"
            strokeLinecap="round"
            strokeLinejoin="round"
          />
        </svg>
      </button>
      {open && (
        <dl id={detailsId} className="connection-quality__details" aria-label={t("quality.label")}>
          {rows.map(([name, value]) => (
            <div key={name} className="connection-quality__row">
              <dt>{name}</dt>
              <dd>{value}</dd>
            </div>
          ))}
        </dl>
      )}
    </div>
  );
}
